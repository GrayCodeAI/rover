//! Bounded argv-only local process execution.
//!
//! Local execution runs with the current operating-system user's authority. It
//! is not a sandbox. The crate implements process and task supervision, native
//! PTYs, bounded terminal screens, and Unix local-session brokering. It does
//! not provide Docker isolation or durable session restart restoration.

pub mod layout;
#[cfg(any(unix, windows))]
pub mod pty;
#[cfg(unix)]
pub mod sessions;
pub mod supervisor;
pub mod terminal;

#[cfg(unix)]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::ffi::OsString;
use std::fmt;
#[cfg(unix)]
use std::fs::{self, File};
use std::io;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(unix)]
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
use rover_core::Sha256Digest;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

pub const OUTPUT_LIMIT: usize = 1 << 20;
#[cfg(unix)]
const EXECUTABLE_HASH_LIMIT: u64 = 128 << 20;
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(1);

/// Inputs for one local process invocation.
#[derive(Clone, Debug)]
pub struct Options {
    pub dir: PathBuf,
    pub output_dir: PathBuf,
    pub argv: Vec<String>,
    pub timeout: Duration,
    pub pass_env: Vec<String>,
}

/// Cooperative cancellation handle. Dropping a handle does not cancel a run.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Request cancellation of an active or future run using this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Return whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Serializable execution evidence fields compatible with Rover's Go model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessResult {
    pub started_at: String,
    pub finished_at: String,
    pub exit_code: i32,
    pub error: Option<String>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub truncated: bool,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

/// Process output and executable identity retained for downstream evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultRecord {
    pub process: ProcessResult,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub executable: PathBuf,
    pub executable_sha256: Option<String>,
}

/// Validation, setup, persistence, or process I/O failure.
#[derive(Debug)]
pub struct RunError(io::Error);

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<io::Error> for RunError {
    fn from(error: io::Error) -> Self {
        Self(error)
    }
}

#[cfg(unix)]
struct Capture {
    file: File,
    bytes: Vec<u8>,
    total: u64,
    truncated: bool,
}

#[cfg(unix)]
impl Capture {
    fn new(file: File) -> Self {
        Self {
            file,
            bytes: Vec::with_capacity(4096),
            total: 0,
            truncated: false,
        }
    }

    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.total = self.total.saturating_add(bytes.len() as u64);
        let remaining = OUTPUT_LIMIT.saturating_sub(self.bytes.len());
        let kept = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..kept]);
        self.file.write_all(&bytes[..kept])?;
        self.truncated |= kept != bytes.len();
        Ok(())
    }

    fn complete(self) -> io::Result<CapturedOutput> {
        self.file.sync_all()?;
        Ok(CapturedOutput {
            bytes: self.bytes,
            total: self.total,
            truncated: self.truncated,
        })
    }
}

#[cfg(unix)]
struct CapturedOutput {
    bytes: Vec<u8>,
    total: u64,
    truncated: bool,
}

#[cfg(unix)]
struct RunArtifacts {
    stdout_log: File,
    stderr_log: File,
    executable: PathBuf,
    executable_sha256: Option<String>,
}

