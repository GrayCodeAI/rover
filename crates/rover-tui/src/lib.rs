//! Interactive terminal presentation and input for Rover workspaces.
//!
//! This increment renders validated layout metadata. It intentionally does
//! not render PTY output; session screens must pass through Rover's terminal
//! emulator before they can be shown.

use std::io::{self, Write};
#[cfg(unix)]
use std::sync::mpsc::{self, Receiver, SyncSender};
#[cfg(unix)]
use std::thread;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
#[cfg(test)]
use ratatui::backend::TestBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
#[cfg(unix)]
use rover_agents::AgentSessionBinding;
use rover_agents::{
    bundled_manifest_for_executable, detect_agent, evaluate_herdr_manifest,
    identify_foreground_process_group, AgentState, Confidence, DetectionEvidence, DetectionRequest,
    DetectionResult, DetectionVisibility, ProcessArgvSample, ProcessSample,
};
use rover_execution::layout::{Pane, PaneKind, PaneNode, Split, SplitAxis, Workspace};
#[cfg(unix)]
use rover_execution::sessions::{SessionClient, SessionFrame, SessionReader, SessionWriter};
use rover_execution::terminal::TerminalScreen;
#[cfg(unix)]
use rover_execution::terminal::TerminalSize;
#[cfg(unix)]
use rover_source::{
    BrowserEntry, BrowserEntryKind, BrowserGitStatus, BrowserListing, MAX_BROWSER_ENTRIES,
};
#[cfg(unix)]
use rover_store::files::SafeDir;

const MAX_SCREEN_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_TASK_BOARD_ITEMS: usize = 12;
const MAX_TASK_LOG_BYTES: usize = 64 * 1024;
const MAX_TASK_EVIDENCE_ITEMS: usize = 256;
const MAX_TASK_DIFF_ITEMS: usize = 256;
const MAX_TASK_DIFF_RECORDS: usize = 20_000;
const MAX_TASK_FILTER_BYTES: usize = 128;
#[cfg(unix)]
const MAX_PALETTE_QUERY_BYTES: usize = 96;
#[cfg(unix)]
/// Maximum UTF-8 bytes stored in one repository note.
pub const MAX_WORKSPACE_NOTES_BYTES: usize = 32 * 1024;
#[cfg(unix)]
/// Maximum number of notes stored for one repository.
pub const MAX_WORKSPACE_NOTES_COUNT: usize = 32;
#[cfg(unix)]
/// Maximum combined UTF-8 bytes stored across repository notes.
pub const MAX_WORKSPACE_NOTES_TOTAL_BYTES: usize = 128 * 1024;
#[cfg(unix)]
/// Maximum saved shell commands per repository.
pub const MAX_SAVED_COMMANDS: usize = 32;
#[cfg(unix)]
/// Maximum UTF-8 bytes in a saved command name.
pub const MAX_SAVED_COMMAND_NAME_BYTES: usize = 80;
#[cfg(unix)]
/// Maximum UTF-8 bytes in a saved shell command line.
pub const MAX_SAVED_COMMAND_LINE_BYTES: usize = 256;
#[cfg(unix)]
/// Maximum saved task briefing drafts for one repository.
pub const MAX_TASK_DRAFTS: usize = 32;
#[cfg(unix)]
/// Maximum UTF-8 bytes in a task draft title.
pub const MAX_TASK_DRAFT_TITLE_BYTES: usize = 120;
#[cfg(unix)]
/// Maximum UTF-8 bytes in task draft path globs.
pub const MAX_TASK_DRAFT_PATHS_BYTES: usize = 8 * 1024;
#[cfg(unix)]
/// Maximum UTF-8 bytes in task draft dependencies.
pub const MAX_TASK_DRAFT_DEPENDENCIES_BYTES: usize = 4 * 1024;
#[cfg(unix)]
/// Maximum UTF-8 bytes in a task draft quality-gate command.
pub const MAX_TASK_DRAFT_GATE_BYTES: usize = 1024;
#[cfg(unix)]
/// Maximum UTF-8 bytes in a task draft prompt.
pub const MAX_TASK_DRAFT_PROMPT_BYTES: usize = 16 * 1024;
#[cfg(unix)]
const MAX_FILE_BROWSER_ROWS: usize = 48;
#[cfg(unix)]
const MAX_FILE_QUERY_BYTES: usize = 128;
#[cfg(unix)]
const MAX_FILE_EDIT_BYTES: usize = 8 * 1024 * 1024;

type TaskRefresh<'a> = dyn FnMut() -> io::Result<Vec<TaskSummary>> + 'a;
type TaskBlobLoader<'a> = dyn FnMut(&str) -> io::Result<Vec<u8>> + 'a;
type TaskCanceler<'a> = dyn FnMut(&str) -> io::Result<()> + 'a;
type TaskEvidenceLoader<'a> =
    dyn FnMut(&TaskSummary) -> io::Result<Option<TaskInvestigationEvidence>> + 'a;
type TaskDiffLoader<'a> = dyn FnMut(&TaskSummary) -> io::Result<Option<TaskDiffEvidence>> + 'a;
type TaskViewPreferenceSaver<'a> = dyn FnMut(&TaskViewPreferences) -> io::Result<()> + 'a;
#[cfg(unix)]
type WorkspaceNotesSaver<'a> = dyn FnMut(&[String]) -> io::Result<()> + 'a;
#[cfg(unix)]
type SavedCommandsSaver<'a> = dyn FnMut(&[SavedCommand]) -> io::Result<()> + 'a;
#[cfg(unix)]
type TaskDraftsSaver<'a> = dyn FnMut(&[TaskDraft]) -> io::Result<()> + 'a;
#[cfg(unix)]
type TaskPlansLoader<'a> = dyn FnMut() -> io::Result<Vec<TaskPlanSummary>> + 'a;
#[cfg(unix)]
type TaskPlanCreator<'a> = dyn FnMut(&TaskDraft) -> io::Result<TaskPlanSummary> + 'a;
#[cfg(unix)]
type AgentSessionClearer<'a> = dyn FnMut(&str) -> io::Result<()> + 'a;

#[cfg(unix)]
struct TaskPlanCallbacks<'a> {
    summaries: Vec<TaskPlanSummary>,
    load: Box<TaskPlansLoader<'a>>,
    create: Box<TaskPlanCreator<'a>>,
}

#[cfg(unix)]
impl TaskPlanCallbacks<'_> {
    fn unavailable() -> Self {
        TaskPlanCallbacks {
            summaries: Vec::new(),
            load: Box::new(|| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task plans are unavailable",
                ))
            }),
            create: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task plan creation is unavailable",
                ))
            }),
        }
    }
}
#[cfg(unix)]
type FileBrowserLoader<'a> = dyn FnMut(&str, bool) -> io::Result<BrowserListing> + 'a;
#[cfg(unix)]
type QuickOpenLoader<'a> = dyn FnMut() -> io::Result<BrowserListing> + 'a;
#[cfg(unix)]
type FilePreviewLoader<'a> = dyn FnMut(&str) -> io::Result<(Vec<u8>, bool)> + 'a;
#[cfg(unix)]
type FileEditLoader<'a> = dyn FnMut(&str) -> io::Result<Vec<u8>> + 'a;
#[cfg(unix)]
type FileSaver<'a> = dyn FnMut(&str, &str, &[u8]) -> io::Result<()> + 'a;
#[cfg(unix)]
type ExternalFileEditor<'a> = dyn FnMut(&str, &[u8]) -> io::Result<Vec<u8>> + 'a;

#[derive(Clone, Copy)]
struct TaskBoardView<'a> {
    preferences: &'a TaskViewPreferences,
    search_draft: Option<&'a str>,
}

/// Persisted TUI task-board search and ordering preferences.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskViewPreferences {
    /// Case-insensitive substring matched against task ID, status, objective,
    /// candidate, and task error. Limited to 128 UTF-8 bytes.
    pub query: String,
    /// Stable task-board ordering mode.
    pub sort: TaskSortOrder,
}

/// A user-authored, single-line shell command stored for one repository.
#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedCommand {
    /// User-visible command name.
    pub name: String,
    /// Exact shell input line sent after explicit run confirmation.
    pub command: String,
}

/// A local task briefing draft that has not been dispatched to a worker.
#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDraft {
    /// Short title shown in the draft list.
    pub title: String,
    /// Newline-separated repository-relative path globs.
    pub paths: String,
    /// Newline-separated IDs of prerequisite tasks.
    pub dependencies: String,
    /// Optional quality-gate command, stored as text and never run by draft handling.
    pub quality_gate: String,
    /// Multiline task briefing text.
    pub prompt: String,
}

/// Bounded display data for a durable task plan that has not been dispatched.
#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskPlanSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub readiness: String,
    pub dependency_count: usize,
    pub attempt_count: usize,
    pub updated_at: String,
}

/// Validated, pane-bound native session offered for explicit TUI resume.
#[cfg(unix)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSessionSummary {
    /// Pane associated with the exact saved native session.
    pub pane_id: String,
    /// Native agent profile used to resume this session.
    pub adapter: String,
    /// Exact provider session ID.
    pub session_id: String,
    /// Origin recorded for the session identity.
    pub source: String,
    /// Interactive native command as argv; never shell-parsed here.
    argv: Vec<String>,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AgentSessionPanel {
    #[default]
    Closed,
    Details,
    ConfirmResume,
    ConfirmClear,
}

/// Deterministic task-board sort choices.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TaskSortOrder {
    /// Newest recorded update first, then task ID.
    #[default]
    Updated,
    /// Status, then most recently updated, then task ID.
    Status,
    /// Objective, then task ID.
    Objective,
}

impl TaskSortOrder {
    /// Parse the stable persisted spelling for this sort mode.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "updated" => Some(Self::Updated),
            "status" => Some(Self::Status),
            "objective" => Some(Self::Objective),
            _ => None,
        }
    }

    /// Return the stable persisted spelling for this sort mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Status => "status",
            Self::Objective => "objective",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Updated => Self::Status,
            Self::Status => Self::Objective,
            Self::Objective => Self::Updated,
        }
    }
}

/// Read-only task data prepared from Rover's durable task records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSummary {
    /// Stable Rover task ID.
    pub id: String,
    /// User-authored objective.
    pub objective: String,
    /// Persisted Rover lifecycle status.
    pub status: String,
    /// Last durable update timestamp.
    pub updated_at: String,
    /// Optional persisted task error.
    pub error: Option<String>,
    /// Candidate snapshot or commit reference, when one exists.
    pub candidate: Option<String>,
    /// Frozen base snapshot used to compare a task candidate.
    pub base_snapshot: Option<String>,
    /// Investigation record linked by the task, if verification has started.
    pub investigation_id: Option<String>,
    /// Number of durable execution attempts recorded.
    pub attempt_count: usize,
    /// Bounded process evidence from the most recent task execution.
    pub process: Option<TaskProcessEvidence>,
}

/// Stored investigation summary loaded lazily from the task's linked record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskInvestigationEvidence {
    /// Stable ID of the persisted investigation.
    pub id: String,
    /// Candidate snapshot that the recorded assessment covers.
    pub candidate: String,
    /// Persisted policy decision, such as `BLOCKED` or `READY`.
    pub decision: String,
    /// Human-readable reason stored with the decision.
    pub decision_reason: String,
    /// Individual checks recorded by the investigation.
    pub checks: Vec<TaskCheckEvidence>,
    /// Unresolved conditions recorded by the investigation.
    pub unknowns: Vec<String>,
    /// Findings recorded by the investigation.
    pub findings: Vec<TaskEvidenceFinding>,
}

/// One persisted check outcome; it is displayed as recorded evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCheckEvidence {
    /// Stable check identifier.
    pub id: String,
    /// Recorded outcome, not a fresh check run.
    pub outcome: String,
    /// Explanation recorded for the check.
    pub meaning: String,
    /// Whether the check was required by the stored policy.
    pub required: bool,
    /// Number of tests reported by the check.
    pub tests: i64,
    /// Number of skipped tests reported by the check.
    pub skipped: i64,
}

/// A persisted finding associated with an investigation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskEvidenceFinding {
    /// Optional repository-relative finding path.
    pub path: String,
    /// Finding text from the stored investigation.
    pub message: String,
    /// Optional recorded severity label.
    pub severity: Option<String>,
}

/// Bounded path-level changes between a task's retained base and candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDiffEvidence {
    /// Stable base snapshot identity.
    pub base_snapshot: String,
    /// Stable candidate snapshot identity.
    pub candidate: String,
    /// Path-sorted changes from Rover's source comparison.
    pub changes: Vec<TaskDiffChange>,
}

/// One path-level status in a verified candidate comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDiffChange {
    /// Repository-relative path.
    pub path: String,
    /// Added, deleted, or modified.
    pub status: String,
    /// Rover's filename-based risk category.
    pub category: String,
}

/// Task data and callbacks for the attached task workbench.
pub struct TaskProvider<'a> {
    tasks: Vec<TaskSummary>,
    refresh: Box<TaskRefresh<'a>>,
    load_blob: Box<TaskBlobLoader<'a>>,
    cancel_task: Box<TaskCanceler<'a>>,
    load_evidence: Box<TaskEvidenceLoader<'a>>,
    load_diff: Box<TaskDiffLoader<'a>>,
    save_view_preferences: Box<TaskViewPreferenceSaver<'a>>,
    view_preferences: TaskViewPreferences,
    #[cfg(unix)]
    workspace_notes: Vec<String>,
    #[cfg(unix)]
    save_workspace_notes: Box<WorkspaceNotesSaver<'a>>,
    #[cfg(unix)]
    saved_commands: Vec<SavedCommand>,
    #[cfg(unix)]
    save_saved_commands: Box<SavedCommandsSaver<'a>>,
    #[cfg(unix)]
    task_drafts: Vec<TaskDraft>,
    #[cfg(unix)]
    save_task_drafts: Box<TaskDraftsSaver<'a>>,
    #[cfg(unix)]
    task_plans: TaskPlanCallbacks<'a>,
    #[cfg(unix)]
    agent_session: Option<AgentSessionSummary>,
    #[cfg(unix)]
    clear_agent_session: Box<AgentSessionClearer<'a>>,
    #[cfg(unix)]
    load_directory: Box<FileBrowserLoader<'a>>,
    #[cfg(unix)]
    load_quick_open: Box<QuickOpenLoader<'a>>,
    #[cfg(unix)]
    load_file_preview: Box<FilePreviewLoader<'a>>,
    #[cfg(unix)]
    load_file_for_edit: Box<FileEditLoader<'a>>,
    #[cfg(unix)]
    save_file: Box<FileSaver<'a>>,
    #[cfg(unix)]
    external_file_editor: Box<ExternalFileEditor<'a>>,
    capabilities: TaskCapabilities,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TaskCapabilities(u16);

impl TaskCapabilities {
    const REFRESH: u16 = 1 << 0;
    const LOGS: u16 = 1 << 1;
    const CANCEL: u16 = 1 << 2;
    const EVIDENCE: u16 = 1 << 3;
    const DIFF: u16 = 1 << 4;
    const FILES: u16 = 1 << 5;
    const FILE_EDIT: u16 = 1 << 6;
    #[cfg(unix)]
    const NOTES: u16 = 1 << 7;
    #[cfg(unix)]
    const SAVED_COMMANDS: u16 = 1 << 8;
    #[cfg(unix)]
    const TASK_DRAFTS: u16 = 1 << 9;
    #[cfg(unix)]
    const TASK_PLANS: u16 = 1 << 10;
    #[cfg(unix)]
    const AGENT_SESSION: u16 = 1 << 11;

    const ALL: Self = Self(Self::REFRESH | Self::LOGS | Self::CANCEL | Self::EVIDENCE | Self::DIFF);

    fn read_only(logs: bool) -> Self {
        Self(if logs { Self::LOGS } else { 0 })
    }

    fn supports(self, capability: u16) -> bool {
        self.0 & capability != 0
    }
}

impl<'a> TaskProvider<'a> {
    /// Create a task snapshot provider with refresh, blob, mutation, evidence, and diff callbacks.
    pub fn new(
        tasks: Vec<TaskSummary>,
        refresh: impl FnMut() -> io::Result<Vec<TaskSummary>> + 'a,
        load_blob: impl FnMut(&str) -> io::Result<Vec<u8>> + 'a,
        cancel_task: impl FnMut(&str) -> io::Result<()> + 'a,
        load_evidence: impl FnMut(&TaskSummary) -> io::Result<Option<TaskInvestigationEvidence>> + 'a,
        load_diff: impl FnMut(&TaskSummary) -> io::Result<Option<TaskDiffEvidence>> + 'a,
    ) -> Self {
        Self {
            tasks,
            refresh: Box::new(refresh),
            load_blob: Box::new(load_blob),
            cancel_task: Box::new(cancel_task),
            load_evidence: Box::new(load_evidence),
            load_diff: Box::new(load_diff),
            save_view_preferences: Box::new(|_| Ok(())),
            view_preferences: TaskViewPreferences::default(),
            #[cfg(unix)]
            workspace_notes: Vec::new(),
            #[cfg(unix)]
            save_workspace_notes: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "workspace notes are unavailable",
                ))
            }),
            #[cfg(unix)]
            saved_commands: Vec::new(),
            #[cfg(unix)]
            save_saved_commands: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "saved commands are unavailable",
                ))
            }),
            #[cfg(unix)]
            task_drafts: Vec::new(),
            #[cfg(unix)]
            save_task_drafts: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task drafts are unavailable",
                ))
            }),
            #[cfg(unix)]
            task_plans: TaskPlanCallbacks::unavailable(),
            #[cfg(unix)]
            agent_session: None,
            #[cfg(unix)]
            clear_agent_session: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "agent session removal is unavailable",
                ))
            }),
            #[cfg(unix)]
            load_directory: Box::new(|_, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file browser unavailable",
                ))
            }),
            #[cfg(unix)]
            load_quick_open: Box::new(|| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "quick open unavailable",
                ))
            }),
            #[cfg(unix)]
            load_file_preview: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file preview unavailable",
                ))
            }),
            #[cfg(unix)]
            load_file_for_edit: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file editing unavailable",
                ))
            }),
            #[cfg(unix)]
            save_file: Box::new(|_, _, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file saving unavailable",
                ))
            }),
            #[cfg(unix)]
            external_file_editor: Box::new(|_, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "external editor unavailable",
                ))
            }),
            capabilities: TaskCapabilities::ALL,
        }
    }

    /// Install this repository's saved task-board view and its durable saver.
    pub fn configure_view_preferences(
        &mut self,
        preferences: &TaskViewPreferences,
        save: impl FnMut(&TaskViewPreferences) -> io::Result<()> + 'a,
    ) {
        self.view_preferences = TaskViewPreferences {
            query: truncate_utf8(&preferences.query, MAX_TASK_FILTER_BYTES),
            sort: preferences.sort,
        };
        self.save_view_preferences = Box::new(save);
    }

    /// Install a bounded repository-scoped note collection and its durable saver.
    #[cfg(unix)]
    pub fn configure_workspace_notes(
        &mut self,
        notes: &[String],
        save: impl FnMut(&[String]) -> io::Result<()> + 'a,
    ) {
        self.workspace_notes = bounded_workspace_notes(notes);
        self.save_workspace_notes = Box::new(save);
        self.capabilities.0 |= TaskCapabilities::NOTES;
    }

    /// Install bounded repository-scoped shell commands and their durable saver.
    #[cfg(unix)]
    pub fn configure_saved_commands(
        &mut self,
        commands: &[SavedCommand],
        save: impl FnMut(&[SavedCommand]) -> io::Result<()> + 'a,
    ) {
        self.saved_commands = bounded_saved_commands(commands);
        self.save_saved_commands = Box::new(save);
        self.capabilities.0 |= TaskCapabilities::SAVED_COMMANDS;
    }

    /// Install this repository's saved task briefing drafts and durable saver.
    #[cfg(unix)]
    pub fn configure_task_drafts(
        &mut self,
        drafts: &[TaskDraft],
        save: impl FnMut(&[TaskDraft]) -> io::Result<()> + 'a,
    ) {
        self.task_drafts = bounded_task_drafts(drafts);
        self.save_task_drafts = Box::new(save);
        self.capabilities.0 |= TaskCapabilities::TASK_DRAFTS;
    }

    /// Install this repository's plan snapshot and draft-to-plan creator.
    #[cfg(unix)]
    pub fn configure_task_plans(
        &mut self,
        plans: &[TaskPlanSummary],
        load: impl FnMut() -> io::Result<Vec<TaskPlanSummary>> + 'a,
        create: impl FnMut(&TaskDraft) -> io::Result<TaskPlanSummary> + 'a,
    ) {
        self.task_plans = TaskPlanCallbacks {
            summaries: bounded_task_plan_summaries(plans),
            load: Box::new(load),
            create: Box::new(create),
        };
        self.capabilities.0 |= TaskCapabilities::TASK_PLANS;
    }

    /// Offer one exact native session in the TUI with explicit resume/clear confirmations.
    ///
    /// # Errors
    ///
    /// Returns an error if the binding cannot produce a reviewed interactive
    /// resume command for its adapter.
    #[cfg(unix)]
    pub fn configure_agent_session(
        &mut self,
        binding: &AgentSessionBinding,
        clear: impl FnMut(&str) -> io::Result<()> + 'a,
    ) -> io::Result<()> {
        let argv = binding
            .resume_tui_argv()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        self.agent_session = Some(AgentSessionSummary {
            pane_id: binding.pane_id().to_owned(),
            adapter: binding.adapter().to_owned(),
            session_id: binding.session_id().to_owned(),
            source: binding.source().as_str().to_owned(),
            argv,
        });
        self.clear_agent_session = Box::new(clear);
        self.capabilities.0 |= TaskCapabilities::AGENT_SESSION;
        Ok(())
    }

    /// Install read-only repository file-tree, quick-open, and preview loaders.
    #[cfg(unix)]
    pub fn configure_file_browser(
        &mut self,
        load_directory: impl FnMut(&str, bool) -> io::Result<BrowserListing> + 'a,
        load_quick_open: impl FnMut() -> io::Result<BrowserListing> + 'a,
        load_file_preview: impl FnMut(&str) -> io::Result<(Vec<u8>, bool)> + 'a,
    ) {
        self.load_directory = Box::new(load_directory);
        self.load_quick_open = Box::new(load_quick_open);
        self.load_file_preview = Box::new(load_file_preview);
        self.capabilities.0 |= TaskCapabilities::FILES;
    }

    /// Install bounded inline-editor readers and the repository's safe saver.
    #[cfg(unix)]
    pub fn configure_file_editor(
        &mut self,
        load_file: impl FnMut(&str) -> io::Result<Vec<u8>> + 'a,
        save_file: impl FnMut(&str, &str, &[u8]) -> io::Result<()> + 'a,
        external_editor: impl FnMut(&str, &[u8]) -> io::Result<Vec<u8>> + 'a,
    ) {
        self.load_file_for_edit = Box::new(load_file);
        self.save_file = Box::new(save_file);
        self.external_file_editor = Box::new(external_editor);
        self.capabilities.0 |= TaskCapabilities::FILE_EDIT;
    }

    #[allow(clippy::too_many_lines)] // Keep the unsupported read-only callback wiring explicit.
    fn read_only_snapshot(
        tasks: Vec<TaskSummary>,
        load_blob: impl FnMut(&str) -> io::Result<Vec<u8>> + 'a,
        logs: bool,
    ) -> Self {
        Self {
            tasks,
            refresh: Box::new(|| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task refresh is unavailable",
                ))
            }),
            load_blob: Box::new(load_blob),
            cancel_task: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task cancellation is unavailable",
                ))
            }),
            load_evidence: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task evidence is unavailable",
                ))
            }),
            load_diff: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task diff is unavailable",
                ))
            }),
            save_view_preferences: Box::new(|_| Ok(())),
            view_preferences: TaskViewPreferences::default(),
            #[cfg(unix)]
            workspace_notes: Vec::new(),
            #[cfg(unix)]
            save_workspace_notes: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "workspace notes are unavailable",
                ))
            }),
            #[cfg(unix)]
            saved_commands: Vec::new(),
            #[cfg(unix)]
            save_saved_commands: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "saved commands are unavailable",
                ))
            }),
            #[cfg(unix)]
            task_drafts: Vec::new(),
            #[cfg(unix)]
            save_task_drafts: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task drafts are unavailable",
                ))
            }),
            #[cfg(unix)]
            task_plans: TaskPlanCallbacks::unavailable(),
            #[cfg(unix)]
            agent_session: None,
            #[cfg(unix)]
            clear_agent_session: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "agent session removal is unavailable",
                ))
            }),
            #[cfg(unix)]
            load_directory: Box::new(|_, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file browser unavailable",
                ))
            }),
            #[cfg(unix)]
            load_quick_open: Box::new(|| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "quick open unavailable",
                ))
            }),
            #[cfg(unix)]
            load_file_preview: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file preview unavailable",
                ))
            }),
            #[cfg(unix)]
            load_file_for_edit: Box::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file editing unavailable",
                ))
            }),
            #[cfg(unix)]
            save_file: Box::new(|_, _, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file saving unavailable",
                ))
            }),
            #[cfg(unix)]
            external_file_editor: Box::new(|_, _| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "external editor unavailable",
                ))
            }),
            capabilities: TaskCapabilities::read_only(logs),
        }
    }
}

/// Process result metadata and output hashes retained with one task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskProcessEvidence {
    /// Child exit code.
    pub exit_code: i64,
    /// Optional execution error.
    pub error: Option<String>,
    /// Whether the process exceeded its time limit.
    pub timed_out: bool,
    /// Whether Rover cancelled the process.
    pub cancelled: bool,
    /// Whether captured output exceeded Rover's retained output cap.
    pub truncated: bool,
    /// Total observed stdout bytes.
    pub stdout_bytes: i64,
    /// Total observed stderr bytes.
    pub stderr_bytes: i64,
    /// SHA-256 digest recorded by Rover for captured stdout.
    pub stdout_sha256: String,
    /// SHA-256 digest recorded by Rover for captured stderr.
    pub stderr_sha256: String,
}

/// Filtered screen text associated with a pane.
///
/// Construct values from [`TerminalScreen`]; raw PTY bytes do not have a
/// constructor in this crate and must be processed by Rover's emulator first.
pub struct ScreenText {
    pane_id: String,
    text: String,
}

impl ScreenText {
    /// Capture bounded, control-filtered screen text for this pane.
    #[must_use]
    pub fn from_terminal_screen(pane: &Pane, screen: &TerminalScreen) -> Self {
        Self {
            pane_id: pane.id().to_owned(),
            text: screen.text(),
        }
    }
}

/// Render one frame from validated workspace metadata.
pub fn render_workspace(frame: &mut Frame<'_>, workspace: &Workspace) {
    render_workspace_with_screens(frame, workspace, &[]);
}

/// Render workspace metadata and screens previously parsed by Rover's
/// bounded terminal emulator.
pub fn render_workspace_with_screens(
    frame: &mut Frame<'_>,
    workspace: &Workspace,
    screens: &[ScreenText],
) {
    render_workspace_mode(frame, workspace, screens, false, false);
}

fn render_workspace_mode(
    frame: &mut Frame<'_>,
    workspace: &Workspace,
    screens: &[ScreenText],
    attached: bool,
    help_open: bool,
) {
    let task_capabilities = attached.then_some(TaskCapabilities::ALL);
    render_workspace_mode_for_provider(
        frame,
        workspace,
        screens,
        attached,
        help_open,
        task_capabilities,
        None,
    );
}

fn render_workspace_mode_for_provider(
    frame: &mut Frame<'_>,
    workspace: &Workspace,
    screens: &[ScreenText],
    attached: bool,
    help_open: bool,
    task_capabilities: Option<TaskCapabilities>,
    agent_detection: Option<&DetectionResult>,
) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(frame, sections[0], workspace);
    render_node(
        frame,
        workspace.active_tab().render_root(),
        sections[1],
        workspace.active_tab().active_pane(),
        screens,
    );
    let footer = if attached {
        let mut spans = vec![
            Span::styled("Ctrl-]", Style::default().fg(Color::Yellow)),
            Span::raw(" detach   "),
            Span::styled("Ctrl-B", Style::default().fg(Color::Yellow)),
            Span::raw(" then q quit, t tasks, ? help"),
        ];
        if let Some(detection) = agent_detection {
            spans.push(Span::raw("   Agent: "));
            spans.push(Span::styled(
                detection.agent.as_deref().unwrap_or("unknown"),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(format!(
                " · {}",
                agent_state_label(detection.state)
            )));
        }
        Paragraph::new(Line::from(spans))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit   "),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(" next pane   "),
            Span::styled("n", Style::default().fg(Color::Yellow)),
            Span::raw(" new tab   "),
            Span::styled("| / -", Style::default().fg(Color::Yellow)),
            Span::raw(" split   "),
            Span::styled("z", Style::default().fg(Color::Yellow)),
            Span::raw(" zoom   "),
            Span::styled("x", Style::default().fg(Color::Yellow)),
            Span::raw(" close pane   "),
            Span::styled("?", Style::default().fg(Color::Yellow)),
            Span::raw(" help"),
        ]))
    };
    frame.render_widget(footer, sections[2]);

    render_pane_popup(frame, area, workspace.active_tab().popup_pane().is_some());
    if help_open && area.width >= 20 && area.height >= 8 {
        let popup = centered_rect(76, 76, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::from("Workspace"),
            Line::from("Tab / Shift-Tab   focus next / previous pane"),
            Line::from("n                 create a scratch tab"),
            Line::from("[ / ]             previous / next tab"),
            Line::from("| / -             split horizontally / vertically"),
            Line::from("z                 toggle pane zoom"),
            Line::from("p                 toggle pane popup"),
            Line::from("x                 close focused pane"),
            Line::from("q or Ctrl-C       quit Rover TUI"),
            Line::from("? or Esc          close this help"),
        ];
        if attached {
            lines.extend(attached_help_lines(task_capabilities));
        }
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .title(" Rover keyboard help ")
                    .borders(Borders::ALL),
            ),
            popup,
        );
    }
}

fn attached_help_lines(capabilities: Option<TaskCapabilities>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Attached session"),
        Line::from("Ctrl-B then t    open the task board"),
        Line::from("Ctrl-B then p    open the command palette"),
        Line::from("Ctrl-]            detach and leave the session running"),
        Line::from("Ctrl-B then key   run a Rover workspace shortcut"),
    ];
    if let Some(capabilities) = capabilities {
        lines.push(Line::from("Enter           inspect selected task"));
        #[cfg(unix)]
        if capabilities.supports(TaskCapabilities::NOTES) {
            lines.push(Line::from("Ctrl-B then n    open repository notes"));
        }
        #[cfg(unix)]
        if capabilities.supports(TaskCapabilities::SAVED_COMMANDS) {
            lines.push(Line::from("Ctrl-B then k    manage saved shell commands"));
        }
        #[cfg(unix)]
        if capabilities.supports(TaskCapabilities::TASK_DRAFTS) {
            lines.push(Line::from(
                "Ctrl-B then d    manage local task briefing drafts",
            ));
        }
        #[cfg(unix)]
        if capabilities.supports(TaskCapabilities::AGENT_SESSION) {
            lines.push(Line::from(
                "Ctrl-B then a    inspect or resume saved agent session",
            ));
        }
        if capabilities.supports(TaskCapabilities::FILES) {
            lines.extend([
                Line::from("Ctrl-B then f    open the repository file tree"),
                Line::from("Ctrl-B then .    open hidden-file quick open"),
                Line::from("In files: h toggles hidden · . quick open · r refresh"),
            ]);
        }
        if capabilities.supports(TaskCapabilities::FILE_EDIT) {
            lines.push(Line::from(
                "File preview: e edit · Ctrl-S save · Ctrl-D diff · Ctrl-E external editor",
            ));
        }
        if capabilities.supports(TaskCapabilities::REFRESH) {
            lines.push(Line::from("r               refresh project tasks"));
        }
        if capabilities.supports(TaskCapabilities::CANCEL) {
            lines.push(Line::from("c               cancel task (confirmation)"));
        }
        if capabilities.supports(TaskCapabilities::EVIDENCE) {
            lines.push(Line::from("i               inspect recorded investigation"));
        }
        if capabilities.supports(TaskCapabilities::DIFF) {
            lines.push(Line::from(
                "d               inspect verified candidate changes",
            ));
        }
        if capabilities.supports(TaskCapabilities::LOGS) {
            lines.extend([
                Line::from("o / e           open verified stdout / stderr"),
                Line::from("↑/↓, PgUp/PgDn scroll task output"),
            ]);
        }
    }
    lines
}

