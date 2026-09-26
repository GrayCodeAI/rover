use std::collections::BTreeMap;
use std::env;
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use rover_agents::{clear_session_binding, load_session_binding, SessionBindingError};
#[cfg(unix)]
use rover_core::RepositoryIdentity;
#[cfg(unix)]
use rover_core::Sha256Digest;
#[cfg(unix)]
use rover_execution::layout::{PaneKind, Workspace};
#[cfg(unix)]
use rover_execution::pty::PtyOptions;
#[cfg(unix)]
use rover_execution::sessions::{SessionClient, SessionKey, SessionServer};
#[cfg(unix)]
use rover_store::files::{SafeDir, StateRoot};
#[cfg(unix)]
use rover_store::RecordValue;
#[cfg(unix)]
use rover_tui::{
    SavedCommand, TaskCheckEvidence, TaskDiffChange, TaskDiffEvidence, TaskDraft,
    TaskEvidenceFinding, TaskInvestigationEvidence, TaskPlanSummary, TaskProcessEvidence,
    TaskProvider, TaskSortOrder, TaskSummary, TaskViewPreferences, MAX_SAVED_COMMANDS,
    MAX_SAVED_COMMAND_LINE_BYTES, MAX_SAVED_COMMAND_NAME_BYTES, MAX_TASK_DRAFTS,
    MAX_TASK_DRAFT_DEPENDENCIES_BYTES, MAX_TASK_DRAFT_GATE_BYTES, MAX_TASK_DRAFT_PATHS_BYTES,
    MAX_TASK_DRAFT_PROMPT_BYTES, MAX_TASK_DRAFT_TITLE_BYTES, MAX_WORKSPACE_NOTES_BYTES,
    MAX_WORKSPACE_NOTES_COUNT, MAX_WORKSPACE_NOTES_TOTAL_BYTES,
};

/// Name of the Rust preview executable. It deliberately differs from the Go
/// `rover` product binary so the two cannot shadow each other on `PATH`.
const BINARY_NAME: &str = "rover-rs";

const USAGE: &str = "usage: rover-rs tui [--state PATH] [--repo PATH] | rover-rs agents [--json] | rover-rs agent list | rover-rs agent capabilities <adapter> | rover-rs agent session <list|status|bind|clear> ... (run `rover-rs agent session` for its options) | rover-rs --version";

fn version_banner() -> String {
    format!(
        "{BINARY_NAME} {} (Rust port preview; unreleased and not the Rover product binary, which is the Go `rover`)",
        env!("CARGO_PKG_VERSION")
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{BINARY_NAME}: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    match args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .as_deref()
    {
        Some("--version" | "version") => {
            println!("{}", version_banner());
            Ok(())
        }
        Some("tui") => run_tui(args.collect()),
        Some("agents") => run_agent_command("agents", args.collect()),
        Some("agent") => run_agent_command("agent", args.collect()),
        #[cfg(unix)]
        Some("__session_daemon") => run_daemon(args.collect()),
        _ => {
            eprintln!("{USAGE}");
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown command",
            ))
        }
    }
}

fn run_agent_command(command: &str, args: Vec<std::ffi::OsString>) -> io::Result<()> {
    #[cfg(unix)]
    if command == "agent" && args.first().and_then(|argument| argument.to_str()) == Some("session")
    {
        return run_agent_session_command(args);
    }
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "agent argument is not UTF-8")
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let value = agent_command_value(command, &args)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[cfg(unix)]
fn run_agent_session_command(args: Vec<std::ffi::OsString>) -> io::Result<()> {
    let mut args = args.into_iter();
    let _ = args.next(); // `session`
    let action = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(agent_session_usage)?;
    if !matches!(action.as_str(), "list" | "status" | "bind" | "clear") {
        return Err(agent_session_usage());
    }
    let mut options = BTreeMap::<String, OsString>::new();
    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "option is not UTF-8"))?;
        if !matches!(
            flag.as_str(),
            "--repo" | "--state" | "--pane" | "--adapter" | "--id"
        ) || options.contains_key(&flag)
        {
            return Err(agent_session_usage());
        }
        let value = args.next().ok_or_else(agent_session_usage)?;
        options.insert(flag, value);
    }
    let repository = options
        .get("--repo")
        .map(PathBuf::from)
        .ok_or_else(agent_session_usage)?;
    let repository = std::fs::canonicalize(repository)?;
    if !repository.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not a directory",
        ));
    }
    let pane = options
        .get("--pane")
        .map(|value| {
            value.to_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "pane identifier is not UTF-8")
            })
        })
        .transpose()?;
    let adapter = options
        .get("--adapter")
        .map(|value| value.to_str().ok_or_else(agent_session_usage))
        .transpose()?;
    let session_id = options
        .get("--id")
        .map(|value| value.to_str().ok_or_else(agent_session_usage))
        .transpose()?;
    let valid_options = match action.as_str() {
        "list" => pane.is_none() && adapter.is_none() && session_id.is_none(),
        "status" | "clear" => pane.is_some() && adapter.is_none() && session_id.is_none(),
        "bind" => pane.is_some() && adapter.is_some() && session_id.is_some(),
        _ => false,
    };
    if !valid_options {
        return Err(agent_session_usage());
    }
    let state_path = options
        .get("--state")
        .map(PathBuf::from)
        .map_or_else(default_state_path, Ok)?;
    let state = StateRoot::open(state_path)?;
    let repository_text = repository.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not valid UTF-8",
        )
    })?;
    let identity = RepositoryIdentity::resolve(repository_text)?;
    let pane = if action == "bind" {
        Some(resolve_agent_session_pane(
            &state,
            &identity,
            pane.expect("validated pane"),
        )?)
    } else {
        pane.map(str::to_owned)
    };
    let value = agent_session_command_value(
        state.records(),
        &identity,
        &action,
        pane.as_deref(),
        adapter,
        session_id,
    )?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[cfg(unix)]
fn agent_session_command_value(
    store: &rover_store::Store,
    project: &RepositoryIdentity,
    action: &str,
    pane: Option<&str>,
    adapter: Option<&str>,
    session_id: Option<&str>,
) -> io::Result<serde_json::Value> {
    let summarize = |binding: &rover_agents::AgentSessionBinding| {
        serde_json::json!({
            "pane_id": binding.pane_id(),
            "adapter": binding.adapter(),
            "session_id": binding.session_id(),
            "source": binding.source().as_str(),
            "binding_status": "bound",
            "agent_state": "unknown",
            "state_reason": "Rover has no live process or lifecycle observation for this binding"
        })
    };
    if action == "bind" {
        let binding = rover_agents::AgentSessionBinding::new(
            project,
            pane.ok_or_else(agent_session_usage)?,
            adapter.ok_or_else(agent_session_usage)?,
            session_id.ok_or_else(agent_session_usage)?,
            rover_agents::AgentSessionSource::ExplicitUser,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        rover_agents::save_session_binding(store, project, &binding)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        return Ok(serde_json::json!({
            "schema": rover_core::SCHEMA,
            "session": summarize(&binding)
        }));
    }
    if action == "clear" {
        let pane_id = pane.ok_or_else(agent_session_usage)?;
        rover_agents::clear_session_binding(store, project, pane_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        return Ok(serde_json::json!({
            "schema": rover_core::SCHEMA,
            "pane_id": pane_id,
            "binding_status": "cleared"
        }));
    }
    let sessions = rover_agents::list_session_bindings(store, project)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    match (action, pane) {
        ("list", None) => Ok(serde_json::json!({
            "schema": rover_core::SCHEMA,
            "sessions": sessions.iter().map(summarize).collect::<Vec<_>>()
        })),
        ("status", Some(pane_id)) => {
            let binding = sessions
                .iter()
                .find(|binding| binding.pane_id() == pane_id)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "agent session not found")
                })?;
            Ok(serde_json::json!({
                "schema": rover_core::SCHEMA,
                "session": summarize(binding)
            }))
        }
        _ => Err(agent_session_usage()),
    }
}

#[cfg(unix)]
fn agent_session_usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "agent session list --repo PATH [--state PATH] | agent session status --repo PATH --pane ID [--state PATH] | agent session bind --repo PATH --pane ID --adapter codex-exec|claude-print --id EXACT_ID [--state PATH] | agent session clear --repo PATH --pane ID [--state PATH]",
    )
}

#[cfg(unix)]
fn resolve_agent_session_pane(
    state: &StateRoot,
    project: &RepositoryIdentity,
    requested_pane_id: &str,
) -> io::Result<String> {
    let namespace = format!(
        "project_{}",
        Sha256Digest::of(project.path().as_os_str().as_encoded_bytes()).to_hex()
    );
    let layout_name = format!(
        "workspace_{}.json",
        Sha256Digest::of(namespace.as_bytes()).to_hex()
    );
    let app = state.root_handle().ensure_subdirectory("rust-tui")?;
    let layouts = app.ensure_subdirectory("layouts")?;
    let workspace = Workspace::load_from(&layouts, &layout_name)?;
    if workspace.namespace() != namespace {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved workspace belongs to another project",
        ));
    }
    let pane_id = if requested_pane_id == "active" {
        workspace.active_tab().active_pane()
    } else {
        requested_pane_id
    };
    if find_active_pane(workspace.active_tab().root(), pane_id).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "pane is not present in this project's saved workspace",
        ));
    }
    Ok(pane_id.to_owned())
}

