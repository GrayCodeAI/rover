//! Project-bound exact agent session identities and explicit resume commands.
//!
//! This module does not scan provider stores or resume automatically. Callers
//! must bind an exact ID from a transcript, reviewed hook, or direct user
//! selection, then explicitly request argv for that same adapter.

use std::fmt;

use rover_core::{RepositoryIdentity, Sha256Digest};
use rover_store::{Store, StoreError};
use serde::{Deserialize, Serialize};

use super::{
    native_executable, nonempty, valid_tool, validate_model, AgentResult, InvocationOptions,
};

const RECORD_KIND: &str = "agent_session";
const RECORD_SCHEMA: &str = "rover/agent-session/v1";
const MAX_PANE_ID_BYTES: usize = 128;
const MAX_SESSION_ID_BYTES: usize = 256;
const MAX_SOURCE_BYTES: usize = 80;
const MAX_SESSION_BINDING_RECORDS: i64 = 1000;

/// Evidence origin for an exact session identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionSource {
    /// Identity was emitted in a provider's structured transcript.
    ProviderTranscript,
    /// Identity was emitted by an installed agent integration hook.
    OfficialHook,
    /// The user explicitly selected or entered this exact identity.
    ExplicitUser,
}

impl AgentSessionSource {
    /// Stable source label for user-visible evidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderTranscript => "provider transcript",
            Self::OfficialHook => "agent integration hook",
            Self::ExplicitUser => "user-selected",
        }
    }
}

/// Exact native agent session bound to a Rover project pane.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSessionBinding {
    schema: String,
    project_id: String,
    repository: String,
    pane_id: String,
    adapter: String,
    session_id: String,
    source: AgentSessionSource,
}

impl AgentSessionBinding {
    /// Bind the session ID emitted by a parsed native provider transcript.
    ///
    /// The transcript parser validates structure and identity consistency; it
    /// does not prove that a provider's ID is genuine or resumable.
    ///
    /// # Errors
    ///
    /// Returns an error when the result lacks a supported adapter or session
    /// identity, or when the project/pane binding is invalid.
    pub fn from_transcript(
        project: &RepositoryIdentity,
        pane_id: impl Into<String>,
        result: &AgentResult,
    ) -> Result<Self, SessionBindingError> {
        if result.session.is_empty()
            || !matches!(result.adapter.as_str(), "codex-exec" | "claude-print")
        {
            return Err(SessionBindingError::InvalidRecord);
        }
        Self::new(
            project,
            pane_id,
            result.adapter.clone(),
            result.session.clone(),
            AgentSessionSource::ProviderTranscript,
        )
    }

    /// Create a validated exact-ID binding for a supported native adapter.
    ///
    /// Only profiles for which Rover currently has reviewed argv contracts are
    /// accepted. Paths and guessed or latest references are not IDs.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported adapters, invalid identifiers, or a
    /// project path that cannot be represented as UTF-8.
    pub fn new(
        project: &RepositoryIdentity,
        pane_id: impl Into<String>,
        adapter: impl Into<String>,
        session_id: impl Into<String>,
        source: AgentSessionSource,
    ) -> Result<Self, SessionBindingError> {
        let repository = project
            .path()
            .to_str()
            .ok_or(SessionBindingError::InvalidRepository)?
            .to_owned();
        let binding = Self {
            schema: RECORD_SCHEMA.to_owned(),
            project_id: project.project_id().to_hex(),
            repository,
            pane_id: pane_id.into(),
            adapter: adapter.into(),
            session_id: session_id.into(),
            source,
        };
        binding.validate()?;
        Ok(binding)
    }

    /// Stable pane identity within the bound project.
    #[must_use]
    pub fn pane_id(&self) -> &str {
        &self.pane_id
    }

    /// Native adapter profile associated with this session.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Exact opaque provider session ID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Evidence origin for this binding.
    #[must_use]
    pub const fn source(&self) -> AgentSessionSource {
        self.source
    }