fn render_pane_popup(frame: &mut Frame<'_>, area: Rect, open: bool) {
    if !open || area.width < 8 || area.height < 5 {
        return;
    }
    let popup = centered_rect(40, 20, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new("Pane actions are controlled by the keyboard")
            .alignment(Alignment::Center)
            .block(Block::default().title(" Pane ").borders(Borders::ALL)),
        popup,
    );
}

#[cfg(test)]
fn render_task_board(
    frame: &mut Frame<'_>,
    tasks: &[TaskSummary],
    selected: usize,
    detail_open: bool,
) {
    render_task_board_with_refresh(
        frame,
        tasks,
        selected,
        detail_open,
        None,
        TaskCapabilities::ALL,
        TaskBoardView {
            preferences: &TaskViewPreferences::default(),
            search_draft: None,
        },
    );
}

fn render_task_board_with_refresh(
    frame: &mut Frame<'_>,
    tasks: &[TaskSummary],
    selected: usize,
    detail_open: bool,
    refresh_message: Option<&str>,
    capabilities: TaskCapabilities,
    view: TaskBoardView<'_>,
) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(86, 82, area);
    frame.render_widget(Clear, popup);
    if detail_open {
        render_task_detail(frame, popup, tasks.get(selected), capabilities);
        if let Some(message) = refresh_message {
            render_task_refresh_message(frame, popup, message);
        }
        return;
    }
    render_task_list(
        frame,
        popup,
        tasks,
        selected,
        refresh_message,
        capabilities,
        view,
    );
}

fn render_task_list(
    frame: &mut Frame<'_>,
    popup: Rect,
    tasks: &[TaskSummary],
    selected: usize,
    refresh_message: Option<&str>,
    capabilities: TaskCapabilities,
    view: TaskBoardView<'_>,
) {
    let preferences = view.preferences;
    let visible_indices = task_visible_indices(tasks, preferences);
    let reserved_rows = 6 + u16::from(view.search_draft.is_some());
    let rows_available = usize::from(popup.height.saturating_sub(reserved_rows)).max(1);
    let visible = visible_indices
        .len()
        .min(MAX_TASK_BOARD_ITEMS)
        .min(rows_available);
    let selected_position = visible_indices
        .iter()
        .position(|index| *index == selected)
        .unwrap_or(0);
    let start = if visible_indices.len() > visible {
        selected_position
            .saturating_sub(visible / 2)
            .min(visible_indices.len().saturating_sub(visible))
    } else {
        0
    };
    let mut lines = vec![Line::from(format!(
        "{} project tasks · sort: {} · search: {}",
        visible_indices.len(),
        preferences.sort.as_str(),
        if preferences.query.is_empty() {
            "all"
        } else {
            &preferences.query
        }
    ))];
    if let Some(draft) = view.search_draft {
        lines.push(Line::from(format!("Search: {draft}_")));
    }
    if tasks.is_empty() {
        lines.push(Line::from("No task records for this project."));
    } else if visible_indices.is_empty() {
        lines.push(Line::from("No tasks match this search."));
    } else {
        for index in visible_indices.iter().copied().skip(start).take(visible) {
            let task = &tasks[index];
            let marker = if index == selected { "> " } else { "  " };
            let id = bounded_display_text(&task.id, 24);
            let status = bounded_display_text(&task.status, 18);
            let objective = bounded_display_text(&task.objective, 72);
            let style = if index == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("{status:<18}"), style),
                Span::styled(format!("{id:<24}"), style),
                Span::styled(objective, style),
            ]));
        }
        if visible_indices.len() > visible {
            lines.push(Line::from(format!(
                "Showing {}–{} of {} tasks.",
                start + 1,
                start + visible,
                visible_indices.len()
            )));
        }
        if let Some(task) = tasks.get(selected) {
            lines.push(Line::from(format!(
                "Updated: {}",
                bounded_display_text(&task.updated_at, 64)
            )));
        }
    }
    if let Some(message) = refresh_message {
        lines.push(Line::from(bounded_display_text(message, 100)));
    }
    let mut footer = String::from("↑/↓ select   / search   s sort");
    if capabilities.supports(TaskCapabilities::REFRESH) {
        footer.push_str("   r refresh");
    }
    if capabilities.supports(TaskCapabilities::CANCEL) {
        footer.push_str("   c cancel");
    }
    footer.push_str("   Esc or t close");
    lines.push(Line::from(footer));
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Rover task board · read-only snapshot ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

fn render_task_refresh_message(frame: &mut Frame<'_>, popup: Rect, message: &str) {
    let footer_area = Rect::new(
        popup.x.saturating_add(2),
        popup.y.saturating_add(popup.height.saturating_sub(2)),
        popup.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new(bounded_display_text(message, 100))
            .style(Style::default().fg(Color::Yellow)),
        footer_area,
    );
}

fn render_task_detail(
    frame: &mut Frame<'_>,
    popup: Rect,
    task: Option<&TaskSummary>,
    capabilities: TaskCapabilities,
) {
    let Some(task) = task else {
        frame.render_widget(
            Paragraph::new("No task is selected.").block(
                Block::default()
                    .title(" Rover task detail ")
                    .borders(Borders::ALL),
            ),
            popup,
        );
        return;
    };
    let mut lines = vec![
        Line::from(format!("ID: {}", bounded_display_text(&task.id, 80))),
        Line::from(format!(
            "Status: {}",
            bounded_display_text(&task.status, 48)
        )),
        Line::from(format!(
            "Updated: {}",
            bounded_display_text(&task.updated_at, 80)
        )),
        Line::from(format!("Attempts: {}", task.attempt_count)),
        Line::from(format!(
            "Objective: {}",
            bounded_display_text(&task.objective, 240)
        )),
    ];
    if let Some(candidate) = &task.candidate {
        lines.push(Line::from(format!(
            "Candidate: {}",
            bounded_display_text(candidate, 120)
        )));
    }
    if let Some(error) = &task.error {
        lines.push(Line::from(format!(
            "Task error: {}",
            bounded_display_text(error, 240)
        )));
    }
    if let Some(process) = &task.process {
        lines.extend([
            Line::from(format!(
                "Process: exit {} · timed out: {} · cancelled: {} · truncated: {}",
                process.exit_code, process.timed_out, process.cancelled, process.truncated
            )),
            Line::from(format!(
                "Stdout observed: {} bytes · recorded SHA-256 {}",
                process.stdout_bytes,
                bounded_display_text(&process.stdout_sha256, 64)
            )),
            Line::from(format!(
                "Stderr observed: {} bytes · recorded SHA-256 {}",
                process.stderr_bytes,
                bounded_display_text(&process.stderr_sha256, 64)
            )),
        ]);
        if let Some(error) = &process.error {
            lines.push(Line::from(format!(
                "Process error: {}",
                bounded_display_text(error, 240)
            )));
        }
    } else {
        lines.push(Line::from("No process result is recorded."));
    }
    let mut footer = Vec::new();
    if capabilities.supports(TaskCapabilities::LOGS) {
        footer.push("o stdout · e stderr");
    }
    if capabilities.supports(TaskCapabilities::REFRESH) {
        footer.push("r refresh");
    }
    if capabilities.supports(TaskCapabilities::CANCEL) {
        footer.push("c cancel");
    }
    if capabilities.supports(TaskCapabilities::EVIDENCE) {
        footer.push("i evidence");
    }
    if capabilities.supports(TaskCapabilities::DIFF) {
        footer.push("d diff");
    }
    footer.push("Esc or Enter returns");
    lines.push(Line::from(footer.join(" · ")));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" Rover task detail · recorded metadata ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

fn prepare_task_log(bytes: &[u8], expected_digest: &str) -> Result<(String, bool), String> {
    let expected = rover_core::Sha256Digest::parse_hex(expected_digest)
        .map_err(|_| "Recorded output digest is invalid.".to_owned())?;
    if rover_core::Sha256Digest::of(bytes) != expected {
        return Err("Output content does not match its recorded SHA-256.".to_owned());
    }
    let clipped = bytes.len() > MAX_TASK_LOG_BYTES;
    let shown = &bytes[..bytes.len().min(MAX_TASK_LOG_BYTES)];
    let mut sanitized = sanitize_screen_text(&String::from_utf8_lossy(shown));
    if clipped {
        sanitized.push_str("\n\n[Output clipped at 64 KiB for TUI display.]\n");
    }
    Ok((sanitized, true))
}

fn render_task_log(frame: &mut Frame<'_>, stream: &str, text: &str, scroll: u16, verified: bool) {
    let title = if verified {
        format!(" Rover task {stream} · SHA-256 verified ")
    } else {
        format!(" Rover task {stream} · unavailable ")
    };
    let content = if text.is_empty() {
        "(empty output)"
    } else {
        text
    };
    frame.render_widget(
        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(Block::default().title(title).borders(Borders::ALL)),
        centered_rect(86, 82, frame.area()),
    );
}

fn bounded_display_text(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if output.chars().count() >= max_chars {
            output.push('…');
            break;
        }
        if !character.is_control() && !is_bidi_format(character) {
            output.push(character);
        }
    }
    output
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(unix)]
fn bounded_workspace_notes(notes: &[String]) -> Vec<String> {
    let mut total = 0usize;
    notes
        .iter()
        .take(MAX_WORKSPACE_NOTES_COUNT)
        .map(|note| {
            truncate_utf8(
                &sanitize_screen_text(&truncate_utf8(note, MAX_WORKSPACE_NOTES_BYTES)),
                MAX_WORKSPACE_NOTES_BYTES,
            )
        })
        .take_while(|note| {
            let next = total.saturating_add(note.len());
            if next > MAX_WORKSPACE_NOTES_TOTAL_BYTES {
                false
            } else {
                total = next;
                true
            }
        })
        .collect()
}

#[cfg(unix)]
fn saved_command_is_valid(command: &SavedCommand) -> bool {
    !command.name.trim().is_empty()
        && command.name.len() <= MAX_SAVED_COMMAND_NAME_BYTES
        && command.command.len() <= MAX_SAVED_COMMAND_LINE_BYTES
        && !command.command.trim().is_empty()
        && command
            .name
            .chars()
            .chain(command.command.chars())
            .all(|character| !character.is_control() && !is_bidi_format(character))
        && !command.command.contains('\n')
        && !command.command.contains('\r')
}

#[cfg(unix)]
fn bounded_saved_commands(commands: &[SavedCommand]) -> Vec<SavedCommand> {
    let mut bounded = Vec::new();
    for command in commands {
        if bounded.len() == MAX_SAVED_COMMANDS {
            break;
        }
        if saved_command_is_valid(command)
            && !bounded
                .iter()
                .any(|saved: &SavedCommand| saved.name == command.name)
        {
            bounded.push(command.clone());
        }
    }
    bounded
}

#[cfg(unix)]
fn task_draft_is_valid(draft: &TaskDraft) -> bool {
    draft.title.len() <= MAX_TASK_DRAFT_TITLE_BYTES
        && draft.paths.len() <= MAX_TASK_DRAFT_PATHS_BYTES
        && draft.dependencies.len() <= MAX_TASK_DRAFT_DEPENDENCIES_BYTES
        && draft.quality_gate.len() <= MAX_TASK_DRAFT_GATE_BYTES
        && draft.prompt.len() <= MAX_TASK_DRAFT_PROMPT_BYTES
        && draft
            .title
            .chars()
            .all(|character| !character.is_control() && !is_bidi_format(character))
        && [
            draft.paths.as_str(),
            draft.dependencies.as_str(),
            draft.quality_gate.as_str(),
            draft.prompt.as_str(),
        ]
        .into_iter()
        .all(|value| {
            value.chars().all(|character| {
                (character == '\n' || !character.is_control()) && !is_bidi_format(character)
            })
        })
}

#[cfg(unix)]
fn bounded_task_drafts(drafts: &[TaskDraft]) -> Vec<TaskDraft> {
    drafts
        .iter()
        .take(MAX_TASK_DRAFTS)
        .filter(|draft| task_draft_is_valid(draft))
        .cloned()
        .collect()
}

#[cfg(unix)]
fn bounded_task_plan_summaries(plans: &[TaskPlanSummary]) -> Vec<TaskPlanSummary> {
    plans
        .iter()
        .take(1_000)
        .filter(|plan| {
            !plan.id.is_empty()
                && plan.id.len() <= 128
                && plan.title.len() <= MAX_TASK_DRAFT_TITLE_BYTES
                && plan.status.len() <= 24
                && plan.readiness.len() <= 24
                && plan.updated_at.len() <= 64
                && [
                    plan.id.as_str(),
                    plan.title.as_str(),
                    plan.status.as_str(),
                    plan.readiness.as_str(),
                    plan.updated_at.as_str(),
                ]
                .into_iter()
                .all(|value| {
                    value
                        .chars()
                        .all(|character| !character.is_control() && !is_bidi_format(character))
                })
        })
        .cloned()
        .collect()
}

fn render_tabs(frame: &mut Frame<'_>, area: Rect, workspace: &Workspace) {
    let active_id = workspace.active_tab().id();
    let mut spans = vec![
        Span::styled(
            format!(" {} ", workspace.title()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("│ "),
    ];
    spans.extend(workspace.tabs().iter().flat_map(|tab| {
        let style = if tab.id() == active_id {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        [
            Span::styled(format!(" {} ", tab.title()), style),
            Span::raw(" "),
        ]
    }));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_node(
    frame: &mut Frame<'_>,
    node: &PaneNode,
    area: Rect,
    active_pane: &str,
    screens: &[ScreenText],
) {
    match node {
        PaneNode::Leaf(pane) => render_pane(
            frame,
            pane,
            area,
            active_pane,
            screens
                .iter()
                .find(|screen| screen.pane_id == pane.id())
                .map(|screen| screen.text.as_str()),
        ),
        PaneNode::Split(split) => render_split(frame, split, area, active_pane, screens),
    }
}

fn render_split(
    frame: &mut Frame<'_>,
    split: &Split,
    area: Rect,
    active_pane: &str,
    screens: &[ScreenText],
) {
    let direction = match split.axis() {
        SplitAxis::Horizontal => Direction::Horizontal,
        SplitAxis::Vertical => Direction::Vertical,
    };
    let first_ratio = u32::from(split.first_basis_points());
    let children = Layout::default()
        .direction(direction)
        .constraints([
            Constraint::Ratio(first_ratio, 10_000),
            Constraint::Ratio(10_000 - first_ratio, 10_000),
        ])
        .split(area);
    render_node(frame, split.first(), children[0], active_pane, screens);
    render_node(frame, split.second(), children[1], active_pane, screens);
}

fn render_pane(
    frame: &mut Frame<'_>,
    pane: &Pane,
    area: Rect,
    active_pane: &str,
    screen_text: Option<&str>,
) {
    let title = match pane.kind() {
        PaneKind::Session { name } => format!(" Session: {name} "),
        PaneKind::Scratch => " Scratch ".to_owned(),
    };
    let active = pane.id() == active_pane;
    let border = if active { Color::Cyan } else { Color::DarkGray };
    let content = screen_text.map_or_else(
        || {
            if matches!(pane.kind(), PaneKind::Session { .. }) {
                "Waiting for a terminal screen".to_owned()
            } else {
                "Scratch pane".to_owned()
            }
        },
        sanitize_screen_text,
    );
    let visible_lines = usize::from(area.height.saturating_sub(2));
    let lines = content
        .lines()
        .skip(content.lines().count().saturating_sub(visible_lines))
        .map(|line| Line::raw(line.to_owned()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        ),
        area,
    );
}

fn sanitize_screen_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len().min(MAX_SCREEN_TEXT_BYTES));
    for character in text.chars() {
        let replacement =
            if (character.is_control() && character != '\n') || is_bidi_format(character) {
                '\u{fffd}'
            } else {
                character
            };
        if output.len() + replacement.len_utf8() > MAX_SCREEN_TEXT_BYTES {
            break;
        }
        output.push(replacement);
    }
    output
}

fn is_bidi_format(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

fn centered_rect(width_percent: u16, height_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

/// Apply a key press. Returns `true` to leave the interactive loop.
pub fn handle_key(workspace: &mut Workspace, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    if key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return true;
    }

    match key.code {
        KeyCode::Esc => {
            if workspace.active_tab().popup_pane().is_some() {
                workspace.close_popup();
            }
        }
        KeyCode::Tab => focus_next_pane(workspace, true),
        KeyCode::BackTab => focus_next_pane(workspace, false),
        KeyCode::Char('n') => {
            let number = workspace.tabs().len() + 1;
            let _ = workspace.add_tab(
                format!("scratch-{number}"),
                rover_execution::layout::PaneSpec::scratch(),
            );
        }
        KeyCode::Char('[') => focus_neighbor_tab(workspace, false),
        KeyCode::Char(']') => focus_neighbor_tab(workspace, true),
        KeyCode::Char('|') => {
            let _ = workspace.split_active(
                SplitAxis::Horizontal,
                5_000,
                rover_execution::layout::PaneSpec::scratch(),
            );
        }
        KeyCode::Char('-') => {
            let _ = workspace.split_active(
                SplitAxis::Vertical,
                5_000,
                rover_execution::layout::PaneSpec::scratch(),
            );
        }
        KeyCode::Char('x') => {
            let pane = workspace.active_tab().active_pane().to_owned();
            let _ = workspace.close_pane(&pane);
        }
        KeyCode::Char('z') => workspace.toggle_zoom(),
        KeyCode::Char('p') => {
            if workspace.active_tab().popup_pane().is_some() {
                workspace.close_popup();
            } else {
                workspace.open_popup();
            }
        }
        _ => {}
    }
    false
}

fn handle_help_key(help_open: &mut bool, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    if *help_open {
        if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc) {
            *help_open = false;
        }
        return true;
    }
    if key.code == KeyCode::Char('?') {
        *help_open = true;
        return true;
    }
    false
}

#[cfg(unix)]
fn attached_help_key(help_open: &mut bool, prefix: &mut bool, key: KeyEvent) -> bool {
    if *help_open {
        return handle_help_key(help_open, key);
    }
    if *prefix && key.kind != KeyEventKind::Release && key.code == KeyCode::Char('?') {
        *prefix = false;
        *help_open = true;
        return true;
    }
    false
}

fn focus_next_pane(workspace: &mut Workspace, forward: bool) {
    let active = workspace.active_tab();
    let mut ids = Vec::new();
    collect_pane_ids(active.root(), &mut ids);
    if ids.len() < 2 {
        return;
    }
    let current = ids
        .iter()
        .position(|id| id == active.active_pane())
        .unwrap_or(0);
    let next = if forward {
        (current + 1) % ids.len()
    } else {
        (current + ids.len() - 1) % ids.len()
    };
    let _ = workspace.focus_pane(&ids[next]);
}

fn focus_neighbor_tab(workspace: &mut Workspace, forward: bool) {
    let tabs = workspace.tabs();
    if tabs.len() < 2 {
        return;
    }
    let current = tabs
        .iter()
        .position(|tab| tab.id() == workspace.active_tab().id())
        .unwrap_or(0);
    let next = if forward {
        (current + 1) % tabs.len()
    } else {
        (current + tabs.len() - 1) % tabs.len()
    };
    let id = tabs[next].id().to_owned();
    let _ = workspace.focus_tab(&id);
}

fn collect_pane_ids(node: &PaneNode, ids: &mut Vec<String>) {
    match node {
        PaneNode::Leaf(pane) => ids.push(pane.id().to_owned()),
        PaneNode::Split(split) => {
            collect_pane_ids(split.first(), ids);
            collect_pane_ids(split.second(), ids);
        }
    }
}

/// Run Rover's interactive workspace shell and restore terminal state on exit.
///
/// # Errors
///
/// Returns an I/O error if raw mode, terminal setup, event polling, rendering,
/// or input reading fails. Terminal state restoration is attempted on return.
pub fn run(workspace: Workspace) -> io::Result<()> {
    run_with_change_hook(workspace, |_| Ok(()))
}

/// Load a workspace if present and persist every keyboard-driven state change.
///
/// The caller supplies an admitted private state directory and a safe file
/// basename. Only `NotFound` selects the provided initial workspace; malformed
/// or unsafe saved state fails closed.
///
/// # Errors
///
/// Returns an I/O error for unsafe or malformed saved state, terminal errors,
/// or failed atomic persistence.
#[cfg(unix)]
pub fn run_persistent(
    initial_workspace: Workspace,
    directory: &SafeDir,
    name: &str,
) -> io::Result<()> {
    let workspace = load_workspace_or_initial(initial_workspace, directory, name)?;
    workspace.save_to(directory, name)?;
    run_with_change_hook(workspace, |workspace| workspace.save_to(directory, name))
}

#[cfg(unix)]
fn load_workspace_or_initial(
    initial_workspace: Workspace,
    directory: &SafeDir,
    name: &str,
) -> io::Result<Workspace> {
    match Workspace::load_from(directory, name) {
        Ok(workspace) => Ok(workspace),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(initial_workspace),
        Err(error) => Err(error),
    }
}

fn run_with_change_hook(
    mut workspace: Workspace,
    mut on_change: impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut help_open = false;
    loop {
        terminal.draw(|frame| {
            render_workspace_mode(frame, &workspace, &[], false, help_open);
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let quit = if handle_help_key(&mut help_open, key) {
                    false
                } else {
                    handle_key(&mut workspace, key)
                };
                on_change(&workspace)?;
                if quit {
                    return Ok(());
                }
            }
        }
    }
}

/// Attach the active workspace pane to one already connected Rover session.
///
/// Session output is read on a dedicated thread into a bounded queue and is
/// parsed by Rover's terminal emulator before rendering. Ctrl-] detaches;
/// Ctrl-B prefixes workspace commands while all other keys go to the child.
///
/// # Errors
///
/// Returns an I/O error when terminal setup, session I/O, event polling, or
/// terminal parsing fails. The session writer is detached on return.
#[cfg(unix)]
pub fn run_attached_session(
    workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
) -> io::Result<()> {
    run_attached_session_with_hook(
        workspace,
        pane_id,
        client,
        TaskProvider::read_only_snapshot(
            Vec::new(),
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task output is unavailable",
                ))
            },
            false,
        ),
        |_| Ok(()),
    )
}

/// Attach to a named session and persist workspace changes under a safe state directory.
///
/// A saved workspace is loaded when present; malformed state fails closed.
///
/// # Errors
///
/// Returns an I/O error for invalid saved state, terminal/session failures, or
/// failed layout persistence.
#[cfg(unix)]
pub fn run_attached_session_persistent(
    initial_workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
    directory: &SafeDir,
    name: &str,
) -> io::Result<()> {
    run_attached_session_persistent_with_tasks(
        initial_workspace,
        pane_id,
        client,
        directory,
        name,
        &[],
    )
}

/// Attach persistently and show the supplied project task summaries in the read-only task board.
///
/// # Errors
///
/// Returns an I/O error for invalid layout state, terminal/session failures, or
/// failed layout persistence.
#[cfg(unix)]
pub fn run_attached_session_persistent_with_tasks(
    initial_workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
    directory: &SafeDir,
    name: &str,
    tasks: &[TaskSummary],
) -> io::Result<()> {
    run_attached_session_persistent_with_task_provider(
        initial_workspace,
        pane_id,
        client,
        directory,
        name,
        TaskProvider::read_only_snapshot(
            tasks.to_vec(),
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "task output is unavailable",
                ))
            },
            false,
        ),
    )
}

/// Attach persistently and load task output lazily by its recorded content digest.
///
/// The TUI verifies returned bytes against the task's recorded SHA-256 before
/// displaying them. Callers should retrieve content from Rover's bounded blob store.
///
/// # Errors
///
/// Returns an I/O error for invalid layout state, terminal/session failures, or
/// failed layout persistence. Blob read errors are rendered in the output panel.
#[cfg(unix)]
pub fn run_attached_session_persistent_with_task_logs(
    initial_workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
    directory: &SafeDir,
    name: &str,
    tasks: &[TaskSummary],
    load_blob: impl FnMut(&str) -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    run_attached_session_persistent_with_task_provider(
        initial_workspace,
        pane_id,
        client,
        directory,
        name,
        TaskProvider::read_only_snapshot(tasks.to_vec(), load_blob, true),
    )
}

/// Attach persistently with a reloadable project task snapshot and lazy output access.
///
/// Press `r` while the task list or detail panel is open to replace the snapshot.
/// The selected task remains selected when it still exists in the refreshed data.
/// Press `c`, then `y`, to invoke the provider's cancellation callback for the
/// selected task; callers own its durable authorization and state update.
///
/// # Errors
///
/// Returns an I/O error for invalid layout state, terminal/session failures, or
/// failed layout persistence. Task refresh and blob read errors are rendered in the TUI.
#[cfg(unix)]
pub fn run_attached_session_persistent_with_task_provider(
    initial_workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
    directory: &SafeDir,
    name: &str,
    task_provider: TaskProvider<'_>,
) -> io::Result<()> {
    let workspace = load_workspace_or_initial(initial_workspace, directory, name)?;
    workspace.save_to(directory, name)?;
    run_attached_session_with_hook(workspace, pane_id, client, task_provider, |workspace| {
        workspace.save_to(directory, name)
    })
}

#[cfg(unix)]
fn run_attached_session_with_hook(
    mut workspace: Workspace,
    pane_id: &str,
    client: SessionClient,
    task_provider: TaskProvider<'_>,
    mut on_change: impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<()> {
    workspace.focus_pane(pane_id)?;
    let pane = find_pane_ref(workspace.active_tab().root(), pane_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "workspace pane not found"))?;
    if !matches!(pane.kind(), PaneKind::Session { .. }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session can only attach to a session pane",
        ));
    }

    let (mut reader, mut writer) = client.into_parts();
    let (sender, receiver) = mpsc::sync_channel(64);
    let reader_thread = thread::spawn(move || read_session_frames(&mut reader, sender));
    let result = run_attached_loop(
        &mut workspace,
        pane_id,
        &mut writer,
        &receiver,
        task_provider,
        &mut on_change,
    );
    drop(receiver);
    let _ = writer.detach();
    let _ = reader_thread.join();
    result
}

#[cfg(unix)]
fn read_session_frames(reader: &mut SessionReader, sender: SyncSender<io::Result<SessionFrame>>) {
    loop {
        let frame = reader.read_frame();
        let terminal = frame.is_err()
            || frame
                .as_ref()
                .is_ok_and(|frame| matches!(frame.kind.as_str(), "exit" | "error"));
        if sender.send(frame).is_err() || terminal {
            break;
        }
    }
    drop(sender);
}

#[cfg(unix)]
fn run_attached_loop(
    workspace: &mut Workspace,
    pane_id: &str,
    writer: &mut SessionWriter,
    receiver: &Receiver<io::Result<SessionFrame>>,
    mut task_provider: TaskProvider<'_>,
    on_change: &mut impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let terminal_size = terminal.size()?;
    let area = Rect::new(0, 0, terminal_size.width, terminal_size.height);
    let initial_size = pane_terminal_size(workspace, area, pane_id)?;
    let mut screen = TerminalScreen::new(initial_size, 1_000)?;
    send_resize(writer, initial_size)?;
    let mut input_state = AttachedInputState::default();
    let mut last_agent_sample_request = Instant::now();

    loop {
        for message in receiver.try_iter() {
            let frame = message?;
            match frame.kind.as_str() {
                "output" => screen.process_output(&frame.data)?,
                "foreground_sample" => {
                    apply_agent_detection(
                        &mut input_state.agent_detection,
                        detect_foreground_group_sample(
                            &frame.data,
                            &frame.processes,
                            &frame.process_details,
                            &screen,
                        ),
                    );
                }
                "exit" => return Ok(()),
                "error" => {
                    return Err(io::Error::other(
                        frame
                            .error
                            .unwrap_or_else(|| "session server error".to_owned()),
                    ));
                }
                _ => {}
            }
        }

        if last_agent_sample_request.elapsed() >= Duration::from_secs(1) {
            writer.request_foreground_sample()?;
            last_agent_sample_request = Instant::now();
        }

        if screen.take_bell() && pane_is_inactive(workspace, pane_id) {
            let mut stdout = io::stdout();
            stdout.write_all(b"\x07")?;
            stdout.flush()?;
        }

        let pane = find_pane_ref(workspace.active_tab().root(), pane_id);
        if pane.is_none() {
            return Ok(());
        }
        let terminal_size = terminal.size()?;
        let area = Rect::new(0, 0, terminal_size.width, terminal_size.height);
        if let Ok(size) = pane_terminal_size(workspace, area, pane_id) {
            if screen.size() != size {
                screen.resize(size)?;
                send_resize(writer, size)?;
            }
        }
        let screen_text = pane.map(|pane| ScreenText::from_terminal_screen(pane, &screen));
        draw_attached_frame(
            &mut terminal,
            workspace,
            screen_text.as_ref(),
            &input_state,
            &task_provider,
        )?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Resize(columns, rows) => {
                    let area = Rect::new(0, 0, columns, rows);
                    if let Ok(size) = pane_terminal_size(workspace, area, pane_id) {
                        if screen.size() != size {
                            screen.resize(size)?;
                            send_resize(writer, size)?;
                        }
                    }
                }
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    if handle_attached_key(
                        workspace,
                        writer,
                        &mut task_provider,
                        &mut input_state,
                        key,
                        on_change,
                    )? {
                        return Ok(());
                    }
                    if input_state.terminal_handoff == TerminalHandoffState::NeedsRedraw {
                        terminal.clear()?;
                        input_state.terminal_handoff = TerminalHandoffState::Idle;
                    }
                    refresh_task_snapshot_if_requested(&mut input_state, &mut task_provider);
                }
                _ => {}
            }
        }
    }
}

#[cfg(unix)]
fn detect_foreground_sample(data: &[u8], screen: &TerminalScreen) -> DetectionResult {
    let foreground = std::str::from_utf8(data)
        .ok()
        .filter(|name| !name.is_empty())
        .map(|name| ProcessSample {
            executable: name.to_owned(),
        });
    if let Some(process) = foreground.as_ref() {
        if let Ok(Some(manifest)) = bundled_manifest_for_executable(&process.executable) {
            let mut result = evaluate_herdr_manifest(
                manifest,
                &screen.visible_text(),
                &screen.osc_title().unwrap_or_default(),
                screen.osc_progress().unwrap_or_default(),
            );
            let value = process
                .executable
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or_default();
            result.evidence.insert(
                0,
                DetectionEvidence {
                    source: "foreground_process",
                    manifest_source: None,
                    manifest_version: None,
                    rule_id: None,
                    value: value.to_owned(),
                },
            );
            return result;
        }
    }
    detect_agent(&DetectionRequest {
        foreground,
        screen: Some(screen.visible_text()),
        ..DetectionRequest::default()
    })
}