/// Start one argv-only local command, retain bounded stdout/stderr, and persist
/// the retained bytes as exclusive private log files under `output_dir`.
///
/// # Errors
///
/// Returns an error for invalid argv/environment/timeout, unsafe private output
/// paths, failure to create logs, or pipe read/persistence failures. A command
/// that cannot be spawned is represented in `ProcessResult.error`, matching
/// Rover's existing evidence behavior.
///
/// # Panics
///
/// Panics only if `std::process::Command` fails to provide a pipe that was
/// explicitly requested as piped.
#[cfg(unix)]
pub fn run(options: &Options, cancellation: &CancellationToken) -> Result<ResultRecord, RunError> {
    validate(options)?;
    let output = rover_store::files::prepare_private_directory(&options.output_dir)?;
    let stdout_log = output.create_new_file_writer("stdout.log", 0o600)?;
    let stderr_log = output.create_new_file_writer("stderr.log", 0o600)?;
    let timeout_deadline = Instant::now()
        .checked_add(options.timeout)
        .unwrap_or_else(Instant::now);
    let (environment, host_path) = clean_environment(options)?;

    let (executable, executable_not_found) =
        resolve_executable(&options.argv[0], &options.dir, &host_path);
    let executable_sha256 = hash_executable(&executable);
    let started_at = rover_core::now().to_string();

    let mut process = ProcessResult {
        started_at,
        finished_at: String::new(),
        exit_code: -1,
        error: None,
        timed_out: false,
        cancelled: false,
        truncated: false,
        stdout_sha256: String::new(),
        stderr_sha256: String::new(),
        stdout_bytes: 0,
        stderr_bytes: 0,
    };

    if executable_not_found {
        process.error = Some(format!(
            "exec: \"{}\": executable file not found in $PATH",
            options.argv[0]
        ));
        process.finished_at = rover_core::now().to_string();
        return finish_record(
            process,
            Capture::new(stdout_log),
            Capture::new(stderr_log),
            executable,
            executable_sha256,
        );
    }

    if cancellation.is_cancelled() {
        process.cancelled = true;
        process.error = Some("context canceled".into());
        process.finished_at = rover_core::now().to_string();
        return finish_record(
            process,
            Capture::new(stdout_log),
            Capture::new(stderr_log),
            executable,
            executable_sha256,
        );
    }

    let mut command = Command::new(&executable);
    command
        .args(&options.argv[1..])
        .current_dir(&options.dir)
        .env_clear()
        .envs(&environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.process_group(0);

    run_child(
        command,
        process,
        cancellation,
        timeout_deadline,
        RunArtifacts {
            stdout_log,
            stderr_log,
            executable,
            executable_sha256,
        },
    )
}

#[cfg(unix)]
fn run_child(
    mut command: Command,
    mut process: ProcessResult,
    cancellation: &CancellationToken,
    deadline: Instant,
    artifacts: RunArtifacts,
) -> Result<ResultRecord, RunError> {
    let RunArtifacts {
        stdout_log,
        stderr_log,
        executable,
        executable_sha256,
    } = artifacts;
    if cancellation.is_cancelled() || Instant::now() >= deadline {
        process.cancelled = cancellation.is_cancelled();
        process.timed_out = !process.cancelled;
        process.error = Some(if process.cancelled {
            "context canceled".into()
        } else {
            "context deadline exceeded".into()
        });
        process.finished_at = rover_core::now().to_string();
        return finish_record(
            process,
            Capture::new(stdout_log),
            Capture::new(stderr_log),
            executable,
            executable_sha256,
        );
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            process.error = Some(format!("fork/exec {}: {error}", executable.display()));
            process.finished_at = rover_core::now().to_string();
            return finish_record(
                process,
                Capture::new(stdout_log),
                Capture::new(stderr_log),
                executable,
                executable_sha256,
            );
        }
    };
    let process_id = rustix::process::Pid::from_child(&child);
    let stdout = child.stdout.take().expect("stdout pipe was requested");
    let stderr = child.stderr.take().expect("stderr pipe was requested");
    let finished = Arc::new(AtomicBool::new(false));
    let stdout_finished = Arc::clone(&finished);
    let stderr_finished = Arc::clone(&finished);
    let stdout_reader = thread::spawn(move || read_capture(stdout, &stdout_finished, stdout_log));
    let stderr_reader = thread::spawn(move || read_capture(stderr, &stderr_finished, stderr_log));
    let mut captures = CaptureThreads {
        finished,
        stdout: Some(stdout_reader),
        stderr: Some(stderr_reader),
    };
    let outcome = wait_for_child(
        &mut child,
        process_id,
        cancellation,
        deadline,
        &mut captures,
    )?;
    process.cancelled = outcome.cancelled;
    process.timed_out = outcome.timed_out;
    set_exit_status(&mut process, outcome.status);
    let (stdout, stderr) = captures.join()?;
    process.finished_at = rover_core::now().to_string();
    finish_record(process, stdout, stderr, executable, executable_sha256)
}

