//! Conservative process-presence checks for detached supervisor recovery.
//!
//! A stale heartbeat alone never proves a supervisor is gone. Linux process
//! identity includes the boot ID and kernel start time, so PID reuse can be
//! detected. On other Unix systems Rover only marks a process gone when the OS
//! explicitly reports that the PID does not exist.

use rover_store::{Store, StoreError};
use serde_json::{Map, Value};
#[cfg(unix)]
use std::collections::BTreeSet;
use std::fmt;
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const LOST_AFTER_SECONDS: i64 = 10;
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);

/// Inputs for launching one durable Rover task worker.
#[derive(Clone, Debug)]
pub struct LaunchOptions {
    /// Existing queued task record to own.
    pub task_id: String,
    /// Absolute executable path followed by arguments; no shell is inserted.
    pub argv: Vec<String>,
    /// Existing absolute directory used as the worker's current directory.
    pub dir: PathBuf,
    /// Private `<state>/tasks/<task-id>` directory where `supervisor.log` is created exclusively.
    pub output_dir: PathBuf,
    /// Explicit host environment names to pass to the worker.
    pub pass_env: Vec<String>,
}

/// Process ownership returned after the task dispatch event is persisted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchReceipt {
    pub pid: i32,
    pub process_identity: Option<String>,
}

/// Failure while validating, starting, or recording a detached task worker.
#[derive(Debug)]
pub enum LaunchError {
    Invalid(String),
    Io(io::Error),
    Store(StoreError),
    StartedButUnrecorded {
        task_id: String,
        receipt: Box<LaunchReceipt>,
        source: Box<StoreError>,
    },
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "invalid worker launch: {reason}"),
            Self::Io(error) => write!(formatter, "worker launch I/O failed: {error}"),
            Self::Store(error) => write!(formatter, "worker state update failed: {error}"),
            Self::StartedButUnrecorded {
                task_id,
                receipt,
                source,
            } => write!(
                formatter,
                "worker for task {task_id} started as PID {} but dispatch persistence failed; inspect before retry: {source}",
                receipt.pid
            ),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::StartedButUnrecorded { source, .. } => Some(source.as_ref()),
            Self::Invalid(_) => None,
        }
    }
}

/// A failure reported by a managed heartbeat loop.
#[derive(Debug)]
pub enum HeartbeatError {
    Store(StoreError),
    WorkerPanicked,
}

impl fmt::Display for HeartbeatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "task heartbeat persistence failed: {error}"),
            Self::WorkerPanicked => formatter.write_str("task heartbeat thread panicked"),
        }
    }
}

impl std::error::Error for HeartbeatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::WorkerPanicked => None,
        }
    }
}

/// Owner for a worker's periodic task heartbeat.
pub struct HeartbeatGuard {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<Result<(), StoreError>>>,
}

impl HeartbeatGuard {
    /// Stop the heartbeat thread and return any persistence failure it saw.
    ///
    /// # Errors
    ///
    /// Returns the first store error from the heartbeat loop or if the loop
    /// thread panicked.
    pub fn stop(mut self) -> Result<(), HeartbeatError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), HeartbeatError> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        match self.thread.take().map(thread::JoinHandle::join) {
            Some(Ok(result)) => result.map_err(HeartbeatError::Store),
            Some(Err(_)) => Err(HeartbeatError::WorkerPanicked),
            None => Ok(()),
        }
    }
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

/// Start a managed 250 ms heartbeat loop for a claimed task.
///
/// A persisted cancellation request cancels the supplied process token. A
/// storage failure also cancels the token and is returned by
/// [`HeartbeatGuard::stop`]. Dropping the guard stops and joins the loop.
///
/// # Errors
///
/// Returns an I/O error if the heartbeat thread cannot be created.
pub fn start_heartbeat(
    store: Arc<Store>,
    task_id: impl Into<String>,
    cancellation: super::CancellationToken,
) -> io::Result<HeartbeatGuard> {
    start_heartbeat_at_interval(store, task_id.into(), cancellation, HEARTBEAT_INTERVAL)
}

fn start_heartbeat_at_interval(
    store: Arc<Store>,
    task_id: String,
    cancellation: super::CancellationToken,
    interval: Duration,
) -> io::Result<HeartbeatGuard> {
    let (stop, stopped) = mpsc::channel();
    let worker = thread::Builder::new()
        .name(format!("rover-heartbeat-{task_id}"))
        .spawn(move || loop {
            match stopped.recv_timeout(interval) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let timestamp = rover_core::now().to_string();
                    match heartbeat(&store, &task_id, &timestamp) {
                        Ok(true) => cancellation.cancel(),
                        Ok(false) => {}
                        Err(error) => {
                            cancellation.cancel();
                            return Err(error);
                        }
                    }
                }
            }
        })?;
    Ok(HeartbeatGuard {
        stop: Some(stop),
        thread: Some(worker),
    })
}