    /// Construct an explicit native resume invocation for this binding.
    ///
    /// The returned argv does not execute the program. It always targets the
    /// bound session ID and validates that the requested adapter matches.
    ///
    /// # Errors
    ///
    /// Returns an error for an adapter mismatch or invalid native options.
    pub fn resume_argv(
        &self,
        adapter: &str,
        options: &InvocationOptions,
        prompt: &str,
    ) -> Result<Vec<String>, SessionBindingError> {
        self.validate()?;
        if adapter != self.adapter {
            return Err(SessionBindingError::AdapterMismatch);
        }
        let executable = native_executable(
            options.executable.as_deref(),
            if adapter == "codex-exec" {
                "codex"
            } else {
                "claude"
            },
        )
        .map_err(|_| SessionBindingError::InvalidOptions)?;
        validate_model(options.model.as_deref())
            .map_err(|_| SessionBindingError::InvalidOptions)?;
        match adapter {
            "codex-exec" => {
                let sandbox = if options.write {
                    "workspace-write"
                } else {
                    "read-only"
                };
                let mut argv = vec![
                    executable,
                    "exec".to_owned(),
                    "resume".to_owned(),
                    self.session_id.clone(),
                    "--json".to_owned(),
                    "--sandbox".to_owned(),
                    sandbox.to_owned(),
                ];
                if let Some(model) = nonempty(options.model.as_deref()) {
                    argv.extend(["--model".to_owned(), model.to_owned()]);
                }
                argv.extend(["--".to_owned(), prompt.to_owned()]);
                Ok(argv)
            }
            "claude-print" => {
                for tool in &options.allowed_tools {
                    if !valid_tool(tool) {
                        return Err(SessionBindingError::InvalidOptions);
                    }
                }
                let mode = if options.write { "acceptEdits" } else { "plan" };
                let mut argv = vec![
                    executable,
                    "--resume".to_owned(),
                    self.session_id.clone(),
                    "--print".to_owned(),
                    "--output-format".to_owned(),
                    "stream-json".to_owned(),
                    "--verbose".to_owned(),
                    "--permission-mode".to_owned(),
                    mode.to_owned(),
                ];
                if options.max_turns > 0 {
                    argv.extend(["--max-turns".to_owned(), options.max_turns.to_string()]);
                }
                if let Some(model) = nonempty(options.model.as_deref()) {
                    argv.extend(["--model".to_owned(), model.to_owned()]);
                }
                if !options.allowed_tools.is_empty() {
                    argv.extend(["--allowedTools".to_owned(), options.allowed_tools.join(",")]);
                }
                argv.extend(["--".to_owned(), prompt.to_owned()]);
                Ok(argv)
            }
            _ => Err(SessionBindingError::InvalidRecord),
        }
    }

    /// Build an interactive native-session resume argv for the Rover TUI.
    ///
    /// The resulting command opens the provider's interactive terminal UI,
    /// unlike `resume_argv` which uses Rover's headless transcript profiles.
    ///
    /// # Errors
    ///
    /// Returns `InvalidRecord` when this binding does not map to a supported
    /// interactive native resume command.
    pub fn resume_tui_argv(&self) -> Result<Vec<String>, SessionBindingError> {
        self.validate()?;
        match self.adapter.as_str() {
            "codex-exec" => Ok(vec![
                "codex".to_owned(),
                "resume".to_owned(),
                self.session_id.clone(),
            ]),
            "claude-print" => Ok(vec![
                "claude".to_owned(),
                "--resume".to_owned(),
                self.session_id.clone(),
            ]),
            _ => Err(SessionBindingError::InvalidRecord),
        }
    }

    fn validate(&self) -> Result<(), SessionBindingError> {
        if self.schema != RECORD_SCHEMA
            || !valid_pane_id(&self.pane_id)
            || !valid_session_id(&self.session_id)
            || !valid_source(&self.adapter)
            || !matches!(self.adapter.as_str(), "codex-exec" | "claude-print")
            || self.repository.is_empty()
            || self.repository.len() > 4096
            || Sha256Digest::parse_hex(&self.project_id).is_err()
        {
            return Err(SessionBindingError::InvalidRecord);
        }
        Ok(())
    }
}

/// Session binding persistence and validation error.
#[derive(Debug)]
pub enum SessionBindingError {
    Store(StoreError),
    InvalidRecord,
    InvalidIdentifier,
    InvalidOptions,
    AdapterMismatch,
    ProjectMismatch,
    InvalidRepository,
}