#[cfg(unix)]
struct CaptureThreads {
    finished: Arc<AtomicBool>,
    stdout: Option<thread::JoinHandle<io::Result<Capture>>>,
    stderr: Option<thread::JoinHandle<io::Result<Capture>>>,
}

#[cfg(unix)]
impl CaptureThreads {
    fn join(&mut self) -> io::Result<(Capture, Capture)> {
        self.finished.store(true, Ordering::Release);
        let stdout = self
            .stdout
            .take()
            .expect("stdout capture thread exists")
            .join()
            .map_err(|_| io::Error::other("stdout capture thread panicked"))??;
        let stderr = self
            .stderr
            .take()
            .expect("stderr capture thread exists")
            .join()
            .map_err(|_| io::Error::other("stderr capture thread panicked"))??;
        Ok((stdout, stderr))
    }

    fn stop_after_wait_error(
        &mut self,
        child: &mut std::process::Child,
        error: io::Error,
    ) -> RunError {
        kill_group(rustix::process::Pid::from_child(child), child);
        let _ = child.wait();
        self.finished.store(true, Ordering::Release);
        if let Some(reader) = self.stdout.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
        RunError(error)
    }
}

#[cfg(unix)]
struct WaitOutcome {
    status: std::process::ExitStatus,
    cancelled: bool,
    timed_out: bool,
}

#[cfg(unix)]
fn wait_for_child(
    child: &mut std::process::Child,
    process_id: rustix::process::Pid,
    cancellation: &CancellationToken,
    deadline: Instant,
    captures: &mut CaptureThreads,
) -> Result<WaitOutcome, RunError> {
    let mut cancelled = false;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(captures.stop_after_wait_error(child, error)),
        }
        if cancellation.is_cancelled() {
            cancelled = true;
            kill_group(process_id, child);
            match child.wait() {
                Ok(status) => break status,
                Err(error) => return Err(captures.stop_after_wait_error(child, error)),
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_group(process_id, child);
            match child.wait() {
                Ok(status) => break status,
                Err(error) => return Err(captures.stop_after_wait_error(child, error)),
            }
        }
        thread::sleep(POLL_INTERVAL);
    };
    Ok(WaitOutcome {
        status,
        cancelled,
        timed_out,
    })
}