/// Start a task worker independently of the caller and persist its ownership.
///
/// The worker has null stdin, a private exclusive combined stdout/stderr log,
/// a separate process group, and only PATH, HOME, plus the named
/// `pass_env` values. This is process supervision, not a sandbox. Dropping the
/// returned receipt does not stop the worker.
///
/// # Errors
///
/// Returns an error for invalid launch fields, unsafe/private log setup,
/// process creation or reaper setup failure, missing task records, or dispatch
/// persistence failure. A persistence failure after spawn returns the PID and
/// explicitly warns callers to inspect state before retrying.
#[cfg(unix)]
pub fn launch_detached(
    store: &Store,
    options: &LaunchOptions,
) -> Result<LaunchReceipt, LaunchError> {
    validate_launch(options)?;
    let task = store
        .get("task", &options.task_id)
        .map_err(LaunchError::Store)?;
    let task_status = task
        .as_value()
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| LaunchError::Invalid("task status is missing or invalid".into()))?;
    if task_status != "QUEUED" {
        return Err(LaunchError::Invalid(format!(
            "task cannot be dispatched from {task_status}"
        )));
    }
    let task_record_id = task
        .as_value()
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| LaunchError::Invalid("task ID field is missing or invalid".into()))?;
    if task_record_id != options.task_id {
        return Err(LaunchError::Invalid(
            "task record ID does not match its storage key".into(),
        ));
    }
    optional_bool(
        task.as_value()
            .as_object()
            .ok_or_else(|| LaunchError::Invalid("task record must be an object".into()))?,
        "cancel_requested",
    )
    .map_err(|error| LaunchError::Invalid(error.to_string()))?;
    let workspace = task
        .as_value()
        .get("workspace")
        .and_then(Value::as_str)
        .ok_or_else(|| LaunchError::Invalid("task workspace is missing or invalid".into()))?;
    let workspace = PathBuf::from(workspace);
    if workspace.file_name().is_none_or(|name| name != "workspace")
        || workspace.parent() != Some(options.output_dir.as_path())
    {
        return Err(LaunchError::Invalid(
            "worker output directory must be the task workspace parent".into(),
        ));
    }

    let (log, stderr_log) = create_worker_log(options).map_err(LaunchError::Io)?;
    let (reaper_tx, reaper) = match start_reaper(&options.task_id) {
        Ok(reaper) => reaper,
        Err(error) => {
            let message = format!("supervisor reaper start failed: {error}");
            record_dispatch_failure(store, &options.task_id, &message)
                .map_err(LaunchError::Store)?;
            return Err(LaunchError::Io(error));
        }
    };
    let child = match spawn_worker(options, log, stderr_log) {
        Ok(child) => child,
        Err(error) => {
            drop(reaper_tx);
            drop(reaper);
            let message = format!("supervisor start failed: {error}");
            record_dispatch_failure(store, &options.task_id, &message)
                .map_err(LaunchError::Store)?;
            return Err(LaunchError::Io(error));
        }
    };
    let pid = match hand_off_to_reaper(child, reaper_tx, reaper) {
        Ok(pid) => pid,
        Err(error) => {
            let message = format!("supervisor reaper handoff failed: {error}");
            record_dispatch_failure(store, &options.task_id, &message)
                .map_err(LaunchError::Store)?;
            return Err(LaunchError::Io(error));
        }
    };
    let receipt = LaunchReceipt {
        pid,
        process_identity: process_identity(pid),
    };
    if let Err(source) = record_dispatch(store, &options.task_id, &receipt) {
        return Err(LaunchError::StartedButUnrecorded {
            task_id: options.task_id.clone(),
            receipt: Box::new(receipt),
            source: Box::new(source),
        });
    }
    Ok(receipt)
}

#[cfg(unix)]
fn create_worker_log(options: &LaunchOptions) -> io::Result<(std::fs::File, std::fs::File)> {
    let output = rover_store::files::prepare_private_directory(&options.output_dir)?;
    let log = output.create_new_file_writer("supervisor.log", 0o600)?;
    let stderr_log = log.try_clone()?;
    Ok((log, stderr_log))
}

#[cfg(unix)]
fn start_reaper(
    task_id: &str,
) -> io::Result<(mpsc::Sender<std::process::Child>, thread::JoinHandle<()>)> {
    let (reaper_tx, reaper_rx) = mpsc::channel::<std::process::Child>();
    let reaper = thread::Builder::new()
        .name(format!("rover-worker-reaper-{task_id}"))
        .spawn(move || {
            if let Ok(mut child) = reaper_rx.recv() {
                let _ = child.wait();
            }
        })?;
    Ok((reaper_tx, reaper))
}

#[cfg(unix)]
fn spawn_worker(
    options: &LaunchOptions,
    log: std::fs::File,
    stderr_log: std::fs::File,
) -> io::Result<std::process::Child> {
    let host_path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(&options.argv[0]);
    command
        .args(&options.argv[1..])
        .current_dir(&options.dir)
        .env_clear()
        .env("PATH", &host_path)
        .env("HOME", std::env::var_os("HOME").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr_log));
    let mut seen_env = BTreeSet::from(["PATH".to_owned(), "HOME".to_owned()]);
    for name in &options.pass_env {
        if seen_env.insert(name.clone()) {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
    }
    command.process_group(0);
    command.spawn()
}

#[cfg(unix)]
fn hand_off_to_reaper(
    child: std::process::Child,
    reaper_tx: mpsc::Sender<std::process::Child>,
    reaper: thread::JoinHandle<()>,
) -> io::Result<i32> {
    let Ok(pid) = i32::try_from(child.id()) else {
        let mut child = child;
        let process_id = rustix::process::Pid::from_child(&child);
        super::kill_group(process_id, &mut child);
        let _ = child.wait();
        drop(reaper_tx);
        drop(reaper);
        return Err(io::Error::other(
            "worker PID exceeds the supported process identifier range",
        ));
    };
    if let Err(error) = reaper_tx.send(child) {
        let mut orphaned_child = error.0;
        let process_id = rustix::process::Pid::from_child(&orphaned_child);
        super::kill_group(process_id, &mut orphaned_child);
        let _ = orphaned_child.wait();
        drop(reaper);
        return Err(io::Error::other(format!(
            "worker reaper stopped before taking ownership: {pid}"
        )));
    }
    drop(reaper_tx);
    drop(reaper);
    Ok(pid)
}

