//! Local pseudo-terminal sessions on Unix and Windows `ConPTY`.
//!
//! This module owns one child process and a native PTY pair. It does not
//! implement scrollback, terminal emulation, persistence, or client attach.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

const MAX_ARGV: usize = 129;
const MAX_ARGUMENT_BYTES: usize = 65_536;
const MAX_DIMENSION: u16 = 1000;
#[cfg(target_os = "linux")]
const MAX_PROC_SCAN: usize = 4096;
#[cfg(target_os = "macos")]
const MAX_PROCESS_LIST_BYTES: usize = 262_144;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_FOREGROUND_GROUP_MEMBERS: usize = 64;
#[cfg(target_os = "linux")]
const MAX_PROCESS_ARG_BYTES: usize = 4096;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_GROUP_ARG_BYTES: usize = 8192;
#[cfg(target_os = "linux")]
const MAX_PROCESS_ARG_COUNT: usize = 64;

/// Bounded identity and argv evidence for one process in an owned PTY group.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForegroundProcessDetails {
    /// Validated executable basename from the platform process table.
    pub name: String,
    /// Process argument vector, when available and within the sampler bounds.
    pub argv: Option<Vec<String>>,
}
#[cfg(target_os = "macos")]
const MAX_PROCESS_NAME_BYTES: usize = 256;

/// Process, environment, working directory, and initial terminal dimensions.
#[derive(Clone, Debug)]
pub struct PtyOptions {
    /// Absolute executable and arguments. Rover does not insert a shell.
    pub argv: Vec<String>,
    /// Existing absolute directory used as the child's working directory.
    pub cwd: PathBuf,
    /// Complete child environment. No parent variables are inherited.
    pub env: BTreeMap<String, String>,
    /// Initial terminal rows.
    pub rows: u16,
    /// Initial terminal columns.
    pub cols: u16,
}

/// One owned native PTY session and its directly spawned child.
pub struct PtySession {
    master: Option<Box<dyn MasterPty + Send>>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
}

impl PtySession {
    /// Create a native PTY and spawn the exact argv with a cleared environment.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for invalid argv, environment, directory, or
    /// dimensions, and an I/O error when the native PTY or process cannot be
    /// created.
    pub fn spawn(options: &PtyOptions) -> io::Result<Self> {
        validate_options(options)?;

        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: options.rows,
                cols: options.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(backend_error)?;

        let reader = pair.master.try_clone_reader().map_err(backend_error)?;
        let writer = pair.master.take_writer().map_err(backend_error)?;
        let mut command = CommandBuilder::new(&options.argv[0]);
        command.args(options.argv[1..].iter());
        command.env_clear();
        for (name, value) in &options.env {
            command.env(name, value);
        }
        command.cwd(&options.cwd);
        let child = pair.slave.spawn_command(command).map_err(backend_error)?;
        drop(pair.slave);

        Ok(Self {
            master: Some(pair.master),
            reader: Some(reader),
            writer: Some(writer),
            child: Some(child),
        })
    }

    /// Read bytes from the PTY child output.
    ///
    /// This is a blocking read and returns the native PTY's EOF/error when
    /// the child or session closes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the session is closed or reading fails.
    pub fn read_output(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reader
            .as_mut()
            .ok_or_else(session_closed)?
            .read(buffer)
    }