impl fmt::Display for SessionBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "agent session storage failed: {error}"),
            Self::InvalidRecord => formatter.write_str("agent session binding is invalid"),
            Self::InvalidIdentifier => formatter.write_str("agent session identifier is invalid"),
            Self::InvalidOptions => formatter.write_str("agent resume options are invalid"),
            Self::AdapterMismatch => {
                formatter.write_str("agent session adapter does not match the requested adapter")
            }
            Self::ProjectMismatch => {
                formatter.write_str("agent session binding belongs to another project")
            }
            Self::InvalidRepository => {
                formatter.write_str("repository path cannot be represented as UTF-8")
            }
        }
    }
}

impl std::error::Error for SessionBindingError {}

impl From<StoreError> for SessionBindingError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// Save or replace the explicit binding for one pane in the project's store.
///
/// # Errors
///
/// Returns an error if the binding does not belong to project or storage
/// rejects the record.
pub fn save_session_binding(
    store: &Store,
    project: &RepositoryIdentity,
    binding: &AgentSessionBinding,
) -> Result<(), SessionBindingError> {
    validate_project(binding, project)?;
    let id = session_record_id(project, binding.pane_id());
    store.put(RECORD_KIND, &id, binding, "agent_session.bound")?;
    Ok(())
}

/// Load and verify the exact binding for one pane and project.
///
/// # Errors
///
/// Returns `NotFound` through the store error when no binding exists; rejects
/// malformed records or project/pane identity mismatches.
pub fn load_session_binding(
    store: &Store,
    project: &RepositoryIdentity,
    pane_id: &str,
) -> Result<AgentSessionBinding, SessionBindingError> {
    if !valid_pane_id(pane_id) {
        return Err(SessionBindingError::InvalidIdentifier);
    }
    let id = session_record_id(project, pane_id);
    let binding: AgentSessionBinding = store.get_typed(RECORD_KIND, &id)?;
    binding.validate()?;
    validate_project(&binding, project)?;
    if binding.pane_id != pane_id {
        return Err(SessionBindingError::InvalidRecord);
    }
    Ok(binding)
}

/// List exact session bindings for one canonical project, ordered by pane ID.
///
/// Records for other projects are omitted. Malformed records claiming this
/// project's identity fail the entire query rather than producing a partial
/// inventory.
///
/// # Errors
///
/// Returns an error for an invalid project binding, malformed matching record,
/// duplicate pane binding, or a state store with more than 1,000 session rows.
pub fn list_session_bindings(
    store: &Store,
    project: &RepositoryIdentity,
) -> Result<Vec<AgentSessionBinding>, SessionBindingError> {
    let project_id = project.project_id().to_hex();
    let repository = project
        .path()
        .to_str()
        .ok_or(SessionBindingError::InvalidRepository)?;
    let records = store.list_all(RECORD_KIND, MAX_SESSION_BINDING_RECORDS)?;
    let mut bindings = Vec::new();
    for record in records {
        if record.string_at_path(&["project_id"]) != Some(project_id.as_str()) {
            continue;
        }
        let binding: AgentSessionBinding = serde_json::from_value(record.as_value().clone())
            .map_err(|_| SessionBindingError::InvalidRecord)?;
        binding.validate()?;
        if binding.repository != repository {
            return Err(SessionBindingError::ProjectMismatch);
        }
        bindings.push(binding);
    }
    bindings.sort_by(|left, right| left.pane_id.cmp(&right.pane_id));
    if bindings
        .windows(2)
        .any(|pair| pair[0].pane_id == pair[1].pane_id)
    {
        return Err(SessionBindingError::InvalidRecord);
    }
    Ok(bindings)
}

/// Clear the explicit session binding for one project pane.
///
/// # Errors
///
/// Returns an error for an invalid pane ID, absent binding, or store failure.
pub fn clear_session_binding(
    store: &Store,
    project: &RepositoryIdentity,
    pane_id: &str,
) -> Result<(), SessionBindingError> {
    if !valid_pane_id(pane_id) {
        return Err(SessionBindingError::InvalidIdentifier);
    }
    let id = session_record_id(project, pane_id);
    store.delete(RECORD_KIND, &id, "agent_session.cleared")?;
    Ok(())
}