#[cfg(unix)]
fn validate_launch(options: &LaunchOptions) -> Result<(), LaunchError> {
    if options.task_id.is_empty() || rover_core::Id::parse(options.task_id.clone()).is_err() {
        return Err(LaunchError::Invalid("task ID is invalid".into()));
    }
    if options.argv.is_empty() || options.argv.len() > 129 {
        return Err(LaunchError::Invalid(
            "argv must include an executable and at most 128 arguments".into(),
        ));
    }
    if !PathBuf::from(&options.argv[0]).is_absolute() {
        return Err(LaunchError::Invalid(
            "worker executable path must be absolute".into(),
        ));
    }
    if options
        .argv
        .iter()
        .any(|arg| arg.contains('\0') || arg.len() > 65_536)
    {
        return Err(LaunchError::Invalid(
            "argv contains NUL or an oversized argument".into(),
        ));
    }
    if !options.dir.is_absolute() || !options.dir.is_dir() {
        return Err(LaunchError::Invalid(
            "worker directory must be an existing absolute directory".into(),
        ));
    }
    for name in &options.pass_env {
        if !super::valid_env_name(name) {
            return Err(LaunchError::Invalid(format!(
                "invalid environment variable name {name:?}"
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn record_dispatch(
    store: &Store,
    task_id: &str,
    receipt: &LaunchReceipt,
) -> Result<(), StoreError> {
    let now = rover_core::now().to_string();
    store.mutate_with_event("task", task_id, |old| {
        let mut next = old.clone();
        let object = as_object_mut(&mut next)?;
        if required_string(object, "status")? == "QUEUED" {
            object.insert("pid".into(), Value::from(receipt.pid));
            if let Some(identity) = &receipt.process_identity {
                object.insert("process_identity".into(), Value::String(identity.clone()));
            }
            object.insert("heartbeat".into(), Value::String(now.clone()));
            object.insert("updated_at".into(), Value::String(now.clone()));
        }
        Ok((next, Some("task.dispatched".into())))
    })
}

#[cfg(unix)]
fn record_dispatch_failure(store: &Store, task_id: &str, message: &str) -> Result<(), StoreError> {
    let now = rover_core::now().to_string();
    store.mutate_with_event("task", task_id, |old| {
        let mut next = old.clone();
        let object = as_object_mut(&mut next)?;
        if required_string(object, "status")? == "QUEUED" {
            object.insert("status".into(), Value::String("ERROR".into()));
            object.insert("error".into(), Value::String(message.to_owned()));
            object.insert("updated_at".into(), Value::String(now.clone()));
            Ok((next, Some("task.dispatch_failed".into())))
        } else {
            Ok((next, None))
        }
    })
}

/// Return an OS-stable process identity when the platform exposes one.
///
/// Linux identities bind boot ID, PID, and `/proc/<pid>/stat` start time.
/// macOS currently returns `None`, matching Rover's Go behavior: a live or
/// reused PID remains ambiguous and must not be declared lost from heartbeat
/// age alone.
#[must_use]
pub fn process_identity(pid: i32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        linux_process_identity(pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// Return true only when the OS and available identity evidence prove that the
/// recorded process is gone or its PID now belongs to a different process.
#[must_use]
pub fn definitely_gone(pid: i32, identity: &str) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(unix)]
    {
        definitely_gone_unix(pid, identity)
    }
    #[cfg(not(unix))]
    {
        let _ = identity;
        false
    }
}

#[cfg(unix)]
fn definitely_gone_unix(pid: i32, identity: &str) -> bool {
    let Some(pid) = rustix::process::Pid::from_raw(pid) else {
        return false;
    };
    if matches!(
        rustix::process::test_kill_process(pid),
        Err(rustix::io::Errno::SRCH)
    ) {
        return true;
    }

    #[cfg(target_os = "linux")]
    {
        if !identity.is_empty() {
            if let Some(current) = linux_process_identity(pid.as_raw_nonzero().get()) {
                if identity != current {
                    return true;
                }
            }
        }
        linux_process_is_zombie(pid.as_raw_nonzero().get())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = identity;
        false
    }
}

/// Result of atomically claiming a queued task for a supervisor process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimOutcome {
    /// The queued task is now owned by this process.
    Claimed,
    /// Cancellation was already requested, so the queued task became terminal.
    Cancelled,
}

/// Atomically claim a queued Rover task and persist process ownership.
///
/// `now` must be an RFC 3339 timestamp. A task that has already transitioned
/// cannot be claimed again, and a pre-cancelled task is made terminal without
/// launching work.
///
/// # Errors
///
/// Returns a store error for malformed records, invalid timestamps, missing
/// tasks, repeated claims, or database failures.
pub fn claim(
    store: &Store,
    task_id: &str,
    pid: i32,
    process_identity: Option<&str>,
    now: &str,
) -> Result<ClaimOutcome, StoreError> {
    if pid <= 0 {
        return Err(StoreError::MutationRejected(
            "supervisor PID must be positive".into(),
        ));
    }
    parse_time(now)?;
    let mut outcome = None;
    store.mutate_with_event("task", task_id, |old| {
        let mut next = old.clone();
        let object = as_object_mut(&mut next)?;
        let status = required_string(object, "status")?;
        if status != "QUEUED" {
            return Err(StoreError::MutationRejected(format!(
                "task cannot be claimed from {status}; automatic re-execution is disabled"
            )));
        }
        let cancelled = optional_bool(object, "cancel_requested")?;
        if cancelled {
            object.insert("status".into(), Value::String("CANCELLED".into()));
            outcome = Some(ClaimOutcome::Cancelled);
        } else {
            object.insert("status".into(), Value::String("PREPARING".into()));
            object.insert("pid".into(), Value::from(pid));
            if let Some(identity) = process_identity.filter(|value| !value.is_empty()) {
                object.insert(
                    "process_identity".into(),
                    Value::String(identity.to_owned()),
                );
            }
            object.insert("heartbeat".into(), Value::String(now.to_owned()));
            outcome = Some(ClaimOutcome::Claimed);
        }
        object.insert("updated_at".into(), Value::String(now.to_owned()));
        Ok((next, Some("task.claimed".into())))
    })?;
    outcome.ok_or_else(|| StoreError::MutationRejected("task claim produced no outcome".into()))
}

/// Refresh a live task heartbeat and return whether the worker should cancel.
/// Terminal records are left unchanged. Heartbeats append no event rows.
///
/// # Errors
///
/// Returns a store error for malformed records, invalid timestamps, missing
/// tasks, or database failures.
pub fn heartbeat(store: &Store, task_id: &str, now: &str) -> Result<bool, StoreError> {
    parse_time(now)?;
    let mut cancel_requested = false;
    store.mutate_with_event("task", task_id, |old| {
        let mut next = old.clone();
        let object = as_object_mut(&mut next)?;
        let status = required_string(object, "status")?;
        if is_terminal(status) {
            return Ok((next, None));
        }
        cancel_requested = optional_bool(object, "cancel_requested")?;
        object.insert("heartbeat".into(), Value::String(now.to_owned()));
        object.insert("updated_at".into(), Value::String(now.to_owned()));
        Ok((next, None))
    })?;
    Ok(cancel_requested)
}

/// Persist cancellation intent unless the task is already terminal.
///
/// # Errors
///
/// Returns a store error for malformed records, invalid timestamps, terminal
/// tasks, missing tasks, or database failures.
pub fn request_cancel(store: &Store, task_id: &str, now: &str) -> Result<(), StoreError> {
    parse_time(now)?;
    store.mutate_with_event("task", task_id, |old| {
        let mut next = old.clone();
        let object = as_object_mut(&mut next)?;
        let status = required_string(object, "status")?;
        if is_terminal(status) {
            return Err(StoreError::MutationRejected(
                "task is already terminal".into(),
            ));
        }
        object.insert("cancel_requested".into(), Value::Bool(true));
        object.insert("updated_at".into(), Value::String(now.to_owned()));
        Ok((next, Some("task.cancel_requested".into())))
    })
}

/// Mark stale tasks LOST only after the process-presence check proves loss.
/// Workspaces and process records are retained; this function never restarts a
/// task or treats a stale heartbeat alone as evidence of process death.
///
/// # Errors
///
/// Returns a store error for malformed records, missing tasks, or database
/// failures.
pub fn reconcile(store: &Store, now: &str) -> Result<usize, StoreError> {
    reconcile_with_presence(store, now, definitely_gone)
}

fn reconcile_with_presence<F>(
    store: &Store,
    now: &str,
    mut process_is_gone: F,
) -> Result<usize, StoreError>
where
    F: FnMut(i32, &str) -> bool,
{
    let now_text = now.to_owned();
    let now = parse_time(now)?;
    let stale_before = now - time::Duration::seconds(LOST_AFTER_SECONDS);
    let records = store.list("task", 1000)?;
    let mut lost = 0;
    for record in records {
        let value = record.as_value();
        let Some(object) = value.as_object() else {
            return Err(StoreError::MutationRejected(
                "task record must be an object".into(),
            ));
        };
        let status = required_string(object, "status")?;
        if is_terminal(status) {
            // A previous reconciliation may have persisted LOST and then
            // failed while releasing leases. Retrying cleanup is safe because
            // resource release is owner-scoped and idempotent.
            if status == "LOST" {
                let task_id = object
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| StoreError::MutationRejected("task record has no ID".into()))?;
                store.release_resources(task_id)?;
            }
            continue;
        }
        let Some(pid) = object.get("pid").and_then(Value::as_i64) else {
            continue;
        };
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        if pid <= 0 {
            continue;
        }
        let Some(heartbeat) = object.get("heartbeat").and_then(Value::as_str) else {
            continue;
        };
        let Ok(heartbeat_time) = parse_time(heartbeat) else {
            continue;
        };
        if heartbeat_time > stale_before {
            continue;
        }
        let identity = object
            .get("process_identity")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if !process_is_gone(pid, &identity) {
            continue;
        }
        let task_id = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| StoreError::MutationRejected("task record has no ID".into()))?
            .to_owned();
        let original_heartbeat = heartbeat.to_owned();
        let mut transitioned = false;
        store.mutate_with_event("task", &task_id, |old| {
            let mut next = old.clone();
            let object = as_object_mut(&mut next)?;
            let current_status = required_string(object, "status")?;
            let current_pid = object.get("pid").and_then(Value::as_i64);
            let current_heartbeat = object.get("heartbeat").and_then(Value::as_str);
            if is_terminal(current_status)
                || current_pid != Some(i64::from(pid))
                || current_heartbeat != Some(original_heartbeat.as_str())
            {
                return Ok((next, None));
            }
            object.insert("status".into(), Value::String("LOST".into()));
            object.insert(
                "error".into(),
                Value::String(
                    "supervisor is no longer present; workspace retained; no automatic restart"
                        .into(),
                ),
            );
            object.insert("updated_at".into(), Value::String(now_text.clone()));
            transitioned = true;
            Ok((next, Some("task.lost".into())))
        })?;
        if transitioned {
            // The process-presence check and conditional state transition
            // establish that the old owner is gone. Releasing by exact task
            // ID cannot free a lease belonging to a different worker.
            store.release_resources(&task_id)?;
            lost += 1;
        }
    }
    Ok(lost)
}

fn as_object_mut(value: &mut Value) -> Result<&mut Map<String, Value>, StoreError> {
    value
        .as_object_mut()
        .ok_or_else(|| StoreError::MutationRejected("task record must be a JSON object".into()))
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, StoreError> {
    object.get(key).and_then(Value::as_str).ok_or_else(|| {
        StoreError::MutationRejected(format!("task record field {key} must be a string"))
    })
}

fn optional_bool(object: &Map<String, Value>, key: &str) -> Result<bool, StoreError> {
    match object.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(StoreError::MutationRejected(format!(
            "task record field {key} must be a boolean"
        ))),
    }
}

fn parse_time(value: &str) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|error| StoreError::Timestamp(error.to_string()))
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "CANDIDATE_READY"
            | "REVIEW_READY"
            | "CHECKS_BLOCKED"
            | "FAILED"
            | "ERROR"
            | "CANCELLED"
            | "TIMED_OUT"
            | "LOST"
    )
}