    /// Move the PTY output reader to a dedicated broker thread.
    ///
    /// This is used by the session server so blocking output reads do not
    /// prevent client input, resizing, or process shutdown. A reader can be
    /// taken only once; subsequent reads through this session report closed.
    ///
    /// # Errors
    ///
    /// Returns `BrokenPipe` if the reader was already moved or the session was
    /// closed.
    pub fn take_output_reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
        self.reader.take().ok_or_else(session_closed)
    }

    /// Write input bytes to the PTY child.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the session is closed or writing fails.
    pub fn write_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer
            .as_mut()
            .ok_or_else(session_closed)?
            .write_all(bytes)
    }

    /// Change the terminal size and notify the attached child.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for zero or oversized dimensions, or an I/O
    /// error if the native terminal resize fails.
    pub fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        validate_dimensions(rows, cols)?;
        self.master
            .as_ref()
            .ok_or_else(session_closed)?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(backend_error)
    }

    /// Return the directly spawned child PID where the platform exposes it.
    #[must_use]
    pub fn child_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|child| child.process_id())
    }

    /// Sample the executable name of the PTY's current foreground process-group leader.
    ///
    /// A command pipeline reports its group leader only; Rover does not search
    /// child processes. A missing process or unsupported platform returns
    /// `None` so callers can retain an unknown state.
    #[cfg(unix)]
    #[must_use]
    pub fn foreground_process_name(&self) -> Option<String> {
        let pid = self.master.as_ref()?.process_group_leader()?;
        foreground_process_name(pid)
    }

    /// Sample executable basenames in the PTY's foreground process group.
    ///
    /// Sampling is bounded and platform owned. Unsupported systems and
    /// incomplete or unstable scans return `None` rather than a partial list.
    #[cfg(unix)]
    #[must_use]
    pub fn foreground_process_group_names(&self) -> Option<Vec<String>> {
        let pgid = self.master.as_ref()?.process_group_leader()?;
        let details = Self::foreground_process_group_details_for(pgid)?;
        let mut names = details
            .into_iter()
            .map(|process| process.name)
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Some(names)
    }

    /// Sample process names and bounded argv vectors from the PTY foreground
    /// process group. The list is never accepted from a client.
    #[cfg(unix)]
    #[must_use]
    pub fn foreground_process_group_details(&self) -> Option<Vec<ForegroundProcessDetails>> {
        let pgid = self.master.as_ref()?.process_group_leader()?;
        Self::foreground_process_group_details_for(pgid)
    }

    #[cfg(unix)]
    fn foreground_process_group_details_for(pgid: i32) -> Option<Vec<ForegroundProcessDetails>> {
        #[cfg(target_os = "linux")]
        {
            linux_foreground_process_group_details(pgid)
        }
        #[cfg(target_os = "macos")]
        {
            macos_foreground_process_group_details(pgid)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = pgid;
            None
        }
    }

    /// Check whether the direct child has exited without blocking.
    ///
    /// # Errors
    ///
    /// Returns `BrokenPipe` if the child handle is no longer owned, or an I/O
    /// error if the platform backend cannot inspect it.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(session_closed)?
            .try_wait()
            .map_err(backend_error)
    }

    /// Stop and reap the child, then close all PTY handles. Repeated calls are
    /// harmless.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if querying, stopping, or reaping the child fails.
    pub fn close(&mut self) -> io::Result<()> {
        let result = if let Some(child) = self.child.as_mut() {
            if child.try_wait()?.is_some() {
                Ok(())
            } else {
                child.kill()?;
                child.wait().map(|_| ())
            }
        } else {
            Ok(())
        };
        self.child.take();
        self.writer.take();
        self.reader.take();
        self.master.take();
        result
    }
}

#[cfg(target_os = "linux")]
fn linux_foreground_process_group_details(pgid: i32) -> Option<Vec<ForegroundProcessDetails>> {
    if pgid <= 0 {
        return None;
    }
    let entries = std::fs::read_dir("/proc").ok()?;
    let mut processes = Vec::new();
    let mut argv_bytes = 0usize;
    let skip_argv =
        std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some();
    let mut scanned = 0;
    for entry in entries {
        let entry = entry.ok()?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        scanned += 1;
        if scanned > MAX_PROC_SCAN {
            return None;
        }
        let Some((group, start_time)) = linux_process_group_identity(pid) else {
            continue;
        };
        if group != pgid {
            continue;
        }
        let identity = crate::supervisor::process_identity(pid)?;
        if identity.rsplit_once(':').map(|(_, start)| start) != Some(start_time.as_str()) {
            return None;
        }
        let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        if crate::supervisor::process_identity(pid).as_deref() != Some(identity.as_str())
            || linux_process_group_identity(pid).map(|sample| sample.0) != Some(pgid)
        {
            return None;
        }
        let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
            return None;
        };
        let name = valid_executable_name(name)?;
        if processes.len() == MAX_FOREGROUND_GROUP_MEMBERS {
            return None;
        }
        let argv = if skip_argv {
            None
        } else {
            linux_process_argv(pid)
        };
        let argv = argv.filter(|argv| {
            let bytes = argv.iter().map(String::len).sum::<usize>();
            if argv_bytes.saturating_add(bytes) > MAX_GROUP_ARG_BYTES {
                false
            } else {
                argv_bytes += bytes;
                true
            }
        });
        if crate::supervisor::process_identity(pid).as_deref() != Some(identity.as_str())
            || linux_process_group_identity(pid).map(|sample| sample.0) != Some(pgid)
        {
            return None;
        }
        processes.push(ForegroundProcessDetails { name, argv });
    }
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    Some(processes)
}