#[cfg(unix)]
fn detect_foreground_group_sample(
    leader: &[u8],
    processes: &[String],
    details: &[rover_execution::pty::ForegroundProcessDetails],
    screen: &TerminalScreen,
) -> DetectionResult {
    let leader_name = std::str::from_utf8(leader)
        .ok()
        .filter(|name| !name.is_empty());
    let mut sampled = details
        .iter()
        .map(|process| ProcessArgvSample {
            executable: process.name.clone(),
            argv: process.argv.clone(),
        })
        .collect::<Vec<_>>();
    if sampled.is_empty() {
        sampled.extend(
            processes
                .iter()
                .cloned()
                .map(|executable| ProcessArgvSample {
                    executable,
                    argv: None,
                }),
        );
    }
    let leader_sample = leader_name.map(|executable| {
        sampled
            .iter()
            .find(|process| process.executable == executable)
            .cloned()
            .unwrap_or_else(|| ProcessArgvSample {
                executable: executable.to_owned(),
                argv: None,
            })
    });
    if let Some(identity) = identify_foreground_process_group(leader_sample.as_ref(), &sampled) {
        if identity.ambiguous {
            // One evidence entry per equally ranked candidate, so the operator
            // can see which agents disagreed rather than only that they did.
            let evidence = if identity.conflicting.is_empty() {
                vec![DetectionEvidence {
                    source: "foreground_process",
                    manifest_source: None,
                    manifest_version: None,
                    rule_id: None,
                    value: "conflicting equally ranked process identities".to_owned(),
                }]
            } else {
                identity
                    .conflicting
                    .iter()
                    .map(|candidate| DetectionEvidence {
                        source: "foreground_process",
                        manifest_source: None,
                        manifest_version: None,
                        rule_id: None,
                        value: candidate.clone(),
                    })
                    .collect()
            };
            return DetectionResult {
                agent: None,
                state: AgentState::Unknown,
                confidence: Confidence::None,
                reason: "ambiguous_foreground_process_group",
                evidence,
                visibility: DetectionVisibility::default(),
                skip_state_update: false,
            };
        }
        if bundled_manifest_for_executable(&identity.executable)
            .is_ok_and(|manifest| manifest.is_some())
        {
            return detect_foreground_sample(identity.executable.as_bytes(), screen);
        }
        return DetectionResult {
            agent: Some(identity.executable.clone()),
            state: AgentState::Unknown,
            confidence: Confidence::None,
            reason: "agent_manifest_unavailable",
            evidence: vec![DetectionEvidence {
                source: "foreground_process",
                manifest_source: None,
                manifest_version: None,
                rule_id: None,
                value: identity.executable,
            }],
            visibility: DetectionVisibility::default(),
            skip_state_update: false,
        };
    }
    detect_foreground_sample(leader, screen)
}

#[cfg(unix)]
fn apply_agent_detection(current: &mut Option<DetectionResult>, next: DetectionResult) {
    let Some(previous) = current.as_ref() else {
        *current = Some(next);
        return;
    };
    if !next.skip_state_update || previous.agent != next.agent {
        *current = Some(next);
        return;
    }

    let mut preserved = previous.clone();
    preserved.skip_state_update = true;
    preserved.reason = "screen_state_preserved_for_skip_update";
    for evidence in next.evidence {
        if !preserved.evidence.iter().any(|previous| {
            previous.source == evidence.source
                && previous.rule_id == evidence.rule_id
                && previous.value == evidence.value
        }) {
            preserved.evidence.push(evidence);
        }
    }
    *current = Some(preserved);
}

#[cfg(unix)]
fn pane_is_inactive(workspace: &Workspace, pane_id: &str) -> bool {
    workspace.active_tab().active_pane() != pane_id
}

#[cfg(unix)]
fn draw_attached_frame(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    workspace: &Workspace,
    screen_text: Option<&ScreenText>,
    input_state: &AttachedInputState,
    task_provider: &TaskProvider<'_>,
) -> io::Result<()> {
    terminal.draw(|frame| {
        let screens = screen_text.map_or(&[] as &[ScreenText], std::slice::from_ref);
        render_workspace_mode_for_provider(
            frame,
            workspace,
            screens,
            true,
            input_state.help_open,
            Some(task_provider.capabilities),
            input_state.agent_detection.as_ref(),
        );
        match input_state.task_panel {
            TaskPanelState::Closed => {}
            TaskPanelState::List | TaskPanelState::Detail => render_task_board_with_refresh(
                frame,
                &task_provider.tasks,
                input_state.selected_task,
                input_state.task_panel == TaskPanelState::Detail,
                input_state.task_refresh_message.as_deref(),
                task_provider.capabilities,
                TaskBoardView {
                    preferences: &task_provider.view_preferences,
                    search_draft: input_state.task_search_draft.as_deref(),
                },
            ),
            TaskPanelState::Stdout => render_task_log(
                frame,
                "stdout",
                &input_state.task_log,
                input_state.task_log_scroll,
                input_state.task_log_verification == TaskLogVerification::Verified,
            ),
            TaskPanelState::Stderr => render_task_log(
                frame,
                "stderr",
                &input_state.task_log,
                input_state.task_log_scroll,
                input_state.task_log_verification == TaskLogVerification::Verified,
            ),
            TaskPanelState::Evidence => render_task_evidence(
                frame,
                input_state.task_evidence.as_ref(),
                input_state.task_evidence_message.as_deref(),
                input_state.task_evidence_scroll,
            ),
            TaskPanelState::Diff => render_task_diff(
                frame,
                input_state.task_diff.as_ref(),
                input_state.task_diff_message.as_deref(),
                input_state.task_diff_scroll,
            ),
        }
        match input_state.file_panel {
            FilePanelState::Closed => {}
            FilePanelState::Tree => render_file_tree(frame, input_state),
            FilePanelState::QuickOpen => render_quick_open(frame, input_state),
            FilePanelState::Viewer => render_file_preview(frame, input_state),
            FilePanelState::Editing => render_file_editor(frame, input_state),
            FilePanelState::DiffPreview => render_file_diff(frame, input_state),
        }
        if let Some(task_id) = input_state.pending_cancel.as_deref() {
            render_task_cancel_confirmation(frame, task_id);
        }
        if let Some(query) = input_state.command_palette_query.as_deref() {
            render_command_palette(
                frame,
                query,
                input_state.command_palette_selected,
                task_provider.capabilities,
            );
        }
        if let Some(draft) = input_state.workspace_notes_draft.as_deref() {
            render_workspace_notes(
                frame,
                draft,
                input_state.workspace_notes_cursor,
                input_state.workspace_notes_state == WorkspaceNotesState::ConfirmDiscard,
                input_state.workspace_notes_message.as_deref(),
            );
        } else if input_state.workspace_notes_panel == WorkspaceNotesPanel::List {
            render_workspace_notes_list(
                frame,
                &task_provider.workspace_notes,
                input_state.selected_workspace_note,
                input_state.workspace_notes_state == WorkspaceNotesState::ConfirmDelete,
                input_state.workspace_notes_message.as_deref(),
            );
        }
        render_saved_commands_overlay(frame, input_state, task_provider);
        render_task_drafts_overlay(frame, input_state, task_provider);
        render_agent_session_panel(
            frame,
            task_provider.agent_session.as_ref(),
            input_state.agent_session_panel,
            input_state.agent_session_message.as_deref(),
        );
    })?;
    Ok(())
}

#[cfg(unix)]
fn render_agent_session_panel(
    frame: &mut Frame<'_>,
    session: Option<&AgentSessionSummary>,
    panel: AgentSessionPanel,
    message: Option<&str>,
) {
    if panel == AgentSessionPanel::Closed {
        return;
    }
    let area = frame.area();
    if area.width < 36 || area.height < 10 {
        return;
    }
    let popup = centered_rect(76, 58, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    let title = match panel {
        AgentSessionPanel::Closed => return,
        AgentSessionPanel::Details => " Saved agent session ",
        AgentSessionPanel::ConfirmResume => " Confirm agent resume ",
        AgentSessionPanel::ConfirmClear => " Confirm binding removal ",
    };
    if let Some(session) = session {
        lines.extend([
            Line::from(format!("Pane: {}", session.pane_id)),
            Line::from(format!("Agent: {}", session.adapter)),
            Line::from(format!("Session ID: {}", session.session_id)),
            Line::from(format!("Identity source: {}", session.source)),
        ]);
        match panel {
            AgentSessionPanel::Details => {
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "r resume interactively · d forget saved binding · Esc close",
                ));
            }
            AgentSessionPanel::ConfirmResume => {
                let command = agent_resume_display(&session.argv);
                lines.push(Line::from(""));
                lines.push(Line::from(format!("Command: {command}")));
                lines.push(Line::from("This runs in the attached project shell using the agent's configured permissions."));
                lines.push(Line::from(
                    "Rover does not mediate the agent's permission or sandbox settings.",
                ));
                lines.push(Line::from(
                    "Press y to send this command · n or Esc to cancel",
                ));
            }
            AgentSessionPanel::ConfirmClear => {
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "Remove only Rover's saved association? A running agent is unaffected.",
                ));
                lines.push(Line::from(
                    "Press y to remove the binding · n or Esc to cancel",
                ));
            }
            AgentSessionPanel::Closed => unreachable!(),
        }
    } else {
        lines.push(Line::from("The saved session is no longer available."));
        lines.push(Line::from("Press any key to close this panel."));
    }
    if let Some(message) = message {
        lines.push(Line::from(""));
        lines.push(Line::from(bounded_display_text(message, 512)));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().title(title).borders(Borders::ALL)),
        popup,
    );
}

#[cfg(unix)]
fn agent_resume_display(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| format!("'{}'", argument.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(unix)]
fn agent_resume_shell_line(argv: &[String]) -> io::Result<Vec<u8>> {
    if argv.is_empty()
        || argv.iter().any(|argument| {
            argument.is_empty()
                || argument
                    .chars()
                    .any(|character| character.is_control() || character == '\0')
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "agent resume arguments are empty or contain control characters",
        ));
    }
    let mut line = agent_resume_display(argv).into_bytes();
    line.push(b'\n');
    Ok(line)
}

#[cfg(unix)]
fn handle_agent_session_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
    mut send: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }
    if provider.agent_session.is_none() {
        state.agent_session_panel = AgentSessionPanel::Closed;
        return Ok(());
    }
    match state.agent_session_panel {
        AgentSessionPanel::Details => match key.code {
            KeyCode::Esc => state.agent_session_panel = AgentSessionPanel::Closed,
            KeyCode::Char('r') => state.agent_session_panel = AgentSessionPanel::ConfirmResume,
            KeyCode::Char('d') => state.agent_session_panel = AgentSessionPanel::ConfirmClear,
            _ => {}
        },
        AgentSessionPanel::ConfirmResume => match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                state.agent_session_panel = AgentSessionPanel::Details;
            }
            KeyCode::Char('y') => {
                let command = agent_resume_shell_line(
                    &provider.agent_session.as_ref().expect("checked above").argv,
                )?;
                send(&command)?;
                state.agent_session_panel = AgentSessionPanel::Closed;
                state.agent_session_message = None;
            }
            _ => {}
        },
        AgentSessionPanel::ConfirmClear => match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                state.agent_session_panel = AgentSessionPanel::Details;
            }
            KeyCode::Char('y') => {
                let pane_id = provider
                    .agent_session
                    .as_ref()
                    .expect("checked above")
                    .pane_id
                    .clone();
                match (provider.clear_agent_session)(&pane_id) {
                    Ok(()) => {
                        provider.agent_session = None;
                        provider.capabilities.0 &= !TaskCapabilities::AGENT_SESSION;
                        state.agent_session_panel = AgentSessionPanel::Closed;
                        state.agent_session_message = None;
                    }
                    Err(error) => {
                        state.agent_session_message =
                            Some(format!("Could not clear binding: {error}"));
                        state.agent_session_panel = AgentSessionPanel::Details;
                    }
                }
            }
            _ => {}
        },
        AgentSessionPanel::Closed => {}
    }
    Ok(())
}