fn agent_command_value(command: &str, args: &[String]) -> io::Result<serde_json::Value> {
    let has_json = args.last().is_some_and(|arg| arg == "--json");
    let args = if has_json {
        &args[..args.len() - 1]
    } else {
        args
    };
    let string_args = args.iter().map(String::as_str).collect::<Vec<_>>();
    match (command, string_args.as_slice()) {
        ("agents", [] | ["list"]) => {
            serde_json::to_value(rover_agents::profiles()).map_err(io::Error::other)
        }
        ("agent", [] | ["list"]) => Ok(serde_json::json!({
            "schema": rover_core::SCHEMA,
            "adapters": rover_agents::profiles(),
        })),
        ("agent", ["capabilities", name]) => {
            let descriptor = rover_agents::profile(name)?;
            serde_json::to_value(descriptor).map_err(io::Error::other)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "agents [--json] | agent list [--json] | agent capabilities <adapter> [--json]",
        )),
    }
}

#[cfg(unix)]
fn run_tui(args: Vec<std::ffi::OsString>) -> io::Result<()> {
    let options = parse_options(args)?;
    let cwd = options.repo.unwrap_or(env::current_dir()?);
    let cwd = std::fs::canonicalize(cwd)?;
    if !cwd.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not a directory",
        ));
    }
    let state_path = options.state.unwrap_or(default_state_path()?);
    let state = StateRoot::open(state_path)?;
    let tasks = load_project_tasks(&state, &cwd)?;
    let view_preferences = load_project_task_view_preferences(&state, &cwd)?;
    let workspace_notes = load_project_workspace_notes(&state, &cwd)?;
    let saved_commands = load_project_saved_commands(&state, &cwd)?;
    let app_dir = state.root_handle().ensure_subdirectory("rust-tui")?;
    let layouts = app_dir.ensure_subdirectory("layouts")?;
    let namespace = format!(
        "project_{}",
        Sha256Digest::of(cwd.as_os_str().as_encoded_bytes()).to_hex()
    );
    // Keep AF_UNIX socket names below platform path limits even when the state
    // root lives under a deeply nested home or temporary directory.
    let owner_uid = std::fs::metadata(state.path())?.uid();
    let state_hash = Sha256Digest::of(state.path().as_os_str().as_encoded_bytes()).to_hex();
    let session_dir = PathBuf::from("/tmp").join(format!("rv-{owner_uid}-{}", &state_hash[..24]));
    let key = SessionKey::new(&namespace, "shell")?;
    let title = cwd
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("workspace");
    let initial = Workspace::new(&namespace, title, "shell", &cwd)?;
    let layout_name = format!(
        "workspace_{}.json",
        Sha256Digest::of(namespace.as_bytes()).to_hex()
    );
    let workspace = match Workspace::load_from(&layouts, &layout_name) {
        Ok(workspace) => workspace,
        Err(error) if error.kind() == io::ErrorKind::NotFound => initial,
        Err(error) => return Err(error),
    };
    if workspace.namespace() != namespace {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved workspace belongs to a different project",
        ));
    }
    let pane_id = workspace.active_tab().active_pane().to_owned();
    let pane = find_active_pane(workspace.active_tab().root(), &pane_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "active pane is missing"))?;
    if !matches!(pane.kind(), PaneKind::Session { name } if name == key.name()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active pane does not refer to the project's shell session",
        ));
    }
    let (client, _daemon) = connect_or_start(&session_dir, &key, &cwd)?;
    let mut task_provider = TaskProvider::new(
        tasks,
        || load_project_tasks(&state, &cwd),
        |digest| state.read_blob(digest),
        |task_id| {
            let now = rover_core::Timestamp::now()
                .to_rfc3339()
                .map_err(|error| io::Error::other(error.to_string()))?;
            rover_execution::supervisor::request_cancel(state.records(), task_id, &now)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
        },
        |task| load_project_investigation(&state, &cwd, task),
        |task| load_project_diff(&state, &cwd, task),
    );
    task_provider.configure_view_preferences(&view_preferences, |preferences| {
        save_project_task_view_preferences(&state, &cwd, preferences)
    });
    task_provider.configure_workspace_notes(&workspace_notes, |notes| {
        save_project_workspace_notes(&state, &cwd, notes)
    });
    task_provider.configure_saved_commands(&saved_commands, |commands| {
        save_project_saved_commands(&state, &cwd, commands)
    });
    configure_task_drafts(&mut task_provider, &state, &cwd, &pane_id)?;
    configure_task_plans(&mut task_provider, &state, &cwd)?;
    task_provider.configure_file_browser(
        |directory, include_hidden| rover_source::browse_directory(&cwd, directory, include_hidden),
        || rover_source::quick_open_files(&cwd),
        |path| rover_source::read_repository_file(&cwd, path),
    );
    task_provider.configure_file_editor(
        |path| rover_source::read_repository_file_for_edit(&cwd, path),
        |path, expected_sha256, bytes| {
            rover_source::save_repository_file(&cwd, path, expected_sha256, bytes)
        },
        run_configured_editor,
    );
    rover_tui::run_attached_session_persistent_with_task_provider(
        workspace,
        &pane_id,
        client,
        &layouts,
        &layout_name,
        task_provider,
    )
}

#[cfg(unix)]
fn configure_saved_agent_session<'a>(
    provider: &mut TaskProvider<'a>,
    state: &'a StateRoot,
    project: &Path,
    pane_id: &str,
) -> io::Result<()> {
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not valid UTF-8",
        )
    })?;
    let identity = RepositoryIdentity::resolve(repository)?;
    match load_session_binding(state.records(), &identity, pane_id) {
        Ok(binding) => {
            let repository_for_clear = repository.to_owned();
            provider.configure_agent_session(&binding, move |bound_pane| {
                let identity =
                    RepositoryIdentity::resolve(&repository_for_clear).map_err(|error| {
                        io::Error::new(io::ErrorKind::InvalidData, error.to_string())
                    })?;
                clear_session_binding(state.records(), &identity, bound_pane)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
            })?;
        }
        Err(SessionBindingError::Store(rover_store::StoreError::NotFound)) => {}
        Err(error) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("could not load saved agent session: {error}"),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn configure_task_drafts<'a>(
    provider: &mut TaskProvider<'a>,
    state: &'a StateRoot,
    project: &'a Path,
    pane_id: &str,
) -> io::Result<()> {
    configure_saved_agent_session(provider, state, project, pane_id)?;
    let drafts = load_project_task_drafts(state, project)?;
    provider.configure_task_drafts(&drafts, |drafts| {
        save_project_task_drafts(state, project, drafts)
    });
    Ok(())
}

#[cfg(unix)]
fn configure_task_plans<'a>(
    provider: &mut TaskProvider<'a>,
    state: &'a StateRoot,
    project: &'a Path,
) -> io::Result<()> {
    let plans = load_project_task_plan_summaries(state, project)?;
    provider.configure_task_plans(
        &plans,
        || load_project_task_plan_summaries(state, project),
        |draft| {
            let repository = project.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "repository path is not valid UTF-8",
                )
            })?;
            let identity = rover_core::RepositoryIdentity::resolve(repository)?;
            let brief = rover_core::TaskBrief {
                title: draft.title.clone(),
                paths: draft.paths.clone(),
                dependencies: draft.dependencies.clone(),
                quality_gate: draft.quality_gate.clone(),
                prompt: draft.prompt.clone(),
            };
            let plan = rover_tasks::TaskPlanService::new(state.records())
                .create(&identity, brief)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
            let service = rover_tasks::TaskPlanService::new(state.records());
            let readiness = service
                .readiness(&identity, &plan.id)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            Ok(task_plan_summary(&plan, readiness))
        },
    );
    Ok(())
}

#[cfg(unix)]
fn load_project_task_plan_summaries(
    state: &StateRoot,
    project: &Path,
) -> io::Result<Vec<TaskPlanSummary>> {
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository path is not valid UTF-8",
        )
    })?;
    let identity = rover_core::RepositoryIdentity::resolve(repository)?;
    let service = rover_tasks::TaskPlanService::new(state.records());
    service
        .list_with_readiness(&identity)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
        .iter()
        .map(|(plan, readiness)| Ok(task_plan_summary(plan, *readiness)))
        .collect()
}

#[cfg(unix)]
fn task_plan_summary(
    plan: &rover_tasks::TaskPlan,
    readiness: rover_tasks::TaskReadiness,
) -> TaskPlanSummary {
    let readiness = match readiness {
        rover_tasks::TaskReadiness::Ready => "READY",
        rover_tasks::TaskReadiness::Waiting => "WAITING",
        rover_tasks::TaskReadiness::Blocked => "BLOCKED",
    };
    TaskPlanSummary {
        id: plan.id.to_string(),
        title: plan.brief.title.clone(),
        status: plan.status.as_str().to_owned(),
        readiness: readiness.to_owned(),
        dependency_count: plan.dependencies.len(),
        attempt_count: plan.attempts.len(),
        updated_at: plan.updated_at.clone(),
    }
}

#[cfg(unix)]
static EDITOR_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
struct EditorTempDirectory(PathBuf);

#[cfg(unix)]
impl Drop for EditorTempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the user's configured editor against a private copy, returning its
/// bytes for the TUI's normal stale-check and atomic-save path.
#[cfg(unix)]
fn run_configured_editor(relative_path: &str, original: &[u8]) -> io::Result<Vec<u8>> {
    let editor = env::var_os("VISUAL")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("EDITOR").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "vi".into());
    run_editor_program(relative_path, original, &editor)
}