fn validate_project(
    binding: &AgentSessionBinding,
    project: &RepositoryIdentity,
) -> Result<(), SessionBindingError> {
    binding.validate()?;
    let repository = project
        .path()
        .to_str()
        .ok_or(SessionBindingError::InvalidRepository)?;
    if binding.project_id != project.project_id().to_hex() || binding.repository != repository {
        return Err(SessionBindingError::ProjectMismatch);
    }
    Ok(())
}

fn session_record_id(project: &RepositoryIdentity, pane_id: &str) -> String {
    let project_id = project.project_id().to_hex();
    format!(
        "as_{}",
        Sha256Digest::of(format!("{project_id}\0{pane_id}").as_bytes()).to_hex()
    )
}

fn valid_pane_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PANE_ID_BYTES
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '.' | '_' | '-')
        })
}

fn valid_session_id(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.len() <= MAX_SESSION_ID_BYTES
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn valid_source(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SOURCE_BYTES
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '.' | '_' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn store() -> Store {
        Store::from_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn project(path: &str) -> RepositoryIdentity {
        RepositoryIdentity::resolve(path).unwrap()
    }

    #[test]
    fn exact_binding_is_project_pane_bound_and_reloadable() {
        let store = store();
        let project = project("/tmp/rover-agent-session-project");
        let binding = AgentSessionBinding::new(
            &project,
            "w1:p2",
            "codex-exec",
            "019d1c08-c1b4-70c2-955f-840a5022c017",
            AgentSessionSource::ProviderTranscript,
        )
        .unwrap();
        save_session_binding(&store, &project, &binding).unwrap();
        assert_eq!(
            load_session_binding(&store, &project, "w1:p2").unwrap(),
            binding
        );
        assert!(matches!(
            load_session_binding(&store, &project, "w1:p3"),
            Err(SessionBindingError::Store(StoreError::NotFound))
        ));
    }

    #[test]
    fn binding_from_transcript_preserves_adapter_identity_and_claim_provenance() {
        let project = project("/tmp/rover-agent-session-transcript");
        let result = AgentResult {
            adapter: "codex-exec".to_owned(),
            session: "thread-abc".to_owned(),
            ..AgentResult::default()
        };
        let binding = AgentSessionBinding::from_transcript(&project, "pane-1", &result).unwrap();
        assert_eq!(binding.adapter(), "codex-exec");
        assert_eq!(binding.session_id(), "thread-abc");
        assert_eq!(binding.source(), AgentSessionSource::ProviderTranscript);
        let mut invalid = result;
        invalid.adapter = "generic-pty".to_owned();
        assert!(AgentSessionBinding::from_transcript(&project, "pane-1", &invalid).is_err());
    }

    #[test]
    fn explicit_resume_uses_bound_identity_and_rejects_mismatch() {
        let project = project("/tmp/rover-agent-resume-project");
        let binding = AgentSessionBinding::new(
            &project,
            "pane-1",
            "claude-print",
            "session_123",
            AgentSessionSource::OfficialHook,
        )
        .unwrap();
        let argv = binding
            .resume_argv("claude-print", &InvocationOptions::default(), "continue")
            .unwrap();
        assert_eq!(argv[0], "claude");
        assert_eq!(&argv[1..4], ["--resume", "session_123", "--print"]);
        assert_eq!(&argv[argv.len() - 2..], ["--", "continue"]);
        assert!(matches!(
            binding.resume_argv("codex-exec", &InvocationOptions::default(), "x"),
            Err(SessionBindingError::AdapterMismatch)
        ));
    }

    #[test]
    fn tui_resume_uses_interactive_native_commands() {
        let project = project("/tmp/rover-agent-tui-resume");
        let codex = AgentSessionBinding::new(
            &project,
            "pane-c",
            "codex-exec",
            "thread-abc",
            AgentSessionSource::ProviderTranscript,
        )
        .unwrap();
        let claude = AgentSessionBinding::new(
            &project,
            "pane-a",
            "claude-print",
            "session-123",
            AgentSessionSource::OfficialHook,
        )
        .unwrap();
        assert_eq!(
            codex.resume_tui_argv().unwrap(),
            ["codex", "resume", "thread-abc"]
        );
        assert_eq!(
            claude.resume_tui_argv().unwrap(),
            ["claude", "--resume", "session-123"]
        );
    }

    #[test]
    fn codex_resume_keeps_permission_mode_and_arguments_separate() {
        let project = project("/tmp/rover-agent-codex-resume");
        let binding = AgentSessionBinding::new(
            &project,
            "pane-2",
            "codex-exec",
            "thread-abc",
            AgentSessionSource::ExplicitUser,
        )
        .unwrap();
        let argv = binding
            .resume_argv(
                "codex-exec",
                &InvocationOptions {
                    write: true,
                    model: Some("gpt-5-codex".to_owned()),
                    ..InvocationOptions::default()
                },
                "follow-up; do not split",
            )
            .unwrap();
        assert_eq!(&argv[..4], ["codex", "exec", "resume", "thread-abc"]);
        assert!(argv
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        assert_eq!(&argv[argv.len() - 2..], ["--", "follow-up; do not split"]);
    }

    #[test]
    fn malformed_and_cross_project_bindings_fail_closed() {
        let store = store();
        let other = project("/tmp/rover-agent-session-b");
        let project = project("/tmp/rover-agent-session-a");
        let too_long = "x".repeat(257);
        for session in ["", "-option", "id/../../other", "bad id", too_long.as_str()] {
            assert!(AgentSessionBinding::new(
                &project,
                "pane",
                "codex-exec",
                session,
                AgentSessionSource::ExplicitUser
            )
            .is_err());
        }
        let binding = AgentSessionBinding::new(
            &project,
            "pane",
            "codex-exec",
            "session-1",
            AgentSessionSource::ExplicitUser,
        )
        .unwrap();
        save_session_binding(&store, &project, &binding).unwrap();
        assert!(matches!(
            load_session_binding(&store, &other, "pane"),
            Err(SessionBindingError::Store(StoreError::NotFound)
                | SessionBindingError::ProjectMismatch)
        ));
        assert!(AgentSessionBinding::new(
            &project,
            "pane",
            "generic-pty",
            "session-1",
            AgentSessionSource::ExplicitUser
        )
        .is_err());
    }

    #[test]
    fn clearing_binding_removes_it_and_records_deletion() {
        let store = store();
        let project = project("/tmp/rover-agent-session-clear");
        let binding = AgentSessionBinding::new(
            &project,
            "pane",
            "claude-print",
            "session-1",
            AgentSessionSource::ExplicitUser,
        )
        .unwrap();
        save_session_binding(&store, &project, &binding).unwrap();
        clear_session_binding(&store, &project, "pane").unwrap();
        assert!(matches!(
            load_session_binding(&store, &project, "pane"),
            Err(SessionBindingError::Store(StoreError::NotFound))
        ));
        assert!(matches!(
            clear_session_binding(&store, &project, "pane"),
            Err(SessionBindingError::Store(StoreError::NotFound))
        ));
    }

    #[test]
    fn binding_inventory_is_project_scoped_sorted_and_exact() {
        let store = store();
        let repository = project("/tmp/rover-agent-inventory-project");
        let other_project = project("/tmp/rover-agent-inventory-other-project");
        for (identity, pane, session) in [
            (&repository, "pane-z", "session-z"),
            (&other_project, "pane-a", "session-a"),
            (&repository, "pane-b", "session-b"),
        ] {
            let binding = AgentSessionBinding::new(
                identity,
                pane,
                "codex-exec",
                session,
                AgentSessionSource::ExplicitUser,
            )
            .unwrap();
            save_session_binding(&store, identity, &binding).unwrap();
        }
        let bindings = list_session_bindings(&store, &repository).unwrap();
        assert_eq!(
            bindings
                .iter()
                .map(|binding| (binding.pane_id(), binding.session_id()))
                .collect::<Vec<_>>(),
            [("pane-b", "session-b"), ("pane-z", "session-z")]
        );
        assert!(list_session_bindings(&store, &other_project)
            .unwrap()
            .iter()
            .all(|binding| binding.pane_id() == "pane-a"));
    }
}
