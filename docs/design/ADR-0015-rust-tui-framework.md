# ADR-0015: Rust TUI framework and terminal backend

Status: accepted for the interactive workspace shell.

## Context

Rover's product boundary is CLI and TUI only. The Rust port needs a maintained
renderer with deterministic offscreen testing, a cross-platform terminal
backend, and explicit raw-mode restoration. It must render the validated
workspace model without treating session output as trusted screen text.

## Decision

- Use Ratatui 0.30.2 for layout and widget rendering. Its published MSRV is
  Rust 1.88, matching Rover's pinned toolchain. Use its `TestBackend` for
  deterministic frame tests.
- Use Crossterm 0.29.0 for input, resize, raw mode, and alternate-screen
  handling. Ratatui 0.30.2 selects Crossterm 0.29 by default; pin the same
  exact version directly so event types and terminal state are not duplicated.
- Disable default features on Rover's direct dependencies and enable Ratatui's
  `crossterm` backend plus Crossterm's `events`, `bracketed-paste`, and
  `windows` features. Ratatui's backend integration also activates Crossterm's
  `derive-more` feature through its dependency graph. The resolved graph has a
  single Crossterm 0.29.0 and does not enable its `osc52` clipboard feature or
  Ratatui's calendar widget.
- Put presentation and input mapping in `rover-tui`, separate from the future
  CLI parser and the session process owner. Restore raw mode, alternate screen,
  cursor visibility, and bracketed paste through an RAII guard on normal return,
  I/O error, or unwind.
- Render only validated workspace metadata in the current increment. PTY bytes
  must pass through Rover's terminal screen model before any later screen pane
  is rendered.

## Alternatives

- Direct Crossterm drawing would require Rover to build its own cell buffer,
  split layout, clipping, diff, and offscreen test machinery.
- Termion is not an option for Rover's Windows support envelope. Ratatui also
  documents Termwiz and Termina backends, but neither is selected because the
  current PTY and terminal input paths already use Crossterm semantics.
- Cursive uses a different rendering model and adds a curses dependency; it is
  not selected for this pane-oriented layout model.

## Dependency and test contract

Both selected crates declare MIT licenses; the locked dependency audit must
bundle and hash their exact license and author files and reject any unresolved
transitive SPDX expression. Keep the lockfile committed. Tests cover the
workspace render buffer, narrow dimensions, key-to-action behavior, terminal
state restoration, and one Crossterm version in the resolved graph. No live
terminal or provider result is inferred from `TestBackend`.

## References

- [Ratatui installation and MSRV](https://ratatui.rs/installation/)
- [Ratatui backend/version guidance](https://ratatui.rs/concepts/backends/)
- [Ratatui feature flags](https://ratatui.rs/installation/feature-flags/)
- [Crossterm 0.29.0 manifest and feature set](https://docs.rs/crate/crossterm/0.29.0/source/Cargo.toml.orig)
- [Ratatui TestBackend API](https://docs.rs/ratatui/0.30.2/ratatui/backend/struct.TestBackend.html)
- [Rust parity plan](RUST_PARITY_PLAN.md)