#[cfg(unix)]
fn run_editor_program(relative_path: &str, original: &[u8], editor: &OsStr) -> io::Result<Vec<u8>> {
    const MAX_EDIT_BYTES: usize = 8 * 1024 * 1024;
    if original.len() > MAX_EDIT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file exceeds the 8 MiB external-edit limit",
        ));
    }
    let temporary = create_editor_temp_directory()?;
    let safe_directory = SafeDir::open(&temporary.0)?;
    let extension = Path::new(relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .unwrap_or("txt");
    let file_name = format!("working-copy.{extension}");
    let file_path = temporary.0.join(&file_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(&file_path)?;
    file.write_all(original)?;
    file.sync_all()?;
    drop(file);

    // VISUAL and EDITOR are executable paths, not shell command lines. This
    // avoids evaluating user configuration through a shell.
    let status = Command::new(editor).arg(&file_path).status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "external editor exited with {status}"
        )));
    }
    let (bytes, truncated) = safe_directory.read_regular_file_path(&file_name, MAX_EDIT_BYTES)?;
    if truncated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "external editor output exceeds the 8 MiB edit limit",
        ));
    }
    if bytes.len() > MAX_EDIT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "external editor output exceeds the 8 MiB edit limit",
        ));
    }
    if std::str::from_utf8(&bytes).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "external editor output is not valid UTF-8",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn create_editor_temp_directory() -> io::Result<EditorTempDirectory> {
    for _ in 0..128 {
        let sequence = EDITOR_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("rover-edit-{}-{sequence}", std::process::id()));
        match DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(EditorTempDirectory(path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private external-editor directory",
    ))
}

#[cfg(unix)]
fn project_task_view_preferences_id(project: &Path) -> String {
    format!(
        "tui_{}",
        Sha256Digest::of(project.as_os_str().as_encoded_bytes()).to_hex()
    )
}

#[cfg(unix)]
fn load_project_task_view_preferences(
    state: &StateRoot,
    project: &Path,
) -> io::Result<TaskViewPreferences> {
    let id = project_task_view_preferences_id(project);
    let record = match state
        .records()
        .get_typed::<serde_json::Value>("tui_preferences", &id)
    {
        Ok(record) => record,
        Err(rover_store::StoreError::NotFound) => return Ok(TaskViewPreferences::default()),
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Rover TUI preferences");
    if record.get("schema").and_then(serde_json::Value::as_str) != Some("rover/tui-task-view/v1")
        || record.get("repository").and_then(serde_json::Value::as_str) != project.to_str()
    {
        return Err(invalid());
    }
    let query = record
        .get("task_query")
        .and_then(serde_json::Value::as_str)
        .filter(|query| query.len() <= 128)
        .ok_or_else(invalid)?;
    let sort = record
        .get("task_sort")
        .and_then(serde_json::Value::as_str)
        .and_then(TaskSortOrder::parse)
        .ok_or_else(invalid)?;
    Ok(TaskViewPreferences {
        query: query.to_owned(),
        sort,
    })
}

#[cfg(unix)]
fn save_project_task_view_preferences(
    state: &StateRoot,
    project: &Path,
    preferences: &TaskViewPreferences,
) -> io::Result<()> {
    if preferences.query.len() > 128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task search exceeds 128 UTF-8 bytes",
        ));
    }
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "repository path is not UTF-8")
    })?;
    let record = serde_json::json!({
        "schema": "rover/tui-task-view/v1",
        "repository": repository,
        "task_query": preferences.query,
        "task_sort": preferences.sort.as_str(),
    });
    state
        .records()
        .put(
            "tui_preferences",
            &project_task_view_preferences_id(project),
            &record,
            "tui.preferences",
        )
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn project_workspace_notes_id(project: &Path) -> String {
    format!(
        "tui_notes_{}",
        Sha256Digest::of(project.as_os_str().as_encoded_bytes()).to_hex()
    )
}

#[cfg(unix)]
fn load_project_workspace_notes(state: &StateRoot, project: &Path) -> io::Result<Vec<String>> {
    let record = match state
        .records()
        .get_typed::<serde_json::Value>("tui_notes", &project_workspace_notes_id(project))
    {
        Ok(record) => record,
        Err(rover_store::StoreError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Rover repository notes");
    if record.get("repository").and_then(serde_json::Value::as_str) != project.to_str() {
        return Err(invalid());
    }
    let schema = record.get("schema").and_then(serde_json::Value::as_str);
    let notes = match schema {
        Some("rover/tui-notes/v1") => record
            .get("notes")
            .and_then(serde_json::Value::as_str)
            .filter(|note| note.len() <= MAX_WORKSPACE_NOTES_BYTES)
            .map(|note| {
                if note.is_empty() {
                    Vec::new()
                } else {
                    vec![note.to_owned()]
                }
            })
            .ok_or_else(invalid)?,
        Some("rover/tui-notes/v2") => record
            .get("notes")
            .and_then(serde_json::Value::as_array)
            .filter(|notes| notes.len() <= MAX_WORKSPACE_NOTES_COUNT)
            .and_then(|notes| {
                notes
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            })
            .filter(|notes| {
                notes
                    .iter()
                    .all(|note| note.len() <= MAX_WORKSPACE_NOTES_BYTES)
                    && notes.iter().map(|note| note.len()).sum::<usize>()
                        <= MAX_WORKSPACE_NOTES_TOTAL_BYTES
            })
            .map(|notes| notes.into_iter().map(str::to_owned).collect())
            .ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    Ok(notes)
}

#[cfg(unix)]
fn save_project_workspace_notes(
    state: &StateRoot,
    project: &Path,
    notes: &[String],
) -> io::Result<()> {
    if notes.len() > MAX_WORKSPACE_NOTES_COUNT
        || notes
            .iter()
            .any(|note| note.len() > MAX_WORKSPACE_NOTES_BYTES)
        || notes.iter().map(String::len).sum::<usize>() > MAX_WORKSPACE_NOTES_TOTAL_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace notes exceed collection limits",
        ));
    }
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "repository path is not UTF-8")
    })?;
    let record = serde_json::json!({
        "schema": "rover/tui-notes/v2",
        "repository": repository,
        "notes": notes,
    });
    state
        .records()
        .put(
            "tui_notes",
            &project_workspace_notes_id(project),
            &record,
            "tui.notes",
        )
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn project_saved_commands_id(project: &Path) -> String {
    format!(
        "tui_commands_{}",
        Sha256Digest::of(project.as_os_str().as_encoded_bytes()).to_hex()
    )
}