#[cfg(target_os = "linux")]
fn linux_process_identity(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat.get(stat.rfind(')')?.checked_add(1)?..)?;
    let fields = fields.split_ascii_whitespace().collect::<Vec<_>>();
    let start_time = fields.get(19)?;
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let boot_id = boot_id.trim();
    if boot_id.is_empty() || start_time.is_empty() {
        return None;
    }
    Some(format!("{boot_id}:{pid}:{start_time}"))
}

#[cfg(target_os = "linux")]
fn linux_process_is_zombie(pid: i32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(fields) = stat
        .rfind(')')
        .and_then(|end| stat.get(end.saturating_add(1)..))
    else {
        return false;
    };
    fields.split_ascii_whitespace().next() == Some("Z")
}

#[cfg(test)]
mod tests {
    use super::{
        claim, definitely_gone, heartbeat, launch_detached, process_identity,
        reconcile_with_presence, record_dispatch, request_cancel, start_heartbeat_at_interval,
        ClaimOutcome, HeartbeatError, LaunchError, LaunchOptions, LaunchReceipt,
    };
    use crate::CancellationToken;
    use rover_store::{Store, StoreError};
    use rusqlite::Connection;
    use serde_json::{json, Value};
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::{Path, PathBuf};
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[cfg(unix)]
    static LAUNCH_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[cfg(unix)]
    struct LaunchTemp(PathBuf);

