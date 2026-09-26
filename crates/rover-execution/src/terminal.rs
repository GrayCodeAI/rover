//! Bounded screen model for untrusted local PTY output.
//!
//! The terminal core interprets VT control sequences into cells. Rover exports
//! only filtered cell text plus a coalesced bell signal; terminal sequences
//! never reach the TUI renderer.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::Processor;

const MIN_COLUMNS: usize = 2;
const MAX_ROWS: usize = 1000;
const MAX_COLUMNS: usize = 1000;
const MAX_VISIBLE_CELLS: usize = 250_000;
const MAX_SCROLLBACK_LINES: usize = 10_000;
const MAX_SCROLLBACK_CELLS: usize = 1_000_000;
const MAX_OUTPUT_CHUNK: usize = 64 * 1024;
const MAX_OSC_TITLE_BYTES: usize = 4096;
const MAX_OSC_PROGRESS_BYTES: usize = 4096;

/// One validated terminal viewport size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    /// Number of visible rows.
    pub rows: usize,
    /// Number of visible columns.
    pub columns: usize,
}

impl TerminalSize {
    /// Validate and construct a viewport size.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for dimensions outside the supported bounds or
    /// for viewports exceeding the cell budget.
    pub fn new(rows: usize, columns: usize) -> io::Result<Self> {
        validate_size(rows, columns)?;
        Ok(Self { rows, columns })
    }
}

/// VT screen emulator with bounded viewport and scrollback storage.
pub struct TerminalScreen {
    terminal: Term<BellListener>,
    parser: Processor,
    size: TerminalSize,
    requested_scrollback: usize,
    history_limit: usize,
    bell: Arc<AtomicBool>,
    osc_title: Arc<Mutex<Option<String>>>,
    osc_progress: OscProgressCapture,
}

#[derive(Clone)]
struct BellListener {
    bell: Arc<AtomicBool>,
    osc_title: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
struct OscProgressCapture {
    payload: Option<Vec<u8>>,
    pending_escape: bool,
    progress: Option<String>,
}

impl OscProgressCapture {
    fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.payload.is_none() {
                if self.pending_escape {
                    self.pending_escape = false;
                    if byte == b']' {
                        self.payload = Some(Vec::new());
                    } else if byte == 0x1b {
                        self.pending_escape = true;
                    }
                } else if byte == 0x1b {
                    self.pending_escape = true;
                }
                continue;
            }

            if byte == 0x07 || byte == 0x9c || (self.pending_escape && byte == b'\\') {
                let completed = self.payload.take().unwrap_or_default();
                self.pending_escape = false;
                self.update_progress(&completed);
                continue;
            }
            if self.pending_escape {
                self.payload = if byte == b']' { Some(Vec::new()) } else { None };
                self.pending_escape = false;
                self.progress = None;
                continue;
            }
            if byte == 0x1b {
                self.pending_escape = true;
                continue;
            }
            if byte < 0x20 {
                self.payload = None;
                self.progress = None;
                continue;
            }
            let Some(payload) = self.payload.as_mut() else {
                continue;
            };
            if payload.len() >= MAX_OSC_PROGRESS_BYTES {
                self.payload = None;
                self.progress = None;
                continue;
            }
            payload.push(byte);
        }
    }

    fn update_progress(&mut self, payload: &[u8]) {
        let Some(value) = payload.strip_prefix(b"9;") else {
            return;
        };
        if !value.starts_with(b"4;") {
            return;
        }
        if !value
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b';' | b'-' | b'?'))
        {
            self.progress = None;
            return;
        }
        self.progress = Some(String::from_utf8_lossy(value).into_owned());
    }
}

impl EventListener for BellListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Bell => self.bell.store(true, Ordering::Relaxed),
            Event::Title(title) if !title.is_empty() && title.len() <= MAX_OSC_TITLE_BYTES => {
                *self
                    .osc_title
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(title);
            }
            Event::Title(_) | Event::ResetTitle => {
                *self
                    .osc_title
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            _ => {}
        }
    }
}