#[cfg(target_os = "linux")]
fn linux_process_argv(pid: i32) -> Option<Vec<String>> {
    let file = std::fs::File::open(format!("/proc/{pid}/cmdline")).ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_PROCESS_ARG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    parse_nul_terminated_argv(&bytes)
}

// Only the Linux /proc reader produces a real argv vector; the macOS backend
// deliberately reports unknown argv rather than parse a lossy `ps` command
// line, so this helper and its bounds are Linux-only.
#[cfg(target_os = "linux")]
fn parse_nul_terminated_argv(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.is_empty() || bytes.len() > MAX_PROCESS_ARG_BYTES {
        return None;
    }
    let mut argv = bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| {
            let value = std::str::from_utf8(argument).ok()?;
            (!value.chars().any(char::is_control)).then(|| value.to_owned())
        })
        .collect::<Option<Vec<_>>>()?;
    if argv.is_empty() || argv.len() > MAX_PROCESS_ARG_COUNT {
        return None;
    }
    Some(std::mem::take(&mut argv))
}

#[cfg(target_os = "linux")]
fn linux_process_group_identity(pid: i32) -> Option<(i32, String)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat.get(stat.rfind(')')?.checked_add(1)?..)?;
    let fields = fields.split_ascii_whitespace().collect::<Vec<_>>();
    // The first field after the command is state (field 3); pgrp is field 5
    // and starttime is field 22 in procfs' stat format.
    Some((fields.get(2)?.parse().ok()?, fields.get(19)?.to_string()))
}

#[cfg(target_os = "macos")]
fn macos_foreground_process_group_details(pgid: i32) -> Option<Vec<ForegroundProcessDetails>> {
    if pgid <= 0 {
        return None;
    }
    let mut child = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=", "-o", "pgid=", "-o", "comm="])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let mut output = Vec::new();
    if stdout
        .take((MAX_PROCESS_LIST_BYTES + 1) as u64)
        .read_to_end(&mut output)
        .is_err()
        || output.len() > MAX_PROCESS_LIST_BYTES
    {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    let output = std::str::from_utf8(&output).ok()?;
    let mut processes = Vec::new();
    let mut argv_bytes = 0usize;
    for line in output.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(process_id) = fields
            .next()
            .and_then(|process_id| process_id.parse::<i32>().ok())
        else {
            continue;
        };
        let Some(group) = fields.next().and_then(|group| group.parse::<i32>().ok()) else {
            continue;
        };
        if group != pgid {
            continue;
        }
        let first = macos_process_sample(process_id)?;
        if first.1 != pgid {
            return None;
        }
        let name = valid_executable_name(PathBuf::from(&first.2).file_name()?.to_str()?)?;
        if processes.len() == MAX_FOREGROUND_GROUP_MEMBERS {
            return None;
        }
        // `ps` output is untrusted input here; a negative pid must be rejected
        // rather than wrapped into a valid-looking u32.
        let argv = u32::try_from(process_id).ok().and_then(macos_process_argv);
        let second = macos_process_sample(process_id)?;
        if first != second {
            return None;
        }
        let argv = argv.filter(|argv| {
            let bytes = argv.iter().map(String::len).sum::<usize>();
            if argv_bytes.saturating_add(bytes) > MAX_GROUP_ARG_BYTES {
                false
            } else {
                argv_bytes += bytes;
                true
            }
        });
        processes.push(ForegroundProcessDetails { name, argv });
    }
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    Some(processes)
}

#[cfg(target_os = "macos")]
fn macos_process_argv(_pid: u32) -> Option<Vec<String>> {
    // `ps` exposes a display command line, not argv boundaries. The workspace
    // forbids unsafe code, so we preserve unknown wrapper identity here rather
    // than parse a lossy command string as an argument vector.
    None
}

#[cfg(target_os = "linux")]
fn foreground_process_name(pid: i32) -> Option<String> {
    let before = crate::supervisor::process_identity(pid)?;
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let after = crate::supervisor::process_identity(pid)?;
    if before != after {
        return None;
    }
    valid_executable_name(executable.file_name()?.to_str()?)
}