#[cfg(unix)]
fn handle_agent_session_input(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
    send: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<bool> {
    if state.agent_session_panel == AgentSessionPanel::Closed {
        return Ok(false);
    }
    handle_agent_session_key(state, provider, key, send)?;
    Ok(true)
}

#[cfg(unix)]
#[derive(Default)]
struct AttachedInputState {
    prefix: bool,
    help_open: bool,
    task_panel: TaskPanelState,
    selected_task: usize,
    task_log: String,
    task_log_scroll: u16,
    task_log_verification: TaskLogVerification,
    refresh_request: RefreshRequest,
    task_refresh_message: Option<String>,
    pending_cancel: Option<String>,
    task_evidence: Option<TaskInvestigationEvidence>,
    task_evidence_message: Option<String>,
    task_evidence_scroll: u16,
    task_diff: Option<TaskDiffEvidence>,
    task_diff_message: Option<String>,
    task_diff_scroll: u16,
    task_search_draft: Option<String>,
    file_panel: FilePanelState,
    file_directory: String,
    file_listing: Option<BrowserListing>,
    file_selected: usize,
    file_include_hidden: bool,
    quick_open_entries: Vec<BrowserEntry>,
    quick_open_query: Option<String>,
    quick_open_selected: usize,
    quick_open_return: FilePanelState,
    file_preview_return: FilePanelState,
    file_preview: Option<FilePreviewState>,
    file_preview_scroll: u16,
    file_edit_capability: FileEditCapability,
    file_edit: Option<FileEditState>,
    file_diff_scroll: u16,
    terminal_handoff: TerminalHandoffState,
    command_palette_query: Option<String>,
    command_palette_selected: usize,
    workspace_notes_draft: Option<String>,
    workspace_notes_cursor: usize,
    workspace_notes_state: WorkspaceNotesState,
    workspace_notes_panel: WorkspaceNotesPanel,
    selected_workspace_note: usize,
    workspace_notes_editing_index: Option<usize>,
    workspace_notes_message: Option<String>,
    saved_commands_panel: SavedCommandsPanel,
    selected_saved_command: usize,
    saved_command_draft: Option<SavedCommandDraft>,
    saved_command_dialog: SavedCommandDialog,
    saved_command_message: Option<String>,
    pending_saved_command: Option<usize>,
    task_drafts_panel: SavedCommandsPanel,
    selected_task_draft: usize,
    task_draft_panel: TaskDraftPanel,
    selected_task_plan: usize,
    task_draft: Option<TaskDraftEdit>,
    task_draft_dialog: SavedCommandDialog,
    task_draft_message: Option<String>,
    file_message: Option<String>,
    agent_session_panel: AgentSessionPanel,
    agent_session_message: Option<String>,
    agent_detection: Option<DetectionResult>,
}

fn agent_state_label(state: AgentState) -> &'static str {
    match state {
        AgentState::Unknown => "unknown",
        AgentState::Idle => "idle",
        AgentState::Working => "working",
        AgentState::Blocked => "blocked",
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FilePanelState {
    #[default]
    Closed,
    Tree,
    QuickOpen,
    Viewer,
    Editing,
    DiffPreview,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FileEditCapability {
    #[default]
    Disabled,
    Enabled,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TerminalHandoffState {
    #[default]
    Idle,
    NeedsRedraw,
}

#[cfg(unix)]
struct FilePreviewState {
    path: String,
    text: String,
    truncated: bool,
    binary: bool,
}

#[cfg(unix)]
struct FileEditState {
    path: String,
    original: String,
    buffer: String,
    original_sha256: String,
    cursor: usize,
    discard_confirmation: bool,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskPanelState {
    #[default]
    Closed,
    List,
    Detail,
    Stdout,
    Stderr,
    Evidence,
    Diff,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WorkspaceNotesState {
    #[default]
    Editing,
    ConfirmDiscard,
    ConfirmDelete,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WorkspaceNotesPanel {
    #[default]
    Closed,
    List,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskDraftPanel {
    #[default]
    Drafts,
    Plans,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SavedCommandsPanel {
    #[default]
    Closed,
    List,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SavedCommandDialog {
    #[default]
    Editing,
    ConfirmDiscard,
    ConfirmDelete,
    ConfirmCreatePlan,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SavedCommandField {
    #[default]
    Name,
    Command,
}

#[cfg(unix)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SavedCommandDraft {
    editing_index: Option<usize>,
    name: String,
    command: String,
    name_cursor: usize,
    command_cursor: usize,
    active_field: SavedCommandField,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskDraftField {
    #[default]
    Title,
    Paths,
    Dependencies,
    QualityGate,
    Prompt,
}

#[cfg(unix)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TaskDraftEdit {
    editing_index: Option<usize>,
    title: String,
    paths: String,
    dependencies: String,
    quality_gate: String,
    prompt: String,
    title_cursor: usize,
    paths_cursor: usize,
    dependencies_cursor: usize,
    quality_gate_cursor: usize,
    prompt_cursor: usize,
    active_field: TaskDraftField,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaletteAction {
    OpenTasks,
    OpenFiles,
    QuickOpen,
    OpenNotes,
    OpenSavedCommands,
    OpenTaskDrafts,
    OpenAgentSession,
    RefreshTasks,
    KeyboardHelp,
    NextTab,
    PreviousTab,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskLogVerification {
    Verified,
    #[default]
    Unavailable,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RefreshRequest {
    #[default]
    Idle,
    Requested,
}

#[cfg(unix)]
fn handle_attached_key(
    workspace: &mut Workspace,
    writer: &mut SessionWriter,
    task_provider: &mut TaskProvider<'_>,
    state: &mut AttachedInputState,
    key: KeyEvent,
    on_change: &mut impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<bool> {
    if is_detach_key(key) {
        return Ok(true);
    }
    if state.pending_saved_command.is_some() {
        handle_saved_command_confirmation(state, task_provider, key, |bytes| {
            writer.send_input(bytes)
        });
        return Ok(false);
    }
    if handle_agent_session_input(state, task_provider, key, |bytes| writer.send_input(bytes))? {
        return Ok(false);
    }
    if state.saved_commands_panel == SavedCommandsPanel::List || state.saved_command_draft.is_some()
    {
        handle_saved_commands_key(state, task_provider, key);
        return Ok(false);
    }
    if state.task_drafts_panel == SavedCommandsPanel::List || state.task_draft.is_some() {
        handle_task_drafts_key(state, task_provider, key);
        return Ok(false);
    }
    if state.file_panel != FilePanelState::Closed {
        handle_file_panel_key(state, task_provider, key);
        return Ok(false);
    }
    if state.workspace_notes_panel == WorkspaceNotesPanel::List {
        handle_workspace_notes_key(state, task_provider, key);
        return Ok(false);
    }
    if state.command_palette_query.is_some() {
        handle_command_palette_key(workspace, state, task_provider, key, on_change)?;
        return Ok(false);
    }
    if state.task_search_draft.is_some() {
        handle_task_search_edit(state, task_provider, key);
        return Ok(false);
    }
    if state.pending_cancel.is_some() {
        handle_task_cancel_confirmation(state, task_provider, key);
        return Ok(false);
    }
    if state.task_panel == TaskPanelState::Evidence && attached_task_evidence_key(state, key) {
        return Ok(false);
    }
    if state.task_panel == TaskPanelState::Diff && attached_task_diff_key(state, key) {
        return Ok(false);
    }
    if handle_task_view_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if handle_task_panel_action(state, task_provider, key) {
        return Ok(false);
    }
    if attached_help_key(&mut state.help_open, &mut state.prefix, key) {
        on_change(workspace)?;
        return Ok(false);
    }
    if open_workspace_notes_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if open_saved_commands_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if open_task_drafts_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if open_agent_session_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if open_command_palette_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if open_file_browser_shortcut(state, task_provider, key) {
        return Ok(false);
    }
    if attached_task_board_key(
        &mut state.task_panel,
        &mut state.prefix,
        &mut state.selected_task,
        &task_provider.tasks,
        &task_provider.view_preferences,
        key,
    ) {
        return Ok(false);
    }
    if state.prefix {
        state.prefix = false;
        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            writer.send_input(&[0x02])?;
        } else {
            let should_detach = attached_workspace_key(workspace, key);
            on_change(workspace)?;
            return Ok(should_detach);
        }
    } else if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.prefix = true;
    } else if let Some(bytes) = key_input_bytes(key) {
        writer.send_input(&bytes)?;
    }
    Ok(false)
}

#[cfg(unix)]
fn handle_task_panel_action(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if provider.capabilities.supports(TaskCapabilities::REFRESH)
        && attached_task_refresh_key(state.task_panel, key)
    {
        state.refresh_request = RefreshRequest::Requested;
        return true;
    }
    if provider.capabilities.supports(TaskCapabilities::CANCEL)
        && attached_task_cancel_key(state.task_panel, key)
    {
        state.pending_cancel = provider
            .tasks
            .get(state.selected_task)
            .map(|task| task.id.clone());
        if state.pending_cancel.is_none() {
            state.task_refresh_message = Some("No task is selected to cancel.".to_owned());
        }
        return true;
    }
    if matches!(
        state.task_panel,
        TaskPanelState::Stdout | TaskPanelState::Stderr
    ) && attached_task_log_key(&mut state.task_panel, &mut state.task_log_scroll, key)
    {
        return true;
    }
    if provider.capabilities.supports(TaskCapabilities::LOGS)
        && state.task_panel == TaskPanelState::Detail
        && open_selected_task_log(state, provider, key)
    {
        return true;
    }
    if provider.capabilities.supports(TaskCapabilities::EVIDENCE)
        && state.task_panel == TaskPanelState::Detail
        && open_selected_task_evidence(state, provider, key)
    {
        return true;
    }
    provider.capabilities.supports(TaskCapabilities::DIFF)
        && state.task_panel == TaskPanelState::Detail
        && open_selected_task_diff(state, provider, key)
}

#[cfg(unix)]
fn handle_task_view_shortcut(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if state.task_panel != TaskPanelState::List || key.kind != KeyEventKind::Press {
        return false;
    }
    match key.code {
        KeyCode::Char('/') => {
            state.task_search_draft = Some(provider.view_preferences.query.clone());
            true
        }
        KeyCode::Char('s') => {
            provider.view_preferences.sort = provider.view_preferences.sort.next();
            save_task_view_preferences(state, provider);
            true
        }
        _ => false,
    }
}

#[cfg(unix)]
fn open_command_palette_shortcut(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix || key.kind != KeyEventKind::Press || key.code != KeyCode::Char('p') {
        return false;
    }
    state.prefix = false;
    state.command_palette_query = Some(String::new());
    state.command_palette_selected = 0;
    let commands = palette_actions(provider.capabilities, "");
    if commands.is_empty() {
        state.command_palette_query = None;
    }
    true
}

#[cfg(unix)]
fn open_workspace_notes_shortcut(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix
        || key.kind != KeyEventKind::Press
        || key.code != KeyCode::Char('n')
        || !provider.capabilities.supports(TaskCapabilities::NOTES)
    {
        return false;
    }
    state.prefix = false;
    state.workspace_notes_panel = WorkspaceNotesPanel::List;
    state.selected_workspace_note = 0;
    state.workspace_notes_draft = None;
    state.workspace_notes_editing_index = None;
    state.workspace_notes_state = WorkspaceNotesState::Editing;
    state.workspace_notes_message = None;
    true
}

#[cfg(unix)]
fn handle_workspace_notes_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    if state.workspace_notes_draft.is_none() {
        handle_workspace_notes_list_key(state, provider, key);
        return;
    }
    if state.workspace_notes_state == WorkspaceNotesState::ConfirmDiscard {
        handle_workspace_notes_discard_confirmation(state, key);
        return;
    }
    handle_workspace_note_edit_key(state, provider, key);
}

#[cfg(unix)]
fn handle_workspace_notes_list_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if state.workspace_notes_state == WorkspaceNotesState::ConfirmDelete {
        match key.code {
            KeyCode::Char('y') => {
                if state.selected_workspace_note < provider.workspace_notes.len() {
                    let mut updated = provider.workspace_notes.clone();
                    updated.remove(state.selected_workspace_note);
                    match (provider.save_workspace_notes)(&updated) {
                        Ok(()) => provider.workspace_notes = updated,
                        Err(error) => {
                            state.workspace_notes_message =
                                Some(format!("Could not delete note: {}", error.kind()));
                        }
                    }
                    state.selected_workspace_note = state
                        .selected_workspace_note
                        .min(provider.workspace_notes.len().saturating_sub(1));
                }
                state.workspace_notes_state = WorkspaceNotesState::Editing;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.workspace_notes_state = WorkspaceNotesState::Editing;
            }
            _ => {}
        }
        return;
    }
    state.workspace_notes_message = None;
    match key.code {
        KeyCode::Esc => {
            state.workspace_notes_panel = WorkspaceNotesPanel::Closed;
            state.workspace_notes_message = None;
        }
        KeyCode::Up if !provider.workspace_notes.is_empty() => {
            state.selected_workspace_note = if state.selected_workspace_note == 0 {
                provider.workspace_notes.len() - 1
            } else {
                state.selected_workspace_note - 1
            };
        }
        KeyCode::Down if !provider.workspace_notes.is_empty() => {
            state.selected_workspace_note =
                (state.selected_workspace_note + 1) % provider.workspace_notes.len();
        }
        KeyCode::Char('n') if provider.workspace_notes.len() < MAX_WORKSPACE_NOTES_COUNT => {
            state.workspace_notes_editing_index = None;
            state.workspace_notes_draft = Some(String::new());
            state.workspace_notes_cursor = 0;
        }
        KeyCode::Enter if state.selected_workspace_note < provider.workspace_notes.len() => {
            let note = provider.workspace_notes[state.selected_workspace_note].clone();
            state.workspace_notes_editing_index = Some(state.selected_workspace_note);
            state.workspace_notes_cursor = note.len();
            state.workspace_notes_draft = Some(note);
        }
        KeyCode::Char('d') if state.selected_workspace_note < provider.workspace_notes.len() => {
            state.workspace_notes_state = WorkspaceNotesState::ConfirmDelete;
        }
        KeyCode::Char('n') => {
            state.workspace_notes_message = Some(format!(
                "At most {MAX_WORKSPACE_NOTES_COUNT} notes can be stored."
            ));
        }
        _ => {}
    }
}

#[cfg(unix)]
fn handle_workspace_note_edit_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    let Some(mut draft) = state.workspace_notes_draft.take() else {
        return;
    };
    state.workspace_notes_message = None;
    match key.code {
        KeyCode::Esc => {
            let original = state
                .workspace_notes_editing_index
                .and_then(|index| provider.workspace_notes.get(index));
            if state.workspace_notes_editing_index.is_some() && original == Some(&draft) {
                state.workspace_notes_editing_index = None;
                state.workspace_notes_state = WorkspaceNotesState::Editing;
                return;
            }
            state.workspace_notes_state = WorkspaceNotesState::ConfirmDiscard;
            state.workspace_notes_draft = Some(draft);
            return;
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            save_workspace_note(state, provider, &draft);
            return;
        }
        KeyCode::Left => {
            while state.workspace_notes_cursor > 0 {
                state.workspace_notes_cursor -= 1;
                if draft.is_char_boundary(state.workspace_notes_cursor) {
                    break;
                }
            }
        }
        KeyCode::Right => {
            if state.workspace_notes_cursor < draft.len() {
                state.workspace_notes_cursor += 1;
                while !draft.is_char_boundary(state.workspace_notes_cursor) {
                    state.workspace_notes_cursor += 1;
                }
            }
        }
        KeyCode::Home => {
            state.workspace_notes_cursor = draft[..state.workspace_notes_cursor]
                .rfind('\n')
                .map_or(0, |index| index + 1);
        }
        KeyCode::End => {
            state.workspace_notes_cursor += draft[state.workspace_notes_cursor..]
                .find('\n')
                .unwrap_or(draft.len() - state.workspace_notes_cursor);
        }
        KeyCode::Backspace if state.workspace_notes_cursor > 0 => {
            let mut start = state.workspace_notes_cursor - 1;
            while !draft.is_char_boundary(start) {
                start -= 1;
            }
            draft.drain(start..state.workspace_notes_cursor);
            state.workspace_notes_cursor = start;
        }
        KeyCode::Delete if state.workspace_notes_cursor < draft.len() => {
            let mut end = state.workspace_notes_cursor + 1;
            while !draft.is_char_boundary(end) {
                end += 1;
            }
            draft.drain(state.workspace_notes_cursor..end);
        }
        KeyCode::Enter if draft.len() < MAX_WORKSPACE_NOTES_BYTES => {
            draft.insert(state.workspace_notes_cursor, '\n');
            state.workspace_notes_cursor += 1;
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && draft.len().saturating_add(character.len_utf8())
                    <= MAX_WORKSPACE_NOTES_BYTES =>
        {
            draft.insert(state.workspace_notes_cursor, character);
            state.workspace_notes_cursor += character.len_utf8();
        }
        _ => {}
    }
    if state.workspace_notes_draft.is_none() {
        state.workspace_notes_draft = Some(draft);
    }
}

#[cfg(unix)]
fn save_workspace_note(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    draft: &str,
) {
    let mut updated = provider.workspace_notes.clone();
    let selected = if let Some(index) = state.workspace_notes_editing_index {
        if index >= updated.len() {
            state.workspace_notes_message = Some("Selected note no longer exists.".to_owned());
            state.workspace_notes_draft = Some(draft.to_owned());
            return;
        }
        draft.clone_into(&mut updated[index]);
        index
    } else {
        if updated.len() >= MAX_WORKSPACE_NOTES_COUNT {
            state.workspace_notes_message = Some(format!(
                "At most {MAX_WORKSPACE_NOTES_COUNT} notes can be stored."
            ));
            state.workspace_notes_draft = Some(draft.to_owned());
            return;
        }
        updated.push(draft.to_owned());
        updated.len() - 1
    };
    let total_bytes = updated.iter().map(String::len).sum::<usize>();
    if total_bytes > MAX_WORKSPACE_NOTES_TOTAL_BYTES {
        state.workspace_notes_message = Some(format!(
            "Notes exceed the {MAX_WORKSPACE_NOTES_TOTAL_BYTES}-byte collection limit."
        ));
        state.workspace_notes_draft = Some(draft.to_owned());
        return;
    }
    match (provider.save_workspace_notes)(&updated) {
        Ok(()) => {
            provider.workspace_notes = updated;
            state.selected_workspace_note = selected;
            state.workspace_notes_editing_index = None;
            state.workspace_notes_state = WorkspaceNotesState::Editing;
            state.workspace_notes_draft = None;
        }
        Err(error) => {
            state.workspace_notes_message = Some(format!("Could not save notes: {}", error.kind()));
            state.workspace_notes_draft = Some(draft.to_owned());
        }
    }
}

#[cfg(unix)]
fn handle_workspace_notes_discard_confirmation(state: &mut AttachedInputState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') => {
            state.workspace_notes_draft = None;
            state.workspace_notes_editing_index = None;
        }
        KeyCode::Char('n') | KeyCode::Esc => {}
        _ => return,
    }
    state.workspace_notes_state = WorkspaceNotesState::Editing;
    state.workspace_notes_message = None;
}

#[cfg(unix)]
fn render_workspace_notes(
    frame: &mut Frame<'_>,
    draft: &str,
    cursor: usize,
    discard_confirmation: bool,
    message: Option<&str>,
) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(88, 82, area);
    frame.render_widget(Clear, popup);
    let cursor = cursor.min(draft.len());
    let prefix = &draft[..cursor];
    let cursor_line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let cursor_column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, line)| line)
        .chars()
        .count();
    let mut lines = draft
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == cursor_line {
                let split = line
                    .char_indices()
                    .nth(cursor_column)
                    .map_or(line.len(), |(offset, _)| offset);
                Line::from(format!(
                    "{}▏{}",
                    bounded_display_text(&line[..split], 240),
                    bounded_display_text(&line[split..], 240)
                ))
            } else {
                Line::from(bounded_display_text(line, 240))
            }
        })
        .collect::<Vec<_>>();
    if draft.is_empty() {
        lines.push(Line::from(
            "No notes yet. Type reusable prompt or context snippets here.",
        ));
    }
    if let Some(message) = message {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    lines.push(Line::from(format!(
        "{} / {MAX_WORKSPACE_NOTES_BYTES} bytes · Enter newline · Ctrl-S save · Esc return",
        draft.len()
    )));
    if discard_confirmation {
        lines.push(Line::from(
            "Discard unsaved notes? y discard · n/Esc keep editing",
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Rover repository notes · local state ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_workspace_notes_list(
    frame: &mut Frame<'_>,
    notes: &[String],
    selected: usize,
    confirm_delete: bool,
    message: Option<&str>,
) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(78, 70, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if notes.is_empty() {
        lines.push(Line::from(
            "No saved notes. Press n to create a reusable note.",
        ));
    } else {
        for (index, note) in notes.iter().enumerate().take(MAX_WORKSPACE_NOTES_COUNT) {
            let title = note
                .lines()
                .next()
                .filter(|line| !line.is_empty())
                .unwrap_or("(untitled)");
            let marker = if index == selected { ">" } else { " " };
            lines.push(Line::from(format!(
                "{marker} {:02}. {}",
                index + 1,
                bounded_display_text(title, 96)
            )));
        }
    }
    if let Some(message) = message {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    if confirm_delete {
        lines.push(Line::from("Delete selected note? y delete · n/Esc keep it"));
    } else {
        lines.push(Line::from(
            "↑/↓ select · n new · Enter edit · d delete · Esc close",
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Rover repository notes · local state ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_saved_commands_list(
    frame: &mut Frame<'_>,
    commands: &[SavedCommand],
    selected: usize,
    confirm_delete: bool,
    message: Option<&str>,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(82, 72, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if commands.is_empty() {
        lines.push(Line::from("No saved commands. Press n to add one."));
    } else {
        for (index, command) in commands.iter().enumerate().take(MAX_SAVED_COMMANDS) {
            lines.push(Line::from(format!(
                "{} {:02}. {} · {}",
                if index == selected { ">" } else { " " },
                index + 1,
                bounded_display_text(&command.name, MAX_SAVED_COMMAND_NAME_BYTES),
                bounded_display_text(&command.command, MAX_SAVED_COMMAND_LINE_BYTES)
            )));
        }
    }
    if let Some(message) = message {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    lines.push(Line::from(if confirm_delete {
        "Delete selected command? y delete · n/Esc keep it"
    } else {
        "↑/↓ select · n new · Enter edit · d delete · r run with confirmation · Esc close"
    }));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Rover saved shell commands · local state ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_saved_commands_overlay(
    frame: &mut Frame<'_>,
    state: &AttachedInputState,
    provider: &TaskProvider<'_>,
) {
    if let Some(draft) = state.saved_command_draft.as_ref() {
        render_saved_command_editor(
            frame,
            draft,
            state.saved_command_dialog,
            state.saved_command_message.as_deref(),
        );
    } else if state.saved_commands_panel == SavedCommandsPanel::List {
        render_saved_commands_list(
            frame,
            &provider.saved_commands,
            state.selected_saved_command,
            state.saved_command_dialog == SavedCommandDialog::ConfirmDelete,
            state.saved_command_message.as_deref(),
        );
    }
    if let Some(index) = state.pending_saved_command {
        render_saved_command_confirmation(frame, provider.saved_commands.get(index));
    }
}

#[cfg(unix)]
fn render_saved_command_editor(
    frame: &mut Frame<'_>,
    draft: &SavedCommandDraft,
    dialog: SavedCommandDialog,
    message: Option<&str>,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(82, 58, area);
    frame.render_widget(Clear, popup);
    let name_cursor = draft.name_cursor.min(draft.name.len());
    let command_cursor = draft.command_cursor.min(draft.command.len());
    let name_split = name_cursor;
    let command_split = command_cursor;
    let name = format!(
        "{}{}{}",
        bounded_display_text(&draft.name[..name_cursor], MAX_SAVED_COMMAND_NAME_BYTES),
        if draft.active_field == SavedCommandField::Name {
            "▏"
        } else {
            ""
        },
        bounded_display_text(&draft.name[name_split..], MAX_SAVED_COMMAND_NAME_BYTES)
    );
    let command = format!(
        "{}{}{}",
        bounded_display_text(
            &draft.command[..command_cursor],
            MAX_SAVED_COMMAND_LINE_BYTES
        ),
        if draft.active_field == SavedCommandField::Command {
            "▏"
        } else {
            ""
        },
        bounded_display_text(
            &draft.command[command_split..],
            MAX_SAVED_COMMAND_LINE_BYTES
        )
    );
    let mut lines = vec![
        Line::from("Tab switches fields · Ctrl-S saves · Esc returns"),
        Line::from(format!(
            "Name (max {MAX_SAVED_COMMAND_NAME_BYTES} bytes): {name}"
        )),
        Line::from(format!(
            "Command (max {MAX_SAVED_COMMAND_LINE_BYTES} bytes): {command}"
        )),
    ];
    if let Some(message) = message {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    if dialog == SavedCommandDialog::ConfirmDiscard {
        lines.push(Line::from(
            "Discard unsaved command? y discard · n/Esc keep editing",
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Edit saved shell command ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_saved_command_confirmation(frame: &mut Frame<'_>, command: Option<&SavedCommand>) {
    let area = frame.area();
    if area.width < 32 || area.height < 8 {
        return;
    }
    let popup = centered_rect(90, 62, area);
    frame.render_widget(Clear, popup);
    let lines = match command {
        Some(command) => vec![
            Line::from(format!(
                "Run saved command: {}",
                bounded_display_text(&command.name, MAX_SAVED_COMMAND_NAME_BYTES)
            )),
            Line::from(format!(
                "Command: {}",
                bounded_display_text(&command.command, MAX_SAVED_COMMAND_LINE_BYTES)
            )),
            Line::from(
                "Runs in the attached shell with Rover's OS-user authority; it is not sandboxed.",
            ),
            Line::from("y send once · n/Esc cancel"),
        ],
        None => vec![
            Line::from("The selected saved command is unavailable."),
            Line::from("Press y to dismiss."),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Confirm shell input ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_task_drafts_overlay(
    frame: &mut Frame<'_>,
    state: &AttachedInputState,
    provider: &TaskProvider<'_>,
) {
    if let Some(draft) = state.task_draft.as_ref() {
        render_task_draft_editor(frame, draft, state);
    } else if state.task_drafts_panel == SavedCommandsPanel::List {
        if state.task_draft_panel == TaskDraftPanel::Plans {
            render_task_plan_list(frame, state, provider);
        } else {
            render_task_draft_list(frame, state, provider);
        }
    }
}

#[cfg(unix)]
fn render_task_draft_editor(
    frame: &mut Frame<'_>,
    draft: &TaskDraftEdit,
    state: &AttachedInputState,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(88, 70, area);
    frame.render_widget(Clear, popup);
    let title = task_draft_line_window(
        &draft.title,
        draft.title_cursor,
        draft.active_field == TaskDraftField::Title,
        160,
    );
    let mut lines = vec![
        Line::from(
            "Tab changes field · Enter adds line/advances title · Ctrl-S saves · Esc returns",
        ),
        Line::from(format!("Title: {title}")),
    ];
    lines.extend(task_draft_field_view(draft));
    if let Some(message) = state.task_draft_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    if state.task_draft_dialog == SavedCommandDialog::ConfirmDiscard {
        lines.push(Line::from(
            "Discard unsaved draft? y discard · n/Esc keep editing",
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Edit local task briefing draft ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_task_draft_list(
    frame: &mut Frame<'_>,
    state: &AttachedInputState,
    provider: &TaskProvider<'_>,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(84, 70, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if provider.task_drafts.is_empty() {
        lines.push(Line::from("No task drafts. Press n to add one."));
    } else {
        for (index, draft) in provider
            .task_drafts
            .iter()
            .enumerate()
            .take(MAX_TASK_DRAFTS)
        {
            let title = if draft.title.trim().is_empty() {
                "(untitled draft)"
            } else {
                &draft.title
            };
            let preview = draft.prompt.lines().next().unwrap_or_default();
            lines.push(Line::from(format!(
                "{} {:02}. {} · {}",
                if index == state.selected_task_draft {
                    ">"
                } else {
                    " "
                },
                index + 1,
                bounded_display_text(title, MAX_TASK_DRAFT_TITLE_BYTES),
                bounded_display_text(preview, 96)
            )));
        }
    }
    if let Some(message) = state.task_draft_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    let help = match state.task_draft_dialog {
        SavedCommandDialog::ConfirmDelete => "Delete selected draft? y delete · n/Esc keep it",
        SavedCommandDialog::ConfirmCreatePlan => {
            "Create a durable manual plan from this draft? y create · n/Esc cancel · no worker starts"
        }
        _ => "↑/↓ select · n new · Enter edit · p create plan · l plans · d delete · Esc close",
    };
    lines.push(Line::from(help));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Rover task briefing drafts · local state ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_task_plan_list(
    frame: &mut Frame<'_>,
    state: &AttachedInputState,
    provider: &TaskProvider<'_>,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(84, 70, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if provider.task_plans.summaries.is_empty() {
        lines.push(Line::from(
            "No durable task plans. Press Esc to return to drafts.",
        ));
    } else {
        for (index, plan) in provider.task_plans.summaries.iter().enumerate().take(1_000) {
            lines.push(Line::from(format!(
                "{} {} · {} · {} · {} deps · {} attempts",
                if index == state.selected_task_plan {
                    ">"
                } else {
                    " "
                },
                bounded_display_text(&plan.title, MAX_TASK_DRAFT_TITLE_BYTES),
                bounded_display_text(&plan.status, 24),
                bounded_display_text(&plan.readiness, 24),
                plan.dependency_count,
                plan.attempt_count,
            )));
            lines.push(Line::from(format!(
                "  {} · {}",
                bounded_display_text(&plan.id, 128),
                bounded_display_text(&plan.updated_at, 64),
            )));
        }
    }
    if let Some(message) = state.task_draft_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    lines.push(Line::from(
        "↑/↓ select · r refresh · Esc return · plans are not dispatched",
    ));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Rover task plans · project state ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn task_draft_line_window(value: &str, cursor: usize, active: bool, limit: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    let cursor = value[..cursor.min(value.len())]
        .chars()
        .count()
        .min(chars.len());
    let start = cursor
        .saturating_sub(limit / 2)
        .min(chars.len().saturating_sub(limit));
    let end = (start + limit).min(chars.len());
    let visible = chars[start..end].iter().collect::<String>();
    let cursor_in_view = cursor.saturating_sub(start).min(visible.chars().count());
    let mut result = String::new();
    if start > 0 {
        result.push('…');
    }
    for (index, character) in visible.chars().enumerate() {
        if active && index == cursor_in_view {
            result.push('▏');
        }
        result.push(character);
    }
    if active && cursor_in_view == visible.chars().count() {
        result.push('▏');
    }
    if end < chars.len() {
        result.push('…');
    }
    result
}

#[cfg(unix)]
fn task_draft_multiline_lines(
    value: &str,
    cursor: usize,
    active: bool,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let cursor = cursor.min(value.len());
    let prefix = &value[..cursor];
    let cursor_line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let cursor_column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count();
    let all_lines: Vec<&str> = value.split('\n').collect();
    let start = cursor_line
        .saturating_sub(max_rows / 2)
        .min(all_lines.len().saturating_sub(max_rows));
    all_lines
        .iter()
        .enumerate()
        .skip(start)
        .take(max_rows)
        .map(|(index, line)| {
            if index == cursor_line && active {
                Line::from(task_draft_line_window(line, cursor_column, true, 180))
            } else {
                Line::from(bounded_display_text(line, 180))
            }
        })
        .collect()
}

#[cfg(unix)]
fn task_draft_field_view(draft: &TaskDraftEdit) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    append_draft_multiline_field(
        &mut lines,
        "Paths",
        &draft.paths,
        draft.paths_cursor,
        TaskDraftField::Paths,
        draft,
        MAX_TASK_DRAFT_PATHS_BYTES,
    );
    append_draft_multiline_field(
        &mut lines,
        "Dependencies",
        &draft.dependencies,
        draft.dependencies_cursor,
        TaskDraftField::Dependencies,
        draft,
        MAX_TASK_DRAFT_DEPENDENCIES_BYTES,
    );
    append_draft_multiline_field(
        &mut lines,
        "Quality gate",
        &draft.quality_gate,
        draft.quality_gate_cursor,
        TaskDraftField::QualityGate,
        draft,
        MAX_TASK_DRAFT_GATE_BYTES,
    );
    append_draft_multiline_field(
        &mut lines,
        "Prompt (unsent)",
        &draft.prompt,
        draft.prompt_cursor,
        TaskDraftField::Prompt,
        draft,
        MAX_TASK_DRAFT_PROMPT_BYTES,
    );
    lines
}

#[cfg(unix)]
fn append_draft_multiline_field(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    cursor: usize,
    field: TaskDraftField,
    draft: &TaskDraftEdit,
    max_bytes: usize,
) {
    let active = draft.active_field == field;
    let count = value.lines().count();
    lines.push(Line::from(format!(
        "{label}{} ({} bytes max):{}",
        if active { " ◀" } else { "" },
        max_bytes,
        if active { "" } else { " (press Tab to edit)" },
    )));
    if active {
        lines.extend(task_draft_multiline_lines(value, cursor, true, 8));
    } else {
        lines.push(Line::from(format!(
            "{}{}",
            bounded_display_text(value.lines().next().unwrap_or_default(), 100),
            if count > 1 { " …" } else { "" },
        )));
    }
}

#[cfg(unix)]
fn open_saved_commands_shortcut(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix
        || key.kind != KeyEventKind::Press
        || key.code != KeyCode::Char('k')
        || !provider
            .capabilities
            .supports(TaskCapabilities::SAVED_COMMANDS)
    {
        return false;
    }
    state.prefix = false;
    state.saved_commands_panel = SavedCommandsPanel::List;
    state.selected_saved_command = 0;
    state.saved_command_message = None;
    true
}

#[cfg(unix)]
fn open_task_drafts_shortcut(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix
        || key.kind != KeyEventKind::Press
        || key.code != KeyCode::Char('d')
        || !provider
            .capabilities
            .supports(TaskCapabilities::TASK_DRAFTS)
    {
        return false;
    }
    state.prefix = false;
    state.task_drafts_panel = SavedCommandsPanel::List;
    state.selected_task_draft = 0;
    state.task_draft_panel = TaskDraftPanel::Drafts;
    state.selected_task_plan = 0;
    state.task_draft_message = None;
    true
}

#[cfg(unix)]
fn open_agent_session_shortcut(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix
        || key.kind != KeyEventKind::Press
        || key.code != KeyCode::Char('a')
        || !provider
            .capabilities
            .supports(TaskCapabilities::AGENT_SESSION)
    {
        return false;
    }
    state.prefix = false;
    state.agent_session_panel = AgentSessionPanel::Details;
    state.agent_session_message = None;
    true
}

#[cfg(unix)]
fn handle_task_drafts_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    if let Some(mut draft) = state.task_draft.take() {
        handle_task_draft_edit_key(state, provider, &mut draft, key);
        return;
    }
    if state.task_draft_dialog == SavedCommandDialog::ConfirmDelete {
        handle_task_draft_delete_confirmation(state, provider, key);
        return;
    }
    if state.task_draft_dialog == SavedCommandDialog::ConfirmCreatePlan {
        handle_task_plan_confirmation(state, provider, key);
        return;
    }
    handle_task_draft_list_key(state, provider, key);
}

#[cfg(unix)]
fn handle_task_plan_confirmation(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Char('y')
            if provider.capabilities.supports(TaskCapabilities::TASK_PLANS)
                && state.selected_task_draft < provider.task_drafts.len() =>
        {
            let draft = provider.task_drafts[state.selected_task_draft].clone();
            match (provider.task_plans.create)(&draft) {
                Ok(plan) => {
                    provider.task_plans.summaries.push(plan.clone());
                    provider.task_plans.summaries =
                        bounded_task_plan_summaries(&provider.task_plans.summaries);
                    state.selected_task_plan =
                        provider.task_plans.summaries.len().saturating_sub(1);
                    state.task_draft_message = Some(format!(
                        "Plan {} created; no worker was started.",
                        bounded_display_text(&plan.id, 128)
                    ));
                }
                Err(error) => {
                    state.task_draft_message =
                        Some(format!("Could not create plan: {}", error.kind()));
                }
            }
            state.task_draft_dialog = SavedCommandDialog::Editing;
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            state.task_draft_dialog = SavedCommandDialog::Editing;
        }
        _ => {}
    }
}

#[cfg(unix)]
fn handle_task_draft_edit_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    draft: &mut TaskDraftEdit,
    key: KeyEvent,
) {
    if state.task_draft_dialog == SavedCommandDialog::ConfirmDiscard {
        match key.code {
            KeyCode::Char('y') => {
                state.task_draft = None;
                state.task_draft_dialog = SavedCommandDialog::Editing;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.task_draft = Some(draft.clone());
                state.task_draft_dialog = SavedCommandDialog::Editing;
            }
            _ => state.task_draft = Some(draft.clone()),
        }
        return;
    }
    state.task_draft_message = None;
    match key.code {
        KeyCode::Esc => {
            let original = draft
                .editing_index
                .and_then(|index| provider.task_drafts.get(index));
            if original
                .is_some_and(|saved| saved.title == draft.title && saved.prompt == draft.prompt)
            {
                state.task_draft = None;
            } else {
                state.task_draft = Some(draft.clone());
                state.task_draft_dialog = SavedCommandDialog::ConfirmDiscard;
            }
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            save_task_draft(state, provider, draft);
            if state.task_draft_message.is_some() && state.task_draft.is_none() {
                state.task_draft = Some(draft.clone());
            }
        }
        KeyCode::Tab => {
            draft.active_field = match draft.active_field {
                TaskDraftField::Title => TaskDraftField::Paths,
                TaskDraftField::Paths => TaskDraftField::Dependencies,
                TaskDraftField::Dependencies => TaskDraftField::QualityGate,
                TaskDraftField::QualityGate => TaskDraftField::Prompt,
                TaskDraftField::Prompt => TaskDraftField::Title,
            };
            state.task_draft = Some(draft.clone());
        }
        KeyCode::Enter if draft.active_field == TaskDraftField::Title => {
            draft.active_field = TaskDraftField::Paths;
            state.task_draft = Some(draft.clone());
        }
        _ => {
            edit_task_draft(draft, key);
            state.task_draft = Some(draft.clone());
        }
    }
}

#[cfg(unix)]
fn handle_task_draft_delete_confirmation(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Char('y') => {
            if state.selected_task_draft < provider.task_drafts.len() {
                let mut updated = provider.task_drafts.clone();
                updated.remove(state.selected_task_draft);
                match (provider.save_task_drafts)(&updated) {
                    Ok(()) => provider.task_drafts = updated,
                    Err(error) => {
                        state.task_draft_message =
                            Some(format!("Could not delete draft: {}", error.kind()));
                    }
                }
                state.selected_task_draft = state
                    .selected_task_draft
                    .min(provider.task_drafts.len().saturating_sub(1));
            }
            state.task_draft_dialog = SavedCommandDialog::Editing;
        }
        KeyCode::Char('n') | KeyCode::Esc => state.task_draft_dialog = SavedCommandDialog::Editing,
        _ => {}
    }
}

#[cfg(unix)]
fn handle_task_draft_list_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    state.task_draft_message = None;
    if state.task_draft_panel == TaskDraftPanel::Plans {
        match key.code {
            KeyCode::Esc => state.task_draft_panel = TaskDraftPanel::Drafts,
            KeyCode::Up if !provider.task_plans.summaries.is_empty() => {
                state.selected_task_plan = if state.selected_task_plan == 0 {
                    provider.task_plans.summaries.len() - 1
                } else {
                    state.selected_task_plan - 1
                };
            }
            KeyCode::Down if !provider.task_plans.summaries.is_empty() => {
                state.selected_task_plan =
                    (state.selected_task_plan + 1) % provider.task_plans.summaries.len();
            }
            KeyCode::Char('r') if provider.capabilities.supports(TaskCapabilities::TASK_PLANS) => {
                match (provider.task_plans.load)() {
                    Ok(plans) => {
                        provider.task_plans.summaries = bounded_task_plan_summaries(&plans);
                        state.selected_task_plan = state
                            .selected_task_plan
                            .min(provider.task_plans.summaries.len().saturating_sub(1));
                    }
                    Err(error) => {
                        state.task_draft_message =
                            Some(format!("Could not refresh plans: {}", error.kind()));
                    }
                }
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => state.task_drafts_panel = SavedCommandsPanel::Closed,
        KeyCode::Up if !provider.task_drafts.is_empty() => {
            state.selected_task_draft = if state.selected_task_draft == 0 {
                provider.task_drafts.len() - 1
            } else {
                state.selected_task_draft - 1
            };
        }
        KeyCode::Down if !provider.task_drafts.is_empty() => {
            state.selected_task_draft =
                (state.selected_task_draft + 1) % provider.task_drafts.len();
        }
        KeyCode::Char('n') if provider.task_drafts.len() < MAX_TASK_DRAFTS => {
            state.task_draft = Some(TaskDraftEdit::default());
        }
        KeyCode::Char('n') => {
            state.task_draft_message =
                Some(format!("At most {MAX_TASK_DRAFTS} drafts can be saved."));
        }
        KeyCode::Enter if state.selected_task_draft < provider.task_drafts.len() => {
            let saved = &provider.task_drafts[state.selected_task_draft];
            state.task_draft = Some(TaskDraftEdit {
                editing_index: Some(state.selected_task_draft),
                title_cursor: saved.title.len(),
                paths_cursor: saved.paths.len(),
                dependencies_cursor: saved.dependencies.len(),
                quality_gate_cursor: saved.quality_gate.len(),
                prompt_cursor: saved.prompt.len(),
                title: saved.title.clone(),
                paths: saved.paths.clone(),
                dependencies: saved.dependencies.clone(),
                quality_gate: saved.quality_gate.clone(),
                prompt: saved.prompt.clone(),
                active_field: TaskDraftField::Title,
            });
        }
        KeyCode::Char('d') if state.selected_task_draft < provider.task_drafts.len() => {
            state.task_draft_dialog = SavedCommandDialog::ConfirmDelete;
        }
        KeyCode::Char('l') if provider.capabilities.supports(TaskCapabilities::TASK_PLANS) => {
            state.task_draft_panel = TaskDraftPanel::Plans;
            state.selected_task_plan = 0;
            match (provider.task_plans.load)() {
                Ok(plans) => provider.task_plans.summaries = bounded_task_plan_summaries(&plans),
                Err(error) => {
                    state.task_draft_message =
                        Some(format!("Could not load plans: {}", error.kind()));
                }
            }
        }
        KeyCode::Char('p')
            if provider.capabilities.supports(TaskCapabilities::TASK_PLANS)
                && state.selected_task_draft < provider.task_drafts.len() =>
        {
            state.task_draft_dialog = SavedCommandDialog::ConfirmCreatePlan;
        }
        _ => {}
    }
}

#[cfg(unix)]
fn edit_task_draft(draft: &mut TaskDraftEdit, key: KeyEvent) {
    let (value, cursor, limit) = match draft.active_field {
        TaskDraftField::Title => (
            &mut draft.title,
            &mut draft.title_cursor,
            MAX_TASK_DRAFT_TITLE_BYTES,
        ),
        TaskDraftField::Paths => (
            &mut draft.paths,
            &mut draft.paths_cursor,
            MAX_TASK_DRAFT_PATHS_BYTES,
        ),
        TaskDraftField::Dependencies => (
            &mut draft.dependencies,
            &mut draft.dependencies_cursor,
            MAX_TASK_DRAFT_DEPENDENCIES_BYTES,
        ),
        TaskDraftField::QualityGate => (
            &mut draft.quality_gate,
            &mut draft.quality_gate_cursor,
            MAX_TASK_DRAFT_GATE_BYTES,
        ),
        TaskDraftField::Prompt => (
            &mut draft.prompt,
            &mut draft.prompt_cursor,
            MAX_TASK_DRAFT_PROMPT_BYTES,
        ),
    };
    if key.code == KeyCode::Enter && draft.active_field != TaskDraftField::Title {
        if value.len() < limit {
            value.insert(*cursor, '\n');
            *cursor += 1;
        }
        return;
    }
    match key.code {
        KeyCode::Left if *cursor > 0 => {
            *cursor -= 1;
            while !value.is_char_boundary(*cursor) {
                *cursor -= 1;
            }
        }
        KeyCode::Right if *cursor < value.len() => {
            *cursor += 1;
            while !value.is_char_boundary(*cursor) {
                *cursor += 1;
            }
        }
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = value.len(),
        KeyCode::Backspace if *cursor > 0 => {
            let mut start = *cursor - 1;
            while !value.is_char_boundary(start) {
                start -= 1;
            }
            value.drain(start..*cursor);
            *cursor = start;
        }
        KeyCode::Delete if *cursor < value.len() => {
            let mut end = *cursor + 1;
            while !value.is_char_boundary(end) {
                end += 1;
            }
            value.drain(*cursor..end);
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && !is_bidi_format(character)
                && value.len().saturating_add(character.len_utf8()) <= limit =>
        {
            value.insert(*cursor, character);
            *cursor += character.len_utf8();
        }
        _ => {}
    }
}

#[cfg(unix)]
fn save_task_draft(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    draft: &TaskDraftEdit,
) {
    let value = TaskDraft {
        title: draft.title.clone(),
        paths: draft.paths.clone(),
        dependencies: draft.dependencies.clone(),
        quality_gate: draft.quality_gate.clone(),
        prompt: draft.prompt.clone(),
    };
    if !task_draft_is_valid(&value) {
        state.task_draft_message = Some(
            "Draft exceeds its title or prompt byte limit or contains unsupported control text."
                .to_owned(),
        );
        return;
    }
    let mut updated = provider.task_drafts.clone();
    let selected = if let Some(index) = draft.editing_index {
        let Some(existing) = updated.get_mut(index) else {
            state.task_draft_message = Some("Selected draft no longer exists.".to_owned());
            return;
        };
        *existing = value;
        index
    } else {
        if updated.len() >= MAX_TASK_DRAFTS {
            state.task_draft_message =
                Some(format!("At most {MAX_TASK_DRAFTS} drafts can be saved."));
            return;
        }
        updated.push(value);
        updated.len() - 1
    };
    match (provider.save_task_drafts)(&updated) {
        Ok(()) => {
            provider.task_drafts = updated;
            state.selected_task_draft = selected;
            state.task_draft = None;
            state.task_draft_dialog = SavedCommandDialog::Editing;
        }
        Err(error) => {
            state.task_draft_message = Some(format!("Could not save draft: {}", error.kind()));
        }
    }
}

#[cfg(unix)]
fn handle_saved_commands_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    if let Some(draft) = state.saved_command_draft.take() {
        handle_saved_command_editor_key(state, provider, draft, key);
        return;
    }
    handle_saved_command_list_key(state, provider, key);
}

#[cfg(unix)]
fn handle_saved_command_list_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if state.saved_command_dialog == SavedCommandDialog::ConfirmDelete {
        match key.code {
            KeyCode::Char('y') if state.selected_saved_command < provider.saved_commands.len() => {
                let mut updated = provider.saved_commands.clone();
                updated.remove(state.selected_saved_command);
                match (provider.save_saved_commands)(&updated) {
                    Ok(()) => provider.saved_commands = updated,
                    Err(error) => {
                        state.saved_command_message =
                            Some(format!("Could not delete command: {}", error.kind()));
                    }
                }
                state.selected_saved_command = state
                    .selected_saved_command
                    .min(provider.saved_commands.len().saturating_sub(1));
                state.saved_command_dialog = SavedCommandDialog::Editing;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                state.saved_command_dialog = SavedCommandDialog::Editing;
            }
            _ => {}
        }
        return;
    }
    state.saved_command_message = None;
    match key.code {
        KeyCode::Esc => state.saved_commands_panel = SavedCommandsPanel::Closed,
        KeyCode::Up if !provider.saved_commands.is_empty() => {
            state.selected_saved_command = if state.selected_saved_command == 0 {
                provider.saved_commands.len() - 1
            } else {
                state.selected_saved_command - 1
            };
        }
        KeyCode::Down if !provider.saved_commands.is_empty() => {
            state.selected_saved_command =
                (state.selected_saved_command + 1) % provider.saved_commands.len();
        }
        KeyCode::Char('n') if provider.saved_commands.len() < MAX_SAVED_COMMANDS => {
            state.saved_command_draft = Some(SavedCommandDraft::default());
        }
        KeyCode::Enter if state.selected_saved_command < provider.saved_commands.len() => {
            let saved = &provider.saved_commands[state.selected_saved_command];
            state.saved_command_draft = Some(SavedCommandDraft {
                editing_index: Some(state.selected_saved_command),
                name: saved.name.clone(),
                command: saved.command.clone(),
                name_cursor: saved.name.len(),
                command_cursor: saved.command.len(),
                active_field: SavedCommandField::Name,
            });
        }
        KeyCode::Char('d') if state.selected_saved_command < provider.saved_commands.len() => {
            state.saved_command_dialog = SavedCommandDialog::ConfirmDelete;
        }
        KeyCode::Char('r') if state.selected_saved_command < provider.saved_commands.len() => {
            state.pending_saved_command = Some(state.selected_saved_command);
        }
        KeyCode::Char('n') => {
            state.saved_command_message = Some(format!(
                "At most {MAX_SAVED_COMMANDS} commands can be saved."
            ));
        }
        _ => {}
    }
}

#[cfg(unix)]
fn handle_saved_command_editor_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    mut draft: SavedCommandDraft,
    key: KeyEvent,
) {
    if state.saved_command_dialog == SavedCommandDialog::ConfirmDiscard {
        state.saved_command_draft = Some(draft);
        handle_saved_command_discard_confirmation(state, key);
        return;
    }
    state.saved_command_message = None;
    match key.code {
        KeyCode::Esc => {
            let original = draft
                .editing_index
                .and_then(|index| provider.saved_commands.get(index));
            if original
                .is_some_and(|saved| saved.name == draft.name && saved.command == draft.command)
            {
                state.saved_command_draft = None;
                return;
            }
            state.saved_command_dialog = SavedCommandDialog::ConfirmDiscard;
            state.saved_command_draft = Some(draft);
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            save_saved_command(state, provider, &draft);
            if state.saved_command_message.is_some() && state.saved_command_draft.is_none() {
                state.saved_command_draft = Some(draft);
            }
        }
        KeyCode::Tab => {
            draft.active_field = match draft.active_field {
                SavedCommandField::Name => SavedCommandField::Command,
                SavedCommandField::Command => SavedCommandField::Name,
            };
            state.saved_command_draft = Some(draft);
        }
        KeyCode::Enter if draft.active_field == SavedCommandField::Name => {
            draft.active_field = SavedCommandField::Command;
            state.saved_command_draft = Some(draft);
        }
        _ => {
            edit_saved_command_field(&mut draft, key);
            state.saved_command_draft = Some(draft);
        }
    }
}

#[cfg(unix)]
fn edit_saved_command_field(draft: &mut SavedCommandDraft, key: KeyEvent) {
    let (value, cursor, limit) = match draft.active_field {
        SavedCommandField::Name => (
            &mut draft.name,
            &mut draft.name_cursor,
            MAX_SAVED_COMMAND_NAME_BYTES,
        ),
        SavedCommandField::Command => (
            &mut draft.command,
            &mut draft.command_cursor,
            MAX_SAVED_COMMAND_LINE_BYTES,
        ),
    };
    match key.code {
        KeyCode::Left => {
            while *cursor > 0 {
                *cursor -= 1;
                if value.is_char_boundary(*cursor) {
                    break;
                }
            }
        }
        KeyCode::Right => {
            if *cursor < value.len() {
                *cursor += 1;
                while !value.is_char_boundary(*cursor) {
                    *cursor += 1;
                }
            }
        }
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = value.len(),
        KeyCode::Backspace if *cursor > 0 => {
            let mut start = *cursor - 1;
            while !value.is_char_boundary(start) {
                start -= 1;
            }
            value.drain(start..*cursor);
            *cursor = start;
        }
        KeyCode::Delete if *cursor < value.len() => {
            let mut end = *cursor + 1;
            while !value.is_char_boundary(end) {
                end += 1;
            }
            value.drain(*cursor..end);
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && !is_bidi_format(character)
                && value.len().saturating_add(character.len_utf8()) <= limit =>
        {
            value.insert(*cursor, character);
            *cursor += character.len_utf8();
        }
        _ => {}
    }
}

#[cfg(unix)]
fn save_saved_command(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    draft: &SavedCommandDraft,
) {
    let command = SavedCommand {
        name: draft.name.clone(),
        command: draft.command.clone(),
    };
    if !saved_command_is_valid(&command) {
        state.saved_command_message = Some(
            "Name and one-line command must be nonempty, printable, and within their byte limits."
                .to_owned(),
        );
        return;
    }
    if provider
        .saved_commands
        .iter()
        .enumerate()
        .any(|(index, saved)| Some(index) != draft.editing_index && saved.name == command.name)
    {
        state.saved_command_message = Some("Saved command names must be unique.".to_owned());
        return;
    }
    let mut updated = provider.saved_commands.clone();
    let selected = if let Some(index) = draft.editing_index {
        let Some(existing) = updated.get_mut(index) else {
            state.saved_command_message = Some("Selected command no longer exists.".to_owned());
            return;
        };
        *existing = command;
        index
    } else {
        if updated.len() >= MAX_SAVED_COMMANDS {
            state.saved_command_message = Some(format!(
                "At most {MAX_SAVED_COMMANDS} commands can be saved."
            ));
            return;
        }
        updated.push(command);
        updated.len() - 1
    };
    match (provider.save_saved_commands)(&updated) {
        Ok(()) => {
            provider.saved_commands = updated;
            state.selected_saved_command = selected;
            state.saved_command_draft = None;
            state.saved_command_dialog = SavedCommandDialog::Editing;
        }
        Err(error) => {
            state.saved_command_message = Some(format!("Could not save command: {}", error.kind()));
        }
    }
}

#[cfg(unix)]
fn handle_saved_command_discard_confirmation(state: &mut AttachedInputState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') => {
            state.saved_command_draft = None;
            state.saved_command_dialog = SavedCommandDialog::Editing;
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            state.saved_command_dialog = SavedCommandDialog::Editing;
        }
        _ => {}
    }
}

#[cfg(unix)]
fn handle_saved_command_confirmation(
    state: &mut AttachedInputState,
    provider: &TaskProvider<'_>,
    key: KeyEvent,
    mut send_input: impl FnMut(&[u8]) -> io::Result<()>,
) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    match key.code {
        KeyCode::Char('y') => {
            let Some(command) = state
                .pending_saved_command
                .and_then(|index| provider.saved_commands.get(index))
            else {
                state.pending_saved_command = None;
                state.saved_command_message =
                    Some("Selected saved command is unavailable.".to_owned());
                return;
            };
            if !saved_command_is_valid(command) {
                state.pending_saved_command = None;
                state.saved_command_message = Some("Saved command failed validation.".to_owned());
                return;
            }
            let mut input = command.command.as_bytes().to_vec();
            input.push(b'\r');
            state.pending_saved_command = None;
            if let Err(error) = send_input(&input) {
                state.saved_command_message = Some(format!(
                    "Could not send command input; check the session before retrying: {}",
                    error.kind()
                ));
            }
        }
        KeyCode::Char('n') | KeyCode::Esc => state.pending_saved_command = None,
        _ => {}
    }
}

#[cfg(unix)]
fn handle_command_palette_key(
    workspace: &mut Workspace,
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
    on_change: &mut impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<()> {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return Ok(());
    }
    match key.code {
        KeyCode::Esc => state.command_palette_query = None,
        KeyCode::Enter => {
            let query = state.command_palette_query.as_deref().unwrap_or_default();
            let actions = palette_actions(provider.capabilities, query);
            let Some(action) = actions.get(state.command_palette_selected).copied() else {
                state.command_palette_selected = 0;
                return Ok(());
            };
            state.command_palette_query = None;
            dispatch_palette_action(action, workspace, state, provider, on_change)?;
        }
        KeyCode::Up | KeyCode::Down => {
            let query = state.command_palette_query.as_deref().unwrap_or_default();
            let action_count = palette_actions(provider.capabilities, query).len();
            if action_count > 0 {
                state.command_palette_selected = if key.code == KeyCode::Up {
                    if state.command_palette_selected == 0 {
                        action_count - 1
                    } else {
                        state.command_palette_selected - 1
                    }
                } else {
                    (state.command_palette_selected + 1) % action_count
                };
            } else {
                state.command_palette_selected = 0;
            }
        }
        KeyCode::Backspace => {
            if let Some(query) = state.command_palette_query.as_mut() {
                query.pop();
            }
            state.command_palette_selected = 0;
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && state.command_palette_query.as_ref().is_some_and(|query| {
                    query.len().saturating_add(character.len_utf8()) <= MAX_PALETTE_QUERY_BYTES
                }) =>
        {
            if let Some(query) = state.command_palette_query.as_mut() {
                query.push(character);
            }
            state.command_palette_selected = 0;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(unix)]
fn dispatch_palette_action(
    action: PaletteAction,
    workspace: &mut Workspace,
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    on_change: &mut impl FnMut(&Workspace) -> io::Result<()>,
) -> io::Result<()> {
    match action {
        PaletteAction::OpenTasks => {
            state.task_panel = TaskPanelState::List;
            if !provider.tasks.is_empty() {
                state.selected_task = state.selected_task.min(provider.tasks.len() - 1);
            }
        }
        PaletteAction::OpenFiles => {
            state.file_directory.clear();
            state.file_selected = 0;
            state.file_include_hidden = false;
            state.file_message = None;
            state.file_panel = FilePanelState::Tree;
            refresh_file_directory(state, provider, None);
        }
        PaletteAction::QuickOpen => start_quick_open(state, provider, FilePanelState::Closed),
        PaletteAction::OpenNotes => {
            state.workspace_notes_panel = WorkspaceNotesPanel::List;
            state.selected_workspace_note = 0;
            state.workspace_notes_draft = None;
            state.workspace_notes_editing_index = None;
            state.workspace_notes_state = WorkspaceNotesState::Editing;
            state.workspace_notes_message = None;
        }
        PaletteAction::OpenSavedCommands => {
            state.saved_commands_panel = SavedCommandsPanel::List;
            state.selected_saved_command = 0;
            state.saved_command_message = None;
        }
        PaletteAction::OpenTaskDrafts => {
            state.task_drafts_panel = SavedCommandsPanel::List;
            state.selected_task_draft = 0;
            state.task_draft_message = None;
        }
        PaletteAction::OpenAgentSession => {
            state.agent_session_panel = AgentSessionPanel::Details;
            state.agent_session_message = None;
        }
        PaletteAction::RefreshTasks => {
            state.refresh_request = RefreshRequest::Requested;
            state.task_refresh_message = None;
        }
        PaletteAction::KeyboardHelp => state.help_open = true,
        PaletteAction::NextTab | PaletteAction::PreviousTab => {
            focus_neighbor_tab(workspace, action == PaletteAction::NextTab);
            on_change(workspace)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn palette_actions(capabilities: TaskCapabilities, query: &str) -> Vec<PaletteAction> {
    let all = [
        (PaletteAction::OpenTasks, "Open task board"),
        (PaletteAction::KeyboardHelp, "Keyboard help"),
        (PaletteAction::NextTab, "Next workspace tab"),
        (PaletteAction::PreviousTab, "Previous workspace tab"),
        (PaletteAction::RefreshTasks, "Refresh task board"),
        (PaletteAction::OpenFiles, "Open repository file tree"),
        (PaletteAction::QuickOpen, "Quick open repository file"),
        (PaletteAction::OpenNotes, "Open repository notes"),
        (
            PaletteAction::OpenSavedCommands,
            "Manage saved shell commands",
        ),
        (PaletteAction::OpenTaskDrafts, "Manage task briefing drafts"),
        (PaletteAction::OpenAgentSession, "Open saved agent session"),
    ];
    let query = query.to_lowercase();
    all.into_iter()
        .filter(|(action, _label)| match action {
            PaletteAction::RefreshTasks => capabilities.supports(TaskCapabilities::REFRESH),
            PaletteAction::OpenFiles | PaletteAction::QuickOpen => {
                capabilities.supports(TaskCapabilities::FILES)
            }
            PaletteAction::OpenNotes => capabilities.supports(TaskCapabilities::NOTES),
            PaletteAction::OpenSavedCommands => {
                capabilities.supports(TaskCapabilities::SAVED_COMMANDS)
            }
            PaletteAction::OpenTaskDrafts => capabilities.supports(TaskCapabilities::TASK_DRAFTS),
            PaletteAction::OpenAgentSession => {
                capabilities.supports(TaskCapabilities::AGENT_SESSION)
            }
            _ => true,
        })
        .filter(|(_, label)| label.to_lowercase().contains(&query))
        .map(|(action, _)| action)
        .collect()
}

#[cfg(unix)]
fn render_command_palette(
    frame: &mut Frame<'_>,
    query: &str,
    selected: usize,
    capabilities: TaskCapabilities,
) {
    let area = frame.area();
    if area.width < 28 || area.height < 8 {
        return;
    }
    let popup = centered_rect(64, 62, area);
    frame.render_widget(Clear, popup);
    let actions = palette_actions(capabilities, query);
    let mut lines = vec![Line::from(format!(
        "Command: {}_",
        bounded_display_text(query, MAX_PALETTE_QUERY_BYTES)
    ))];
    if actions.is_empty() {
        lines.push(Line::from("No matching available commands."));
    } else {
        for (index, action) in actions.iter().take(12).enumerate() {
            let marker = if index == selected { "> " } else { "  " };
            let style = if index == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", palette_action_label(*action)),
                style,
            )));
        }
    }
    lines.push(Line::from("Type to filter · Enter run · Esc close"));
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Rover command palette ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn palette_action_label(action: PaletteAction) -> &'static str {
    match action {
        PaletteAction::OpenTasks => "Open task board",
        PaletteAction::OpenFiles => "Open repository file tree",
        PaletteAction::QuickOpen => "Quick open repository file",
        PaletteAction::OpenNotes => "Open repository notes",
        PaletteAction::OpenSavedCommands => "Manage saved shell commands",
        PaletteAction::OpenTaskDrafts => "Manage task briefing drafts",
        PaletteAction::OpenAgentSession => "Open saved agent session",
        PaletteAction::RefreshTasks => "Refresh task board",
        PaletteAction::KeyboardHelp => "Keyboard help",
        PaletteAction::NextTab => "Next workspace tab",
        PaletteAction::PreviousTab => "Previous workspace tab",
    }
}

#[cfg(unix)]
fn open_file_browser_shortcut(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if !state.prefix
        || state.task_panel != TaskPanelState::Closed
        || key.kind != KeyEventKind::Press
        || !provider.capabilities.supports(TaskCapabilities::FILES)
    {
        return false;
    }
    match key.code {
        KeyCode::Char('f') => {
            state.prefix = false;
            state.file_directory.clear();
            state.file_selected = 0;
            state.file_include_hidden = false;
            state.file_message = None;
            state.file_panel = FilePanelState::Tree;
            refresh_file_directory(state, provider, None);
            true
        }
        KeyCode::Char('.') => {
            state.prefix = false;
            start_quick_open(state, provider, FilePanelState::Closed);
            true
        }
        _ => false,
    }
}

#[cfg(unix)]
fn handle_file_panel_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return;
    }
    match state.file_panel {
        FilePanelState::Tree => handle_file_tree_key(state, provider, key),
        FilePanelState::QuickOpen if state.quick_open_query.is_some() => {
            handle_quick_open_edit(state, provider, key);
        }
        FilePanelState::QuickOpen => handle_quick_open_key(state, provider, key),
        FilePanelState::Viewer => handle_file_preview_key(state, provider, key),
        FilePanelState::Editing => handle_file_editor_key(state, provider, key),
        FilePanelState::DiffPreview => handle_file_diff_key(state, key),
        FilePanelState::Closed => {}
    }
}

#[cfg(unix)]
fn handle_file_tree_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    let entries = state
        .file_listing
        .as_ref()
        .map_or(&[][..], |listing| listing.entries.as_slice());
    match key.code {
        KeyCode::Esc => state.file_panel = FilePanelState::Closed,
        KeyCode::Up if !entries.is_empty() => {
            state.file_selected = state.file_selected.saturating_sub(1);
        }
        KeyCode::Down if !entries.is_empty() => {
            state.file_selected = (state.file_selected + 1).min(entries.len() - 1);
        }
        KeyCode::Enter => {
            if let Some(entry) = entries.get(state.file_selected).cloned() {
                match entry.kind {
                    BrowserEntryKind::Directory => {
                        state.file_directory = entry.path;
                        state.file_selected = 0;
                        state.file_message = None;
                        refresh_file_directory(state, provider, None);
                    }
                    BrowserEntryKind::File => {
                        open_file_preview(state, provider, &entry.path, FilePanelState::Tree);
                    }
                    BrowserEntryKind::Symlink | BrowserEntryKind::Other => {
                        state.file_message = Some(
                            "This entry is not opened; symlinks and special files are refused."
                                .to_owned(),
                        );
                    }
                }
            }
        }
        KeyCode::Backspace => {
            if let Some((parent, _)) = state.file_directory.rsplit_once('/') {
                state.file_directory = parent.to_owned();
            } else {
                state.file_directory.clear();
            }
            state.file_selected = 0;
            state.file_message = None;
            refresh_file_directory(state, provider, None);
        }
        KeyCode::Char('h') => {
            let selected = entries
                .get(state.file_selected)
                .map(|entry| entry.path.clone());
            state.file_include_hidden = !state.file_include_hidden;
            refresh_file_directory(state, provider, selected.as_deref());
        }
        KeyCode::Char('.') => start_quick_open(state, provider, FilePanelState::Tree),
        KeyCode::Char('r') => {
            let selected = entries
                .get(state.file_selected)
                .map(|entry| entry.path.clone());
            refresh_file_directory(state, provider, selected.as_deref());
        }
        _ => {}
    }
}

#[cfg(unix)]
fn refresh_file_directory(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    preserve_path: Option<&str>,
) {
    state.file_message = None;
    match (provider.load_directory)(&state.file_directory, state.file_include_hidden) {
        Ok(listing) => {
            state.file_selected = preserve_path
                .and_then(|path| listing.entries.iter().position(|entry| entry.path == path))
                .unwrap_or(0);
            state.file_listing = Some(listing);
        }
        Err(error) => {
            state.file_listing = None;
            state.file_selected = 0;
            state.file_message = Some(bounded_display_text(
                &format!("Unable to list repository files: {error}"),
                120,
            ));
        }
    }
}

#[cfg(unix)]
fn start_quick_open(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    return_to: FilePanelState,
) {
    state.quick_open_return = return_to;
    state.quick_open_query = Some(String::new());
    state.quick_open_selected = 0;
    state.file_message = None;
    state.file_panel = FilePanelState::QuickOpen;
    match (provider.load_quick_open)() {
        Ok(listing) => {
            state.quick_open_entries = listing.entries;
            if listing.truncated {
                state.file_message =
                    Some("Quick-open scan reached its 10,000-entry limit.".to_owned());
            }
            if let Some(error) = listing.git_error {
                state.file_message = Some(bounded_display_text(
                    &format!("Git status unavailable: {error}"),
                    120,
                ));
            }
        }
        Err(error) => {
            state.quick_open_entries.clear();
            state.file_message = Some(bounded_display_text(
                &format!("Quick-open scan failed: {error}"),
                120,
            ));
        }
    }
}

#[cfg(unix)]
fn handle_quick_open_edit(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    let Some(query) = state.quick_open_query.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => {
            state.quick_open_query = None;
            state.file_panel = state.quick_open_return;
        }
        KeyCode::Enter => {
            let selected = quick_open_matches(state)
                .into_iter()
                .find(|index| *index == state.quick_open_selected)
                .and_then(|index| state.quick_open_entries.get(index))
                .map(|entry| entry.path.clone());
            if let Some(path) = selected {
                open_file_preview(state, provider, &path, FilePanelState::QuickOpen);
                state.quick_open_query = None;
            }
        }
        KeyCode::Backspace => {
            query.pop();
            state.quick_open_selected = quick_open_matches(state).first().copied().unwrap_or(0);
        }
        KeyCode::Up => move_quick_open_selection(state, false),
        KeyCode::Down => move_quick_open_selection(state, true),
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && query.len().saturating_add(character.len_utf8()) <= MAX_FILE_QUERY_BYTES =>
        {
            query.push(character);
            state.quick_open_selected = quick_open_matches(state).first().copied().unwrap_or(0);
        }
        _ => {}
    }
}

#[cfg(unix)]
fn handle_quick_open_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    let indices = quick_open_matches(state);
    match key.code {
        KeyCode::Esc => {
            state.file_panel = state.quick_open_return;
        }
        KeyCode::Up if !indices.is_empty() => {
            let position = indices
                .iter()
                .position(|index| *index == state.quick_open_selected)
                .unwrap_or(0);
            let previous = if position == 0 {
                indices.len() - 1
            } else {
                position - 1
            };
            state.quick_open_selected = indices[previous];
        }
        KeyCode::Down if !indices.is_empty() => {
            let position = indices
                .iter()
                .position(|index| *index == state.quick_open_selected)
                .unwrap_or(indices.len() - 1);
            state.quick_open_selected = indices[(position + 1) % indices.len()];
        }
        KeyCode::Enter => {
            if let Some(entry) = state.quick_open_entries.get(state.quick_open_selected) {
                open_file_preview(
                    state,
                    provider,
                    &entry.path.clone(),
                    FilePanelState::QuickOpen,
                );
            }
        }
        _ => {}
    }
}

#[cfg(unix)]
fn move_quick_open_selection(state: &mut AttachedInputState, forward: bool) {
    let indices = quick_open_matches(state);
    if indices.is_empty() {
        state.quick_open_selected = 0;
        return;
    }
    let position = indices
        .iter()
        .position(|index| *index == state.quick_open_selected)
        .unwrap_or(0);
    let next = if forward {
        (position + 1) % indices.len()
    } else if position == 0 {
        indices.len() - 1
    } else {
        position - 1
    };
    state.quick_open_selected = indices[next];
}

#[cfg(unix)]
fn open_file_preview(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    path: &str,
    return_to: FilePanelState,
) {
    state.file_preview_scroll = 0;
    state.file_edit_capability = if provider.capabilities.supports(TaskCapabilities::FILE_EDIT) {
        FileEditCapability::Enabled
    } else {
        FileEditCapability::Disabled
    };
    state.file_message = None;
    match (provider.load_file_preview)(path) {
        Ok((bytes, truncated)) => {
            let binary = bytes.contains(&0);
            let text = if binary {
                String::new()
            } else {
                sanitize_screen_text(&String::from_utf8_lossy(&bytes))
            };
            state.file_preview = Some(FilePreviewState {
                path: path.to_owned(),
                text,
                truncated,
                binary,
            });
            state.file_preview_return = return_to;
            state.file_panel = FilePanelState::Viewer;
        }
        Err(error) => {
            state.file_message = Some(bounded_display_text(
                &format!("Unable to open repository file: {error}"),
                160,
            ));
        }
    }
}

#[cfg(unix)]
fn handle_file_preview_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.file_panel = state.file_preview_return;
            state.file_preview = None;
        }
        KeyCode::Up => state.file_preview_scroll = state.file_preview_scroll.saturating_sub(1),
        KeyCode::Down => state.file_preview_scroll = state.file_preview_scroll.saturating_add(1),
        KeyCode::PageUp => state.file_preview_scroll = state.file_preview_scroll.saturating_sub(12),
        KeyCode::PageDown => {
            state.file_preview_scroll = state.file_preview_scroll.saturating_add(12);
        }
        KeyCode::Char('e') if provider.capabilities.supports(TaskCapabilities::FILE_EDIT) => {
            start_file_edit(state, provider);
        }
        _ => {}
    }
}

#[cfg(unix)]
fn start_file_edit(state: &mut AttachedInputState, provider: &mut TaskProvider<'_>) {
    let Some(preview) = state.file_preview.as_ref() else {
        return;
    };
    let path = preview.path.clone();
    state.file_message = None;
    match (provider.load_file_for_edit)(&path) {
        Ok(bytes) if bytes.len() <= MAX_FILE_EDIT_BYTES && !bytes.contains(&0) => {
            match String::from_utf8(bytes) {
                Ok(text)
                    if !text.chars().any(|character| {
                        character.is_control() && !matches!(character, '\n' | '\r' | '\t')
                    }) =>
                {
                    let digest = rover_core::Sha256Digest::of(text.as_bytes()).to_hex();
                    state.file_edit = Some(FileEditState {
                        path,
                        original: text.clone(),
                        buffer: text,
                        original_sha256: digest,
                        cursor: 0,
                        discard_confirmation: false,
                    });
                    state.file_panel = FilePanelState::Editing;
                }
                Ok(_) => {
                    state.file_message = Some(
                        "File contains terminal control characters; editing refused.".to_owned(),
                    );
                }
                Err(_) => {
                    state.file_message =
                        Some("File is not valid UTF-8; editing refused.".to_owned());
                }
            }
        }
        Ok(_) => {
            state.file_message = Some("File is binary or exceeds the 8 MiB edit limit.".to_owned());
        }
        Err(error) => {
            state.file_message = Some(bounded_display_text(
                &format!("Unable to load file for editing: {error}"),
                160,
            ));
        }
    }
}

#[cfg(unix)]
fn handle_file_editor_key(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return;
    }
    let Some(edit) = state.file_edit.as_mut() else {
        state.file_panel = FilePanelState::Viewer;
        return;
    };
    if edit.discard_confirmation {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                state.file_edit = None;
                state.file_panel = FilePanelState::Viewer;
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                edit.discard_confirmation = false;
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc => {
            if edit.buffer == edit.original {
                state.file_edit = None;
                state.file_panel = FilePanelState::Viewer;
            } else {
                edit.discard_confirmation = true;
            }
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            save_file_edit(state, provider);
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.file_diff_scroll = 0;
            state.file_panel = FilePanelState::DiffPreview;
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            handoff_file_edit_to_external_editor(state, provider);
        }
        KeyCode::Left => edit.cursor = previous_char_boundary(&edit.buffer, edit.cursor),
        KeyCode::Right => edit.cursor = next_char_boundary(&edit.buffer, edit.cursor),
        KeyCode::Home => edit.cursor = current_line_start(&edit.buffer, edit.cursor),
        KeyCode::End => edit.cursor = current_line_end(&edit.buffer, edit.cursor),
        KeyCode::Up => edit.cursor = vertical_cursor_move(&edit.buffer, edit.cursor, false),
        KeyCode::Down => edit.cursor = vertical_cursor_move(&edit.buffer, edit.cursor, true),
        KeyCode::Backspace if edit.cursor > 0 => {
            let previous = previous_char_boundary(&edit.buffer, edit.cursor);
            edit.buffer.replace_range(previous..edit.cursor, "");
            edit.cursor = previous;
        }
        KeyCode::Delete if edit.cursor < edit.buffer.len() => {
            let next = next_char_boundary(&edit.buffer, edit.cursor);
            edit.buffer.replace_range(edit.cursor..next, "");
        }
        KeyCode::Enter if edit.buffer.len() < MAX_FILE_EDIT_BYTES => {
            edit.buffer.insert(edit.cursor, '\n');
            edit.cursor += 1;
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && edit.buffer.len().saturating_add(character.len_utf8())
                    <= MAX_FILE_EDIT_BYTES =>
        {
            edit.buffer.insert(edit.cursor, character);
            edit.cursor += character.len_utf8();
        }
        _ => {}
    }
}

#[cfg(unix)]
fn save_file_edit(state: &mut AttachedInputState, provider: &mut TaskProvider<'_>) {
    let Some(edit) = state.file_edit.as_ref() else {
        return;
    };
    if edit.buffer == edit.original {
        state.file_message = Some("No edits to save.".to_owned());
        return;
    }
    match (provider.save_file)(&edit.path, &edit.original_sha256, edit.buffer.as_bytes()) {
        Ok(()) => {
            let path = edit.path.clone();
            let return_to = state.file_preview_return;
            state.file_edit = None;
            state.file_panel = FilePanelState::Viewer;
            open_file_preview(state, provider, &path, return_to);
            if state.file_preview.is_some() {
                state.file_message = Some("File saved atomically.".to_owned());
            }
        }
        Err(error) => {
            state.file_message = Some(bounded_display_text(
                &format!("Save failed; edits remain in the buffer: {error}"),
                180,
            ));
        }
    }
}

#[cfg(unix)]
fn handoff_file_edit_to_external_editor(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
) {
    let Some(edit) = state.file_edit.as_ref() else {
        return;
    };
    let path = edit.path.clone();
    let input = edit.buffer.as_bytes().to_vec();
    state.file_message = None;
    let result = with_terminal_suspended(|| (provider.external_file_editor)(&path, &input));
    state.terminal_handoff = TerminalHandoffState::NeedsRedraw;
    match result {
        Ok(bytes) if bytes.len() <= MAX_FILE_EDIT_BYTES && !bytes.contains(&0) => {
            match String::from_utf8(bytes) {
                Ok(text)
                    if !text.chars().any(|character| {
                        character.is_control() && !matches!(character, '\n' | '\r' | '\t')
                    }) =>
                {
                    if let Some(edit) = state.file_edit.as_mut() {
                        edit.cursor = text.len();
                        edit.buffer = text;
                        edit.discard_confirmation = false;
                        state.file_message = Some(
                            "External editor returned; Ctrl-S saves through Rover.".to_owned(),
                        );
                    }
                }
                Ok(_) => {
                    state.file_message = Some(
                        "External editor output contains terminal controls; buffer unchanged."
                            .to_owned(),
                    );
                }
                Err(_) => {
                    state.file_message = Some(
                        "External editor output is not valid UTF-8; buffer unchanged.".to_owned(),
                    );
                }
            }
        }
        Ok(_) => {
            state.file_message = Some(
                "External editor output is binary or exceeds the 8 MiB limit; buffer unchanged."
                    .to_owned(),
            );
        }
        Err(error) => {
            state.file_message = Some(bounded_display_text(
                &format!("External editor handoff failed; buffer retained: {error}"),
                180,
            ));
        }
    }
}

#[cfg(unix)]
fn with_terminal_suspended<T>(action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let mut guard = SuspendedTerminal::enter()?;
    let result = action();
    guard.restore()?;
    result
}

#[cfg(unix)]
fn handle_file_diff_key(state: &mut AttachedInputState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => state.file_panel = FilePanelState::Editing,
        KeyCode::Up => state.file_diff_scroll = state.file_diff_scroll.saturating_sub(1),
        KeyCode::Down => state.file_diff_scroll = state.file_diff_scroll.saturating_add(1),
        KeyCode::PageUp => state.file_diff_scroll = state.file_diff_scroll.saturating_sub(12),
        KeyCode::PageDown => state.file_diff_scroll = state.file_diff_scroll.saturating_add(12),
        _ => {}
    }
}

#[cfg(unix)]
fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

#[cfg(unix)]
fn next_char_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(offset, _)| cursor + offset)
}

#[cfg(unix)]
fn current_line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map_or(0, |index| index + 1)
}

#[cfg(unix)]
fn current_line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |offset| cursor + offset)
}

#[cfg(unix)]
fn vertical_cursor_move(text: &str, cursor: usize, down: bool) -> usize {
    let line_start = current_line_start(text, cursor);
    let column = text[line_start..cursor].chars().count();
    let target_start = if down {
        let line_end = current_line_end(text, cursor);
        if line_end == text.len() {
            return cursor;
        }
        line_end + 1
    } else {
        if line_start == 0 {
            return cursor;
        }
        let previous_end = line_start - 1;
        text[..previous_end]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    };
    let target_end = text[target_start..]
        .find('\n')
        .map_or(text.len(), |offset| target_start + offset);
    text[target_start..target_end]
        .char_indices()
        .nth(column)
        .map_or(target_end, |(offset, _)| target_start + offset)
}

#[cfg(unix)]
fn quick_open_matches(state: &AttachedInputState) -> Vec<usize> {
    let query = state
        .quick_open_query
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    state
        .quick_open_entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| entry.path.to_lowercase().contains(&query).then_some(index))
        .collect()
}

#[cfg(unix)]
fn attached_task_refresh_key(panel: TaskPanelState, key: KeyEvent) -> bool {
    matches!(panel, TaskPanelState::List | TaskPanelState::Detail)
        && key.code == KeyCode::Char('r')
        && (key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat)
}

#[cfg(unix)]
fn attached_task_cancel_key(panel: TaskPanelState, key: KeyEvent) -> bool {
    matches!(panel, TaskPanelState::List | TaskPanelState::Detail)
        && key.code == KeyCode::Char('c')
        && (key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat)
}

fn task_visible_indices(tasks: &[TaskSummary], preferences: &TaskViewPreferences) -> Vec<usize> {
    let query = preferences.query.to_lowercase();
    let mut indices = tasks
        .iter()
        .enumerate()
        .filter_map(|(index, task)| {
            let matches = query.is_empty()
                || [
                    task.id.as_str(),
                    task.status.as_str(),
                    task.objective.as_str(),
                    task.candidate.as_deref().unwrap_or_default(),
                    task.error.as_deref().unwrap_or_default(),
                ]
                .into_iter()
                .any(|field| field.to_lowercase().contains(&query));
            matches.then_some(index)
        })
        .collect::<Vec<_>>();
    indices.sort_by(|left, right| {
        let left = &tasks[*left];
        let right = &tasks[*right];
        let order = match preferences.sort {
            TaskSortOrder::Updated => right.updated_at.cmp(&left.updated_at),
            TaskSortOrder::Status => left
                .status
                .to_lowercase()
                .cmp(&right.status.to_lowercase())
                .then_with(|| right.updated_at.cmp(&left.updated_at)),
            TaskSortOrder::Objective => left
                .objective
                .to_lowercase()
                .cmp(&right.objective.to_lowercase()),
        };
        order.then_with(|| left.id.cmp(&right.id))
    });
    indices
}

#[cfg(unix)]
fn save_task_view_preferences(state: &mut AttachedInputState, provider: &mut TaskProvider<'_>) {
    if let Err(error) = (provider.save_view_preferences)(&provider.view_preferences) {
        state.task_refresh_message = Some(bounded_display_text(
            &format!("Task view preference save failed: {error}"),
            100,
        ));
    }
}

#[cfg(unix)]
fn handle_task_search_edit(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return;
    }
    let Some(draft) = state.task_search_draft.as_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => state.task_search_draft = None,
        KeyCode::Enter => {
            let committed = draft.trim().to_owned();
            state.task_search_draft = None;
            provider.view_preferences.query = truncate_utf8(&committed, MAX_TASK_FILTER_BYTES);
            state.selected_task = task_visible_indices(&provider.tasks, &provider.view_preferences)
                .first()
                .copied()
                .unwrap_or(0);
            save_task_view_preferences(state, provider);
        }
        KeyCode::Backspace => {
            draft.pop();
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !character.is_control()
                && draft.len().saturating_add(character.len_utf8()) <= MAX_TASK_FILTER_BYTES =>
        {
            draft.push(character);
        }
        _ => {}
    }
}

#[cfg(unix)]
fn attached_task_evidence_key(state: &mut AttachedInputState, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Enter => state.task_panel = TaskPanelState::Detail,
        KeyCode::Up => state.task_evidence_scroll = state.task_evidence_scroll.saturating_sub(1),
        KeyCode::Down => state.task_evidence_scroll = state.task_evidence_scroll.saturating_add(1),
        KeyCode::PageUp => {
            state.task_evidence_scroll = state.task_evidence_scroll.saturating_sub(12);
        }
        KeyCode::PageDown => {
            state.task_evidence_scroll = state.task_evidence_scroll.saturating_add(12);
        }
        _ => {}
    }
    true
}

#[cfg(unix)]
fn open_selected_task_evidence(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if (key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat)
        || key.code != KeyCode::Char('i')
    {
        return false;
    }
    state.task_evidence = None;
    state.task_evidence_message = None;
    state.task_evidence_scroll = 0;
    if let Some(task) = provider.tasks.get(state.selected_task).cloned() {
        match (provider.load_evidence)(&task) {
            Ok(Some(evidence))
                if task.investigation_id.as_deref() == Some(evidence.id.as_str())
                    && task
                        .candidate
                        .as_deref()
                        .is_none_or(|candidate| candidate == evidence.candidate) =>
            {
                state.task_evidence = Some(evidence);
            }
            Ok(Some(_)) => {
                state.task_evidence_message =
                    Some("Evidence record does not match the selected task.".to_owned());
            }
            Ok(None) => {
                state.task_evidence_message = Some(
                    "No investigation yet; execution completion is not acceptance.".to_owned(),
                );
            }
            Err(error) => {
                state.task_evidence_message = Some(bounded_display_text(
                    &format!("Unable to load task evidence: {error}"),
                    180,
                ));
            }
        }
    } else {
        state.task_evidence_message = Some("No task is selected.".to_owned());
    }
    state.task_panel = TaskPanelState::Evidence;
    true
}

#[cfg(unix)]
fn attached_task_diff_key(state: &mut AttachedInputState, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Enter => state.task_panel = TaskPanelState::Detail,
        KeyCode::Up => state.task_diff_scroll = state.task_diff_scroll.saturating_sub(1),
        KeyCode::Down => state.task_diff_scroll = state.task_diff_scroll.saturating_add(1),
        KeyCode::PageUp => state.task_diff_scroll = state.task_diff_scroll.saturating_sub(12),
        KeyCode::PageDown => state.task_diff_scroll = state.task_diff_scroll.saturating_add(12),
        _ => {}
    }
    true
}

#[cfg(unix)]
fn open_selected_task_diff(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if (key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat)
        || key.code != KeyCode::Char('d')
    {
        return false;
    }
    state.task_diff = None;
    state.task_diff_message = None;
    state.task_diff_scroll = 0;
    if let Some(task) = provider.tasks.get(state.selected_task).cloned() {
        match (provider.load_diff)(&task) {
            Ok(Some(diff))
                if task.base_snapshot.as_deref() == Some(diff.base_snapshot.as_str())
                    && task.candidate.as_deref() == Some(diff.candidate.as_str())
                    && diff.changes.len() <= MAX_TASK_DIFF_RECORDS
                    && diff.changes.iter().all(|change| {
                        matches!(change.status.as_str(), "added" | "deleted" | "modified")
                    }) =>
            {
                state.task_diff = Some(diff);
            }
            Ok(Some(_)) => {
                state.task_diff_message =
                    Some("Snapshot comparison does not match the selected task.".to_owned());
            }
            Ok(None) => {
                state.task_diff_message = Some(
                    "No retained base and candidate snapshots are linked to this task.".to_owned(),
                );
            }
            Err(error) => {
                state.task_diff_message = Some(bounded_display_text(
                    &format!("Unable to load candidate changes: {error}"),
                    180,
                ));
            }
        }
    } else {
        state.task_diff_message = Some("No task is selected.".to_owned());
    }
    state.task_panel = TaskPanelState::Diff;
    true
}

#[cfg(unix)]
fn handle_task_cancel_confirmation(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return;
    }
    match key.code {
        KeyCode::Char('y') => {
            if let Some(task_id) = state.pending_cancel.as_deref() {
                state.task_refresh_message = Some(match (provider.cancel_task)(task_id) {
                    Ok(()) => {
                        "Cancellation requested; prior external actions are not undone.".to_owned()
                    }
                    Err(error) => {
                        bounded_display_text(&format!("Cancellation failed: {error}"), 100)
                    }
                });
            }
            state.pending_cancel = None;
        }
        KeyCode::Char('n') | KeyCode::Esc => state.pending_cancel = None,
        _ => {}
    }
}

fn render_task_cancel_confirmation(frame: &mut Frame<'_>, task_id: &str) {
    let area = frame.area();
    if area.width < 32 || area.height < 8 {
        return;
    }
    let popup = centered_rect(74, 30, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "Cancel task {}?",
                bounded_display_text(task_id, 48)
            )),
            Line::from("Cancellation does not undo external actions."),
            Line::from("y confirm · n or Esc keep task"),
        ])
        .block(
            Block::default()
                .title(" Confirm task cancellation ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

fn render_task_evidence(
    frame: &mut Frame<'_>,
    evidence: Option<&TaskInvestigationEvidence>,
    message: Option<&str>,
    scroll: u16,
) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(86, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(evidence) = evidence {
        lines.extend([
            Line::from(format!(
                "Investigation: {}",
                bounded_display_text(&evidence.id, 72)
            )),
            Line::from(format!(
                "Candidate: {}",
                bounded_display_text(&evidence.candidate, 100)
            )),
            Line::from(format!(
                "Stored decision: {}",
                bounded_display_text(&evidence.decision, 72)
            )),
            Line::from(format!(
                "Decision reason: {}",
                bounded_display_text(&evidence.decision_reason, 220)
            )),
            Line::from(
                "Recorded checks (stored outcomes are evidence, not independent verification):",
            ),
        ]);
        for check in evidence.checks.iter().take(MAX_TASK_EVIDENCE_ITEMS) {
            lines.push(Line::from(format!(
                "CHECK {} · {} · required: {} · tests: {} · skipped: {} · {}",
                bounded_display_text(&check.id, 36),
                bounded_display_text(&check.outcome, 20),
                check.required,
                check.tests,
                check.skipped,
                bounded_display_text(&check.meaning, 180)
            )));
        }
        if evidence.checks.len() > MAX_TASK_EVIDENCE_ITEMS {
            lines.push(Line::from(
                "Additional check records omitted at the TUI limit.",
            ));
        }
        if !evidence.unknowns.is_empty() {
            lines.push(Line::from("Recorded unknowns:"));
            for unknown in evidence.unknowns.iter().take(MAX_TASK_EVIDENCE_ITEMS) {
                lines.push(Line::from(format!(
                    "UNKNOWN: {}",
                    bounded_display_text(unknown, 200)
                )));
            }
            if evidence.unknowns.len() > MAX_TASK_EVIDENCE_ITEMS {
                lines.push(Line::from(
                    "Additional unknown records omitted at the TUI limit.",
                ));
            }
        }
        if !evidence.findings.is_empty() {
            lines.push(Line::from("Recorded findings:"));
            for finding in evidence.findings.iter().take(MAX_TASK_EVIDENCE_ITEMS) {
                let severity = finding
                    .severity
                    .as_deref()
                    .map(|value| format!("{} · ", bounded_display_text(value, 20)))
                    .unwrap_or_default();
                lines.push(Line::from(format!(
                    "FINDING: {severity}{} {}",
                    bounded_display_text(&finding.path, 120),
                    bounded_display_text(&finding.message, 200)
                )));
            }
            if evidence.findings.len() > MAX_TASK_EVIDENCE_ITEMS {
                lines.push(Line::from(
                    "Additional finding records omitted at the TUI limit.",
                ));
            }
        }
    } else {
        lines.push(Line::from(
            message.unwrap_or("No investigation record is available."),
        ));
    }
    lines.push(Line::from(
        "↑/↓, PgUp/PgDn scroll · Esc returns to task detail",
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(" Rover task evidence · recorded assessment ")
                    .borders(Borders::ALL),
            ),
        popup,
    );
}

fn render_task_diff(
    frame: &mut Frame<'_>,
    diff: Option<&TaskDiffEvidence>,
    message: Option<&str>,
    scroll: u16,
) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(86, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(diff) = diff {
        lines.extend([
            Line::from(format!(
                "Base snapshot: {}",
                bounded_display_text(&diff.base_snapshot, 88)
            )),
            Line::from(format!(
                "Candidate snapshot: {}",
                bounded_display_text(&diff.candidate, 88)
            )),
            Line::from(format!("{} verified path changes", diff.changes.len())),
        ]);
        for change in diff.changes.iter().take(MAX_TASK_DIFF_ITEMS) {
            lines.push(Line::from(format!(
                "{:<10} {:<20} {}",
                bounded_display_text(&change.status, 10),
                bounded_display_text(&change.category, 20),
                bounded_display_text(&change.path, 180)
            )));
        }
        if diff.changes.len() > MAX_TASK_DIFF_ITEMS {
            lines.push(Line::from(format!(
                "{} additional paths omitted from this bounded view; use `rover diff` for the full patch.",
                diff.changes.len() - MAX_TASK_DIFF_ITEMS
            )));
        } else {
            lines.push(Line::from(
                "Use `rover diff --base <snapshot> --candidate <snapshot>` for the complete applicable patch.",
            ));
        }
    } else {
        lines.push(Line::from(
            message.unwrap_or("No retained snapshot comparison is available."),
        ));
    }
    lines.push(Line::from(
        "↑/↓, PgUp/PgDn scroll · Esc returns to task detail",
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(
                Block::default()
                    .title(" Rover candidate diff · verified snapshot inventory ")
                    .borders(Borders::ALL),
            ),
        popup,
    );
}

#[cfg(unix)]
fn render_file_tree(frame: &mut Frame<'_>, state: &AttachedInputState) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(88, 86, area);
    frame.render_widget(Clear, popup);
    let listing = state.file_listing.as_ref();
    let entries = listing.map_or(&[][..], |listing| listing.entries.as_slice());
    let rows_available = usize::from(popup.height.saturating_sub(7)).max(1);
    let visible = entries.len().min(MAX_FILE_BROWSER_ROWS).min(rows_available);
    let start = if entries.len() > visible {
        state
            .file_selected
            .saturating_sub(visible / 2)
            .min(entries.len().saturating_sub(visible))
    } else {
        0
    };
    let directory = if state.file_directory.is_empty() {
        "/".to_owned()
    } else {
        state.file_directory.clone()
    };
    let mut lines = vec![Line::from(format!(
        "{} · {} entries · hidden files {}",
        bounded_display_text(&directory, 128),
        entries.len(),
        if state.file_include_hidden {
            "shown"
        } else {
            "hidden"
        }
    ))];
    if let Some(error) = listing.and_then(|listing| listing.git_error.as_deref()) {
        lines.push(Line::from(Span::styled(
            bounded_display_text(&format!("Git status unavailable: {error}"), 120),
            Style::default().fg(Color::Yellow),
        )));
    }
    if entries.is_empty() {
        lines.push(Line::from("This directory has no visible entries."));
    } else {
        append_file_tree_rows(&mut lines, entries, state.file_selected, start, visible);
        if entries.len() > visible {
            lines.push(Line::from(format!(
                "Showing {}–{} of {} entries.",
                start + 1,
                start + visible,
                entries.len()
            )));
        }
    }
    if listing.is_some_and(|listing| listing.truncated) {
        lines.push(Line::from(
            "Directory listing reached the 10,000-entry limit.",
        ));
    }
    if let Some(message) = state.file_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 120)));
    }
    lines.push(Line::from(
        "↑/↓ select · Enter open · Backspace parent · h hidden · . quick open · Esc close",
    ));
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Rover files · read only · Git status ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn append_file_tree_rows(
    lines: &mut Vec<Line<'static>>,
    entries: &[BrowserEntry],
    selected: usize,
    start: usize,
    visible: usize,
) {
    for (position, entry) in entries.iter().enumerate().skip(start).take(visible) {
        let marker = if position == selected { "> " } else { "  " };
        let icon = match entry.kind {
            BrowserEntryKind::Directory => "▸ ",
            BrowserEntryKind::File => "  ",
            BrowserEntryKind::Symlink => "↗ ",
            BrowserEntryKind::Other => "? ",
        };
        let (status, color) = git_status_label(entry.git_status);
        let selected_style = if position == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let path_style = if position == selected {
            selected_style
        } else {
            Style::default().fg(color)
        };
        lines.push(Line::from(vec![
            Span::styled(marker, selected_style),
            Span::styled(status, path_style),
            Span::styled(icon, path_style),
            Span::styled(bounded_display_text(&entry.path, 140), path_style),
        ]));
    }
}

#[cfg(unix)]
fn render_quick_open(frame: &mut Frame<'_>, state: &AttachedInputState) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(88, 86, area);
    frame.render_widget(Clear, popup);
    let indices = quick_open_matches(state);
    let rows_available = usize::from(popup.height.saturating_sub(7)).max(1);
    let visible = indices.len().min(MAX_FILE_BROWSER_ROWS).min(rows_available);
    let selected_position = indices
        .iter()
        .position(|index| *index == state.quick_open_selected)
        .unwrap_or(0);
    let start = if indices.len() > visible {
        selected_position
            .saturating_sub(visible / 2)
            .min(indices.len().saturating_sub(visible))
    } else {
        0
    };
    let query = state.quick_open_query.as_deref().unwrap_or_default();
    let mut lines = vec![Line::from(format!(
        "Quick open: {}_",
        bounded_display_text(query, MAX_FILE_QUERY_BYTES)
    ))];
    if indices.is_empty() {
        lines.push(Line::from("No files match. Hidden files are included."));
    } else {
        for index in indices.iter().copied().skip(start).take(visible) {
            let entry = &state.quick_open_entries[index];
            let marker = if index == state.quick_open_selected {
                "> "
            } else {
                "  "
            };
            let (_, color) = git_status_label(entry.git_status);
            let style = if index == state.quick_open_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            let (status, _) = git_status_label(entry.git_status);
            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(status, style),
                Span::styled(bounded_display_text(&entry.path, 160), style),
            ]));
        }
        if indices.len() > visible {
            lines.push(Line::from(format!(
                "Showing {}–{} of {} matches.",
                start + 1,
                start + visible,
                indices.len()
            )));
        }
    }
    if state.quick_open_entries.len() >= MAX_BROWSER_ENTRIES {
        lines.push(Line::from("Quick-open scan is bounded at 10,000 paths."));
    }
    if let Some(message) = state.file_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 120)));
    }
    lines.push(Line::from("Type to filter · Enter preview · Esc return"));
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Rover quick open · hidden files included ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn render_file_preview(frame: &mut Frame<'_>, state: &AttachedInputState) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(92, 90, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(preview) = state.file_preview.as_ref() {
        lines.push(Line::from(format!(
            "{} · preview limit: 256 KiB{}",
            bounded_display_text(&preview.path, 120),
            if preview.truncated {
                " · truncated"
            } else {
                ""
            }
        )));
        if preview.binary {
            lines.push(Line::from("Binary content is not rendered as text."));
        } else {
            lines.extend(preview.text.lines().map(Line::from));
        }
    } else if let Some(message) = state.file_message.as_deref() {
        lines.push(Line::from(bounded_display_text(message, 160)));
    }
    lines.push(Line::from(
        if state.file_edit_capability == FileEditCapability::Enabled {
            "↑/↓, PgUp/PgDn scroll · e edit · Esc return"
        } else {
            "↑/↓, PgUp/PgDn scroll · Esc return"
        },
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.file_preview_scroll, 0))
            .block(
                Block::default()
                    .title(" Rover file preview · bounded ")
                    .borders(Borders::ALL),
            ),
        popup,
    );
}