impl TerminalScreen {
    /// Create a screen with at most `scrollback_lines` history rows.
    ///
    /// The effective history cap is reduced for wide terminals so the retained
    /// scrollback never exceeds the configured cell budget. OSC 52 clipboard
    /// requests and all outward terminal events except a coalesced bell are
    /// disabled.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for invalid size or a history request above the
    /// supported 10,000-line maximum.
    pub fn new(size: TerminalSize, scrollback_lines: usize) -> io::Result<Self> {
        validate_size(size.rows, size.columns)?;
        if scrollback_lines > MAX_SCROLLBACK_LINES {
            return Err(invalid_input("scrollback exceeds 10000 lines"));
        }
        let history_limit = history_capacity(scrollback_lines, size.columns);
        let config = Config {
            scrolling_history: history_limit,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let dimensions = GridSize::from(size);
        let bell = Arc::new(AtomicBool::new(false));
        let osc_title = Arc::new(Mutex::new(None));
        let terminal = Term::new(
            config,
            &dimensions,
            BellListener {
                bell: Arc::clone(&bell),
                osc_title: Arc::clone(&osc_title),
            },
        );
        Ok(Self {
            terminal,
            parser: Processor::new(),
            size,
            requested_scrollback: scrollback_lines,
            history_limit,
            bell,
            osc_title,
            osc_progress: OscProgressCapture::default(),
        })
    }

    /// Parse one bounded byte chunk from the PTY into terminal screen state.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` if one chunk exceeds 64 KiB.
    pub fn process_output(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_OUTPUT_CHUNK {
            return Err(invalid_input("PTY output chunk exceeds 64 KiB"));
        }
        self.osc_progress.advance(bytes);
        self.parser.advance(&mut self.terminal, bytes);
        Ok(())
    }

    /// Resize the viewport while maintaining the bounded history budget.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for unsupported dimensions or a viewport above
    /// the cell budget.
    pub fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        validate_size(size.rows, size.columns)?;
        let config = Config {
            scrolling_history: history_capacity(self.requested_scrollback, size.columns),
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        self.terminal.set_options(config);
        self.terminal.resize(GridSize::from(size));
        self.size = size;
        self.history_limit = history_capacity(self.requested_scrollback, size.columns);
        Ok(())
    }

    /// Current viewport dimensions.
    #[must_use]
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    /// Number of retained history rows after applying the cell budget.
    #[must_use]
    pub fn history_size(&self) -> usize {
        self.terminal.grid().history_size()
    }

    /// Maximum history rows retained for the current width.
    #[must_use]
    pub const fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Consume one or more terminal bell events since the previous call.
    /// Multiple events are coalesced to prevent a stalled UI from flooding
    /// the host terminal with bell requests.
    #[must_use]
    pub fn take_bell(&self) -> bool {
        self.bell.swap(false, Ordering::Relaxed)
    }

    /// Plain, filtered text from history and the visible screen.
    ///
    /// This contains cell characters only; ANSI/VT sequences and hyperlink
    /// targets are not returned. Control and bidirectional formatting
    /// characters are replaced before callers render or export the text.
    #[must_use]
    pub fn text(&self) -> String {
        let grid = self.terminal.grid();
        let start = Point::new(grid.topmost_line(), Column(0));
        let end = Point::new(grid.bottommost_line(), Column(grid.columns()));
        self.terminal
            .bounds_to_string(start, end)
            .chars()
            .map(safe_text_character)
            .collect()
    }

    /// Plain, filtered text from the live viewport only, excluding scrollback.
    #[must_use]
    pub fn visible_text(&self) -> String {
        let grid = self.terminal.grid();
        let last = grid.bottommost_line();
        let visible_rows = i32::try_from(self.size.rows).unwrap_or_default();
        let first = Line(last.0 - visible_rows + 1);
        self.terminal
            .bounds_to_string(
                Point::new(first, Column(0)),
                Point::new(last, Column(grid.columns())),
            )
            .chars()
            .map(safe_text_character)
            .collect()
    }

    /// Current OSC window title, if set and within the evidence limit.
    #[must_use]
    pub fn osc_title(&self) -> Option<String> {
        self.osc_title
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Most recently observed bounded OSC 9;4 payload, without the OSC
    /// command prefix. Other OSC commands are ignored.
    #[must_use]
    pub fn osc_progress(&self) -> Option<&str> {
        self.osc_progress.progress.as_deref()
    }
}

#[derive(Clone, Copy)]
struct GridSize {
    rows: usize,
    columns: usize,
}

impl From<TerminalSize> for GridSize {
    fn from(size: TerminalSize) -> Self {
        Self {
            rows: size.rows,
            columns: size.columns,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

fn validate_size(rows: usize, columns: usize) -> io::Result<()> {
    let cells = rows.checked_mul(columns);
    if rows == 0
        || rows > MAX_ROWS
        || !(MIN_COLUMNS..=MAX_COLUMNS).contains(&columns)
        || cells.is_none_or(|count| count > MAX_VISIBLE_CELLS)
    {
        return Err(invalid_input(
            "terminal dimensions exceed supported cell limits",
        ));
    }
    Ok(())
}

fn history_capacity(requested_lines: usize, columns: usize) -> usize {
    requested_lines.min(MAX_SCROLLBACK_CELLS / columns)
}

fn safe_text_character(character: char) -> char {
    let codepoint = u32::from(character);
    let bidi_format = matches!(
        codepoint,
        0x061C | 0x200E..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069 | 0xFEFF
    );
    if bidi_format || (character.is_control() && !matches!(character, '\n' | '\t')) {
        '\u{FFFD}'
    } else {
        character
    }
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_styling_without_returning_terminal_controls() {
        let mut screen = TerminalScreen::new(TerminalSize::new(4, 40).unwrap(), 20).unwrap();
        screen
            .process_output(b"\x1b[31mred\x1b[0m visible\r\n")
            .unwrap();
        let text = screen.text();
        assert!(text.contains("red visible"));
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn live_viewport_excludes_scrollback_and_osc_title_is_bounded() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 20).unwrap(), 10).unwrap();
        screen.process_output(b"first\r\nsecond\r\nthird").unwrap();
        assert!(screen.text().contains("first"));
        let visible = screen.visible_text();
        assert!(!visible.contains("first"));
        assert!(visible.contains("second"));
        assert!(visible.contains("third"));

        screen.process_output(b"\x1b]0;agent title\x07").unwrap();
        assert_eq!(screen.osc_title().as_deref(), Some("agent title"));
        screen.process_output(b"\x1b]0;\x07").unwrap();
        assert_eq!(screen.osc_title(), None);
    }

    #[test]
    fn osc_9_4_progress_capture_handles_fragmented_bel_and_st_terminators() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 20).unwrap(), 0).unwrap();
        screen.process_output(b"\x1b]9;4;1;50").unwrap();
        assert_eq!(screen.osc_progress(), None);
        screen.process_output(b"\x07").unwrap();
        assert_eq!(screen.osc_progress(), Some("4;1;50"));

        screen.process_output(b"\x1b]9;5;ignored\x07").unwrap();
        assert_eq!(screen.osc_progress(), Some("4;1;50"));

        screen.process_output(b"\x1b]9;4;3;?\x1b\\").unwrap();
        assert_eq!(screen.osc_progress(), Some("4;3;?"));

        screen.process_output(b"\x1b]9;4;invalid\x07").unwrap();
        assert_eq!(screen.osc_progress(), None);

        let oversize = format!("\x1b]9;4;{}\x07", "1".repeat(MAX_OSC_PROGRESS_BYTES));
        screen.process_output(oversize.as_bytes()).unwrap();
        assert_eq!(screen.osc_progress(), None);
    }