#[cfg(unix)]
fn load_project_saved_commands(state: &StateRoot, project: &Path) -> io::Result<Vec<SavedCommand>> {
    let record = match state
        .records()
        .get_typed::<serde_json::Value>("tui_saved_commands", &project_saved_commands_id(project))
    {
        Ok(record) => record,
        Err(rover_store::StoreError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Rover saved commands");
    if record.get("schema").and_then(serde_json::Value::as_str)
        != Some("rover/tui-saved-commands/v1")
        || record.get("repository").and_then(serde_json::Value::as_str) != project.to_str()
    {
        return Err(invalid());
    }
    let commands = record
        .get("commands")
        .and_then(serde_json::Value::as_array)
        .filter(|commands| commands.len() <= MAX_SAVED_COMMANDS)
        .and_then(|commands| {
            commands
                .iter()
                .map(|entry| {
                    Some(SavedCommand {
                        name: entry.get("name")?.as_str()?.to_owned(),
                        command: entry.get("command")?.as_str()?.to_owned(),
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(invalid)?;
    if commands
        .iter()
        .any(|command| !valid_saved_command_record(command))
        || commands.iter().enumerate().any(|(index, command)| {
            commands[..index]
                .iter()
                .any(|prior| prior.name == command.name)
        })
    {
        return Err(invalid());
    }
    Ok(commands)
}

#[cfg(unix)]
fn save_project_saved_commands(
    state: &StateRoot,
    project: &Path,
    commands: &[SavedCommand],
) -> io::Result<()> {
    if commands.len() > MAX_SAVED_COMMANDS
        || commands
            .iter()
            .any(|command| !valid_saved_command_record(command))
        || commands.iter().enumerate().any(|(index, command)| {
            commands[..index]
                .iter()
                .any(|prior| prior.name == command.name)
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "saved commands exceed limits or contain invalid/duplicate entries",
        ));
    }
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "repository path is not UTF-8")
    })?;
    let record = serde_json::json!({
        "schema": "rover/tui-saved-commands/v1",
        "repository": repository,
        "commands": commands.iter().map(|command| {
            serde_json::json!({"name": command.name, "command": command.command})
        }).collect::<Vec<_>>(),
    });
    state
        .records()
        .put(
            "tui_saved_commands",
            &project_saved_commands_id(project),
            &record,
            "tui.saved-command",
        )
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn valid_saved_command_record(command: &SavedCommand) -> bool {
    !command.name.trim().is_empty()
        && command.name.len() <= MAX_SAVED_COMMAND_NAME_BYTES
        && !command.command.trim().is_empty()
        && command.command.len() <= MAX_SAVED_COMMAND_LINE_BYTES
        && !command.command.contains('\n')
        && !command.command.contains('\r')
        && command
            .name
            .chars()
            .chain(command.command.chars())
            .all(|character| {
                !character.is_control()
                    && !matches!(
                        character,
                        '\u{061c}'
                            | '\u{200e}'
                            | '\u{200f}'
                            | '\u{202a}'..='\u{202e}'
                            | '\u{2066}'..='\u{2069}'
                            | '\u{feff}'
                    )
            })
}

#[cfg(unix)]
fn project_task_drafts_id(project: &Path) -> String {
    format!(
        "tui_drafts_{}",
        Sha256Digest::of(project.as_os_str().as_encoded_bytes()).to_hex()
    )
}

#[cfg(unix)]
fn load_project_task_drafts(state: &StateRoot, project: &Path) -> io::Result<Vec<TaskDraft>> {
    let record = match state
        .records()
        .get_typed::<serde_json::Value>("tui_task_drafts", &project_task_drafts_id(project))
    {
        Ok(record) => record,
        Err(rover_store::StoreError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid Rover task drafts");
    let schema = record.get("schema").and_then(serde_json::Value::as_str);
    let legacy_v1 = schema == Some("rover/tui-task-drafts/v1");
    if (!legacy_v1 && schema != Some("rover/tui-task-drafts/v2"))
        || record.get("repository").and_then(serde_json::Value::as_str) != project.to_str()
    {
        return Err(invalid());
    }
    let drafts = record
        .get("drafts")
        .and_then(serde_json::Value::as_array)
        .filter(|drafts| drafts.len() <= MAX_TASK_DRAFTS)
        .and_then(|drafts| {
            drafts
                .iter()
                .map(|entry| {
                    Some(TaskDraft {
                        title: entry.get("title")?.as_str()?.to_owned(),
                        paths: if legacy_v1 {
                            String::new()
                        } else {
                            entry.get("paths")?.as_str()?.to_owned()
                        },
                        dependencies: if legacy_v1 {
                            String::new()
                        } else {
                            entry.get("dependencies")?.as_str()?.to_owned()
                        },
                        quality_gate: if legacy_v1 {
                            String::new()
                        } else {
                            entry.get("quality_gate")?.as_str()?.to_owned()
                        },
                        prompt: entry.get("prompt")?.as_str()?.to_owned(),
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(invalid)?;
    if drafts.iter().any(|draft| !valid_task_draft_record(draft)) {
        return Err(invalid());
    }
    Ok(drafts)
}

#[cfg(unix)]
fn save_project_task_drafts(
    state: &StateRoot,
    project: &Path,
    drafts: &[TaskDraft],
) -> io::Result<()> {
    if drafts.len() > MAX_TASK_DRAFTS || drafts.iter().any(|draft| !valid_task_draft_record(draft))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task drafts exceed limits or contain unsupported control text",
        ));
    }
    let repository = project.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "repository path is not UTF-8")
    })?;
    let record = serde_json::json!({
        "schema": "rover/tui-task-drafts/v2",
        "repository": repository,
        "drafts": drafts.iter().map(|draft| {
            serde_json::json!({
                "title": draft.title,
                "paths": draft.paths,
                "dependencies": draft.dependencies,
                "quality_gate": draft.quality_gate,
                "prompt": draft.prompt,
            })
        }).collect::<Vec<_>>(),
    });
    state
        .records()
        .put(
            "tui_task_drafts",
            &project_task_drafts_id(project),
            &record,
            "tui.task-drafts",
        )
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn valid_task_draft_record(draft: &TaskDraft) -> bool {
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

#[cfg(unix)]
fn load_project_tasks(state: &StateRoot, project: &Path) -> io::Result<Vec<TaskSummary>> {
    let records = state
        .records()
        .list_all("task", 1_000)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut summaries = Vec::new();
    for record in records {
        let id = required_record_string(&record, &["id"])?;
        let status = required_record_string(&record, &["status"])?;
        let updated_at = required_record_string(&record, &["updated_at"])?;
        let objective = required_record_string(&record, &["contract", "objective"])?;
        let repository = required_record_string(&record, &["contract", "repository"])?;
        let error = optional_record_string(&record, &["error"])?.filter(|text| !text.is_empty());
        let candidate =
            optional_record_string(&record, &["candidate"])?.filter(|text| !text.is_empty());
        let investigation_id =
            optional_record_string(&record, &["investigation_id"])?.filter(|text| !text.is_empty());
        let base_snapshot =
            optional_record_string(&record, &["base_snapshot"])?.filter(|text| !text.is_empty());
        rover_core::Id::parse(id.to_owned()).map_err(|_| invalid_task_record("id"))?;
        if let Some(investigation_id) = investigation_id {
            rover_core::Id::parse(investigation_id.to_owned())
                .map_err(|_| invalid_task_record("investigation_id"))?;
        }
        let repository = Path::new(repository);
        if !repository.is_absolute() {
            return Err(invalid_task_record("contract.repository"));
        }
        let same_project = repository == project
            || std::fs::canonicalize(repository).is_ok_and(|canonical| canonical == project);
        if same_project {
            let attempt_count = if record.contains_path(&["attempts"]) {
                record
                    .array_len_at_path(&["attempts"])
                    .ok_or_else(|| invalid_task_record("attempts"))?
            } else {
                0
            };
            let process = if record.contains_path(&["process"]) {
                Some(parse_task_process(&record)?)
            } else {
                None
            };
            summaries.push(TaskSummary {
                id: id.to_owned(),
                objective: objective.to_owned(),
                status: status.to_owned(),
                updated_at: updated_at.to_owned(),
                error: error.map(str::to_owned),
                candidate: candidate.map(str::to_owned),
                base_snapshot: base_snapshot.map(str::to_owned),
                investigation_id: investigation_id.map(str::to_owned),
                attempt_count,
                process,
            });
        }
    }
    Ok(summaries)
}

#[cfg(unix)]
fn load_project_investigation(
    state: &StateRoot,
    project: &Path,
    task: &TaskSummary,
) -> io::Result<Option<TaskInvestigationEvidence>> {
    let Some(investigation_id) = task.investigation_id.as_deref() else {
        return Ok(None);
    };
    let record = state
        .records()
        .get("investigation", investigation_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let id = required_record_string(&record, &["id"])?;
    if id != investigation_id {
        return Err(invalid_evidence_record("id does not match task reference"));
    }
    let schema = required_record_string(&record, &["schema"])?;
    if schema != "rover/v1alpha1" {
        return Err(invalid_evidence_record("schema"));
    }
    let repository = required_record_string(&record, &["repository"])?;
    let repository = Path::new(repository);
    if !repository.is_absolute()
        || !(repository == project
            || std::fs::canonicalize(repository).is_ok_and(|canonical| canonical == project))
    {
        return Err(invalid_evidence_record("repository does not match project"));
    }
    let candidate = required_record_string(&record, &["candidate"])?;
    if task.candidate.as_deref() != Some(candidate) {
        return Err(invalid_evidence_record("candidate does not match task"));
    }
    let decision = required_record_string(&record, &["decision"])?;
    let decision_reason = optional_record_string(&record, &["decision_reason"])?
        .unwrap_or_default()
        .to_owned();

    let check_values = record
        .array_values_at_path(&["checks"])
        .ok_or_else(|| invalid_evidence_record("checks"))?;
    if check_values.len() > 256 {
        return Err(invalid_evidence_record("too many checks"));
    }
    let mut checks = Vec::with_capacity(check_values.len());
    for value in check_values {
        checks.push(TaskCheckEvidence {
            id: required_evidence_value_string(value, "id")?.to_owned(),
            outcome: required_evidence_value_string(value, "outcome")?.to_owned(),
            meaning: required_evidence_value_string(value, "meaning")?.to_owned(),
            required: required_evidence_value_bool(value, "required")?,
            tests: required_evidence_value_integer(value, "tests")?,
            skipped: required_evidence_value_integer(value, "skipped")?,
        });
        if checks
            .last()
            .is_some_and(|check| check.tests < 0 || check.skipped < 0)
        {
            return Err(invalid_evidence_record("negative check counts"));
        }
    }

    let unknown_values = record
        .array_values_at_path(&["unknowns"])
        .ok_or_else(|| invalid_evidence_record("unknowns"))?;
    if unknown_values.len() > 256 {
        return Err(invalid_evidence_record("too many unknowns"));
    }
    let unknowns = unknown_values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid_evidence_record("unknowns item"))
        })
        .collect::<io::Result<Vec<_>>>()?;

    let finding_values = record
        .array_values_at_path(&["findings"])
        .ok_or_else(|| invalid_evidence_record("findings"))?;
    if finding_values.len() > 256 {
        return Err(invalid_evidence_record("too many findings"));
    }
    let mut findings = Vec::with_capacity(finding_values.len());
    for value in finding_values {
        findings.push(TaskEvidenceFinding {
            path: optional_evidence_value_string(value, "path")?
                .unwrap_or_default()
                .to_owned(),
            message: required_evidence_value_string(value, "message")?.to_owned(),
            severity: optional_evidence_value_string(value, "severity")?.map(str::to_owned),
        });
    }

    Ok(Some(TaskInvestigationEvidence {
        id: id.to_owned(),
        candidate: candidate.to_owned(),
        decision: decision.to_owned(),
        decision_reason,
        checks,
        unknowns,
        findings,
    }))
}

#[cfg(unix)]
fn load_project_diff(
    state: &StateRoot,
    project: &Path,
    task: &TaskSummary,
) -> io::Result<Option<TaskDiffEvidence>> {
    let (Some(base_id), Some(candidate_id)) =
        (task.base_snapshot.as_deref(), task.candidate.as_deref())
    else {
        return Ok(None);
    };
    rover_core::Id::parse(base_id.to_owned()).map_err(|_| invalid_task_record("base_snapshot"))?;
    rover_core::Id::parse(candidate_id.to_owned()).map_err(|_| invalid_task_record("candidate"))?;

    let base = state
        .records()
        .get_typed::<rover_source::Snapshot>("snapshot", base_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let candidate = state
        .records()
        .get_typed::<rover_source::Snapshot>("snapshot", candidate_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if base.id != base_id || candidate.id != candidate_id {
        return Err(invalid_task_record("snapshot record ID"));
    }
    for snapshot in [&base, &candidate] {
        let repository = Path::new(&snapshot.repository);
        if !repository.is_absolute()
            || !(repository == project
                || std::fs::canonicalize(repository).is_ok_and(|canonical| canonical == project))
        {
            return Err(invalid_task_record(
                "snapshot repository does not match project",
            ));
        }
        rover_source::verify_retained_snapshot(state, &snapshot.id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }
    let comparison = rover_source::compare_snapshots(&base, &candidate)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if comparison.changes.len() > rover_source::MAX_FILES.saturating_mul(2) {
        return Err(invalid_task_record("too many snapshot changes"));
    }
    Ok(Some(TaskDiffEvidence {
        base_snapshot: base.id,
        candidate: candidate.id,
        changes: comparison
            .changes
            .into_iter()
            .map(|change| TaskDiffChange {
                path: change.path,
                status: change.status,
                category: change.category,
            })
            .collect(),
    }))
}

#[cfg(unix)]
fn required_evidence_value_string<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> io::Result<&'a str> {
    optional_evidence_value_string(value, field)?
        .filter(|text| !text.is_empty())
        .ok_or_else(|| invalid_evidence_record(field))
}

#[cfg(unix)]
fn optional_evidence_value_string<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> io::Result<Option<&'a str>> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_evidence_record("array item must be an object"))?;
    match object.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| invalid_evidence_record(field)),
    }
}

#[cfg(unix)]
fn required_evidence_value_bool(value: &serde_json::Value, field: &str) -> io::Result<bool> {
    value
        .as_object()
        .and_then(|object| object.get(field))
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| invalid_evidence_record(field))
}

#[cfg(unix)]
fn required_evidence_value_integer(value: &serde_json::Value, field: &str) -> io::Result<i64> {
    value
        .as_object()
        .and_then(|object| object.get(field))
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| invalid_evidence_record(field))
}

#[cfg(unix)]
fn invalid_evidence_record(field: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid investigation record: {field}"),
    )
}

#[cfg(unix)]
fn parse_task_process(record: &RecordValue) -> io::Result<TaskProcessEvidence> {
    let stdout_sha256 = required_record_string(record, &["process", "stdout_sha256"])?;
    let stderr_sha256 = required_record_string(record, &["process", "stderr_sha256"])?;
    rover_core::Sha256Digest::parse_hex(stdout_sha256)
        .map_err(|_| invalid_task_record("process.stdout_sha256"))?;
    rover_core::Sha256Digest::parse_hex(stderr_sha256)
        .map_err(|_| invalid_task_record("process.stderr_sha256"))?;
    let stdout_bytes = required_record_integer(record, &["process", "stdout_bytes"])?;
    let stderr_bytes = required_record_integer(record, &["process", "stderr_bytes"])?;
    if stdout_bytes < 0 || stderr_bytes < 0 {
        return Err(invalid_task_record("process output byte count"));
    }
    Ok(TaskProcessEvidence {
        exit_code: required_record_integer(record, &["process", "exit_code"])?,
        error: optional_record_string(record, &["process", "error"])?
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        timed_out: required_record_boolean(record, &["process", "timed_out"])?,
        cancelled: required_record_boolean(record, &["process", "cancelled"])?,
        truncated: required_record_boolean(record, &["process", "truncated"])?,
        stdout_bytes,
        stderr_bytes,
        stdout_sha256: stdout_sha256.to_owned(),
        stderr_sha256: stderr_sha256.to_owned(),
    })
}

#[cfg(unix)]
fn required_record_string<'a>(record: &'a RecordValue, path: &[&str]) -> io::Result<&'a str> {
    record
        .string_at_path(path)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| invalid_task_record(&path.join(".")))
}