#[cfg(unix)]
fn render_file_editor(frame: &mut Frame<'_>, state: &AttachedInputState) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(94, 92, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(edit) = state.file_edit.as_ref() {
        lines.push(Line::from(format!(
            "{} · {} bytes · {}",
            bounded_display_text(&edit.path, 100),
            edit.buffer.len(),
            if edit.buffer == edit.original {
                "clean"
            } else {
                "modified"
            }
        )));
        lines.extend(file_editor_visible_lines(edit));
        if edit.discard_confirmation {
            lines.push(Line::from(
                "Discard unsaved changes? y discard · n/Esc keep editing",
            ));
        } else if let Some(message) = state.file_message.as_deref() {
            lines.push(Line::from(bounded_display_text(message, 160)));
        }
    }
    lines.push(Line::from(
        "Ctrl-S save · Ctrl-D diff · Ctrl-E external editor · Esc return",
    ));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Rover inline editor · 8 MiB limit ")
                .borders(Borders::ALL),
        ),
        popup,
    );
}

#[cfg(unix)]
fn file_editor_visible_lines(edit: &FileEditState) -> Vec<Line<'static>> {
    const CONTEXT_LINES: usize = 40;
    const MAX_RENDERED_LINES: usize = 120;
    const MAX_RENDERED_LINE_CHARS: usize = 2048;
    let source_lines = edit.buffer.split('\n').collect::<Vec<_>>();
    let cursor_line = edit.buffer[..edit.cursor]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();
    let start = cursor_line.saturating_sub(CONTEXT_LINES);
    let end = source_lines
        .len()
        .min(start.saturating_add(MAX_RENDERED_LINES));
    let mut visible = Vec::with_capacity(end.saturating_sub(start).saturating_add(2));
    if start > 0 {
        visible.push(Line::from(
            "… earlier lines hidden · move cursor up to view …",
        ));
    }
    for (index, source_line) in source_lines.iter().enumerate().take(end).skip(start) {
        let source_line = source_line.trim_end_matches('\r');
        let mut display = source_line.to_owned();
        if index == cursor_line {
            let column = edit.buffer[current_line_start(&edit.buffer, edit.cursor)..edit.cursor]
                .chars()
                .count()
                .min(display.chars().count());
            let byte_position = display
                .char_indices()
                .nth(column)
                .map_or(display.len(), |(byte_position, _)| byte_position);
            display.insert(byte_position, '▏');
        }
        visible.push(Line::from(bounded_display_text(
            &sanitize_screen_text(&display),
            MAX_RENDERED_LINE_CHARS,
        )));
    }
    if end < source_lines.len() {
        visible.push(Line::from(
            "… later lines hidden · move cursor down to view …",
        ));
    }
    visible
}