    #[test]
    fn clipboard_title_query_and_device_control_sequences_have_no_outward_effect() {
        let mut screen = TerminalScreen::new(TerminalSize::new(4, 40).unwrap(), 20).unwrap();
        screen
            .process_output(
                b"before\x1b]52;c;U0VDUkVU\x07\x1b]0;untrusted title\x07\x1b[6n\x1bP1;2|device payload\x1b\\after",
            )
            .unwrap();
        let text = screen.text();
        assert!(text.contains("before"));
        assert!(text.contains("after"));
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("untrusted title"));
        assert!(!text.contains("device payload"));
        assert!(!text.contains('\x1b'));
        assert!(!screen.take_bell());
    }

    #[test]
    fn terminal_bell_is_reported_once_and_repeated_bells_are_coalesced() {
        let mut screen = TerminalScreen::new(TerminalSize::new(4, 40).unwrap(), 20).unwrap();
        screen.process_output(b"text\x07\x07").unwrap();
        assert!(screen.take_bell());
        assert!(!screen.take_bell());
        screen.process_output(b"\x1b]0;title\x07").unwrap();
        assert!(!screen.take_bell());
    }

    #[test]
    fn scrollback_is_capped_by_requested_lines_and_cell_budget() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 20).unwrap(), 5).unwrap();
        for index in 0..12 {
            screen
                .process_output(format!("line-{index:02}\r\n").as_bytes())
                .unwrap();
        }
        assert!(screen.history_size() <= 5);
        let text = screen.text();
        assert!(!text.contains("line-00"));
        assert!(text.contains("line-11"));

        let wide = TerminalScreen::new(TerminalSize::new(2, 1000).unwrap(), 10_000).unwrap();
        assert_eq!(wide.history_limit(), MAX_SCROLLBACK_CELLS / 1000);
    }

    #[test]
    fn resize_reduces_retained_history_to_the_new_cell_budget() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 20).unwrap(), 5000).unwrap();
        for index in 0..1200 {
            screen
                .process_output(format!("row-{index:03}\r\n").as_bytes())
                .unwrap();
        }
        assert!(screen.history_size() > 1000);

        screen.resize(TerminalSize::new(2, 1000).unwrap()).unwrap();

        assert_eq!(screen.history_limit(), 1000);
        assert!(screen.history_size() <= screen.history_limit());
        assert!(screen.text().contains("row-1199"));
    }

    #[test]
    fn shrinking_after_wide_glyphs_and_hostile_erase_sequences_does_not_panic() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 8).unwrap(), 8).unwrap();
        screen
            .process_output("ab界\r\nwide text".as_bytes())
            .unwrap();
        screen.resize(TerminalSize::new(2, 2).unwrap()).unwrap();
        screen.process_output(b"\x1b[K\x1b[2Jsafe").unwrap();
        assert!(screen.text().contains("safe"));
        assert_eq!(
            screen.size(),
            TerminalSize {
                rows: 2,
                columns: 2
            }
        );
    }

    #[test]
    fn screen_dimensions_and_input_chunks_are_bounded() {
        assert!(TerminalSize::new(1000, 1000).is_err());
        assert!(TerminalSize::new(1, 1).is_err());
        assert!(TerminalSize::new(0, 80).is_err());

        let mut screen = TerminalScreen::new(TerminalSize::new(24, 80).unwrap(), 100).unwrap();
        assert!(screen
            .process_output(&vec![b'x'; MAX_OUTPUT_CHUNK + 1])
            .is_err());
        assert!(TerminalScreen::new(TerminalSize::new(24, 80).unwrap(), 10_001).is_err());
    }

    #[test]
    fn bidi_controls_are_replaced_in_exported_text() {
        let mut screen = TerminalScreen::new(TerminalSize::new(2, 40).unwrap(), 0).unwrap();
        screen.process_output("safe\u{202e}txt".as_bytes()).unwrap();
        assert!(screen.text().contains("safe�txt"));
        assert!(!screen.text().contains('\u{202e}'));
    }
}