#[cfg(unix)]
fn optional_record_string<'a>(
    record: &'a RecordValue,
    path: &[&str],
) -> io::Result<Option<&'a str>> {
    if record.contains_path(path) {
        record
            .string_at_path(path)
            .map(Some)
            .ok_or_else(|| invalid_task_record(&path.join(".")))
    } else {
        Ok(None)
    }
}

#[cfg(unix)]
fn required_record_integer(record: &RecordValue, path: &[&str]) -> io::Result<i64> {
    record
        .integer_at_path(path)
        .ok_or_else(|| invalid_task_record(&path.join(".")))
}

#[cfg(unix)]
fn required_record_boolean(record: &RecordValue, path: &[&str]) -> io::Result<bool> {
    record
        .bool_at_path(path)
        .ok_or_else(|| invalid_task_record(&path.join(".")))
}

#[cfg(unix)]
fn invalid_task_record(field: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("stored Rover task has missing or invalid {field}"),
    )
}

#[cfg(unix)]
fn find_active_pane<'a>(
    node: &'a rover_execution::layout::PaneNode,
    pane_id: &str,
) -> Option<&'a rover_execution::layout::Pane> {
    match node {
        rover_execution::layout::PaneNode::Leaf(pane) => (pane.id() == pane_id).then_some(pane),
        rover_execution::layout::PaneNode::Split(split) => find_active_pane(split.first(), pane_id)
            .or_else(|| find_active_pane(split.second(), pane_id)),
    }
}

#[cfg(unix)]
fn connect_or_start(
    dir: &Path,
    key: &SessionKey,
    cwd: &Path,
) -> io::Result<(SessionClient, Option<Child>)> {
    match SessionClient::connect_named(dir, key) {
        Ok(client) => return Ok((client, None)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let shell = env::var_os("SHELL")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "SHELL is not set"))?;
    if !shell.is_absolute() || !shell.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SHELL must name an existing absolute executable",
        ));
    }
    let executable = env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("__session_daemon")
        .arg("--sessions")
        .arg(dir)
        .arg("--namespace")
        .arg(key.namespace())
        .arg("--name")
        .arg(key.name())
        .arg("--cwd")
        .arg(cwd)
        .arg("--shell")
        .arg(&shell)
        .env_clear();
    for name in ["PATH", "HOME", "USER", "LOGNAME", "TERM", "LANG", "SHELL"] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in env::vars_os() {
        if name.to_string_lossy().starts_with("LC_") {
            command.env(name, value);
        }
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(client) = SessionClient::connect_named(dir, key) {
            return Ok((client, Some(child)));
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "local session owner exited during startup ({status})"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "local session owner did not become ready",
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn run_daemon(args: Vec<std::ffi::OsString>) -> io::Result<()> {
    let values = parse_named(args)?;
    let sessions = required_path(&values, "sessions")?;
    let namespace = required_string(&values, "namespace")?;
    let name = required_string(&values, "name")?;
    let cwd = required_path(&values, "cwd")?;
    let shell = required_path(&values, "shell")?;
    let key = SessionKey::new(namespace, name)?;
    let mut child_env = BTreeMap::new();
    for name in ["PATH", "HOME", "USER", "LOGNAME", "TERM", "LANG", "SHELL"] {
        if let Some(value) = env::var_os(name).and_then(|value| value.into_string().ok()) {
            child_env.insert(name.to_owned(), value);
        }
    }
    for (name, value) in env::vars() {
        if name.starts_with("LC_") {
            child_env.insert(name, value);
        }
    }
    let shell = shell
        .into_os_string()
        .into_string()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "shell path is not UTF-8"))?;
    let server = SessionServer::new(sessions).map_err(|error| {
        io::Error::new(error.kind(), format!("prepare session server: {error}"))
    })?;
    server
        .spawn(
            key.clone(),
            &PtyOptions {
                argv: vec![shell, "-l".to_owned()],
                cwd,
                env: child_env,
                rows: 24,
                cols: 80,
            },
        )
        .map_err(|error| io::Error::new(error.kind(), format!("start shell PTY: {error}")))?;
    while server
        .list()?
        .iter()
        .any(|session| session.key == key && session.running)
    {
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(unix)]
fn parse_options(args: Vec<std::ffi::OsString>) -> io::Result<CliOptions> {
    let mut result = CliOptions::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let flag = flag.to_string_lossy();
        let value = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("missing value for {flag}"),
            )
        })?;
        match flag.as_ref() {
            "--state" => result.state = Some(value.into()),
            "--repo" => result.repo = Some(value.into()),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown option {flag}"),
                ))
            }
        }
    }
    Ok(result)
}

#[cfg(unix)]
#[derive(Default)]
struct CliOptions {
    state: Option<PathBuf>,
    repo: Option<PathBuf>,
}

#[cfg(unix)]
fn default_state_path() -> io::Result<PathBuf> {
    if let Some(path) = env::var_os("ROVER_RUST_HOME") {
        return Ok(PathBuf::from(path));
    }
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "set ROVER_RUST_HOME, XDG_STATE_HOME, or HOME",
            )
        })?;
    Ok(base.join("rover-rust"))
}

#[cfg(unix)]
fn parse_named(args: Vec<std::ffi::OsString>) -> io::Result<BTreeMap<String, std::ffi::OsString>> {
    let mut values = BTreeMap::new();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let flag = flag.to_string_lossy().into_owned();
        let flag = flag.strip_prefix("--").map(str::to_owned).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid internal option")
        })?;
        if !["sessions", "namespace", "name", "cwd", "shell"].contains(&flag.as_str())
            || values.contains_key(&flag)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown or duplicate internal option",
            ));
        }
        values.insert(
            flag,
            args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "missing internal option value")
            })?,
        );
    }
    Ok(values)
}