    #[cfg(unix)]
    impl LaunchTemp {
        fn new() -> Self {
            let sequence = LAUNCH_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rover-supervisor-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create private temp root");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("secure private temp root");
            Self(path)
        }

        fn worker_dir(&self) -> PathBuf {
            let path = self.0.join("task_launch");
            fs::create_dir(&path).expect("create worker log directory");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("secure worker log directory");
            path
        }
    }

    #[cfg(unix)]
    impl Drop for LaunchTemp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn launch_options(task_id: &str, dir: &Path, output_dir: &Path) -> LaunchOptions {
        LaunchOptions {
            task_id: task_id.into(),
            argv: vec!["/bin/sleep".into(), "1".into()],
            dir: dir.to_path_buf(),
            output_dir: output_dir.to_path_buf(),
            pass_env: Vec::new(),
        }
    }

    fn memory_store() -> Store {
        Store::from_connection(Connection::open_in_memory().expect("open in-memory database"))
            .expect("configure test store")
    }

    fn queued_task(cancel_requested: bool, heartbeat: &str) -> Value {
        json!({
            "schema": "rover/v1alpha1",
            "id": "task_one",
            "status": "QUEUED",
            "workspace": "/state/tasks/task_one/workspace",
            "heartbeat": heartbeat,
            "pid": 0,
            "cancel_requested": cancel_requested,
            "updated_at": heartbeat,
        })
    }

    fn put_task(store: &Store, task: &Value) {
        store
            .put("task", "task_one", task, "task.created")
            .expect("insert task fixture");
    }

    fn task(store: &Store) -> Value {
        store
            .get("task", "task_one")
            .expect("read task")
            .as_value()
            .clone()
    }

    #[cfg(unix)]
    fn insert_launch_task(store: &Store, output_dir: &Path) {
        store
            .put(
                "task",
                "task_launch",
                &json!({
                    "id": "task_launch",
                    "status": "QUEUED",
                    "workspace": output_dir.join("workspace"),
                    "cancel_requested": false,
                }),
                "task.created",
            )
            .expect("insert launch task");
    }

    #[test]
    fn current_process_is_not_reported_gone_without_identity() {
        let pid = i32::try_from(std::process::id()).expect("test process ID fits i32");
        assert!(!definitely_gone(pid, ""));
    }