#[cfg(target_os = "macos")]
fn foreground_process_name(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let first = macos_process_sample(pid)?;
    let second = macos_process_sample(pid)?;
    if first != second || first.1 != pid {
        return None;
    }
    valid_executable_name(PathBuf::from(first.2).file_name()?.to_str()?)
}

#[cfg(target_os = "macos")]
fn macos_process_sample(pid: i32) -> Option<(String, i32, String)> {
    let mut child = Command::new("/bin/ps")
        .args([
            "-p",
            &pid.to_string(),
            "-o",
            "lstart=",
            "-o",
            "pgid=",
            "-o",
            "comm=",
        ])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut output = Vec::new();
    let stdout = child.stdout.take()?;
    if stdout
        .take((MAX_PROCESS_NAME_BYTES + 1) as u64)
        .read_to_end(&mut output)
        .is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if output.len() > MAX_PROCESS_NAME_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    let process = std::str::from_utf8(&output).ok()?.trim();
    let fields = process.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() < 7 {
        return None;
    }
    let start = fields[..5].join(" ");
    let group = fields[5].parse().ok()?;
    let command = fields[6..].join(" ");
    (!start.is_empty() && !command.is_empty()).then_some((start, group, command))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn foreground_process_name(_pid: i32) -> Option<String> {
    None
}

#[cfg(unix)]
fn valid_executable_name(name: &str) -> Option<String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+'))
    {
        return None;
    }
    Some(name.to_owned())
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn validate_options(options: &PtyOptions) -> io::Result<()> {
    if options.argv.is_empty() || options.argv.len() > MAX_ARGV {
        return Err(invalid_input(
            "argv must contain an executable and at most 128 arguments",
        ));
    }
    if options.argv[0].trim().is_empty()
        || !PathBuf::from(&options.argv[0]).is_absolute()
        || options
            .argv
            .iter()
            .any(|argument| argument.contains('\0') || argument.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(invalid_input(
            "PTY executable must be absolute and argv must not contain NUL or oversized arguments",
        ));
    }
    validate_dimensions(options.rows, options.cols)?;
    if !options.cwd.is_absolute() || !options.cwd.is_dir() {
        return Err(invalid_input(
            "PTY working directory must be an existing absolute directory",
        ));
    }
    for (name, value) in &options.env {
        if !valid_env_name(name) || value.contains('\0') {
            return Err(invalid_input(
                "PTY environment contains an invalid name or value",
            ));
        }
    }
    Ok(())
}

fn validate_dimensions(rows: u16, cols: u16) -> io::Result<()> {
    if rows == 0 || cols == 0 || rows > MAX_DIMENSION || cols > MAX_DIMENSION {
        return Err(invalid_input("PTY dimensions must be between 1 and 1000"));
    }
    Ok(())
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn session_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "PTY session is closed")
}