#[cfg(unix)]
fn render_file_diff(frame: &mut Frame<'_>, state: &AttachedInputState) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = centered_rect(92, 90, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(edit) = state.file_edit.as_ref() {
        lines.extend(file_diff_lines(&edit.path, &edit.original, &edit.buffer));
    }
    lines.push(Line::from("↑/↓, PgUp/PgDn scroll · Esc return to editor"));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.file_diff_scroll, 0))
            .block(
                Block::default()
                    .title(" Rover unsaved file diff ")
                    .borders(Borders::ALL),
            ),
        popup,
    );
}

#[cfg(unix)]
fn file_diff_lines(path: &str, original: &str, edited: &str) -> Vec<Line<'static>> {
    const MAX_DIFF_LINES: usize = 512;
    const CONTEXT_LINES: usize = 3;
    let old = original.split_inclusive('\n').collect::<Vec<_>>();
    let new = edited.split_inclusive('\n').collect::<Vec<_>>();
    let safe_path = bounded_display_text(&sanitize_screen_text(path).replace('\n', "�"), 120);
    let mut output = vec![
        Line::from(format!("--- a/{safe_path}")),
        Line::from(format!("+++ b/{safe_path}")),
    ];
    if original == edited {
        output.push(Line::from("No content differences."));
        return output;
    }

    let Some(operations) = bounded_line_diff(&old, &new) else {
        output.push(Line::from(
            "Diff preview unavailable: comparison exceeds its bounded line-work budget.",
        ));
        return output;
    };
    let mut old_before = vec![0_usize; operations.len() + 1];
    let mut new_before = vec![0_usize; operations.len() + 1];
    for (index, operation) in operations.iter().enumerate() {
        old_before[index + 1] = old_before[index] + usize::from(operation.consumes_old());
        new_before[index + 1] = new_before[index] + usize::from(operation.consumes_new());
    }

    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        if operation.is_equal() {
            continue;
        }
        let start = index.saturating_sub(CONTEXT_LINES);
        let end = index
            .saturating_add(CONTEXT_LINES + 1)
            .min(operations.len());
        if let Some((_, previous_end)) = hunks.last_mut() {
            if start <= *previous_end {
                *previous_end = (*previous_end).max(end);
                continue;
            }
        }
        hunks.push((start, end));
    }

    let mut shown_old = 0_usize;
    let mut shown_new = 0_usize;
    for (hunk_index, (start, end)) in hunks.iter().copied().enumerate() {
        if hunk_index >= 64 {
            output.push(Line::from("… diff preview truncated after 64 hunks …"));
            break;
        }
        let hunk_old = old_before[end] - old_before[start];
        let hunk_new = new_before[end] - new_before[start];
        let changed_old = operations[start..end]
            .iter()
            .filter(|operation| matches!(operation, LineDiffOperation::Removed(_)))
            .count();
        let changed_new = operations[start..end]
            .iter()
            .filter(|operation| matches!(operation, LineDiffOperation::Added(_)))
            .count();
        if shown_old.saturating_add(changed_old) > MAX_DIFF_LINES
            || shown_new.saturating_add(changed_new) > MAX_DIFF_LINES
        {
            output.push(Line::from(
                "… diff preview truncated at 512 changed lines per side …",
            ));
            break;
        }
        shown_old += changed_old;
        shown_new += changed_new;
        let old_start = if hunk_old == 0 {
            old_before[start]
        } else {
            old_before[start] + 1
        };
        let new_start = if hunk_new == 0 {
            new_before[start]
        } else {
            new_before[start] + 1
        };
        output.push(Line::from(format!(
            "@@ -{old_start},{hunk_old} +{new_start},{hunk_new} @@"
        )));
        for operation in &operations[start..end] {
            match operation {
                LineDiffOperation::Equal(line) => push_file_diff_line(&mut output, ' ', line),
                LineDiffOperation::Removed(line) => push_file_diff_line(&mut output, '-', line),
                LineDiffOperation::Added(line) => push_file_diff_line(&mut output, '+', line),
            }
        }
    }
    output
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum LineDiffOperation<'a> {
    Equal(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

#[cfg(unix)]
impl LineDiffOperation<'_> {
    fn consumes_old(self) -> bool {
        !matches!(self, Self::Added(_))
    }

    fn consumes_new(self) -> bool {
        !matches!(self, Self::Removed(_))
    }

    fn is_equal(self) -> bool {
        matches!(self, Self::Equal(_))
    }
}

#[cfg(unix)]
fn bounded_line_diff<'a>(old: &[&'a str], new: &[&'a str]) -> Option<Vec<LineDiffOperation<'a>>> {
    const MAX_LINES_PER_SIDE: usize = 10_000;
    const MAX_LCS_CELLS: usize = 1_000_000;
    let rows = old.len().checked_add(1)?;
    let columns = new.len().checked_add(1)?;
    if old.len() > MAX_LINES_PER_SIDE
        || new.len() > MAX_LINES_PER_SIDE
        || rows.checked_mul(columns)? > MAX_LCS_CELLS
    {
        return None;
    }
    let mut lcs = vec![0_u32; rows * columns];
    for old_index in 1..rows {
        for new_index in 1..columns {
            let index = old_index * columns + new_index;
            lcs[index] = if old[old_index - 1] == new[new_index - 1] {
                lcs[(old_index - 1) * columns + new_index - 1] + 1
            } else {
                lcs[(old_index - 1) * columns + new_index]
                    .max(lcs[old_index * columns + new_index - 1])
            };
        }
    }

    let mut operations = Vec::with_capacity(old.len().saturating_add(new.len()));
    let (mut old_index, mut new_index) = (old.len(), new.len());
    while old_index > 0 || new_index > 0 {
        if old_index > 0 && new_index > 0 && old[old_index - 1] == new[new_index - 1] {
            operations.push(LineDiffOperation::Equal(old[old_index - 1]));
            old_index -= 1;
            new_index -= 1;
        } else if old_index > 0
            && (new_index == 0
                || lcs[(old_index - 1) * columns + new_index]
                    >= lcs[old_index * columns + new_index - 1])
        {
            operations.push(LineDiffOperation::Removed(old[old_index - 1]));
            old_index -= 1;
        } else {
            operations.push(LineDiffOperation::Added(new[new_index - 1]));
            new_index -= 1;
        }
    }
    operations.reverse();
    Some(operations)
}

#[cfg(unix)]
fn push_file_diff_line(output: &mut Vec<Line<'static>>, prefix: char, line: &str) {
    const MAX_DIFF_LINE_CHARS: usize = 2048;
    let content = line.trim_end_matches('\n').trim_end_matches('\r');
    output.push(Line::from(format!(
        "{prefix}{}",
        bounded_display_text(&sanitize_screen_text(content), MAX_DIFF_LINE_CHARS)
    )));
    if !line.ends_with('\n') {
        output.push(Line::from("\\ No newline at end of file"));
    }
}

#[cfg(unix)]
fn git_status_label(status: Option<BrowserGitStatus>) -> (&'static str, Color) {
    match status {
        Some(BrowserGitStatus::Added) => ("[A] ", Color::Green),
        Some(BrowserGitStatus::Modified) => ("[M] ", Color::Yellow),
        Some(BrowserGitStatus::Deleted) => ("[D] ", Color::Red),
        Some(BrowserGitStatus::Renamed) => ("[R] ", Color::Cyan),
        Some(BrowserGitStatus::Untracked) => ("[?] ", Color::Green),
        Some(BrowserGitStatus::Conflict) => ("[!] ", Color::Magenta),
        None => ("    ", Color::Reset),
    }
}

#[cfg(unix)]
fn open_selected_task_log(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
    key: KeyEvent,
) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    let stdout = match key.code {
        KeyCode::Char('o') => true,
        KeyCode::Char('e') => false,
        _ => return false,
    };
    let digest = provider
        .tasks
        .get(state.selected_task)
        .and_then(|task| task.process.as_ref())
        .map(|process| {
            if stdout {
                &process.stdout_sha256
            } else {
                &process.stderr_sha256
            }
        });
    state.task_log = if let Some(digest) = digest {
        match (provider.load_blob)(digest).and_then(|bytes| {
            prepare_task_log(&bytes, digest)
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))
        }) {
            Ok((text, true)) => {
                state.task_log_verification = TaskLogVerification::Verified;
                text
            }
            Ok((text, false)) => {
                state.task_log_verification = TaskLogVerification::Unavailable;
                text
            }
            Err(error) => {
                state.task_log_verification = TaskLogVerification::Unavailable;
                bounded_display_text(&format!("Unable to load task output: {error}"), 256)
            }
        }
    } else {
        state.task_log_verification = TaskLogVerification::Unavailable;
        "No process output is recorded for this task.".to_owned()
    };
    state.task_log_scroll = 0;
    state.task_panel = if stdout {
        TaskPanelState::Stdout
    } else {
        TaskPanelState::Stderr
    };
    true
}

#[cfg(unix)]
fn refresh_task_snapshot(
    tasks: &mut Vec<TaskSummary>,
    selected: &mut usize,
    message: &mut Option<String>,
    refresh: &mut impl FnMut() -> io::Result<Vec<TaskSummary>>,
) {
    let selected_id = tasks.get(*selected).map(|task| task.id.as_str());
    match refresh() {
        Ok(refreshed) if refreshed.len() <= 1_000 => {
            *selected = selected_id
                .and_then(|id| refreshed.iter().position(|task| task.id == id))
                .unwrap_or_else(|| (*selected).min(refreshed.len().saturating_sub(1)));
            let count = refreshed.len();
            *tasks = refreshed;
            *message = Some(format!("Refreshed {count} project tasks."));
        }
        Ok(_) => {
            *message = Some("Refresh refused: more than 1,000 project tasks.".to_owned());
        }
        Err(error) => {
            *message = Some(bounded_display_text(
                &format!("Refresh failed: {error}"),
                96,
            ));
        }
    }
}

#[cfg(unix)]
fn refresh_task_snapshot_if_requested(
    state: &mut AttachedInputState,
    provider: &mut TaskProvider<'_>,
) {
    if state.refresh_request != RefreshRequest::Requested {
        return;
    }
    state.refresh_request = RefreshRequest::Idle;
    refresh_task_snapshot(
        &mut provider.tasks,
        &mut state.selected_task,
        &mut state.task_refresh_message,
        &mut provider.refresh,
    );
    let visible = task_visible_indices(&provider.tasks, &provider.view_preferences);
    if !visible.contains(&state.selected_task) {
        state.selected_task = visible.first().copied().unwrap_or(0);
    }
}

#[cfg(unix)]
fn pane_terminal_size(
    workspace: &Workspace,
    area: Rect,
    pane_id: &str,
) -> io::Result<TerminalSize> {
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area)[1];
    let pane_rect =
        find_pane_rect(workspace.active_tab().render_root(), body, pane_id).unwrap_or(body);
    let columns = usize::from(pane_rect.width.saturating_sub(2).clamp(2, 1000));
    let rows = usize::from(pane_rect.height.saturating_sub(2).clamp(1, 1000));
    let rows = rows.min(250_000 / columns);
    TerminalSize::new(rows.max(1), columns)
}

#[cfg(unix)]
fn send_resize(writer: &mut SessionWriter, size: TerminalSize) -> io::Result<()> {
    let rows = u16::try_from(size.rows).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "terminal rows exceed protocol")
    })?;
    let columns = u16::try_from(size.columns).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal columns exceed protocol",
        )
    })?;
    writer.resize(rows, columns)
}

#[cfg(unix)]
fn find_pane_rect(node: &PaneNode, area: Rect, pane_id: &str) -> Option<Rect> {
    match node {
        PaneNode::Leaf(pane) => (pane.id() == pane_id).then_some(area),
        PaneNode::Split(split) => {
            let direction = match split.axis() {
                SplitAxis::Horizontal => Direction::Horizontal,
                SplitAxis::Vertical => Direction::Vertical,
            };
            let ratio = u32::from(split.first_basis_points());
            let children = Layout::default()
                .direction(direction)
                .constraints([
                    Constraint::Ratio(ratio, 10_000),
                    Constraint::Ratio(10_000 - ratio, 10_000),
                ])
                .split(area);
            find_pane_rect(split.first(), children[0], pane_id)
                .or_else(|| find_pane_rect(split.second(), children[1], pane_id))
        }
    }
}

#[cfg(unix)]
fn is_detach_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char(']') && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(unix)]
fn attached_workspace_key(workspace: &mut Workspace, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => handle_key(workspace, key),
    }
}

#[cfg(unix)]
fn attached_task_board_key(
    panel: &mut TaskPanelState,
    prefix: &mut bool,
    selected: &mut usize,
    tasks: &[TaskSummary],
    preferences: &TaskViewPreferences,
    key: KeyEvent,
) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    let visible = task_visible_indices(tasks, preferences);
    let selected_position = visible.iter().position(|index| index == selected);
    match *panel {
        TaskPanelState::Detail => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => *panel = TaskPanelState::List,
                KeyCode::Up if !visible.is_empty() => {
                    let position = selected_position.unwrap_or(0);
                    *selected = if position == 0 {
                        *visible.last().unwrap_or(&0)
                    } else {
                        visible[position - 1]
                    };
                }
                KeyCode::Down if !visible.is_empty() => {
                    let position = selected_position.unwrap_or(visible.len() - 1);
                    *selected = visible[(position + 1) % visible.len()];
                }
                _ => {}
            }
            true
        }
        TaskPanelState::List => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('t') => *panel = TaskPanelState::Closed,
                KeyCode::Enter if !visible.is_empty() => {
                    if selected_position.is_none() {
                        *selected = visible[0];
                    }
                    *panel = TaskPanelState::Detail;
                }
                KeyCode::Up if !visible.is_empty() => {
                    let position = selected_position.unwrap_or(0);
                    *selected = if position == 0 {
                        *visible.last().unwrap_or(&0)
                    } else {
                        visible[position - 1]
                    };
                }
                KeyCode::Down if !visible.is_empty() => {
                    let position = selected_position.unwrap_or(visible.len() - 1);
                    *selected = visible[(position + 1) % visible.len()];
                }
                _ => {}
            }
            true
        }
        TaskPanelState::Closed => {
            if *prefix && key.code == KeyCode::Char('t') {
                *prefix = false;
                *panel = TaskPanelState::List;
                *selected = visible.first().copied().unwrap_or(0);
                return true;
            }
            false
        }
        TaskPanelState::Stdout
        | TaskPanelState::Stderr
        | TaskPanelState::Evidence
        | TaskPanelState::Diff => false,
    }
}