    #[test]
    fn invalid_process_ids_are_ambiguous() {
        assert!(!definitely_gone(0, ""));
        assert!(!definitely_gone(-1, ""));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_identity_binds_boot_id_pid_and_start_time() {
        let pid = i32::try_from(std::process::id()).expect("test process ID fits i32");
        let identity = process_identity(pid).expect("Linux process identity is available");
        let parts = identity.split(':').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1], pid.to_string());
        assert!(parts[0].contains('-'));
        assert!(parts[2].parse::<u64>().is_ok());
        assert!(!definitely_gone(pid, &identity));
        assert!(definitely_gone(pid, "different-boot:1:1"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn absent_linux_process_is_definitely_gone() {
        assert!(definitely_gone(i32::MAX, ""));
        assert!(process_identity(i32::MAX).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keeps_live_pid_reuse_ambiguous() {
        let pid = i32::try_from(std::process::id()).expect("test process ID fits i32");
        assert!(process_identity(pid).is_none());
        assert!(!definitely_gone(pid, "old-process-identity"));
    }

    #[cfg(unix)]
    #[test]
    fn detached_launch_records_pid_and_uses_a_private_exclusive_log() {
        let store = memory_store();
        let temp = LaunchTemp::new();
        let output_dir = temp.worker_dir();
        insert_launch_task(&store, &output_dir);
        let options = launch_options("task_launch", &temp.0, &output_dir);

        let receipt = launch_detached(&store, &options).expect("launch worker");
        assert!(receipt.pid > 0);
        let record = store
            .get("task", "task_launch")
            .expect("read dispatched task");
        assert_eq!(record["status"], "QUEUED");
        assert_eq!(record["pid"], receipt.pid);
        assert_eq!(
            record["process_identity"].as_str(),
            receipt.process_identity.as_deref()
        );
        assert!(record["heartbeat"].as_str().is_some());
        assert_eq!(
            store.events("task_launch").expect("events").last().unwrap()["event"],
            "task.dispatched"
        );
        let log = fs::metadata(output_dir.join("supervisor.log")).expect("supervisor log");
        assert_eq!(log.permissions().mode() & 0o777, 0o600);
        assert!(matches!(
            launch_detached(&store, &options),
            Err(LaunchError::Io(_))
        ));
        assert!(!definitely_gone(
            receipt.pid,
            receipt.process_identity.as_deref().unwrap_or("")
        ));
        for _ in 0..300 {
            if definitely_gone(
                receipt.pid,
                receipt.process_identity.as_deref().unwrap_or(""),
            ) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(definitely_gone(
            receipt.pid,
            receipt.process_identity.as_deref().unwrap_or("")
        ));
        assert_eq!(store.events("task_launch").expect("events").len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn detached_worker_stdout_and_stderr_share_the_supervisor_log() {
        let store = memory_store();
        let temp = LaunchTemp::new();
        let output_dir = temp.worker_dir();
        insert_launch_task(&store, &output_dir);
        let mut options = launch_options("task_launch", &temp.0, &output_dir);
        options.argv = vec!["/usr/bin/printf".into(), "worker-output".into()];

        let receipt = launch_detached(&store, &options).expect("launch worker");
        for _ in 0..100 {
            if definitely_gone(receipt.pid, "") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(definitely_gone(receipt.pid, ""));
        assert_eq!(
            fs::read(output_dir.join("supervisor.log")).expect("read worker log"),
            b"worker-output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detached_worker_survives_the_launcher_process_exit() {
        let temp = LaunchTemp::new();
        let helper = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "supervisor::tests::detached_worker_parent_exit_fixture",
            ])
            .env("ROVER_SUPERVISOR_PARENT_EXIT_FIXTURE", &temp.0)
            .output()
            .expect("run subprocess fixture");
        assert!(
            helper.status.success(),
            "subprocess fixture failed: {}",
            String::from_utf8_lossy(&helper.stderr)
        );
        let pid: i32 = fs::read_to_string(temp.0.join("worker.pid"))
            .expect("subprocess writes worker PID")
            .parse()
            .expect("worker PID is numeric");
        assert!(
            !definitely_gone(pid, ""),
            "worker should survive after the launcher process exits"
        );
        for _ in 0..400 {
            if definitely_gone(pid, "") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(definitely_gone(pid, ""));
    }

    #[cfg(unix)]
    #[test]
    fn detached_worker_parent_exit_fixture() {
        let Some(root) = std::env::var_os("ROVER_SUPERVISOR_PARENT_EXIT_FIXTURE") else {
            return;
        };
        let root = PathBuf::from(root);
        let output_dir = root.join("task_launch");
        fs::create_dir(&output_dir).expect("create child task directory");
        fs::set_permissions(&output_dir, fs::Permissions::from_mode(0o700))
            .expect("secure child task directory");
        let store = memory_store();
        insert_launch_task(&store, &output_dir);
        let mut options = launch_options("task_launch", &root, &output_dir);
        options.argv = vec!["/bin/sleep".into(), "2".into()];
        let receipt = launch_detached(&store, &options).expect("launch detached child");
        fs::write(root.join("worker.pid"), receipt.pid.to_string())
            .expect("persist child PID for parent test");
    }

    #[cfg(unix)]
    #[test]
    fn failed_worker_spawn_is_persisted_as_error() {
        let store = memory_store();
        let temp = LaunchTemp::new();
        let output_dir = temp.worker_dir();
        insert_launch_task(&store, &output_dir);
        let mut options = launch_options("task_launch", &temp.0, &output_dir);
        options.argv[0] = "/definitely/missing/rover-worker".into();

        assert!(matches!(
            launch_detached(&store, &options),
            Err(LaunchError::Io(_))
        ));
        let record = store
            .get("task", "task_launch")
            .expect("read failed dispatch");
        assert_eq!(record["status"], "ERROR");
        assert!(record["error"]
            .as_str()
            .unwrap()
            .contains("supervisor start failed"));
        assert_eq!(
            store.events("task_launch").expect("events").last().unwrap()["event"],
            "task.dispatch_failed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn invalid_or_already_logged_launch_does_not_start_a_worker() {
        let store = memory_store();
        let temp = LaunchTemp::new();
        let output_dir = temp.worker_dir();
        insert_launch_task(&store, &output_dir);
        let mut options = launch_options("task_launch", &temp.0, &output_dir);
        options.argv[0] = "true".into();
        assert!(matches!(
            launch_detached(&store, &options),
            Err(LaunchError::Invalid(_))
        ));

        options.argv = vec!["/bin/sleep".into(), "1".into()];
        fs::write(output_dir.join("supervisor.log"), b"keep").expect("precreate log");
        fs::set_permissions(
            output_dir.join("supervisor.log"),
            fs::Permissions::from_mode(0o600),
        )
        .expect("secure preexisting log");
        assert!(matches!(
            launch_detached(&store, &options),
            Err(LaunchError::Io(_))
        ));
        assert_eq!(
            fs::read(output_dir.join("supervisor.log")).unwrap(),
            b"keep"
        );
        assert_eq!(
            store.get("task", "task_launch").unwrap()["status"],
            "QUEUED"
        );
        assert_eq!(store.events("task_launch").expect("events").len(), 1);
    }

    #[test]
    fn claim_is_one_way_and_persists_process_ownership() {
        let store = memory_store();
        let now = "2026-09-25T12:00:00Z";
        put_task(&store, &queued_task(false, now));

        assert_eq!(
            claim(&store, "task_one", 1234, Some("host:1234:boot"), now).expect("claim"),
            ClaimOutcome::Claimed
        );
        let stored = task(&store);
        assert_eq!(stored["status"], "PREPARING");
        assert_eq!(stored["pid"], 1234);
        assert_eq!(stored["process_identity"], "host:1234:boot");
        assert_eq!(stored["heartbeat"], now);
        assert!(matches!(
            claim(&store, "task_one", 1235, None, now),
            Err(StoreError::MutationRejected(_))
        ));
        assert_eq!(store.events("task_one").expect("events").len(), 2);
    }

    #[test]
    fn claim_honors_cancellation_before_launch() {
        let store = memory_store();
        let now = "2026-09-25T12:00:00Z";
        put_task(&store, &queued_task(true, now));

        assert_eq!(
            claim(&store, "task_one", 1234, None, now).expect("claim cancelled task"),
            ClaimOutcome::Cancelled
        );
        let stored = task(&store);
        assert_eq!(stored["status"], "CANCELLED");
        assert_eq!(stored["pid"], 0);
        assert_eq!(store.events("task_one").expect("events").len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn dispatch_recording_preserves_a_worker_that_claimed_first() {
        let store = memory_store();
        let temp = LaunchTemp::new();
        let output_dir = temp.worker_dir();
        insert_launch_task(&store, &output_dir);
        let claim_time = "2026-09-25T12:00:00Z";
        assert_eq!(
            claim(
                &store,
                "task_launch",
                4321,
                Some("child-identity"),
                claim_time,
            )
            .expect("worker claim"),
            ClaimOutcome::Claimed
        );

        record_dispatch(
            &store,
            "task_launch",
            &LaunchReceipt {
                pid: 1234,
                process_identity: Some("parent-dispatch-identity".into()),
            },
        )
        .expect("record late parent dispatch");
        let record = store
            .get("task", "task_launch")
            .expect("read task after racing claim");
        assert_eq!(record["status"], "PREPARING");
        assert_eq!(record["pid"], 4321);
        assert_eq!(record["process_identity"], "child-identity");
        assert_eq!(record["heartbeat"], claim_time);
        assert_eq!(store.events("task_launch").expect("events").len(), 3);
    }

    #[test]
    fn malformed_cancel_state_fails_closed() {
        let store = memory_store();
        let now = "2026-09-25T12:00:00Z";
        let mut value = queued_task(false, now);
        value["cancel_requested"] = Value::String("false".into());
        put_task(&store, &value);

        assert!(matches!(
            claim(&store, "task_one", 1234, None, now),
            Err(StoreError::MutationRejected(_))
        ));
        assert_eq!(task(&store)["status"], "QUEUED");
        assert_eq!(store.events("task_one").expect("events").len(), 1);
    }

    #[test]
    fn heartbeat_observes_cancel_request_without_appending_events() {
        let store = memory_store();
        let original = "2026-09-25T12:00:00Z";
        let now = "2026-09-25T12:00:01Z";
        let mut value = queued_task(false, original);
        value["status"] = Value::String("RUNNING".into());
        value["pid"] = json!(1234);
        put_task(&store, &value);
        request_cancel(&store, "task_one", now).expect("request cancellation");
        let events_before_heartbeat = store.events("task_one").expect("events").len();

        assert!(heartbeat(&store, "task_one", "2026-09-25T12:00:02Z").expect("heartbeat"));
        let stored = task(&store);
        assert_eq!(stored["heartbeat"], "2026-09-25T12:00:02Z");
        assert_eq!(
            store.events("task_one").expect("events").len(),
            events_before_heartbeat
        );
    }

    #[test]
    fn heartbeat_guard_refreshes_state_and_observes_cancellation() {
        let store = Arc::new(memory_store());
        let initial = "2026-09-25T12:00:00Z";
        put_task(&store, &queued_task(false, initial));
        let cancellation = CancellationToken::default();
        let guard = start_heartbeat_at_interval(
            Arc::clone(&store),
            "task_one".into(),
            cancellation.clone(),
            Duration::from_millis(5),
        )
        .expect("start heartbeat loop");

        for _ in 0..100 {
            if task(&store)["heartbeat"] != initial {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert_ne!(task(&store)["heartbeat"], initial);
        request_cancel(&store, "task_one", "2026-09-25T12:00:01Z").expect("request cancellation");
        for _ in 0..100 {
            if cancellation.is_cancelled() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(cancellation.is_cancelled());
        guard.stop().expect("stop healthy heartbeat");
    }

    #[test]
    fn heartbeat_persistence_failure_cancels_worker_and_is_reported() {
        let store = Arc::new(memory_store());
        put_task(&store, &queued_task(false, "2026-09-25T12:00:00Z"));
        let cancellation = CancellationToken::default();
        let guard = start_heartbeat_at_interval(
            Arc::clone(&store),
            "task_one".into(),
            cancellation.clone(),
            Duration::from_millis(5),
        )
        .expect("start heartbeat loop");
        store
            .delete("task", "task_one", "")
            .expect("remove task to simulate lost state");
        for _ in 0..100 {
            if cancellation.is_cancelled() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(cancellation.is_cancelled());
        let error = guard
            .stop()
            .expect_err("heartbeat should report missing task");
        assert!(matches!(error, HeartbeatError::Store(StoreError::NotFound)));
    }

    #[test]
    fn reconciliation_requires_staleness_and_definite_process_loss() {
        let store = memory_store();
        let now = "2026-09-25T12:00:20Z";
        let mut value = queued_task(false, "2026-09-25T12:00:11Z");
        value["status"] = Value::String("RUNNING".into());
        value["pid"] = json!(4321);
        value["process_identity"] = Value::String("old-host:4321:birth".into());
        put_task(&store, &value);

        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| panic!("fresh task must not be checked"))
                .expect("reconcile fresh task"),
            0
        );
        assert_eq!(task(&store)["status"], "RUNNING");

        let stale_time = "2026-09-25T12:00:10Z";
        store
            .mutate("task", "task_one", "", |old| {
                let mut next = old.clone();
                next["heartbeat"] = Value::String(stale_time.into());
                Ok(next)
            })
            .expect("age heartbeat");
        assert_eq!(
            reconcile_with_presence(&store, now, |pid, identity| {
                assert_eq!(pid, 4321);
                assert_eq!(identity, "old-host:4321:birth");
                false
            })
            .expect("ambiguous reconciliation"),
            0
        );
        assert_eq!(task(&store)["status"], "RUNNING");

        let events_before_race = store.events("task_one").expect("events").len();
        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| {
                store
                    .mutate("task", "task_one", "", |old| {
                        let mut next = old.clone();
                        next["heartbeat"] = Value::String(now.into());
                        Ok(next)
                    })
                    .expect("concurrent heartbeat");
                true
            })
            .expect("reconcile after heartbeat race"),
            0
        );
        assert_eq!(task(&store)["status"], "RUNNING");
        assert_eq!(
            store.events("task_one").expect("events").len(),
            events_before_race
        );
        store
            .mutate("task", "task_one", "", |old| {
                let mut next = old.clone();
                next["heartbeat"] = Value::String(stale_time.into());
                Ok(next)
            })
            .expect("restore stale heartbeat fixture");

        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| true)
                .expect("definite-loss reconciliation"),
            1
        );
        let stored = task(&store);
        assert_eq!(stored["status"], "LOST");
        assert!(stored["workspace"].as_str().is_some());
        assert!(stored["error"]
            .as_str()
            .unwrap()
            .contains("no automatic restart"));
        assert_eq!(
            store.events("task_one").expect("events").last().unwrap()["event"],
            "task.lost"
        );
    }

    #[test]
    fn reconciliation_releases_only_a_confirmed_lost_tasks_resources() {
        let store = memory_store();
        let now = "2026-09-25T12:00:20Z";
        let stale = "2026-09-25T12:00:10Z";
        let mut value = queued_task(false, stale);
        value["status"] = Value::String("RUNNING".into());
        value["pid"] = json!(4321);
        put_task(&store, &value);
        let resources = vec!["database_one".to_owned()];
        store
            .acquire_resources("task_one", &resources, 4)
            .expect("reserve task resources");

        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| false).expect("preserve ambiguous owner"),
            0
        );
        assert!(matches!(
            store.acquire_resources("task_two", &resources, 4),
            Err(rover_store::resources::ResourceError::Busy)
        ));

        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| true)
                .expect("reconcile definitely absent process"),
            1
        );
        assert_eq!(task(&store)["status"], "LOST");
        store
            .acquire_resources("task_two", &resources, 4)
            .expect("confirmed loss releases the old owner's reservations");

        // A repeated pass retries owner-scoped cleanup for a LOST task.
        assert_eq!(
            reconcile_with_presence(&store, now, |_, _| panic!("terminal task is not probed"))
                .expect("repeat lost cleanup"),
            0
        );
        assert!(matches!(
            store.acquire_resources("task_one", &resources, 4),
            Err(rover_store::resources::ResourceError::Busy)
        ));
    }

    #[test]
    fn terminal_tasks_cannot_be_cancelled_or_revived_by_heartbeat() {
        let store = memory_store();
        let now = "2026-09-25T12:00:00Z";
        let mut value = queued_task(false, now);
        value["status"] = Value::String("FAILED".into());
        put_task(&store, &value);

        assert!(matches!(
            request_cancel(&store, "task_one", now),
            Err(StoreError::MutationRejected(_))
        ));
        assert!(!heartbeat(&store, "task_one", "2026-09-25T12:00:01Z").expect("terminal heartbeat"));
        assert_eq!(task(&store)["status"], "FAILED");
        assert_eq!(task(&store)["heartbeat"], now);
    }
}