fn backend_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(argv: &[&str]) -> PtyOptions {
        PtyOptions {
            argv: argv.iter().map(|value| (*value).to_owned()).collect(),
            cwd: std::env::current_dir().expect("current directory"),
            env: BTreeMap::new(),
            rows: 24,
            cols: 80,
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn argv_parser_preserves_argument_boundaries_and_fails_closed_on_controls() {
        assert_eq!(
            parse_nul_terminated_argv(
                b"/usr/bin/node\0node_modules/@letta-ai/letta-code/letta\0--backend\0local\0"
            ),
            Some(vec![
                "/usr/bin/node".to_owned(),
                "node_modules/@letta-ai/letta-code/letta".to_owned(),
                "--backend".to_owned(),
                "local".to_owned(),
            ])
        );
        assert_eq!(parse_nul_terminated_argv(b"node\0bad\x1bvalue\0"), None);
        assert_eq!(
            parse_nul_terminated_argv(&vec![b'x'; MAX_PROCESS_ARG_BYTES + 1]),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn pty_round_trip_resize_and_close() {
        let options = options(&["/usr/bin/printf", "rover-pty-ok"]);
        let mut session = PtySession::spawn(&options).expect("spawn PTY child");
        assert!(session.child_id().is_some());
        session.resize(35, 110).expect("resize PTY");
        let size = session
            .master
            .as_ref()
            .expect("open master")
            .get_size()
            .expect("read PTY size");
        assert_eq!((size.rows, size.cols), (35, 110));

        let mut output = [0; 64];
        let count = session.read_output(&mut output).expect("read child output");
        assert_eq!(&output[..count], b"rover-pty-ok");
        session.close().expect("close and reap child");
        assert!(session.write_input(b"after-close").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn closing_a_live_pty_stops_and_reaps_its_child() {
        let options = options(&["/bin/sleep", "30"]);
        let mut session = PtySession::spawn(&options).expect("spawn long-running PTY child");
        session.close().expect("close running PTY session");
        assert!(session.child_id().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn foreground_process_sampler_reads_the_owned_pty_job() {
        let options = options(&["/bin/sleep", "5"]);
        let mut session = PtySession::spawn(&options).expect("spawn PTY process");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let sample = loop {
            if let Some(name) = session.foreground_process_name() {
                break Some(name);
            }
            if std::time::Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        #[cfg(target_os = "linux")]
        assert_eq!(sample.as_deref(), Some("sleep"));
        #[cfg(target_os = "macos")]
        assert_eq!(sample.as_deref(), Some("sleep"));
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        assert!(sample.is_none());
        session.close().expect("reap sampled PTY process");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn foreground_process_group_sampler_includes_owned_group_members() {
        let options = options(&["/bin/sh", "-c", "sleep 5 & wait"]);
        let mut session = PtySession::spawn(&options).expect("spawn PTY process group");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let sample = loop {
            if let Some(names) = session.foreground_process_group_names() {
                if names.iter().any(|name| name == "sleep") {
                    break Some(names);
                }
            }
            if std::time::Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let names = sample.expect("foreground sampler includes the child process");
        assert!(names.contains(&"sleep".to_owned()));
        let details = session
            .foreground_process_group_details()
            .expect("bounded process details are sampled");
        #[cfg(target_os = "linux")]
        assert!(details.iter().any(|process| {
            process.name == "sleep"
                && process
                    .argv
                    .as_ref()
                    .is_some_and(|argv| argv.iter().any(|argument| argument == "5"))
        }));
        #[cfg(target_os = "macos")]
        assert!(details.iter().any(|process| process.name == "sleep"));
        session.close().expect("reap sampled PTY process group");
    }

    #[cfg(unix)]
    #[test]
    fn pty_writes_input_to_the_child() {
        let options = options(&[
            "/bin/sh",
            "-c",
            "IFS= read -r line; printf 'CHILD:%s' \"$line\"",
        ]);
        let mut session = PtySession::spawn(&options).expect("spawn PTY shell");
        session
            .write_input(b"rover-pty-input\n")
            .expect("write PTY input");
        let mut reader = session.reader.take().expect("PTY reader");
        let (sent, received) = std::sync::mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0; 128];
            loop {
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.extend_from_slice(&buffer[..count]);
                if output
                    .windows(b"CHILD:rover-pty-input".len())
                    .any(|window| window == b"CHILD:rover-pty-input")
                {
                    break;
                }
            }
            let _ = sent.send(output);
            Ok::<(), io::Error>(())
        });
        let output = received
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("child must echo input within two seconds");
        assert!(output
            .windows(b"CHILD:rover-pty-input".len())
            .any(|window| window == b"CHILD:rover-pty-input"));
        session.close().expect("close PTY child");
        reader_thread
            .join()
            .expect("reader thread")
            .expect("read PTY");
    }

    #[test]
    fn invalid_argv_environment_directory_and_dimensions_are_rejected() {
        let mut invalid = options(&["/bin/echo", "ok"]);
        invalid.rows = 0;
        assert_eq!(
            PtySession::spawn(&invalid)
                .err()
                .expect("invalid dimensions rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let mut invalid = options(&["/bin/echo", "ok"]);
        invalid.env.insert("BAD=NAME".into(), "value".into());
        assert_eq!(
            PtySession::spawn(&invalid)
                .err()
                .expect("invalid environment rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let mut invalid = options(&["/bin/echo", "ok"]);
        invalid.cwd = PathBuf::from("relative");
        assert_eq!(
            PtySession::spawn(&invalid)
                .err()
                .expect("invalid working directory rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let invalid = options(&["echo", "ok"]);
        assert_eq!(
            PtySession::spawn(&invalid)
                .err()
                .expect("relative executable rejected")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