#[cfg(unix)]
fn attached_task_log_key(panel: &mut TaskPanelState, scroll: &mut u16, key: KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return false;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Enter => *panel = TaskPanelState::Detail,
        KeyCode::Up => *scroll = scroll.saturating_sub(1),
        KeyCode::Down => *scroll = scroll.saturating_add(1),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(12),
        KeyCode::PageDown => *scroll = scroll.saturating_add(12),
        _ => {}
    }
    true
}

#[cfg(unix)]
fn key_input_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let mut bytes = match key.code {
        KeyCode::Char(character) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let value = match character.to_ascii_lowercase() {
                    ' ' | '@' => 0,
                    'a'..='z' => character.to_ascii_lowercase() as u8 - b'a' + 1,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '^' => 0x1e,
                    '_' => 0x1f,
                    _ => return None,
                };
                vec![value]
            } else {
                let mut encoded = [0; 4];
                character.encode_utf8(&mut encoded).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(number @ 5..=12) => format!("\x1b[{}~", 15 + (number - 5) * 3).into_bytes(),
        _ => return None,
    };
    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

#[cfg(unix)]
fn find_pane_ref<'a>(node: &'a PaneNode, pane_id: &str) -> Option<&'a Pane> {
    match node {
        PaneNode::Leaf(pane) if pane.id() == pane_id => Some(pane),
        PaneNode::Leaf(_) => None,
        PaneNode::Split(split) => {
            find_pane_ref(split.first(), pane_id).or_else(|| find_pane_ref(split.second(), pane_id))
        }
    }
}

#[cfg(unix)]
struct SuspendedTerminal {
    active: bool,
}

#[cfg(unix)]
impl SuspendedTerminal {
    fn enter() -> io::Result<Self> {
        disable_raw_mode()?;
        let guard = Self { active: true };
        execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        )?;
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste
        )?;
        enable_raw_mode()?;
        self.active = false;
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for SuspendedTerminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct TerminalGuard<C: TerminalControl> {
    control: C,
}

trait TerminalControl {
    fn enable_raw(&mut self) -> io::Result<()>;
    fn enter_screen(&mut self) -> io::Result<()>;
    fn restore(&mut self);
}

impl TerminalGuard<CrosstermControl> {
    fn enter() -> io::Result<Self> {
        Self::enter_with(CrosstermControl::new())
    }
}

impl<C: TerminalControl> TerminalGuard<C> {
    fn enter_with(mut control: C) -> io::Result<Self> {
        control.enable_raw()?;
        let mut guard = Self { control };
        guard.control.enter_screen()?;
        Ok(guard)
    }
}

impl TerminalControl for CrosstermControl {
    fn enable_raw(&mut self) -> io::Result<()> {
        enable_raw_mode()
    }

    fn enter_screen(&mut self) -> io::Result<()> {
        execute!(
            &mut self.stdout,
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste
        )?;
        Ok(())
    }