#[cfg(unix)]
fn clean_environment(options: &Options) -> io::Result<(BTreeMap<OsString, OsString>, OsString)> {
    let environment_root = options.output_dir.join("environment");
    let environment_dir = rover_store::files::prepare_private_directory(&environment_root)?;
    let mut environment = BTreeMap::<OsString, OsString>::new();
    let host_path = std::env::var_os("PATH").unwrap_or_default();
    environment.insert("PATH".into(), host_path.clone());
    for (name, path) in [
        ("HOME", environment_root.join("home")),
        ("TMPDIR", environment_root.join("tmp")),
        ("GOCACHE", environment_root.join("cache")),
        ("GOPATH", environment_root.join("gopath")),
    ] {
        environment_dir
            .ensure_subdirectory(name.to_ascii_lowercase().as_str())?
            .set_permissions(0o700)?;
        environment.insert(name.into(), path.into_os_string());
    }
    for (name, value) in [
        ("GOTOOLCHAIN", "local"),
        ("GOPROXY", "off"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_TERMINAL_PROMPT", "0"),
    ] {
        environment.insert(name.into(), value.into());
    }
    for name in &options.pass_env {
        if let Some(value) = std::env::var_os(name) {
            environment.insert(name.into(), value);
        }
    }
    Ok((environment, host_path))
}

/// Run is not yet supported on non-Unix targets because safe process-tree
/// cancellation and descriptor-rooted private capture are not implemented.
#[cfg(not(unix))]
pub fn run(
    _options: &Options,
    _cancellation: &CancellationToken,
) -> Result<ResultRecord, RunError> {
    Err(RunError(io::Error::new(
        io::ErrorKind::Unsupported,
        "process execution is not supported on this target",
    )))
}

#[cfg(unix)]
fn validate(options: &Options) -> io::Result<()> {
    if options.argv.is_empty() || options.argv.len() > 129 || options.argv[0].trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "argv must contain an executable and at most 128 arguments",
        ));
    }
    if options
        .argv
        .iter()
        .any(|argument| argument.contains('\0') || argument.len() > 65_536)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "argv contains NUL or oversized argument",
        ));
    }
    if options.timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "positive timeout required",
        ));
    }
    for name in &options.pass_env {
        if !valid_env_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid environment variable name {name:?}"),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(unix)]
fn read_capture<R: Read + std::os::fd::AsFd>(
    mut reader: R,
    finished: &AtomicBool,
    log: File,
) -> io::Result<Capture> {
    use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
    let flags = fcntl_getfl(&reader)?;
    fcntl_setfl(&reader, flags | OFlags::NONBLOCK)?;
    let mut capture = Capture::new(log);
    let mut buffer = [0_u8; 16 * 1024];
    let mut drain_until = None;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(capture),
            Ok(count) => capture.append(&buffer[..count])?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if finished.load(Ordering::Acquire) {
                    let limit =
                        drain_until.get_or_insert_with(|| Instant::now() + PIPE_DRAIN_GRACE);
                    if Instant::now() >= *limit {
                        return Ok(capture);
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn kill_group(pid: rustix::process::Pid, child: &mut std::process::Child) {
    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    let _ = child.kill();
}

#[cfg(unix)]
fn set_exit_status(process: &mut ProcessResult, status: ExitStatus) {
    process.exit_code = status.code().unwrap_or(-1);
    if !status.success() {
        process.error = Some(match status.code() {
            Some(code) => format!("exit status {code}"),
            None => format!(
                "signal: {}",
                unix_signal_name(status.signal().unwrap_or_default())
            ),
        });
    }
}

#[cfg(unix)]
fn unix_signal_name(signal: i32) -> String {
    #[cfg(target_os = "macos")]
    let name = match signal {
        1 => Some("hangup"),
        2 => Some("interrupt"),
        3 => Some("quit"),
        4 => Some("illegal instruction"),
        5 => Some("trace/BPT trap"),
        6 => Some("abort trap"),
        7 => Some("EMT trap"),
        8 => Some("floating point exception"),
        9 => Some("killed"),
        10 => Some("bus error"),
        11 => Some("segmentation fault"),
        12 => Some("bad system call"),
        13 => Some("broken pipe"),
        14 => Some("alarm clock"),
        15 => Some("terminated"),
        16 => Some("urgent I/O condition"),
        17 => Some("suspended (signal)"),
        18 => Some("suspended"),
        19 => Some("continued"),
        20 => Some("child exited"),
        21 => Some("stopped (tty input)"),
        22 => Some("stopped (tty output)"),
        23 => Some("I/O possible"),
        24 => Some("cputime limit exceeded"),
        25 => Some("filesize limit exceeded"),
        26 => Some("virtual timer expired"),
        27 => Some("profiling timer expired"),
        28 => Some("window size changes"),
        _ => None,
    };
    #[cfg(target_os = "linux")]
    let name = match signal {
        1 => Some("hangup"),
        2 => Some("interrupt"),
        3 => Some("quit"),
        4 => Some("illegal instruction"),
        5 => Some("trace/breakpoint trap"),
        6 => Some("aborted"),
        7 => Some("bus error"),
        8 => Some("floating point exception"),
        9 => Some("killed"),
        10 => Some("user defined signal 1"),
        11 => Some("segmentation fault"),
        12 => Some("user defined signal 2"),
        13 => Some("broken pipe"),
        14 => Some("alarm clock"),
        15 => Some("terminated"),
        16 => Some("stack fault"),
        17 => Some("child exited"),
        18 => Some("continued"),
        19 => Some("stopped (signal)"),
        20 => Some("stopped"),
        21 => Some("stopped (tty input)"),
        22 => Some("stopped (tty output)"),
        23 => Some("urgent I/O condition"),
        24 => Some("CPU time limit exceeded"),
        25 => Some("file size limit exceeded"),
        26 => Some("virtual timer expired"),
        27 => Some("profiling timer expired"),
        28 => Some("window changed"),
        29 => Some("I/O possible"),
        30 => Some("power failure"),
        31 => Some("bad system call"),
        _ => None,
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let name: Option<&str> = None;

    name.map_or_else(|| format!("signal {signal}"), str::to_owned)
}

#[cfg(unix)]
fn finish_record(
    mut process: ProcessResult,
    stdout: Capture,
    stderr: Capture,
    executable: PathBuf,
    executable_sha256: Option<String>,
) -> Result<ResultRecord, RunError> {
    let stdout = stdout.complete()?;
    let stderr = stderr.complete()?;
    process.truncated = stdout.truncated || stderr.truncated;
    process.stdout_sha256 = Sha256Digest::of(&stdout.bytes).to_hex();
    process.stderr_sha256 = Sha256Digest::of(&stderr.bytes).to_hex();
    process.stdout_bytes = stdout.total;
    process.stderr_bytes = stderr.total;
    Ok(ResultRecord {
        process,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        executable,
        executable_sha256,
    })
}

#[cfg(unix)]
fn resolve_executable(program: &str, dir: &Path, path: &std::ffi::OsStr) -> (PathBuf, bool) {
    let program_path = Path::new(program);
    if program_path.is_absolute() {
        return (program_path.to_path_buf(), false);
    }
    if program.contains(std::path::MAIN_SEPARATOR) {
        let working_directory = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(dir)
        };
        return (working_directory.join(program_path), false);
    }
    let current_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for directory in std::env::split_paths(path) {
        let candidate = if directory.as_os_str().is_empty() {
            current_directory.join(program)
        } else if directory.is_absolute() {
            directory.join(program)
        } else {
            current_directory.join(directory).join(program)
        };
        if candidate.is_file() && is_executable(&candidate) {
            return (candidate, false);
        }
    }
    (program_path.to_path_buf(), true)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(unix)]
fn hash_executable(path: &Path) -> Option<String> {
    hash_executable_prefix(path, EXECUTABLE_HASH_LIMIT)
}

#[cfg(unix)]
fn hash_executable_prefix(path: &Path, limit: u64) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len().min(limit)).ok()?);
    File::open(path)
        .ok()?
        .take(limit)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(Sha256Digest::of(&bytes).to_hex())
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        hash_executable_prefix, resolve_executable, run, validate, CancellationToken, Options,
        OUTPUT_LIMIT,
    };
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    struct Fixture {
        root: PathBuf,
        work: PathBuf,
        output: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rover-execution-test-{}-{}",
                std::process::id(),
                rover_core::Id::generate("test").unwrap()
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let work = root.join("work");
            fs::create_dir(&work).unwrap();
            Self {
                output: root.join("output"),
                root,
                work,
            }
        }

        fn options(&self, script: &str) -> Options {
            Options {
                dir: self.work.clone(),
                output_dir: self.output.clone(),
                argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
                timeout: Duration::from_secs(3),
                pass_env: Vec::new(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn captures_process_result_and_persists_private_logs() {
        let fixture = Fixture::new();
        let result = run(
            &fixture.options("printf hello; printf error >&2; exit 7"),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(result.process.exit_code, 7);
        assert_eq!(result.process.error.as_deref(), Some("exit status 7"));
        assert_eq!(result.stdout, b"hello");
        assert_eq!(result.stderr, b"error");
        assert_eq!(result.process.stdout_bytes, 5);
        assert_eq!(result.process.stderr_bytes, 5);
        assert_eq!(
            result.process.stdout_sha256,
            rover_core::Sha256Digest::of(b"hello").to_hex()
        );
        assert_eq!(
            fs::read(fixture.output.join("stdout.log")).unwrap(),
            b"hello"
        );
        assert_eq!(
            fs::metadata(fixture.output.join("stdout.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(result.executable_sha256.is_some());
    }

    #[test]
    fn maps_signal_termination_to_go_compatible_negative_one_exit_code() {
        let fixture = Fixture::new();
        let result = run(
            &fixture.options("kill -TERM $$"),
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(result.process.exit_code, -1);
        assert_eq!(result.process.error.as_deref(), Some("signal: terminated"));
    }

    #[test]
    fn clears_environment_and_inherits_only_named_values() {
        let fixture = Fixture::new();
        std::env::set_var("ROVER_EXECUTION_TEST_SECRET", "allowed");
        let mut options = fixture.options("test -z \"$ROVER_EXECUTION_TEST_SECRET\"");
        assert_eq!(
            run(&options, &CancellationToken::default())
                .unwrap()
                .process
                .exit_code,
            0
        );
        options.pass_env.push("ROVER_EXECUTION_TEST_SECRET".into());
        options.argv[2] = "test \"$ROVER_EXECUTION_TEST_SECRET\" = allowed".into();
        options.output_dir = fixture.root.join("output-pass-env");
        assert_eq!(
            run(&options, &CancellationToken::default())
                .unwrap()
                .process
                .exit_code,
            0
        );
        std::env::remove_var("ROVER_EXECUTION_TEST_SECRET");
    }

    #[test]
    fn missing_executable_is_recorded_and_logs_are_created() {
        let fixture = Fixture::new();
        let mut options = fixture.options("ignored");
        options.argv = vec!["rover-does-not-exist".into()];
        let result = run(&options, &CancellationToken::default()).unwrap();
        assert_eq!(result.process.exit_code, -1);
        assert_eq!(
            result.process.error.as_deref(),
            Some("exec: \"rover-does-not-exist\": executable file not found in $PATH")
        );
        assert!(fixture.output.join("stdout.log").is_file());
    }

    #[test]
    fn executable_hash_matches_go_limited_prefix_behavior() {
        let fixture = Fixture::new();
        let file = fixture.root.join("large-executable");
        fs::write(&file, b"abcdefgh").unwrap();
        assert_eq!(
            hash_executable_prefix(&file, 4),
            Some(rover_core::Sha256Digest::of(b"abcd").to_hex())
        );
    }

    #[test]
    fn pre_cancelled_run_records_cancellation_without_starting_the_command() {
        let fixture = Fixture::new();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let result = run(&fixture.options("touch should-not-exist"), &cancellation).unwrap();
        assert!(result.process.cancelled);
        assert_eq!(result.process.exit_code, -1);
        assert_eq!(result.process.error.as_deref(), Some("context canceled"));
        assert!(!fixture.work.join("should-not-exist").exists());
        assert_eq!(fs::read(fixture.output.join("stdout.log")).unwrap(), b"");
    }

    #[test]
    fn expired_deadline_during_environment_setup_prevents_process_start() {
        let fixture = Fixture::new();
        let mut options = fixture.options("touch should-not-exist");
        options.timeout = Duration::from_nanos(1);
        let result = run(&options, &CancellationToken::default()).unwrap();
        assert!(result.process.timed_out);
        assert_eq!(
            result.process.error.as_deref(),
            Some("context deadline exceeded")
        );
        assert!(!fixture.work.join("should-not-exist").exists());
    }

    #[test]
    fn existing_capture_log_is_never_replaced_or_executed_over() {
        let fixture = Fixture::new();
        fs::create_dir(&fixture.output).unwrap();
        fs::set_permissions(&fixture.output, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(fixture.output.join("stdout.log"), b"preserve").unwrap();
        let result = run(
            &fixture.options("touch should-not-exist"),
            &CancellationToken::default(),
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(fixture.output.join("stdout.log")).unwrap(),
            b"preserve"
        );
        assert!(!fixture.work.join("should-not-exist").exists());
    }

    #[test]
    fn output_capture_is_capped_but_counts_all_bytes() {
        let fixture = Fixture::new();
        let result = run(
            &fixture.options("head -c 1200000 /dev/zero; head -c 1300000 /dev/zero >&2"),
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(result.process.truncated);
        assert_eq!(result.stdout.len(), OUTPUT_LIMIT);
        assert_eq!(result.process.stdout_bytes, 1_200_000);
        assert_eq!(result.stderr.len(), OUTPUT_LIMIT);
        assert_eq!(result.process.stderr_bytes, 1_300_000);
    }

    #[test]
    fn output_logs_are_written_while_the_process_runs() {
        let fixture = Fixture::new();
        let mut options = fixture.options("printf live; sleep 0.3; printf done");
        options.timeout = Duration::from_secs(2);
        let worker =
            std::thread::spawn(move || run(&options, &CancellationToken::default()).unwrap());
        let log = fixture.output.join("stdout.log");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut observed_live_output = false;
        while std::time::Instant::now() < deadline {
            if fs::read(&log).is_ok_and(|bytes| bytes == b"live") {
                observed_live_output = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = worker.join().unwrap();
        assert!(
            observed_live_output,
            "stdout log should update before process completion"
        );
        assert_eq!(result.stdout, b"livedone");
    }

    #[test]
    fn exact_output_limit_is_not_marked_truncated() {
        let fixture = Fixture::new();
        let result = run(
            &fixture.options("head -c 1048576 /dev/zero"),
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(!result.process.truncated);
        assert_eq!(result.stdout.len(), OUTPUT_LIMIT);
    }

    #[test]
    fn timeout_and_cancellation_terminate_the_process_group() {
        let fixture = Fixture::new();
        let mut options = fixture.options("(sleep 0.3; echo leaked > descendant.txt) & wait");
        options.timeout = Duration::from_millis(80);
        let result = run(&options, &CancellationToken::default()).unwrap();
        assert!(result.process.timed_out);
        assert_eq!(result.process.error.as_deref(), Some("signal: killed"));
        std::thread::sleep(Duration::from_millis(450));
        assert!(!fixture.work.join("descendant.txt").exists());
        let cancellation = CancellationToken::default();
        let request = cancellation.clone();
        let mut worker_options = fixture.options("sleep 20 & wait");
        worker_options.output_dir = fixture.root.join("output-cancel");
        let worker = std::thread::spawn(move || run(&worker_options, &request).unwrap());
        std::thread::sleep(Duration::from_millis(80));
        cancellation.cancel();
        let cancelled = worker.join().unwrap().process;
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.error.as_deref(), Some("signal: killed"));
    }

    #[test]
    fn rejects_invalid_argv_timeout_and_environment_names() {
        let fixture = Fixture::new();
        let mut options = fixture.options("true");
        options.argv[0].clear();
        assert!(run(&options, &CancellationToken::default()).is_err());
        options = fixture.options("true");
        options.timeout = Duration::ZERO;
        assert!(run(&options, &CancellationToken::default()).is_err());
        options = fixture.options("true");
        options.pass_env.push("bad-name".into());
        assert!(run(&options, &CancellationToken::default()).is_err());

        options = fixture.options("true");
        options.argv = vec!["true".into(); 129];
        assert!(
            validate(&options).is_ok(),
            "128 arguments after argv[0] are allowed"
        );
        options.argv.push("extra".into());
        assert!(validate(&options).is_err());
        options.argv = vec!["true".into(), "bad\0arg".into()];
        assert!(validate(&options).is_err());
        options.argv = vec!["true".into(), "x".repeat(65_537)];
        assert!(validate(&options).is_err());
    }

    #[test]
    fn executable_lookup_uses_the_parent_path_and_preserves_explicit_paths() {
        let fixture = Fixture::new();
        let path_dir = fixture.root.join("bin");
        fs::create_dir(&path_dir).unwrap();
        let candidate = path_dir.join("fixture-tool");
        fs::write(&candidate, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o700)).unwrap();
        let (resolved, missing) = resolve_executable(
            "fixture-tool",
            &fixture.work,
            OsStr::new(path_dir.to_str().unwrap()),
        );
        assert!(!missing);
        assert_eq!(resolved, candidate);
        let (explicit, missing) =
            resolve_executable("./fixture-tool", &fixture.work, OsStr::new(""));
        assert!(!missing);
        assert_eq!(explicit, fixture.work.join("fixture-tool"));
    }
}