#[cfg(unix)]
fn required_path(values: &BTreeMap<String, std::ffi::OsString>, key: &str) -> io::Result<PathBuf> {
    values
        .get(key)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {key}")))
}

#[cfg(unix)]
fn required_string(values: &BTreeMap<String, std::ffi::OsString>, key: &str) -> io::Result<String> {
    values
        .get(key)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("missing or invalid {key}"),
            )
        })
}

#[cfg(not(unix))]
fn run_tui(_args: Vec<std::ffi::OsString>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "rover-rs tui session hosting currently requires Unix",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn agent_cli_lists_profiles_and_fails_closed_for_unknown_capabilities() {
        let listed = agent_command_value("agents", &[]).unwrap();
        let profiles = listed.as_array().unwrap();
        assert_eq!(profiles.len(), 4);
        assert!(profiles.iter().any(|profile| {
            profile["name"] == "claude-print"
                && profile["executable"] == "claude"
                && profile["permission_mediation"] == false
        }));

        let descriptor = agent_command_value(
            "agent",
            &["capabilities".to_owned(), "codex-exec".to_owned()],
        )
        .unwrap();
        assert_eq!(descriptor["name"], "codex-exec");
        assert_eq!(descriptor["structured"], true);
        assert!(agent_command_value(
            "agent",
            &["capabilities".to_owned(), "unknown-profile".to_owned()]
        )
        .is_err());
    }

    #[test]
    fn agent_list_command_retains_go_schema_envelope() {
        let listed = agent_command_value("agent", &["list".to_owned()]).unwrap();
        assert_eq!(listed["schema"], rover_core::SCHEMA);
        assert_eq!(listed["adapters"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn agent_session_inventory_is_scoped_and_live_status_stays_unknown() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project_path = root.join("project");
        let other_path = root.join("other");
        fs::create_dir_all(&state_path).unwrap();
        fs::create_dir_all(&project_path).unwrap();
        fs::create_dir_all(&other_path).unwrap();
        let state = StateRoot::open(&state_path).unwrap();
        let project = RepositoryIdentity::resolve(project_path.to_str().unwrap()).unwrap();
        let other = RepositoryIdentity::resolve(other_path.to_str().unwrap()).unwrap();
        for (identity, pane, adapter, session) in [
            (&project, "pane-2", "codex-exec", "thread-two"),
            (&project, "pane-1", "claude-print", "session-one"),
            (&other, "pane-x", "codex-exec", "thread-other"),
        ] {
            let binding = rover_agents::AgentSessionBinding::new(
                identity,
                pane,
                adapter,
                session,
                rover_agents::AgentSessionSource::ExplicitUser,
            )
            .unwrap();
            rover_agents::save_session_binding(state.records(), identity, &binding).unwrap();
        }

        let inventory =
            agent_session_command_value(state.records(), &project, "list", None, None, None)
                .unwrap();
        assert_eq!(inventory["schema"], rover_core::SCHEMA);
        assert_eq!(inventory["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(inventory["sessions"][0]["pane_id"], "pane-1");
        assert_eq!(inventory["sessions"][0]["binding_status"], "bound");
        assert_eq!(inventory["sessions"][0]["agent_state"], "unknown");
        assert!(inventory["sessions"][0]["state_reason"]
            .as_str()
            .unwrap()
            .contains("no live process"));

        let status = agent_session_command_value(
            state.records(),
            &project,
            "status",
            Some("pane-2"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(status["session"]["adapter"], "codex-exec");
        assert_eq!(status["session"]["session_id"], "thread-two");
        assert_eq!(status["session"]["agent_state"], "unknown");
        assert!(agent_session_command_value(
            state.records(),
            &project,
            "status",
            Some("pane-x"),
            None,
            None
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_session_bind_requires_a_saved_project_pane_and_clear_is_explicit() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project_path = root.join("project");
        fs::create_dir_all(&state_path).unwrap();
        fs::create_dir_all(&project_path).unwrap();
        let state = StateRoot::open(&state_path).unwrap();
        let project = RepositoryIdentity::resolve(project_path.to_str().unwrap()).unwrap();
        let namespace = format!(
            "project_{}",
            Sha256Digest::of(project.path().as_os_str().as_encoded_bytes()).to_hex()
        );
        let workspace = Workspace::new(&namespace, "project", "shell", &project_path).unwrap();
        let pane = workspace.active_tab().active_pane().to_owned();
        let app = state.root_handle().ensure_subdirectory("rust-tui").unwrap();
        let layouts = app.ensure_subdirectory("layouts").unwrap();
        let layout_name = format!(
            "workspace_{}.json",
            Sha256Digest::of(namespace.as_bytes()).to_hex()
        );
        workspace.save_to(&layouts, &layout_name).unwrap();
        assert_eq!(
            resolve_agent_session_pane(&state, &project, "active").unwrap(),
            pane
        );
        assert!(resolve_agent_session_pane(&state, &project, "pane-foreign").is_err());

        let bound = agent_session_command_value(
            state.records(),
            &project,
            "bind",
            Some(&pane),
            Some("codex-exec"),
            Some("thread-explicit"),
        )
        .unwrap();
        assert_eq!(bound["session"]["source"], "user-selected");
        assert_eq!(bound["session"]["session_id"], "thread-explicit");

        let cleared = agent_session_command_value(
            state.records(),
            &project,
            "clear",
            Some(&pane),
            None,
            None,
        )
        .unwrap();
        assert_eq!(cleared["binding_status"], "cleared");
        let empty =
            agent_session_command_value(state.records(), &project, "list", None, None, None)
                .unwrap();
        assert!(empty["sessions"].as_array().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_editor_uses_private_temp_directory_and_rooted_readback() {
        let temporary = create_editor_temp_directory().unwrap();
        let metadata = fs::metadata(&temporary.0).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o700);
        let path = temporary.0.clone();
        drop(temporary);
        assert!(!path.exists());

        let source = b"fn main() {}\n";
        let edited =
            run_editor_program("src/main.rs", source, OsStr::new("/usr/bin/true")).unwrap();
        assert_eq!(edited, source);
        let editor_script_dir = create_editor_temp_directory().unwrap();
        let editor_script = editor_script_dir.0.join("editor");
        fs::write(
            &editor_script,
            b"#!/bin/sh\nprintf 'fn main() { edited(); }\\n' > \"$1\"\n",
        )
        .unwrap();
        fs::set_permissions(&editor_script, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            run_editor_program("src/main.rs", source, editor_script.as_os_str()).unwrap(),
            b"fn main() { edited(); }\n"
        );
        fs::write(
            &editor_script,
            b"#!/bin/sh\nrm -f \"$1\"\nln -s /etc/passwd \"$1\"\n",
        )
        .unwrap();
        assert!(run_editor_program("src/main.rs", source, editor_script.as_os_str()).is_err());
        assert_eq!(
            run_editor_program("src/main.rs", source, OsStr::new("/usr/bin/false"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Other
        );
        assert!(run_editor_program(
            "src/main.rs",
            &vec![b'x'; 8 * 1024 * 1024 + 1],
            OsStr::new("/usr/bin/true")
        )
        .is_err());
    }

    fn insert_task(
        database: &Path,
        id: &str,
        repository: &Path,
        objective: &str,
        include_objective: bool,
    ) {
        let contract = if include_objective {
            format!(
                r#"{{"repository":"{}","objective":"{}"}}"#,
                repository.display(),
                objective
            )
        } else {
            format!(r#"{{"repository":"{}"}}"#, repository.display())
        };
        let stdout_hash = "a".repeat(64);
        let stderr_hash = "b".repeat(64);
        let payload = format!(
            r#"{{"id":"{id}","contract":{contract},"status":"RUNNING","updated_at":"2026-09-26T00:00:00Z","candidate":"snapshot_a","attempts":[{{"number":1}},{{"number":2}}],"process":{{"exit_code":3,"error":"process failed","timed_out":false,"cancelled":false,"truncated":true,"stdout_sha256":"{stdout_hash}","stderr_sha256":"{stderr_hash}","stdout_bytes":44,"stderr_bytes":12}}}}"#
        );
        insert_raw_record(database, id, &payload);
    }

    fn insert_raw_record(database: &Path, id: &str, payload: &str) {
        let connection = rusqlite::Connection::open(database).expect("open test state database");
        connection
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES('task',?1,?2,'2026-09-26T00:00:00Z')",
                rusqlite::params![id, payload],
            )
            .expect("insert Go-compatible task record");
    }

    fn insert_investigation_record(database: &Path, id: &str, payload: &str) {
        let connection = rusqlite::Connection::open(database).expect("open test state database");
        connection
            .execute(
                "INSERT INTO records(kind,id,payload,updated) VALUES('investigation',?1,?2,'2026-09-26T00:00:00Z')",
                rusqlite::params![id, payload],
            )
            .expect("insert Go-compatible investigation record");
    }

    fn test_snapshot(
        repository: &Path,
        files: Vec<rover_source::SnapshotFile>,
    ) -> rover_source::Snapshot {
        let repository = repository.display().to_string();
        let files_json = serde_json::to_string(&files).expect("serialize snapshot file fixture");
        let identity = format!(r#"{{"Repository":"{repository}","Files":{files_json}}}"#);
        let id = format!(
            "snap_{}",
            rover_core::Sha256Digest::of(identity.as_bytes()).to_hex()
        );
        rover_source::Snapshot {
            schema: rover_core::SCHEMA.to_owned(),
            id,
            repository,
            source_ref: "HEAD".to_owned(),
            commit: "a".repeat(40),
            files,
            created_at: "2026-09-26T00:00:00Z".to_owned(),
            consistency: "exact Git commit blobs; no checkout filters".to_owned(),
        }
    }

    fn fixture_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "rover-cli-board-{}-{unique}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn project_task_board_filters_go_records_and_fails_closed_on_malformed_rows() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project_a = root.join("repo-a");
        let project_b = root.join("repo-b");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project_a).expect("create project A");
        fs::create_dir_all(&project_b).expect("create project B");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let database = state_path.join("rover.db");
        insert_task(&database, "task_a", &project_a, "Inspect A", true);
        insert_task(&database, "task_b", &project_b, "Inspect B", true);

        let tasks = load_project_tasks(&state, &project_a).expect("load project task summaries");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task_a");
        assert_eq!(tasks[0].objective, "Inspect A");
        assert_eq!(tasks[0].status, "RUNNING");
        assert_eq!(tasks[0].candidate.as_deref(), Some("snapshot_a"));
        assert_eq!(tasks[0].attempt_count, 2);
        let process = tasks[0].process.as_ref().expect("process evidence");
        assert_eq!(process.exit_code, 3);
        assert_eq!(process.stdout_bytes, 44);
        assert_eq!(process.stderr_bytes, 12);
        assert!(process.truncated);
        assert_eq!(process.error.as_deref(), Some("process failed"));

        insert_task(&database, "task_bad", &project_a, "", false);
        assert_eq!(
            load_project_tasks(&state, &project_a)
                .expect_err("malformed task row must fail closed")
                .kind(),
            io::ErrorKind::InvalidData
        );
        state
            .records()
            .delete("task", "task_bad", "test.cleanup")
            .expect("remove malformed task row");
        insert_raw_record(
            &database,
            "task_bad_process",
            &format!(
                r#"{{"id":"task_bad_process","contract":{{"repository":"{}","objective":"bad process"}},"status":"FAILED","updated_at":"2026-09-26T00:00:00Z","process":{{"exit_code":"3"}}}}"#,
                project_a.display()
            ),
        );
        assert_eq!(
            load_project_tasks(&state, &project_a)
                .expect_err("malformed process result must fail closed")
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_view_preferences_round_trip_and_fail_closed_on_invalid_records() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let expected = TaskViewPreferences {
            query: "backend fix".to_owned(),
            sort: TaskSortOrder::Objective,
        };

        assert_eq!(
            load_project_task_view_preferences(&state, &project).unwrap(),
            TaskViewPreferences::default()
        );
        save_project_task_view_preferences(&state, &project, &expected).unwrap();
        assert_eq!(
            load_project_task_view_preferences(&state, &project).unwrap(),
            expected
        );

        let id = project_task_view_preferences_id(&project);
        let malformed = serde_json::json!({
            "schema": "rover/tui-task-view/v1",
            "repository": project.to_str().unwrap(),
            "task_query": "x".repeat(129),
            "task_sort": "updated",
        });
        state
            .records()
            .put("tui_preferences", &id, &malformed, "test.malformed")
            .unwrap();
        assert_eq!(
            load_project_task_view_preferences(&state, &project)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn workspace_notes_round_trip_is_project_bound_and_byte_limited() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");

        assert_eq!(
            load_project_workspace_notes(&state, &project).unwrap(),
            Vec::<String>::new()
        );
        let expected = vec![
            "review prompt\ncheck CI".to_owned(),
            "release checklist".to_owned(),
        ];
        save_project_workspace_notes(&state, &project, &expected).unwrap();
        assert_eq!(
            load_project_workspace_notes(&state, &project).unwrap(),
            expected
        );
        assert_eq!(
            save_project_workspace_notes(
                &state,
                &project,
                &["x".repeat(MAX_WORKSPACE_NOTES_BYTES + 1)]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            save_project_workspace_notes(
                &state,
                &project,
                &vec![String::new(); MAX_WORKSPACE_NOTES_COUNT + 1]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            save_project_workspace_notes(
                &state,
                &project,
                &vec!["x".repeat(MAX_WORKSPACE_NOTES_BYTES); 5]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn workspace_notes_v1_migration_and_repository_binding() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let id = project_workspace_notes_id(&project);
        let legacy = serde_json::json!({
            "schema": "rover/tui-notes/v1",
            "repository": project.to_str().unwrap(),
            "notes": "legacy prompt",
        });
        state
            .records()
            .put("tui_notes", &id, &legacy, "test.legacy")
            .unwrap();
        assert_eq!(
            load_project_workspace_notes(&state, &project).unwrap(),
            ["legacy prompt"]
        );
        save_project_workspace_notes(
            &state,
            &project,
            &load_project_workspace_notes(&state, &project).unwrap(),
        )
        .unwrap();
        assert_eq!(
            state
                .records()
                .get_typed::<serde_json::Value>("tui_notes", &id)
                .unwrap()
                .get("schema")
                .and_then(serde_json::Value::as_str),
            Some("rover/tui-notes/v2")
        );
        let invalid = serde_json::json!({
            "schema": "rover/tui-notes/v2",
            "repository": root.join("other-repo").to_string_lossy(),
            "notes": ["must not cross project boundary"],
        });
        state
            .records()
            .put("tui_notes", &id, &invalid, "test.invalid")
            .unwrap();
        assert_eq!(
            load_project_workspace_notes(&state, &project)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn saved_commands_round_trip_and_reject_invalid_lines_and_count() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let expected = vec![
            SavedCommand {
                name: "Check Rust".to_owned(),
                command: "cargo test --workspace".to_owned(),
            },
            SavedCommand {
                name: "List files".to_owned(),
                command: "git status --short".to_owned(),
            },
        ];

        assert!(load_project_saved_commands(&state, &project)
            .unwrap()
            .is_empty());
        save_project_saved_commands(&state, &project, &expected).unwrap();
        assert_eq!(
            load_project_saved_commands(&state, &project).unwrap(),
            expected
        );
        assert_eq!(
            save_project_saved_commands(
                &state,
                &project,
                &[SavedCommand {
                    name: "multiline".to_owned(),
                    command: "echo safe\nrm -rf /".to_owned(),
                }]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            save_project_saved_commands(
                &state,
                &project,
                &vec![
                    SavedCommand {
                        name: "command".to_owned(),
                        command: "true".to_owned(),
                    };
                    MAX_SAVED_COMMANDS + 1
                ]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn saved_commands_fail_closed_on_cross_project_or_duplicate_records() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let id = project_saved_commands_id(&project);
        let invalid = serde_json::json!({
            "schema": "rover/tui-saved-commands/v1",
            "repository": root.join("other-repo").to_string_lossy(),
            "commands": [
                {"name": "same", "command": "echo 1"},
                {"name": "same", "command": "echo 2"}
            ],
        });
        state
            .records()
            .put("tui_saved_commands", &id, &invalid, "test.invalid")
            .unwrap();
        assert_eq!(
            load_project_saved_commands(&state, &project)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let duplicate = serde_json::json!({
            "schema": "rover/tui-saved-commands/v1",
            "repository": project.to_str().unwrap(),
            "commands": [
                {"name": "same", "command": "echo 1"},
                {"name": "same", "command": "echo 2"}
            ],
        });
        state
            .records()
            .put("tui_saved_commands", &id, &duplicate, "test.duplicate")
            .unwrap();
        assert_eq!(
            load_project_saved_commands(&state, &project)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_briefing_drafts_round_trip_with_limits() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let expected = vec![
            TaskDraft {
                title: "Audit plan".to_owned(),
                paths: "src/**".to_owned(),
                dependencies: "task_base".to_owned(),
                quality_gate: "cargo test".to_owned(),
                prompt: "Compare paths\nRecord evidence".to_owned(),
            },
            TaskDraft {
                title: String::new(),
                paths: String::new(),
                dependencies: String::new(),
                quality_gate: String::new(),
                prompt: String::new(),
            },
        ];
        assert!(load_project_task_drafts(&state, &project)
            .unwrap()
            .is_empty());
        save_project_task_drafts(&state, &project, &expected).unwrap();
        assert_eq!(
            load_project_task_drafts(&state, &project).unwrap(),
            expected
        );
        let legacy = serde_json::json!({
            "schema": "rover/tui-task-drafts/v1",
            "repository": project.to_str().unwrap(),
            "drafts": [{"title": "legacy", "prompt": "old briefing"}],
        });
        let drafts_id = project_task_drafts_id(&project);
        state
            .records()
            .put("tui_task_drafts", &drafts_id, &legacy, "test.legacy")
            .unwrap();
        let migrated = load_project_task_drafts(&state, &project).unwrap();
        assert_eq!(migrated[0].title, "legacy");
        assert!(migrated[0].paths.is_empty());
        assert!(migrated[0].dependencies.is_empty());
        assert!(migrated[0].quality_gate.is_empty());
        save_project_task_drafts(&state, &project, &migrated).unwrap();
        assert_eq!(
            state
                .records()
                .get_typed::<serde_json::Value>("tui_task_drafts", &drafts_id)
                .unwrap()
                .get("schema")
                .and_then(serde_json::Value::as_str),
            Some("rover/tui-task-drafts/v2")
        );
        assert_eq!(
            save_project_task_drafts(
                &state,
                &project,
                &[TaskDraft {
                    title: "x".repeat(MAX_TASK_DRAFT_TITLE_BYTES + 1),
                    paths: String::new(),
                    dependencies: String::new(),
                    quality_gate: String::new(),
                    prompt: String::new()
                }]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            save_project_task_drafts(
                &state,
                &project,
                &[TaskDraft {
                    title: String::new(),
                    paths: String::new(),
                    dependencies: String::new(),
                    quality_gate: String::new(),
                    prompt: "x".repeat(MAX_TASK_DRAFT_PROMPT_BYTES + 1)
                }]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            save_project_task_drafts(
                &state,
                &project,
                &vec![
                    TaskDraft {
                        title: String::new(),
                        paths: String::new(),
                        dependencies: String::new(),
                        quality_gate: String::new(),
                        prompt: String::new()
                    };
                    MAX_TASK_DRAFTS + 1
                ]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut over_paths = TaskDraft {
            title: String::new(),
            paths: String::new(),
            dependencies: String::new(),
            quality_gate: String::new(),
            prompt: String::new(),
        };
        over_paths.paths = "x".repeat(MAX_TASK_DRAFT_PATHS_BYTES + 1);
        let mut over_dependencies = over_paths.clone();
        over_dependencies.paths.clear();
        over_dependencies.dependencies = "x".repeat(MAX_TASK_DRAFT_DEPENDENCIES_BYTES + 1);
        let mut over_gate = over_paths.clone();
        over_gate.paths.clear();
        over_gate.quality_gate = "x".repeat(MAX_TASK_DRAFT_GATE_BYTES + 1);
        for oversized in [over_paths, over_dependencies, over_gate] {
            assert_eq!(
                save_project_task_drafts(&state, &project, &[oversized])
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_plan_lists_are_project_scoped_and_show_transitive_readiness() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        let other_project = root.join("other-repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        fs::create_dir_all(&other_project).expect("create other project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let other_project = fs::canonicalize(other_project).expect("canonicalize other project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let identity = rover_core::RepositoryIdentity::resolve(project.to_str().unwrap()).unwrap();
        let service = rover_tasks::TaskPlanService::new(state.records());
        let parent = service
            .create(
                &identity,
                rover_core::TaskBrief {
                    title: "Parent".into(),
                    paths: String::new(),
                    dependencies: String::new(),
                    quality_gate: String::new(),
                    prompt: "Complete parent".into(),
                },
            )
            .unwrap();
        let child = service
            .create(
                &identity,
                rover_core::TaskBrief {
                    title: "Child".into(),
                    paths: String::new(),
                    dependencies: parent.id.to_string(),
                    quality_gate: String::new(),
                    prompt: "Complete child".into(),
                },
            )
            .unwrap();
        service.start_manual(&identity, &parent.id).unwrap();
        service
            .finish_manual(
                &identity,
                &parent.id,
                rover_tasks::TaskPlanStatus::Cancelled,
                "Cancelled by user",
            )
            .unwrap();

        let summaries = load_project_task_plan_summaries(&state, &project).unwrap();
        assert_eq!(summaries.len(), 2);
        let child_summary = summaries
            .iter()
            .find(|summary| summary.id == child.id.as_str())
            .unwrap();
        assert_eq!(child_summary.status, "WAITING");
        assert_eq!(child_summary.readiness, "BLOCKED");
        assert!(load_project_task_plan_summaries(&state, &other_project)
            .unwrap()
            .is_empty());
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_briefing_drafts_reject_cross_project_and_terminal_controls() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        let project = fs::canonicalize(project).expect("canonicalize project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let id = project_task_drafts_id(&project);
        let invalid = serde_json::json!({
            "schema": "rover/tui-task-drafts/v1",
            "repository": root.join("other-repo").to_string_lossy(),
            "drafts": [{"title": "bad", "prompt": "wrong project"}],
        });
        state
            .records()
            .put("tui_task_drafts", &id, &invalid, "test.invalid")
            .unwrap();
        assert_eq!(
            load_project_task_drafts(&state, &project)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            save_project_task_drafts(
                &state,
                &project,
                &[TaskDraft {
                    title: "bad\u{202e}".to_owned(),
                    paths: String::new(),
                    dependencies: String::new(),
                    quality_gate: String::new(),
                    prompt: String::new()
                }]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_evidence_load_is_project_and_candidate_bound() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        let other_project = root.join("other-repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        fs::create_dir_all(&other_project).expect("create other project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let database = state_path.join("rover.db");
        let task_id = "task_evidence";
        let investigation_id = "inv_evidence";
        insert_raw_record(
            &database,
            task_id,
            &format!(
                r#"{{"id":"{task_id}","contract":{{"repository":"{}","objective":"evidence fixture"}},"status":"CANDIDATE_READY","updated_at":"2026-09-26T00:00:00Z","candidate":"snapshot_a","investigation_id":"{investigation_id}"}}"#,
                project.display()
            ),
        );
        let evidence = |repository: &Path, candidate: &str| {
            format!(
                r#"{{"schema":"rover/v1alpha1","id":"{investigation_id}","repository":"{}","candidate":"{candidate}","decision":"BLOCKED","decision_reason":"required review is pending","checks":[{{"id":"unit","outcome":"PASS","meaning":"3 recorded checks passed","required":true,"tests":3,"skipped":0}}],"findings":[{{"path":"src/lib.rs","message":"review this finding","severity":"warning"}}],"unknowns":["External effects are not rolled back."]}}"#,
                repository.display()
            )
        };
        insert_investigation_record(
            &database,
            investigation_id,
            &evidence(&project, "snapshot_a"),
        );

        let tasks = load_project_tasks(&state, &project).expect("load project task");
        assert_eq!(tasks.len(), 1);
        let summary = load_project_investigation(&state, &project, &tasks[0])
            .expect("load linked investigation")
            .expect("investigation is present");
        assert_eq!(summary.id, investigation_id);
        assert_eq!(summary.candidate, "snapshot_a");
        assert_eq!(summary.decision, "BLOCKED");
        assert_eq!(summary.checks.len(), 1);
        assert_eq!(summary.checks[0].outcome, "PASS");
        assert_eq!(summary.findings[0].path, "src/lib.rs");
        assert_eq!(summary.unknowns.len(), 1);

        state
            .records()
            .delete("investigation", investigation_id, "test.replace")
            .expect("remove accepted investigation");
        insert_investigation_record(
            &database,
            investigation_id,
            &evidence(&other_project, "snapshot_a"),
        );
        assert_eq!(
            load_project_investigation(&state, &project, &tasks[0])
                .expect_err("cross-project evidence is rejected")
                .kind(),
            io::ErrorKind::InvalidData
        );

        state
            .records()
            .delete("investigation", investigation_id, "test.replace")
            .expect("remove cross-project investigation");
        insert_investigation_record(
            &database,
            investigation_id,
            &evidence(&project, "snapshot_other"),
        );
        assert_eq!(
            load_project_investigation(&state, &project, &tasks[0])
                .expect_err("stale candidate evidence is rejected")
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }

    #[test]
    fn task_diff_load_verifies_retained_snapshots_and_project_identity() {
        let root = fixture_root();
        let state_path = root.join("state");
        let project = root.join("repo");
        let other_project = root.join("other-repo");
        fs::create_dir_all(&state_path).expect("create state directory");
        fs::create_dir_all(&project).expect("create project");
        fs::create_dir_all(&other_project).expect("create other project");
        let state = StateRoot::open(&state_path).expect("admit state root");
        let base = test_snapshot(&project, Vec::new());
        let candidate_digest = state
            .put_blob(b"verified candidate bytes")
            .expect("store candidate content");
        let candidate = test_snapshot(
            &project,
            vec![rover_source::SnapshotFile {
                path: "src/main.rs".to_owned(),
                mode: 0o644,
                sha256: candidate_digest.to_hex(),
                size: 24,
            }],
        );
        state
            .records()
            .put("snapshot", &base.id, &base, "test.snapshot")
            .expect("store base snapshot");
        state
            .records()
            .put("snapshot", &candidate.id, &candidate, "test.snapshot")
            .expect("store candidate snapshot");
        let task = TaskSummary {
            id: "task_diff".to_owned(),
            objective: "compare retained snapshots".to_owned(),
            status: "CANDIDATE_READY".to_owned(),
            updated_at: "2026-09-26T00:00:00Z".to_owned(),
            error: None,
            candidate: Some(candidate.id.clone()),
            base_snapshot: Some(base.id.clone()),
            investigation_id: None,
            attempt_count: 0,
            process: None,
        };
        let diff = load_project_diff(&state, &project, &task)
            .expect("load verified snapshot inventory")
            .expect("linked snapshots should have a comparison");
        assert_eq!(diff.base_snapshot, base.id);
        assert_eq!(diff.candidate, candidate.id);
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.changes[0].path, "src/main.rs");
        assert_eq!(diff.changes[0].status, "added");

        state
            .records()
            .delete("snapshot", &candidate.id, "test.replace")
            .expect("remove valid candidate snapshot");
        let cross_project_candidate = test_snapshot(&other_project, Vec::new());
        state
            .records()
            .put(
                "snapshot",
                &cross_project_candidate.id,
                &cross_project_candidate,
                "test.snapshot",
            )
            .expect("store cross-project snapshot");
        let mut cross_project_task = task;
        cross_project_task.candidate = Some(cross_project_candidate.id);
        assert_eq!(
            load_project_diff(&state, &project, &cross_project_task)
                .expect_err("cross-project snapshot must be rejected")
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(state);
        fs::remove_dir_all(root).expect("remove test state");
    }
}