    fn restore(&mut self) {
        let _ = execute!(
            &mut self.stdout,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
        let _ = disable_raw_mode();
    }
}

struct CrosstermControl {
    stdout: io::Stdout,
}

impl CrosstermControl {
    fn new() -> Self {
        Self {
            stdout: io::stdout(),
        }
    }
}

impl<C: TerminalControl> Drop for TerminalGuard<C> {
    fn drop(&mut self) {
        self.control.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    #[cfg(unix)]
    use std::{fs, os::unix::fs::PermissionsExt};

    fn workspace() -> Workspace {
        Workspace::new("project", "Rover 项目", "main", PathBuf::from("/repo"))
            .expect("valid fixture workspace")
    }

    #[cfg(unix)]
    #[test]
    fn terminal_bell_is_suppressed_for_the_focused_pane_only() {
        let mut workspace = workspace();
        let first = workspace.active_tab().active_pane().to_owned();
        assert!(!pane_is_inactive(&workspace, &first));

        let second = workspace
            .split_active(
                SplitAxis::Horizontal,
                5_000,
                rover_execution::layout::PaneSpec::scratch(),
            )
            .unwrap();
        assert!(pane_is_inactive(&workspace, &first));
        assert!(!pane_is_inactive(&workspace, &second));

        workspace.focus_pane(&first).unwrap();
        assert!(!pane_is_inactive(&workspace, &first));
        assert!(pane_is_inactive(&workspace, &second));
    }

    #[cfg(unix)]
    fn private_state_dir() -> (PathBuf, SafeDir) {
        let path = std::env::temp_dir().join(format!(
            "rover-tui-layout-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = SafeDir::open(&path).unwrap();
        (path, directory)
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[cfg(unix)]
    fn test_agent_binding() -> AgentSessionBinding {
        let path = std::env::current_dir().unwrap();
        let repository = path.to_str().unwrap();
        let identity = rover_core::RepositoryIdentity::resolve(repository).unwrap();
        AgentSessionBinding::new(
            &identity,
            "pane-1",
            "codex-exec",
            "session-quoted",
            rover_agents::AgentSessionSource::ExplicitUser,
        )
        .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn agent_resume_shell_line_quotes_exact_arguments_and_rejects_controls() {
        let line = agent_resume_shell_line(&[
            "codex".to_owned(),
            "resume".to_owned(),
            "id with ' quote".to_owned(),
        ])
        .unwrap();
        assert_eq!(
            String::from_utf8(line).unwrap(),
            "'codex' 'resume' 'id with '\\'' quote'\n"
        );
        assert!(agent_resume_shell_line(&["codex".to_owned(), "id\nnext".to_owned()]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn agent_session_requires_confirmation_to_resume_or_clear_binding() {
        let sent = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let sent_by_provider = Arc::clone(&sent);
        let cleared = Arc::new(Mutex::new(Vec::<String>::new()));
        let cleared_by_provider = Arc::clone(&cleared);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Ok(Vec::new()),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider
            .configure_agent_session(&test_agent_binding(), move |pane| {
                cleared_by_provider.lock().unwrap().push(pane.to_owned());
                Ok(())
            })
            .unwrap();
        let mut state = AttachedInputState {
            agent_session_panel: AgentSessionPanel::ConfirmResume,
            ..AttachedInputState::default()
        };
        handle_agent_session_key(
            &mut state,
            &mut provider,
            press(KeyCode::Char('n')),
            |bytes| {
                sent_by_provider.lock().unwrap().push(bytes.to_vec());
                Ok(())
            },
        )
        .unwrap();
        assert!(sent.lock().unwrap().is_empty());
        assert_eq!(state.agent_session_panel, AgentSessionPanel::Details);

        state.agent_session_panel = AgentSessionPanel::ConfirmResume;
        handle_agent_session_key(
            &mut state,
            &mut provider,
            press(KeyCode::Char('y')),
            |bytes| {
                sent.lock().unwrap().push(bytes.to_vec());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(state.agent_session_panel, AgentSessionPanel::Closed);
        assert_eq!(sent.lock().unwrap().len(), 1);

        state.agent_session_panel = AgentSessionPanel::ConfirmClear;
        handle_agent_session_key(&mut state, &mut provider, press(KeyCode::Char('y')), |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(cleared.lock().unwrap().as_slice(), ["pane-1"]);
        assert!(provider.agent_session.is_none());
        assert!(!provider
            .capabilities
            .supports(TaskCapabilities::AGENT_SESSION));
    }

    fn task_summary(id: &str) -> TaskSummary {
        TaskSummary {
            id: id.to_owned(),
            objective: format!("objective for {id}"),
            status: "RUNNING".to_owned(),
            updated_at: "2026-09-26T00:00:00Z".to_owned(),
            error: None,
            candidate: None,
            base_snapshot: None,
            investigation_id: None,
            attempt_count: 0,
            process: None,
        }
    }

    fn investigation_evidence() -> TaskInvestigationEvidence {
        TaskInvestigationEvidence {
            id: "inv_task_a".to_owned(),
            candidate: "snapshot_a".to_owned(),
            decision: "BLOCKED".to_owned(),
            decision_reason: "review is pending".to_owned(),
            checks: vec![TaskCheckEvidence {
                id: "unit".to_owned(),
                outcome: "PASS".to_owned(),
                meaning: "recorded tests passed".to_owned(),
                required: true,
                tests: 4,
                skipped: 0,
            }],
            unknowns: vec!["Unknown security properties remain.".to_owned()],
            findings: vec![TaskEvidenceFinding {
                path: "src/main.rs".to_owned(),
                message: "review this recorded finding".to_owned(),
                severity: Some("warning".to_owned()),
            }],
        }
    }

    fn task_diff() -> TaskDiffEvidence {
        TaskDiffEvidence {
            base_snapshot: "snap_base".to_owned(),
            candidate: "snap_candidate".to_owned(),
            changes: vec![TaskDiffChange {
                path: "src/main.rs".to_owned(),
                status: "modified".to_owned(),
                category: "source".to_owned(),
            }],
        }
    }

    #[cfg(unix)]
    #[test]
    fn task_refresh_preserves_selection_by_id_across_reordering() {
        let mut tasks = vec![task_summary("task_a"), task_summary("task_b")];
        let mut selected = 1;
        let mut message = None;
        refresh_task_snapshot(&mut tasks, &mut selected, &mut message, &mut || {
            Ok(vec![
                task_summary("task_b"),
                task_summary("task_c"),
                task_summary("task_a"),
            ])
        });
        assert_eq!(tasks[0].id, "task_b");
        assert_eq!(selected, 0);
        assert_eq!(message.as_deref(), Some("Refreshed 3 project tasks."));
    }

    #[cfg(unix)]
    #[test]
    fn task_board_search_sort_and_persisted_selection_are_deterministic() {
        let mut alpha = task_summary("task_alpha");
        alpha.status = "WAITING".to_owned();
        alpha.updated_at = "2026-09-24T00:00:00Z".to_owned();
        alpha.candidate = Some("candidate-17".to_owned());
        let mut beta = task_summary("task_beta");
        beta.status = "RUNNING".to_owned();
        beta.objective = "Build the release".to_owned();
        beta.updated_at = "2026-09-26T00:00:00Z".to_owned();
        let tasks = vec![alpha, beta];

        let updated = task_visible_indices(&tasks, &TaskViewPreferences::default());
        assert_eq!(updated, [1, 0]);
        let by_status = task_visible_indices(
            &tasks,
            &TaskViewPreferences {
                query: String::new(),
                sort: TaskSortOrder::Status,
            },
        );
        assert_eq!(by_status, [1, 0]);
        let filtered = task_visible_indices(
            &tasks,
            &TaskViewPreferences {
                query: "CANDIDATE-17".to_owned(),
                sort: TaskSortOrder::Objective,
            },
        );
        assert_eq!(filtered, [0]);

        let saved = Arc::new(Mutex::new(Vec::new()));
        let saved_preferences = Arc::clone(&saved);
        let mut provider = TaskProvider::new(
            tasks,
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_view_preferences(&TaskViewPreferences::default(), move |preferences| {
            saved_preferences.lock().unwrap().push(preferences.clone());
            Ok(())
        });
        let mut state = AttachedInputState {
            task_search_draft: Some("candidate-17".to_owned()),
            ..AttachedInputState::default()
        };
        handle_task_search_edit(&mut state, &mut provider, press(KeyCode::Enter));
        assert_eq!(provider.view_preferences.query, "candidate-17");
        assert_eq!(state.selected_task, 0);
        assert!(state.task_search_draft.is_none());
        assert_eq!(
            saved.lock().unwrap().as_slice(),
            [provider.view_preferences.clone()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn task_search_enforces_utf8_byte_limit_and_escape_discards_draft() {
        let saved = Arc::new(Mutex::new(0));
        let saved_preferences = Arc::clone(&saved);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_view_preferences(&TaskViewPreferences::default(), move |_| {
            *saved_preferences.lock().unwrap() += 1;
            Ok(())
        });
        let mut state = AttachedInputState {
            task_search_draft: Some("é".repeat(MAX_TASK_FILTER_BYTES / 2)),
            ..AttachedInputState::default()
        };
        handle_task_search_edit(&mut state, &mut provider, press(KeyCode::Char('a')));
        assert_eq!(
            state.task_search_draft.as_deref().unwrap().len(),
            MAX_TASK_FILTER_BYTES
        );
        handle_task_search_edit(&mut state, &mut provider, press(KeyCode::Esc));
        assert!(state.task_search_draft.is_none());
        assert!(provider.view_preferences.query.is_empty());
        assert_eq!(*saved.lock().unwrap(), 0);
    }

    #[cfg(unix)]
    fn test_file_browser_provider() -> TaskProvider<'static> {
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_file_browser(
            |directory, include_hidden| {
                let entries = if directory == "src" {
                    vec![BrowserEntry {
                        path: "src/main.rs".to_owned(),
                        kind: BrowserEntryKind::File,
                        git_status: Some(BrowserGitStatus::Modified),
                    }]
                } else {
                    vec![
                        BrowserEntry {
                            path: "src".to_owned(),
                            kind: BrowserEntryKind::Directory,
                            git_status: None,
                        },
                        BrowserEntry {
                            path: "visible.txt".to_owned(),
                            kind: BrowserEntryKind::File,
                            git_status: Some(BrowserGitStatus::Added),
                        },
                        BrowserEntry {
                            path: "link".to_owned(),
                            kind: BrowserEntryKind::Symlink,
                            git_status: None,
                        },
                    ]
                };
                Ok(BrowserListing {
                    entries,
                    truncated: false,
                    git_error: if include_hidden {
                        None
                    } else {
                        Some("fixture warning".to_owned())
                    },
                })
            },
            || {
                Ok(BrowserListing {
                    entries: vec![BrowserEntry {
                        path: ".hidden/config.toml".to_owned(),
                        kind: BrowserEntryKind::File,
                        git_status: Some(BrowserGitStatus::Untracked),
                    }],
                    truncated: false,
                    git_error: None,
                })
            },
            |path| {
                Ok((
                    format!("preview for {path}\u{001b}[31m\nline two").into_bytes(),
                    false,
                ))
            },
        );
        provider.configure_file_editor(
            |_| Ok(b"fn main() {}\n".to_vec()),
            |_, _, _| Ok(()),
            |_, bytes| Ok(bytes.to_vec()),
        );
        provider
    }

    #[cfg(unix)]
    #[test]
    fn file_tree_navigates_and_previews_sanitized_text() {
        let mut provider = test_file_browser_provider();
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        assert!(open_file_browser_shortcut(
            &mut state,
            &mut provider,
            press(KeyCode::Char('f'))
        ));
        assert_eq!(state.file_panel, FilePanelState::Tree);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| render_file_tree(frame, &state))
            .unwrap();
        let rendered = buffer_text(terminal.backend().buffer());
        assert!(rendered.contains("[A]"));
        assert!(rendered.contains("Git status unavailable"));

        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Enter));
        assert_eq!(state.file_directory, "src");
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Enter));
        assert_eq!(state.file_panel, FilePanelState::Viewer);
        let preview = state.file_preview.as_ref().unwrap();
        assert_eq!(preview.path, "src/main.rs");
        assert!(!preview.text.contains('\u{001b}'));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(state.file_panel, FilePanelState::Tree);
    }

    #[cfg(unix)]
    #[test]
    fn quick_open_searches_hidden_files_and_returns_from_preview() {
        let mut provider = test_file_browser_provider();
        let mut state = AttachedInputState {
            file_panel: FilePanelState::Tree,
            ..AttachedInputState::default()
        };
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('.')));
        assert_eq!(state.file_panel, FilePanelState::QuickOpen);
        for character in "hidden".chars() {
            handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Enter));
        assert_eq!(state.file_panel, FilePanelState::Viewer);
        assert_eq!(
            state.file_preview.as_ref().unwrap().path,
            ".hidden/config.toml"
        );
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(state.file_panel, FilePanelState::QuickOpen);
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(state.file_panel, FilePanelState::Tree);
    }

    #[cfg(unix)]
    #[test]
    fn command_palette_filters_available_actions_and_runs_file_commands() {
        let mut provider = test_file_browser_provider();
        provider.configure_workspace_notes(&[], |_| Ok(()));
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        assert!(open_command_palette_shortcut(
            &mut state,
            &provider,
            press(KeyCode::Char('p'))
        ));
        assert_eq!(state.command_palette_query.as_deref(), Some(""));
        let mut workspace = workspace();
        for character in "quick".chars() {
            handle_command_palette_key(
                &mut workspace,
                &mut state,
                &mut provider,
                press(KeyCode::Char(character)),
                &mut |_| Ok(()),
            )
            .unwrap();
        }
        assert_eq!(
            palette_actions(provider.capabilities, "quick"),
            [PaletteAction::QuickOpen]
        );
        handle_command_palette_key(
            &mut workspace,
            &mut state,
            &mut provider,
            press(KeyCode::Enter),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(state.file_panel, FilePanelState::QuickOpen);
        assert!(state.command_palette_query.is_none());
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));

        state.prefix = true;
        open_command_palette_shortcut(&mut state, &provider, press(KeyCode::Char('p')));
        for character in "tree".chars() {
            handle_command_palette_key(
                &mut workspace,
                &mut state,
                &mut provider,
                press(KeyCode::Char(character)),
                &mut |_| Ok(()),
            )
            .unwrap();
        }
        handle_command_palette_key(
            &mut workspace,
            &mut state,
            &mut provider,
            press(KeyCode::Enter),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(state.file_panel, FilePanelState::Tree);
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));

        state.prefix = true;
        open_command_palette_shortcut(&mut state, &provider, press(KeyCode::Char('p')));
        for character in "notes".chars() {
            handle_command_palette_key(
                &mut workspace,
                &mut state,
                &mut provider,
                press(KeyCode::Char(character)),
                &mut |_| Ok(()),
            )
            .unwrap();
        }
        assert_eq!(
            palette_actions(provider.capabilities, "notes"),
            [PaletteAction::OpenNotes]
        );
        handle_command_palette_key(
            &mut workspace,
            &mut state,
            &mut provider,
            press(KeyCode::Enter),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(state.workspace_notes_panel, WorkspaceNotesPanel::List);
        assert!(state.workspace_notes_draft.is_none());

        assert!(palette_actions(TaskCapabilities::read_only(false), "file").is_empty());
        assert!(palette_actions(TaskCapabilities::read_only(false), "notes").is_empty());
        assert!(palette_actions(TaskCapabilities::read_only(false), "shell").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn command_palette_opens_saved_commands_when_supported() {
        let mut provider = test_file_browser_provider();
        provider.configure_saved_commands(&[], |_| Ok(()));
        provider.configure_task_drafts(&[], |_| Ok(()));
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        let mut workspace = workspace();
        assert!(open_command_palette_shortcut(
            &mut state,
            &provider,
            press(KeyCode::Char('p'))
        ));
        for character in "shell".chars() {
            handle_command_palette_key(
                &mut workspace,
                &mut state,
                &mut provider,
                press(KeyCode::Char(character)),
                &mut |_| Ok(()),
            )
            .unwrap();
        }
        assert_eq!(
            palette_actions(provider.capabilities, "shell"),
            [PaletteAction::OpenSavedCommands]
        );
        handle_command_palette_key(
            &mut workspace,
            &mut state,
            &mut provider,
            press(KeyCode::Enter),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(state.saved_commands_panel, SavedCommandsPanel::List);
        assert!(palette_actions(TaskCapabilities::read_only(false), "shell").is_empty());
        assert_eq!(
            palette_actions(provider.capabilities, "draft"),
            [PaletteAction::OpenTaskDrafts]
        );
        assert!(palette_actions(TaskCapabilities::read_only(false), "draft").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn saved_command_runs_only_after_confirmation_and_sends_one_line() {
        let saved = Arc::new(Mutex::new(Vec::<Vec<SavedCommand>>::new()));
        let saved_copy = Arc::clone(&saved);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_saved_commands(&[], move |commands| {
            saved_copy.lock().unwrap().push(commands.to_vec());
            Ok(())
        });
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        assert!(open_saved_commands_shortcut(
            &mut state,
            &provider,
            press(KeyCode::Char('k'))
        ));
        handle_saved_commands_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        for character in "Check".chars() {
            handle_saved_commands_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_saved_commands_key(&mut state, &mut provider, press(KeyCode::Tab));
        for character in "cargo test".chars() {
            handle_saved_commands_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_saved_commands_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            provider.saved_commands,
            [SavedCommand {
                name: "Check".to_owned(),
                command: "cargo test".to_owned(),
            }]
        );
        assert_eq!(saved.lock().unwrap().len(), 1);

        handle_saved_commands_key(&mut state, &mut provider, press(KeyCode::Char('r')));
        assert_eq!(state.pending_saved_command, Some(0));
        let sent = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let sent_copy = Arc::clone(&sent);
        handle_saved_command_confirmation(
            &mut state,
            &provider,
            press(KeyCode::Char('n')),
            move |bytes| {
                sent_copy.lock().unwrap().push(bytes.to_vec());
                Ok(())
            },
        );
        assert!(sent.lock().unwrap().is_empty());

        state.pending_saved_command = Some(0);
        let sent_copy = Arc::clone(&sent);
        handle_saved_command_confirmation(
            &mut state,
            &provider,
            press(KeyCode::Char('y')),
            move |bytes| {
                sent_copy.lock().unwrap().push(bytes.to_vec());
                Ok(())
            },
        );
        assert_eq!(sent.lock().unwrap().as_slice(), [b"cargo test\r".to_vec()]);
        assert!(state.pending_saved_command.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn task_briefing_drafts_are_multiline_persisted_and_never_dispatched() {
        let saved = Arc::new(Mutex::new(Vec::<Vec<TaskDraft>>::new()));
        let saved_copy = Arc::clone(&saved);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_task_drafts(&[], move |drafts| {
            saved_copy.lock().unwrap().push(drafts.to_vec());
            Ok(())
        });
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        assert!(open_task_drafts_shortcut(
            &mut state,
            &provider,
            press(KeyCode::Char('d'))
        ));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        for character in "Audit plan".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Tab));
        for character in "src/**".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Tab));
        for character in "task_base".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Tab));
        for character in "cargo test".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Tab));
        for character in "Compare feature paths".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Enter));
        for character in "Keep findings reviewable".chars() {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char(character)));
        }
        handle_task_drafts_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            provider.task_drafts,
            [TaskDraft {
                title: "Audit plan".to_owned(),
                paths: "src/**".to_owned(),
                dependencies: "task_base".to_owned(),
                quality_gate: "cargo test".to_owned(),
                prompt: "Compare feature paths\nKeep findings reviewable".to_owned()
            }]
        );
        assert_eq!(saved.lock().unwrap().len(), 1);
        assert!(provider.tasks.is_empty());

        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Enter));
        for _ in 0..4 {
            handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Tab));
        }
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('!')));
        handle_task_drafts_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert!(provider.task_drafts[0].prompt.ends_with("reviewable!"));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Enter));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('x')));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(state.task_draft_dialog, SavedCommandDialog::ConfirmDiscard);
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert_eq!(provider.task_drafts[0].title, "Audit plan");

        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('d')));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert!(provider.task_drafts.is_empty());
        assert_eq!(saved.lock().unwrap().len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn task_draft_creation_is_confirmed_and_plan_list_is_project_loaded() {
        let created = Arc::new(Mutex::new(Vec::<TaskDraft>::new()));
        let created_copy = Arc::clone(&created);
        let plan = TaskPlanSummary {
            id: "plan_a".into(),
            title: "Drafted work".into(),
            status: "READY".into(),
            readiness: "READY".into(),
            dependency_count: 0,
            attempt_count: 0,
            updated_at: "2026-09-26T00:00:00Z".into(),
        };
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_task_drafts(
            &[TaskDraft {
                title: "Drafted work".into(),
                paths: "src/**".into(),
                dependencies: String::new(),
                quality_gate: "human review".into(),
                prompt: "Implement a feature".into(),
            }],
            |_| Ok(()),
        );
        let plan_copy = plan.clone();
        let created_plan = plan.clone();
        provider.configure_task_plans(
            &[],
            move || Ok(vec![plan_copy.clone()]),
            move |draft| {
                created_copy.lock().unwrap().push(draft.clone());
                Ok(created_plan.clone())
            },
        );
        let mut state = AttachedInputState {
            task_drafts_panel: SavedCommandsPanel::List,
            ..AttachedInputState::default()
        };
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('p')));
        assert_eq!(
            state.task_draft_dialog,
            SavedCommandDialog::ConfirmCreatePlan
        );
        assert!(created.lock().unwrap().is_empty());
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        assert!(created.lock().unwrap().is_empty());
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('p')));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert_eq!(created.lock().unwrap().len(), 1);
        assert_eq!(provider.task_plans.summaries, [plan.clone()]);
        assert!(state
            .task_draft_message
            .as_deref()
            .is_some_and(|message| message.contains("no worker was started")));

        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('l')));
        assert_eq!(state.task_draft_panel, TaskDraftPanel::Plans);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal
            .draw(|frame| render_task_drafts_overlay(frame, &state, &provider))
            .unwrap();
        assert!(buffer_text(terminal.backend().buffer()).contains("READY"));
        handle_task_drafts_key(&mut state, &mut provider, press(KeyCode::Char('r')));
        assert_eq!(provider.task_plans.summaries, [plan]);
    }

    #[cfg(unix)]
    #[test]
    fn repository_notes_are_bounded_saved_and_dirty_close_requires_confirmation() {
        let saved = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let saved_copy = Arc::clone(&saved);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_workspace_notes(&[], move |notes| {
            saved_copy.lock().unwrap().push(notes.to_vec());
            Ok(())
        });
        let mut state = AttachedInputState {
            prefix: true,
            ..AttachedInputState::default()
        };
        assert!(open_workspace_notes_shortcut(
            &mut state,
            &provider,
            press(KeyCode::Char('n'))
        ));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('p')));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Enter));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('x')));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Left));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('r')));
        assert_eq!(state.workspace_notes_draft.as_deref(), Some("p\nrx"));

        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(
            state.workspace_notes_state,
            WorkspaceNotesState::ConfirmDiscard
        );
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        assert!(state.workspace_notes_draft.is_some());

        handle_workspace_notes_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert!(state.workspace_notes_draft.is_none());
        assert_eq!(provider.workspace_notes, ["p\nrx"]);
        assert_eq!(saved.lock().unwrap().as_slice(), [vec!["p\nrx"]]);

        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Enter));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::End));
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('!')));
        handle_workspace_notes_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(provider.workspace_notes, ["p\nrx!"]);

        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('d')));
        assert_eq!(
            state.workspace_notes_state,
            WorkspaceNotesState::ConfirmDelete
        );
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert!(provider.workspace_notes.is_empty());
        assert_eq!(
            saved.lock().unwrap().as_slice(),
            [
                vec!["p\nrx".to_owned()],
                vec!["p\nrx!".to_owned()],
                Vec::<String>::new()
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn repository_notes_enforce_utf8_byte_limit_and_hide_unsupported_palette_action() {
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        let mut state = AttachedInputState {
            workspace_notes_draft: Some("é".repeat(MAX_WORKSPACE_NOTES_BYTES / 2)),
            ..AttachedInputState::default()
        };
        handle_workspace_notes_key(&mut state, &mut provider, press(KeyCode::Char('a')));
        assert_eq!(
            state.workspace_notes_draft.as_deref().unwrap().len(),
            MAX_WORKSPACE_NOTES_BYTES
        );
        assert!(
            !palette_actions(TaskCapabilities::read_only(false), "notes")
                .contains(&PaletteAction::OpenNotes)
        );
    }

    #[cfg(unix)]
    #[test]
    fn workspace_notes_replace_terminal_and_bidi_controls_on_load() {
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Ok(()),
            |_| Ok(None),
            |_| Ok(None),
        );
        provider.configure_workspace_notes(&["safe\u{1b}[2J \u{202e}text".to_owned()], |_| Ok(()));
        assert_eq!(provider.workspace_notes, ["safe�[2J �text"]);
        provider.configure_workspace_notes(
            &[format!(
                "{}\u{1b}",
                "x".repeat(MAX_WORKSPACE_NOTES_BYTES - 1)
            )],
            |_| Ok(()),
        );
        assert!(provider.workspace_notes[0].len() <= MAX_WORKSPACE_NOTES_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn inline_editor_previews_diff_saves_with_digest_and_confirms_discard() {
        let mut provider = test_file_browser_provider();
        let saved = Arc::new(Mutex::new(None));
        let saved_file = Arc::clone(&saved);
        provider.save_file = Box::new(move |path, digest, bytes| {
            *saved_file.lock().unwrap() =
                Some((path.to_owned(), digest.to_owned(), bytes.to_vec()));
            Ok(())
        });
        let mut state = AttachedInputState {
            file_panel: FilePanelState::Viewer,
            file_preview: Some(FilePreviewState {
                path: "src/main.rs".to_owned(),
                text: "fn main() {}\n".to_owned(),
                truncated: false,
                binary: false,
            }),
            file_preview_return: FilePanelState::Tree,
            ..AttachedInputState::default()
        };
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('e')));
        assert_eq!(state.file_panel, FilePanelState::Editing);
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::End));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('!')));
        handle_file_panel_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.file_panel, FilePanelState::DiffPreview);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| render_file_diff(frame, &state))
            .unwrap();
        let rendered = buffer_text(terminal.backend().buffer());
        assert!(rendered.contains("-fn main() {}"));
        assert!(rendered.contains("+fn main() {}!"));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert_eq!(state.file_panel, FilePanelState::Editing);
        handle_file_panel_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.file_panel, FilePanelState::Viewer);
        let (path, digest, bytes) = saved.lock().unwrap().clone().unwrap();
        assert_eq!(path, "src/main.rs");
        assert_eq!(
            digest,
            rover_core::Sha256Digest::of(b"fn main() {}\n").to_hex()
        );
        assert_eq!(bytes, b"fn main() {}!\n");

        let mut state = AttachedInputState {
            file_panel: FilePanelState::Viewer,
            file_preview: Some(FilePreviewState {
                path: "src/main.rs".to_owned(),
                text: "fn main() {}\n".to_owned(),
                truncated: false,
                binary: false,
            }),
            ..AttachedInputState::default()
        };
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('e')));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('x')));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        assert!(state.file_edit.as_ref().unwrap().discard_confirmation);
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('n')));
        assert_eq!(state.file_panel, FilePanelState::Editing);
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Esc));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert_eq!(state.file_panel, FilePanelState::Viewer);
        assert!(state.file_edit.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn stale_inline_save_keeps_buffer_and_diff_preview_is_bounded() {
        let mut provider = test_file_browser_provider();
        provider.save_file = Box::new(|_, _, _| {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "file changed since editing began; reload before saving",
            ))
        });
        let mut state = AttachedInputState {
            file_panel: FilePanelState::Viewer,
            file_preview: Some(FilePreviewState {
                path: "src/main.rs".to_owned(),
                text: "fn main() {}\n".to_owned(),
                truncated: false,
                binary: false,
            }),
            ..AttachedInputState::default()
        };
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('e')));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::End));
        handle_file_panel_key(&mut state, &mut provider, press(KeyCode::Char('!')));
        handle_file_panel_key(
            &mut state,
            &mut provider,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.file_panel, FilePanelState::Editing);
        assert!(state.file_edit.as_ref().unwrap().buffer.ends_with("!\n"));
        assert!(state
            .file_message
            .as_deref()
            .unwrap()
            .contains("reload before saving"));

        let mut changed = String::new();
        for line in 0..1000 {
            use std::fmt::Write as _;
            writeln!(changed, "line {line}").unwrap();
        }
        let diff = file_diff_lines("large.txt", "old\n", &changed);
        assert!(diff.len() <= 1028);
        assert!(diff
            .iter()
            .any(|line| line.to_string().contains("diff preview truncated")));
        let old_many_lines = "old\n".repeat(1_001);
        let new_many_lines = "new\n".repeat(1_001);
        let unavailable = file_diff_lines("large.txt", &old_many_lines, &new_many_lines);
        assert!(unavailable
            .iter()
            .any(|line| line.to_string().contains("comparison exceeds")));
        let newline_only = file_diff_lines("eof.txt", "line\n", "line");
        assert!(newline_only.iter().any(|line| line.to_string() == "-line"));
        assert!(newline_only.iter().any(|line| line.to_string() == "+line"));
        assert!(newline_only
            .iter()
            .any(|line| line.to_string() == "\\ No newline at end of file"));
    }

    #[cfg(unix)]
    #[test]
    fn file_diff_includes_unified_line_ranges_context_and_sanitized_paths() {
        let diff = file_diff_lines(
            "src/evil\n\u{202e}name.rs",
            "before\nkeep\nold\nafter\n",
            "before\nkeep\nnew\nafter\n",
        );
        let rendered = diff.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert!(rendered[0].contains("src/evil��name.rs"));
        assert!(!rendered[0].contains('\n'));
        assert!(rendered.contains(&"@@ -1,4 +1,4 @@".to_owned()));
        assert!(rendered.contains(&" before".to_owned()));
        assert!(rendered.contains(&" keep".to_owned()));
        assert!(rendered.contains(&"-old".to_owned()));
        assert!(rendered.contains(&"+new".to_owned()));
        assert!(rendered.contains(&" after".to_owned()));

        let mut original = String::new();
        for line in 0..24 {
            use std::fmt::Write as _;
            writeln!(original, "line {line}").unwrap();
        }
        let edited = original
            .replace("line 2\n", "changed two\n")
            .replace("line 17\n", "changed seventeen\n");
        let hunks = file_diff_lines("many.rs", &original, &edited);
        assert_eq!(
            hunks
                .iter()
                .filter(|line| line.to_string().starts_with("@@ "))
                .count(),
            2
        );
        assert!(hunks.iter().any(|line| line.to_string() == "-line 2"));
        assert!(hunks
            .iter()
            .any(|line| line.to_string() == "+changed seventeen"));
        assert!(!hunks.iter().any(|line| line.to_string() == "-line 9"));

        let inserted = file_diff_lines("x", "keep\n", "new\nkeep\n");
        assert!(inserted
            .iter()
            .any(|line| line.to_string() == "@@ -1,1 +1,2 @@"));
        let deleted = file_diff_lines("x", "keep\nremoved\n", "keep\n");
        assert!(deleted
            .iter()
            .any(|line| line.to_string() == "@@ -1,2 +1,1 @@"));
        let unchanged = file_diff_lines("x", "same\n", "same\n");
        assert!(unchanged
            .iter()
            .any(|line| line.to_string() == "No content differences."));
    }

    #[cfg(unix)]
    #[test]
    fn refresh_shortcut_is_scoped_to_task_list_and_detail() {
        let refresh = press(KeyCode::Char('r'));
        assert!(attached_task_refresh_key(TaskPanelState::List, refresh));
        assert!(attached_task_refresh_key(TaskPanelState::Detail, refresh));
        assert!(!attached_task_refresh_key(TaskPanelState::Closed, refresh));
        assert!(!attached_task_refresh_key(TaskPanelState::Stdout, refresh));
        assert!(!attached_task_refresh_key(
            TaskPanelState::List,
            KeyEvent::new_with_kind(
                KeyCode::Char('r'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            )
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cancel_shortcut_is_scoped_to_task_list_and_detail() {
        let cancel = press(KeyCode::Char('c'));
        assert!(attached_task_cancel_key(TaskPanelState::List, cancel));
        assert!(attached_task_cancel_key(TaskPanelState::Detail, cancel));
        assert!(!attached_task_cancel_key(TaskPanelState::Closed, cancel));
        assert!(!attached_task_cancel_key(TaskPanelState::Stdout, cancel));
    }

    #[cfg(unix)]
    #[test]
    fn task_cancel_requires_confirmation_and_calls_selected_task_id() {
        let cancelled = Arc::new(Mutex::new(Vec::new()));
        let cancellation_log = Arc::clone(&cancelled);
        let mut provider = TaskProvider::new(
            Vec::new(),
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |task_id| {
                cancellation_log
                    .lock()
                    .expect("cancellation log mutex")
                    .push(task_id.to_owned());
                Ok(())
            },
            |_| Ok(None),
            |_| Ok(None),
        );
        let mut state = AttachedInputState {
            pending_cancel: Some("task_selected".to_owned()),
            ..AttachedInputState::default()
        };
        handle_task_cancel_confirmation(&mut state, &mut provider, press(KeyCode::Char('n')));
        assert!(state.pending_cancel.is_none());
        assert!(cancelled.lock().unwrap().is_empty());

        state.pending_cancel = Some("task_selected".to_owned());
        handle_task_cancel_confirmation(&mut state, &mut provider, press(KeyCode::Char('y')));
        assert!(state.pending_cancel.is_none());
        assert_eq!(*cancelled.lock().unwrap(), ["task_selected".to_owned()]);
        assert_eq!(
            state.task_refresh_message.as_deref(),
            Some("Cancellation requested; prior external actions are not undone.")
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_task_evidence_opens_scrolls_and_returns_to_detail() {
        let mut task = task_summary("task_a");
        task.candidate = Some("snapshot_a".to_owned());
        task.investigation_id = Some("inv_task_a".to_owned());
        let mut provider = TaskProvider::new(
            vec![task],
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Err(io::Error::new(io::ErrorKind::Unsupported, "cancel unused")),
            |_| Ok(Some(investigation_evidence())),
            |_| Ok(None),
        );
        let mut state = AttachedInputState {
            task_panel: TaskPanelState::Detail,
            ..AttachedInputState::default()
        };
        assert!(open_selected_task_evidence(
            &mut state,
            &mut provider,
            press(KeyCode::Char('i'))
        ));
        assert_eq!(state.task_panel, TaskPanelState::Evidence);
        assert_eq!(state.task_evidence.as_ref().unwrap().decision, "BLOCKED");
        assert!(attached_task_evidence_key(
            &mut state,
            press(KeyCode::PageDown)
        ));
        assert_eq!(state.task_evidence_scroll, 12);
        assert!(attached_task_evidence_key(&mut state, press(KeyCode::Esc)));
        assert_eq!(state.task_panel, TaskPanelState::Detail);
    }

    #[cfg(unix)]
    #[test]
    fn evidence_renderer_labels_recorded_decisions_and_sanitizes_text() {
        let mut evidence = investigation_evidence();
        evidence.decision_reason = "pending\u{001b}[31m review\u{202e}".to_owned();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| render_task_evidence(frame, Some(&evidence), None, 0))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("recorded assessment"));
        assert!(text.contains("Stored decision: BLOCKED"));
        assert!(text.contains("Recorded checks"));
        assert!(text.contains("UNKNOWN:"));
        assert!(text.contains("FINDING:"));
        assert!(!text.contains('\u{001b}'));
        assert!(!text.contains('\u{202e}'));
    }

    #[cfg(unix)]
    #[test]
    fn candidate_diff_view_is_bound_scrollable_and_sanitized() {
        let mut task = task_summary("task_diff");
        task.base_snapshot = Some("snap_base".to_owned());
        task.candidate = Some("snap_candidate".to_owned());
        let mut diff = task_diff();
        diff.changes[0].path = "src/main.rs\u{001b}[31m\u{202e}".to_owned();
        let mut provider = TaskProvider::new(
            vec![task],
            || Ok(Vec::new()),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            |_| Err(io::Error::new(io::ErrorKind::Unsupported, "cancel unused")),
            |_| Ok(None),
            |_| Ok(Some(diff.clone())),
        );
        let mut state = AttachedInputState {
            task_panel: TaskPanelState::Detail,
            ..AttachedInputState::default()
        };
        assert!(open_selected_task_diff(
            &mut state,
            &mut provider,
            press(KeyCode::Char('d'))
        ));
        assert_eq!(state.task_panel, TaskPanelState::Diff);
        assert_eq!(state.task_diff.as_ref().unwrap().changes.len(), 1);
        assert!(attached_task_diff_key(&mut state, press(KeyCode::PageDown)));
        assert_eq!(state.task_diff_scroll, 12);
        assert!(attached_task_diff_key(&mut state, press(KeyCode::Esc)));
        assert_eq!(state.task_panel, TaskPanelState::Detail);
        provider.tasks[0].candidate = Some("snap_stale".to_owned());
        assert!(open_selected_task_diff(
            &mut state,
            &mut provider,
            press(KeyCode::Char('d'))
        ));
        assert!(state.task_diff.is_none());
        assert_eq!(
            state.task_diff_message.as_deref(),
            Some("Snapshot comparison does not match the selected task.")
        );

        let mut terminal = Terminal::new(TestBackend::new(96, 20)).unwrap();
        let display_diff = task_diff();
        terminal
            .draw(|frame| render_task_diff(frame, Some(&display_diff), None, 0))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("verified snapshot inventory"));
        assert!(text.contains("Base snapshot: snap_base"));
        assert!(text.contains("modified"));
        assert!(!text.contains('\u{001b}'));
        assert!(!text.contains('\u{202e}'));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_confirmation_names_task_and_warns_about_external_actions() {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| render_task_cancel_confirmation(frame, "task_selected"))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Confirm task cancellation"));
        assert!(text.contains("task_selected"));
        assert!(text.contains("does not undo external actions"));
        assert!(text.contains("y confirm"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_or_oversized_task_refresh_keeps_the_previous_snapshot() {
        let mut tasks = vec![task_summary("task_a")];
        let original = tasks.clone();
        let mut selected = 0;
        let mut message = None;
        refresh_task_snapshot(&mut tasks, &mut selected, &mut message, &mut || {
            Err(io::Error::other("index is offline\u{001b}"))
        });
        assert_eq!(tasks, original);
        assert_eq!(message.as_deref(), Some("Refresh failed: index is offline"));

        refresh_task_snapshot(&mut tasks, &mut selected, &mut message, &mut || {
            Ok((0..=1_000)
                .map(|index| task_summary(&format!("task_{index}")))
                .collect())
        });
        assert_eq!(tasks, original);
        assert_eq!(
            message.as_deref(),
            Some("Refresh refused: more than 1,000 project tasks.")
        );
    }

    #[test]
    fn task_log_requires_matching_digest_and_sanitizes_controls() {
        let bytes = "safe\x1b[31m red\x1b[0m\u{202e}hidden".as_bytes();
        let digest = rover_core::Sha256Digest::of(bytes).to_hex();
        let (text, verified) = prepare_task_log(bytes, &digest).unwrap();
        assert!(verified);
        assert!(text.contains("safe�[31m red�[0m�hidden"));
        assert!(prepare_task_log(bytes, &"0".repeat(64)).is_err());
        assert!(prepare_task_log(bytes, "BAD").is_err());
    }

    #[test]
    fn task_log_view_clips_large_content_after_verifying_full_blob() {
        let mut bytes = vec![b'x'; MAX_TASK_LOG_BYTES + 20];
        bytes.extend_from_slice(b"verified-tail");
        let digest = rover_core::Sha256Digest::of(&bytes).to_hex();
        let (text, verified) = prepare_task_log(&bytes, &digest).unwrap();
        assert!(verified);
        assert!(text.starts_with(&"x".repeat(MAX_TASK_LOG_BYTES)));
        assert!(text.ends_with("[Output clipped at 64 KiB for TUI display.]\n"));
        assert!(!text.contains("verified-tail"));
    }

    #[test]
    fn task_log_renderer_identifies_verified_content_and_scrolls() {
        let mut terminal = Terminal::new(TestBackend::new(64, 14)).unwrap();
        terminal
            .draw(|frame| render_task_log(frame, "stdout", "first\nsecond\nthird", 1, true))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("stdout · SHA-256 verified"));
        assert!(!text.contains("first"));
        assert!(text.contains("second"));
        assert!(text.contains("third"));
    }

    #[cfg(unix)]
    #[test]
    fn attached_task_log_keys_scroll_and_return_to_details() {
        let mut panel = TaskPanelState::Stdout;
        let mut scroll = 0;
        assert!(attached_task_log_key(
            &mut panel,
            &mut scroll,
            press(KeyCode::Down)
        ));
        assert_eq!(scroll, 1);
        assert!(attached_task_log_key(
            &mut panel,
            &mut scroll,
            press(KeyCode::PageDown)
        ));
        assert_eq!(scroll, 13);
        assert!(attached_task_log_key(
            &mut panel,
            &mut scroll,
            press(KeyCode::PageUp)
        ));
        assert_eq!(scroll, 1);
        assert!(attached_task_log_key(
            &mut panel,
            &mut scroll,
            press(KeyCode::Esc)
        ));
        assert_eq!(panel, TaskPanelState::Detail);
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
            })
            .collect::<String>()
    }

    #[test]
    fn test_backend_renders_tabs_session_pane_and_unicode_title() {
        let mut terminal = Terminal::new(TestBackend::new(48, 12)).unwrap();
        terminal
            .draw(|frame| render_workspace(frame, &workspace()))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Rover"));
        assert!(text.contains('项') && text.contains('目'));
        assert!(text.contains("Session: main"));
        assert!(text.contains("Waiting for a terminal screen"));
    }

    #[test]
    fn attached_rendering_documents_reserved_keys_without_claiming_q_is_global() {
        let mut terminal = Terminal::new(TestBackend::new(100, 5)).unwrap();
        terminal
            .draw(|frame| render_workspace_mode(frame, &workspace(), &[], true, false))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Ctrl-]"));
        assert!(text.contains("Ctrl-B"));
        assert!(text.contains("then q quit"));
    }

    #[test]
    fn keyboard_help_overlay_captures_keys_until_closed() {
        let mut help_open = false;
        assert!(handle_help_key(&mut help_open, press(KeyCode::Char('?'))));
        assert!(help_open);
        assert!(handle_help_key(&mut help_open, press(KeyCode::Char('q'))));
        assert!(help_open);
        assert!(handle_help_key(&mut help_open, press(KeyCode::Esc)));
        assert!(!help_open);
        assert!(!handle_help_key(&mut help_open, press(KeyCode::Char('q'))));

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| render_workspace_mode(frame, &workspace(), &[], true, true))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Rover keyboard help"));
        assert!(text.contains("Ctrl-]"));
        assert!(text.contains("command palette"));
        assert!(text.contains("close this help"));
    }

    #[test]
    fn task_board_renderer_bounds_rows_and_sanitizes_record_text() {
        let tasks = (0..15)
            .map(|index| TaskSummary {
                id: format!("task-{index}"),
                objective: if index == 13 {
                    "inspect\u{1b}[31m candidate".to_owned()
                } else {
                    format!("objective {index}")
                },
                status: "RUNNING".to_owned(),
                updated_at: format!("2026-09-26T00:00:{:02}Z", 59 - index),
                error: (index == 13).then(|| "task error\u{1b}[31m".to_owned()),
                candidate: (index == 13).then(|| "snapshot_13".to_owned()),
                base_snapshot: (index == 13).then(|| "snapshot_base".to_owned()),
                investigation_id: None,
                attempt_count: usize::from(index == 13) * 2,
                process: (index == 13).then(|| TaskProcessEvidence {
                    exit_code: 3,
                    error: Some("process error".to_owned()),
                    timed_out: false,
                    cancelled: false,
                    truncated: true,
                    stdout_bytes: 44,
                    stderr_bytes: 12,
                    stdout_sha256: "a".repeat(64),
                    stderr_sha256: "b".repeat(64),
                }),
            })
            .collect::<Vec<_>>();
        let mut terminal = Terminal::new(TestBackend::new(96, 24)).unwrap();
        terminal
            .draw(|frame| render_task_board(frame, &tasks, 13, false))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Rover task board"));
        assert!(text.contains("Showing 4–15 of 15 tasks"));
        assert!(text.contains("task-13"));
        assert!(text.contains("inspect[31m candidate"));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains("task-0 "));

        let mut narrow = Terminal::new(TestBackend::new(40, 10)).unwrap();
        narrow
            .draw(|frame| render_task_board(frame, &tasks, 0, false))
            .unwrap();
        let narrow_text = buffer_text(narrow.backend().buffer());
        assert!(narrow_text.contains("Showing 1–2 of 15 tasks"));

        let mut detail = Terminal::new(TestBackend::new(96, 24)).unwrap();
        detail
            .draw(|frame| render_task_board(frame, &tasks, 13, true))
            .unwrap();
        let detail_text = buffer_text(detail.backend().buffer());
        assert!(detail_text.contains("Rover task detail"));
        assert!(detail_text.contains("Attempts: 2"));
        assert!(detail_text.contains("Process: exit 3"));
        assert!(detail_text.contains("Stdout observed: 44 bytes"));
        assert!(detail_text.contains("recorded SHA-256"));
        assert!(detail_text.contains("Task error: task error[31m"));
        assert!(!detail_text.contains('\u{1b}'));
    }

    #[test]
    fn read_only_task_provider_hides_unsupported_actions() {
        let provider = TaskProvider::read_only_snapshot(
            vec![task_summary("task_read_only")],
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "no blob")),
            false,
        );
        assert!(!provider.capabilities.supports(TaskCapabilities::REFRESH));
        assert!(!provider.capabilities.supports(TaskCapabilities::LOGS));
        assert!(!provider.capabilities.supports(TaskCapabilities::CANCEL));
        assert!(!provider.capabilities.supports(TaskCapabilities::EVIDENCE));
        assert!(!provider.capabilities.supports(TaskCapabilities::DIFF));

        let mut terminal = Terminal::new(TestBackend::new(96, 24)).unwrap();
        terminal
            .draw(|frame| {
                render_task_board_with_refresh(
                    frame,
                    &provider.tasks,
                    0,
                    true,
                    None,
                    provider.capabilities,
                    TaskBoardView {
                        preferences: &provider.view_preferences,
                        search_draft: None,
                    },
                );
            })
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(!text.contains("r refresh"));
        assert!(!text.contains("c cancel"));
        assert!(!text.contains("o stdout"));
        assert!(!text.contains("i evidence"));
    }

    #[cfg(unix)]
    #[test]
    fn attached_task_board_uses_prefix_and_wraps_selection() {
        let mut panel = TaskPanelState::Closed;
        let mut prefix = false;
        let mut selected = 0;
        let tasks = vec![
            task_summary("task_a"),
            task_summary("task_b"),
            task_summary("task_c"),
        ];
        let preferences = TaskViewPreferences::default();
        assert!(!attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Char('t'))
        ));
        prefix = true;
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Char('t'))
        ));
        assert_eq!(panel, TaskPanelState::List);
        assert!(!prefix);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Enter)
        ));
        assert_eq!(panel, TaskPanelState::Detail);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Up)
        ));
        assert_eq!(selected, 2);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Down)
        ));
        assert_eq!(selected, 0);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Char('q'))
        ));
        assert_eq!(panel, TaskPanelState::Detail);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Esc)
        ));
        assert_eq!(panel, TaskPanelState::List);
        assert!(attached_task_board_key(
            &mut panel,
            &mut prefix,
            &mut selected,
            &tasks,
            &preferences,
            press(KeyCode::Esc)
        ));
        assert_eq!(panel, TaskPanelState::Closed);
    }

    #[test]
    fn parsed_pty_screen_renders_without_terminal_controls() {
        let workspace = workspace();
        let PaneNode::Leaf(pane) = workspace.active_tab().root() else {
            panic!("fixture has one pane");
        };
        let mut screen = TerminalScreen::new(
            rover_execution::terminal::TerminalSize::new(4, 40).unwrap(),
            0,
        )
        .unwrap();
        screen
            .process_output(b"safe\x1b[31m red\x1b[0m\x1b]52;c;AAAA\x07 visible")
            .unwrap();
        let screens = [ScreenText::from_terminal_screen(pane, &screen)];
        let mut terminal = Terminal::new(TestBackend::new(48, 12)).unwrap();
        terminal
            .draw(|frame| render_workspace_with_screens(frame, &workspace, &screens))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("safe red"));
        assert!(text.contains("visible"));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains("52;c"));
    }

    #[cfg(unix)]
    #[test]
    fn attached_agent_detection_uses_the_live_viewport_and_osc_title() {
        let mut screen = TerminalScreen::new(
            rover_execution::terminal::TerminalSize::new(4, 40).unwrap(),
            20,
        )
        .unwrap();
        screen
            .process_output(b"scrollback only\r\n\x1b]0;Amp: Plugin confirmation needed\x07")
            .unwrap();
        let detection = detect_foreground_sample(b"amp", &screen);
        assert_eq!(detection.agent.as_deref(), Some("amp"));
        assert_eq!(detection.state, AgentState::Blocked);
        assert!(detection.visibility.blocker);
        assert_eq!(detection.evidence[0].source, "foreground_process");
        assert_eq!(
            detection.evidence[1].rule_id.as_deref(),
            Some("osc_title_plugin_confirmation_blocked")
        );

        screen.process_output(b"\x1b]9;4;1;-1\x07").unwrap();
        let progress_detection = detect_foreground_sample(b"grok", &screen);
        assert_eq!(progress_detection.state, AgentState::Working);
        assert!(progress_detection.visibility.working);
        assert_eq!(
            progress_detection.evidence[1].rule_id.as_deref(),
            Some("osc_progress_working")
        );
    }

    #[cfg(unix)]
    #[test]
    fn foreground_group_detection_uses_recognized_member_when_leader_is_unknown() {
        let mut screen = TerminalScreen::new(
            rover_execution::terminal::TerminalSize::new(4, 40).unwrap(),
            20,
        )
        .unwrap();
        screen
            .process_output(b"\x1b]0;Amp: Plugin confirmation needed\x07")
            .unwrap();
        let detection = detect_foreground_group_sample(b"sh", &["amp".to_owned()], &[], &screen);
        assert_eq!(detection.agent.as_deref(), Some("amp"));
        assert_eq!(detection.state, AgentState::Blocked);
        assert_eq!(detection.evidence[0].source, "foreground_process");
    }

    #[cfg(unix)]
    #[test]
    fn conflicting_agent_names_in_foreground_group_remain_unknown() {
        let screen = TerminalScreen::new(
            rover_execution::terminal::TerminalSize::new(4, 40).unwrap(),
            20,
        )
        .unwrap();
        let detection = detect_foreground_group_sample(
            b"sh",
            &["amp".to_owned(), "codex".to_owned()],
            &[],
            &screen,
        );
        assert_eq!(detection.agent, None);
        assert_eq!(detection.state, AgentState::Unknown);
        assert_eq!(detection.reason, "ambiguous_foreground_process_group");
        assert_eq!(detection.evidence.len(), 2);
        // The operator must be able to see *which* agents disagreed.
        let mut reported = detection
            .evidence
            .iter()
            .map(|evidence| evidence.value.clone())
            .collect::<Vec<_>>();
        reported.sort();
        assert_eq!(reported, vec!["amp".to_owned(), "codex".to_owned()]);
        assert!(detection
            .evidence
            .iter()
            .all(|evidence| evidence.source == "foreground_process"));

        let leader_wins =
            detect_foreground_group_sample(b"amp", &["codex".to_owned()], &[], &screen);
        assert_eq!(leader_wins.agent.as_deref(), Some("amp"));
        assert_ne!(leader_wins.reason, "ambiguous_foreground_process_group");
    }

    #[cfg(unix)]
    #[test]
    fn transcript_viewer_skip_rule_preserves_the_last_screen_state() {
        let mut screen = TerminalScreen::new(
            rover_execution::terminal::TerminalSize::new(10, 80).unwrap(),
            20,
        )
        .unwrap();
        screen
            .process_output(b"\x1b]0;Codex \xe2\xa0\x8b\x07")
            .unwrap();
        let mut current = None;
        apply_agent_detection(&mut current, detect_foreground_sample(b"codex", &screen));
        assert_eq!(
            current.as_ref().map(|result| result.state),
            Some(AgentState::Working)
        );

        screen.process_output(b"\x1b]0;\x07\r\n\xe2\x80\xba previous prompt\r\n\xe2\x86\x91/\xe2\x86\x93 to scroll\r\npgup/pgdn to move\r\nhome/end to jump\r\nq to quit\r\nesc to edit prev").unwrap();
        let skipped = detect_foreground_sample(b"codex", &screen);
        assert!(skipped.skip_state_update);
        apply_agent_detection(&mut current, skipped);

        let preserved = current.expect("previous state is retained");
        assert_eq!(preserved.state, AgentState::Working);
        assert!(preserved.skip_state_update);
        assert_eq!(preserved.reason, "screen_state_preserved_for_skip_update");
        assert!(preserved
            .evidence
            .iter()
            .any(|evidence| { evidence.rule_id.as_deref() == Some("transcript_viewer") }));

        let previous_evidence_count = preserved.evidence.len();
        let mut current = Some(preserved);
        apply_agent_detection(&mut current, detect_foreground_sample(b"codex", &screen));
        assert_eq!(
            current.as_ref().map(|result| result.evidence.len()),
            Some(previous_evidence_count)
        );

        let mut different_agent = current.unwrap();
        different_agent.agent = Some("claude".to_owned());
        different_agent.state = AgentState::Unknown;
        different_agent.skip_state_update = true;
        let mut current = Some(DetectionResult {
            state: AgentState::Working,
            ..detect_foreground_sample(b"codex", &screen)
        });
        apply_agent_detection(&mut current, different_agent);
        let changed = current.unwrap();
        assert_eq!(changed.agent.as_deref(), Some("claude"));
        assert_eq!(changed.state, AgentState::Unknown);
    }

    #[test]
    fn test_backend_handles_narrow_frames() {
        for (width, height) in [(1, 1), (8, 3), (16, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render_workspace(frame, &workspace()))
                .unwrap();
        }
    }

    #[test]
    fn key_bindings_split_focus_zoom_popup_and_close_layout_panes() {
        let mut workspace = workspace();
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('|'))));
        assert_eq!(workspace.pane_count(), 2);
        let first = match workspace.active_tab().root() {
            PaneNode::Split(split) => match split.first() {
                PaneNode::Leaf(pane) => pane.id().to_owned(),
                PaneNode::Split(_) => panic!("first child must remain the existing leaf"),
            },
            PaneNode::Leaf(_) => panic!("split key must split the focused pane"),
        };
        assert!(!handle_key(&mut workspace, press(KeyCode::Tab)));
        assert_eq!(workspace.active_tab().active_pane(), first);
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('z'))));
        assert_eq!(workspace.active_tab().zoomed_pane(), Some(first.as_str()));
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('p'))));
        assert!(workspace.active_tab().popup_pane().is_some());
        assert!(!handle_key(&mut workspace, press(KeyCode::Esc)));
        assert!(workspace.active_tab().popup_pane().is_none());
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('x'))));
        assert_eq!(workspace.pane_count(), 1);
        assert!(handle_key(&mut workspace, press(KeyCode::Char('q'))));
        assert!(!handle_key(
            &mut workspace,
            KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                KeyEventKind::Release
            )
        ));
    }

    #[test]
    fn tab_shortcuts_preserve_workspace_order_and_focus() {
        let mut workspace = workspace();
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('n'))));
        assert_eq!(workspace.tabs().len(), 2);
        assert_eq!(workspace.active_tab().title(), "scratch-2");
        assert!(!handle_key(&mut workspace, press(KeyCode::Char('['))));
        assert_eq!(workspace.active_tab().title(), "main");
        assert!(!handle_key(&mut workspace, press(KeyCode::Char(']'))));
        assert_eq!(workspace.active_tab().title(), "scratch-2");
    }

    #[cfg(unix)]
    #[test]
    fn attached_key_translation_preserves_text_controls_and_detach_binding() {
        assert_eq!(
            key_input_bytes(press(KeyCode::Char('x'))),
            Some(b"x".to_vec())
        );
        assert_eq!(
            key_input_bytes(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![0x03])
        );
        assert_eq!(key_input_bytes(press(KeyCode::Enter)), Some(vec![b'\r']));
        assert_eq!(
            key_input_bytes(press(KeyCode::Up)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_input_bytes(press(KeyCode::Char('é'))),
            Some("é".as_bytes().to_vec())
        );
        assert!(is_detach_key(KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_detach_key(press(KeyCode::Char(']'))));
    }

    #[cfg(unix)]
    #[test]
    fn attached_workspace_shortcut_dispatch_applies_prefixed_action() {
        let mut workspace = workspace();
        assert_eq!(workspace.tabs().len(), 1);
        assert!(!attached_workspace_key(
            &mut workspace,
            press(KeyCode::Char('n'))
        ));
        assert_eq!(workspace.tabs().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn attached_help_uses_prefix_so_question_marks_reach_the_shell() {
        let mut help_open = false;
        let mut prefix = false;
        assert!(!attached_help_key(
            &mut help_open,
            &mut prefix,
            press(KeyCode::Char('?'))
        ));
        assert!(!help_open);

        prefix = true;
        assert!(attached_help_key(
            &mut help_open,
            &mut prefix,
            press(KeyCode::Char('?'))
        ));
        assert!(help_open);
        assert!(!prefix);
        assert!(attached_help_key(
            &mut help_open,
            &mut prefix,
            press(KeyCode::Char('q'))
        ));
        assert!(help_open);
        assert!(attached_help_key(
            &mut help_open,
            &mut prefix,
            press(KeyCode::Esc)
        ));
        assert!(!help_open);
    }

    #[cfg(unix)]
    #[test]
    fn persistent_tui_restores_saved_layout_and_fails_closed_on_corruption() {
        let (path, directory) = private_state_dir();
        let mut saved = workspace();
        saved
            .split_active(
                SplitAxis::Vertical,
                4_000,
                rover_execution::layout::PaneSpec::scratch(),
            )
            .unwrap();
        saved.save_to(&directory, "layout.json").unwrap();
        assert_eq!(
            load_workspace_or_initial(workspace(), &directory, "layout.json").unwrap(),
            saved
        );

        let initial = workspace();
        assert_eq!(
            load_workspace_or_initial(initial.clone(), &directory, "new.json").unwrap(),
            initial
        );
        directory.atomic_write("broken.json", b"{}", 0o600).unwrap();
        assert_eq!(
            load_workspace_or_initial(workspace(), &directory, "broken.json")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(directory);
        fs::remove_dir_all(path).unwrap();
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct FakeTerminalState {
        raw: bool,
        alternate: bool,
        paste: bool,
    }

    struct FakeTerminalControl {
        state: Arc<Mutex<FakeTerminalState>>,
        fail_enter: bool,
    }

    impl TerminalControl for FakeTerminalControl {
        fn enable_raw(&mut self) -> io::Result<()> {
            self.state.lock().unwrap().raw = true;
            Ok(())
        }

        fn enter_screen(&mut self) -> io::Result<()> {
            if self.fail_enter {
                return Err(io::Error::other("simulated alternate-screen failure"));
            }
            let mut state = self.state.lock().unwrap();
            state.alternate = true;
            state.paste = true;
            Ok(())
        }

        fn restore(&mut self) {
            *self.state.lock().unwrap() = FakeTerminalState::default();
        }
    }

    #[test]
    fn terminal_guard_restores_state_on_normal_exit_unwind_and_partial_setup() {
        let state = Arc::new(Mutex::new(FakeTerminalState::default()));
        let guard = TerminalGuard::enter_with(FakeTerminalControl {
            state: Arc::clone(&state),
            fail_enter: false,
        })
        .unwrap();
        assert!(state.lock().unwrap().raw);
        assert!(state.lock().unwrap().alternate);
        assert!(state.lock().unwrap().paste);
        drop(guard);
        assert_eq!(*state.lock().unwrap(), FakeTerminalState::default());

        let state = Arc::new(Mutex::new(FakeTerminalState::default()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
            let state = Arc::clone(&state);
            move || {
                let _guard = TerminalGuard::enter_with(FakeTerminalControl {
                    state,
                    fail_enter: false,
                })
                .unwrap();
                panic!("exercise terminal guard unwind");
            }
        }));
        assert!(result.is_err());
        assert_eq!(*state.lock().unwrap(), FakeTerminalState::default());

        let state = Arc::new(Mutex::new(FakeTerminalState::default()));
        assert!(TerminalGuard::enter_with(FakeTerminalControl {
            state: Arc::clone(&state),
            fail_enter: true,
        })
        .is_err());
        assert_eq!(*state.lock().unwrap(), FakeTerminalState::default());
    }
}
