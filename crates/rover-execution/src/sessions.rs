//! Named local terminal sessions with reconnectable Unix-socket clients.
//!
//! The server process owns each PTY child. A client disconnect only releases
//! the writer slot; stopping the server terminates its children.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rover_core::{Id, Sha256Digest};
use rover_store::files::prepare_private_directory;
use serde::{Deserialize, Serialize};

use crate::pty::{PtyOptions, PtySession};

const HISTORY_LIMIT: usize = 32 * 1024;
const OUTPUT_CHUNK: usize = 16 * 1024;
const INPUT_LIMIT: usize = 16 * 1024;
const FRAME_LIMIT: usize = 64 * 1024;
const SOCKET_PATH_LIMIT: usize = 103;
const ACCEPT_POLL: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);

/// One validated namespace and session name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionKey {
    namespace: String,
    name: String,
}

impl SessionKey {
    /// Validate a namespace/name pair using Rover's identifier rules.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` if either identifier is invalid.
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> io::Result<Self> {
        let namespace = namespace.into();
        let name = name.into();
        Id::parse(namespace.clone()).map_err(|_| invalid_input("invalid session namespace"))?;
        Id::parse(name.clone()).map_err(|_| invalid_input("invalid session name"))?;
        Ok(Self { namespace, name })
    }

    /// Namespace that scopes this session name.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Name unique within its namespace.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Public, reconnectable description of one session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInfo {
    /// Validated namespace and name.
    pub key: SessionKey,
    /// Private local socket used by clients to attach.
    pub socket_path: PathBuf,
    /// Whether the PTY output stream is still open.
    pub running: bool,
}

/// JSON line exchanged by Rover session clients and the local server.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SessionFrame {
    /// `output`, `input`, `resize`, `sample_foreground`, `foreground_sample`,
    /// `detach`, `exit`, or `error`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Input or output bytes, encoded as JSON base64 for Go protocol compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "base64_bytes")]
    pub data: Vec<u8>,
    /// Validated executable basenames sampled from the owned PTY foreground
    /// process group. Empty when the platform cannot provide a stable sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<String>,
    /// Per-process bounded argv evidence from the same owned group sample.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub process_details: Vec<crate::pty::ForegroundProcessDetails>,
    /// Terminal rows for a resize request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u16>,
    /// Terminal columns for a resize request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u16>,
    /// Bounded server error text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

mod base64_bytes {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

impl SessionFrame {
    fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            data: Vec::new(),
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: None,
            cols: None,
            error: None,
        }
    }
}

/// In-process registry for a long-lived terminal server process.
///
/// Names are unique within namespaces. Client detach preserves PTYs and a
/// bounded output replay ring. Dropping/stopping the server closes every PTY
/// child. Session metadata currently lives for the server process lifetime;
/// crash/restart metadata restoration belongs to P-047.
pub struct SessionServer {
    sessions: Mutex<BTreeMap<SessionKey, Arc<SessionBroker>>>,
    socket_dir: PathBuf,
}

impl SessionServer {
    /// Create a registry whose socket files live in a private state directory.
    /// Relative paths are resolved against the server process's working
    /// directory and retained in that form to support Unix socket path limits.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the path is empty or cannot be safely prepared
    /// as a private directory.
    pub fn new(socket_dir: impl AsRef<Path>) -> io::Result<Self> {
        let path = socket_dir.as_ref();
        if path.as_os_str().is_empty() {
            return Err(invalid_input("session socket directory must not be empty"));
        }
        prepare_private_directory(path)?;
        Ok(Self {
            sessions: Mutex::new(BTreeMap::new()),
            socket_dir: path.to_owned(),
        })
    }

    /// Spawn a named PTY session owned by this server process.
    ///
    /// Existing names are not replaced, and stale socket files are not
    /// removed automatically. Callers must inspect and explicitly resolve
    /// stale state before retrying.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the name already exists, the socket path is too
    /// long or occupied, PTY creation fails, or a worker thread cannot start.
    pub fn spawn(&self, key: SessionKey, options: &PtyOptions) -> io::Result<SessionInfo> {
        let mut sessions = self.sessions.lock().map_err(|_| poisoned())?;
        if sessions.contains_key(&key) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "session name already exists in this namespace",
            ));
        }
        let socket_path = socket_path_for(&self.socket_dir, &key);
        if socket_path.as_os_str().as_encoded_bytes().len() > SOCKET_PATH_LIMIT {
            return Err(invalid_input("session socket path exceeds Unix limit"));
        }
        let broker = Arc::new(SessionBroker::start(
            key.clone(),
            socket_path.clone(),
            options,
        )?);
        let info = broker.info();
        sessions.insert(key, broker);
        Ok(info)
    }

    /// List sessions in deterministic namespace/name order.
    ///
    /// # Errors
    ///
    /// Returns an error if the session registry lock is poisoned.
    pub fn list(&self) -> io::Result<Vec<SessionInfo>> {
        let sessions = self.sessions.lock().map_err(|_| poisoned())?;
        Ok(sessions.values().map(|session| session.info()).collect())
    }

    /// Stop one session and terminate/reap its PTY child.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown session or an I/O error during stop.
    pub fn stop(&self, key: &SessionKey) -> io::Result<()> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| poisoned())?
            .remove(key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        session.shutdown()
    }

    /// Connect a client to a session by its validated name.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown session, or an I/O error if the local
    /// socket cannot be opened.
    pub fn connect(&self, key: &SessionKey) -> io::Result<SessionClient> {
        let path = self
            .sessions
            .lock()
            .map_err(|_| poisoned())?
            .get(key)
            .map(|session| session.socket_path.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        SessionClient::connect(path)
    }
}

impl Drop for SessionServer {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.get_mut() {
            for (_, session) in std::mem::take(sessions) {
                let _ = session.shutdown();
            }
        }
    }
}

/// One connected local session client. Dropping it detaches without stopping
/// the server-owned PTY process.
pub struct SessionClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

/// Read half of a connected session client, movable to a blocking reader
/// thread while the writer half stays with the interactive event loop.
pub struct SessionReader {
    reader: BufReader<UnixStream>,
}

impl SessionReader {
    /// Read the next output/error/exit frame.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for a closed socket, oversized frame, or invalid
    /// JSON payload.
    pub fn read_frame(&mut self) -> io::Result<SessionFrame> {
        read_frame(&mut self.reader)
    }
}

/// Input and resize half of a connected session client.
pub struct SessionWriter {
    writer: UnixStream,
}

impl SessionWriter {
    /// Ask the server to sample the executable owning the PTY foreground group.
    /// The reply is a `foreground_sample` frame with a UTF-8 basename, or an
    /// empty payload when the platform cannot establish one.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the request cannot be sent.
    pub fn request_foreground_sample(&mut self) -> io::Result<()> {
        self.send(&SessionFrame::new("sample_foreground"))
    }

    /// Send bytes to the PTY child.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` above 16 KiB, or an I/O error if the socket
    /// write fails.
    pub fn send_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        validate_input(bytes)?;
        self.send(&SessionFrame {
            kind: "input".to_owned(),
            data: bytes.to_vec(),
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: None,
            cols: None,
            error: None,
        })
    }

    /// Resize the session PTY.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for unsupported dimensions or an I/O error if
    /// the socket write fails.
    pub fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        if rows == 0 || cols == 0 || rows > 1000 || cols > 1000 {
            return Err(invalid_input(
                "terminal dimensions must be between 1 and 1000",
            ));
        }
        self.send(&SessionFrame {
            kind: "resize".to_owned(),
            data: Vec::new(),
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: Some(rows),
            cols: Some(cols),
            error: None,
        })
    }

    /// Release the writer slot while leaving the PTY process alive.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the detach frame cannot be sent.
    pub fn detach(&mut self) -> io::Result<()> {
        self.send(&SessionFrame::new("detach"))?;
        self.writer.shutdown(std::net::Shutdown::Both)
    }

    fn send(&mut self, frame: &SessionFrame) -> io::Result<()> {
        write_frame(&mut self.writer, frame)
    }
}

impl SessionClient {
    /// Connect and claim the session's single-writer slot.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the socket cannot be opened or the server
    /// rejects the connection.
    pub fn connect(socket_path: impl AsRef<Path>) -> io::Result<Self> {
        let stream = UnixStream::connect(socket_path)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            reader,
            writer: stream,
        })
    }

    /// Connect by namespace and session name without an in-memory server
    /// handle. The session owner must use the same private socket directory.
    ///
    /// Socket basenames are derived from the validated key, not from user
    /// names. A stale socket is never removed or taken over automatically.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the session socket is absent, stale, or refuses
    /// a connection.
    pub fn connect_named(socket_dir: impl AsRef<Path>, key: &SessionKey) -> io::Result<Self> {
        Self::connect(socket_path_for(socket_dir.as_ref(), key))
    }

    /// Split this single connection into a reader and writer for separate
    /// threads without claiming a second client slot.
    #[must_use]
    pub fn into_parts(self) -> (SessionReader, SessionWriter) {
        (
            SessionReader {
                reader: self.reader,
            },
            SessionWriter {
                writer: self.writer,
            },
        )
    }

    /// Read the next output/error/exit frame.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for a closed socket, oversized frame, or invalid
    /// JSON payload.
    pub fn read_frame(&mut self) -> io::Result<SessionFrame> {
        read_frame(&mut self.reader)
    }

    /// Send bytes to the PTY child.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` above 16 KiB, or an I/O error if the socket
    /// write fails.
    pub fn send_input(&mut self, bytes: &[u8]) -> io::Result<()> {
        validate_input(bytes)?;
        self.send(&SessionFrame {
            kind: "input".to_owned(),
            data: bytes.to_vec(),
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: None,
            cols: None,
            error: None,
        })
    }

    /// Request a best-effort foreground executable sample from the owned PTY.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the request cannot be sent.
    pub fn request_foreground_sample(&mut self) -> io::Result<()> {
        self.send(&SessionFrame::new("sample_foreground"))
    }

    /// Resize the session PTY.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for unsupported dimensions or an I/O error if
    /// the socket write fails.
    pub fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        if rows == 0 || cols == 0 || rows > 1000 || cols > 1000 {
            return Err(invalid_input(
                "terminal dimensions must be between 1 and 1000",
            ));
        }
        self.send(&SessionFrame {
            kind: "resize".to_owned(),
            data: Vec::new(),
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: Some(rows),
            cols: Some(cols),
            error: None,
        })
    }

    /// Release the writer slot while leaving the PTY process alive.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the detach frame cannot be sent.
    pub fn detach(&mut self) -> io::Result<()> {
        self.send(&SessionFrame::new("detach"))?;
        self.writer.shutdown(std::net::Shutdown::Both)
    }

    fn send(&mut self, frame: &SessionFrame) -> io::Result<()> {
        write_frame(&mut self.writer, frame)
    }
}

fn socket_path_for(socket_dir: &Path, key: &SessionKey) -> PathBuf {
    let mut identity = Vec::with_capacity(key.namespace.len() + key.name.len() + 1);
    identity.extend_from_slice(key.namespace.as_bytes());
    identity.push(0);
    identity.extend_from_slice(key.name.as_bytes());
    let digest = Sha256Digest::of(&identity).to_hex();
    socket_dir.join(format!("{}.sock", &digest[..32]))
}

struct SessionBroker {
    key: SessionKey,
    socket_path: PathBuf,
    core: Arc<BrokerCore>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
    output_thread: Mutex<Option<JoinHandle<()>>>,
}

struct BrokerCore {
    pty: Mutex<PtySession>,
    state: Mutex<BrokerState>,
    stopping: AtomicBool,
    running: AtomicBool,
    next_client: AtomicU64,
}

struct BrokerState {
    history: VecDeque<u8>,
    active_client: Option<u64>,
    output_stream: Option<UnixStream>,
}

impl SessionBroker {
    fn start(key: SessionKey, socket_path: PathBuf, options: &PtyOptions) -> io::Result<Self> {
        if socket_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "session socket exists; takeover is not automatic",
            ));
        }
        let listener = UnixListener::bind(&socket_path)?;
        if let Err(error) = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        if let Err(error) = listener.set_nonblocking(true) {
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        let mut pty = match PtySession::spawn(options) {
            Ok(pty) => pty,
            Err(error) => {
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        let reader = match pty.take_output_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        let core = Arc::new(BrokerCore {
            pty: Mutex::new(pty),
            state: Mutex::new(BrokerState {
                history: VecDeque::with_capacity(HISTORY_LIMIT),
                active_client: None,
                output_stream: None,
            }),
            stopping: AtomicBool::new(false),
            running: AtomicBool::new(true),
            next_client: AtomicU64::new(1),
        });
        let output_core = Arc::clone(&core);
        let output_thread = match thread::Builder::new()
            .name(format!("rover-session-output-{}", key.name))
            .spawn(move || output_loop(&output_core, reader))
        {
            Ok(thread) => thread,
            Err(error) => {
                let _ = core.pty.lock().map(|mut pty| pty.close());
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        let accept_core = Arc::clone(&core);
        let accept_thread = match thread::Builder::new()
            .name(format!("rover-session-accept-{}", key.name))
            .spawn(move || accept_loop(&accept_core, &listener))
        {
            Ok(thread) => thread,
            Err(error) => {
                core.stopping.store(true, Ordering::Release);
                let _ = core.pty.lock().map(|mut pty| pty.close());
                let _ = output_thread.join();
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        Ok(Self {
            key,
            socket_path,
            core,
            accept_thread: Mutex::new(Some(accept_thread)),
            output_thread: Mutex::new(Some(output_thread)),
        })
    }

    fn info(&self) -> SessionInfo {
        SessionInfo {
            key: self.key.clone(),
            socket_path: self.socket_path.clone(),
            running: self.core.running.load(Ordering::Acquire),
        }
    }

    fn shutdown(&self) -> io::Result<()> {
        if self.core.stopping.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Some(mut stream) = self
            .core
            .state
            .lock()
            .map_err(|_| poisoned())?
            .output_stream
            .take()
        {
            let _ = write_frame(&mut stream, &SessionFrame::new("exit"));
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        let close_result = self.core.pty.lock().map_err(|_| poisoned())?.close();
        join_thread(&self.accept_thread)?;
        join_thread(&self.output_thread)?;
        if let Err(error) = fs::remove_file(&self.socket_path) {
            if error.kind() != io::ErrorKind::NotFound && close_result.is_ok() {
                return Err(error);
            }
        }
        close_result
    }
}

impl Drop for SessionBroker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn accept_loop(core: &Arc<BrokerCore>, listener: &UnixListener) {
    while !core.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let client_core = Arc::clone(core);
                let _ = thread::Builder::new()
                    .name("rover-session-client".to_owned())
                    .spawn(move || serve_client(&client_core, stream));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
}

fn serve_client(core: &Arc<BrokerCore>, mut stream: UnixStream) {
    let client_id = core.next_client.fetch_add(1, Ordering::Relaxed);
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let Ok(mut input) = stream.try_clone().map(BufReader::new) else {
        return;
    };
    let Ok(mut state) = core.state.lock() else {
        return;
    };
    if state.active_client.is_some() {
        let mut error = SessionFrame::new("error");
        error.error = Some("session already has an attached writer".to_owned());
        let _ = write_frame(&mut stream, &error);
        return;
    }
    let history = state.history.iter().copied().collect::<Vec<_>>();
    let replay = SessionFrame {
        kind: "output".to_owned(),
        data: history,
        processes: Vec::new(),
        process_details: Vec::new(),
        rows: None,
        cols: None,
        error: None,
    };
    if write_frame(&mut stream, &replay).is_err() {
        return;
    }
    state.active_client = Some(client_id);
    state.output_stream = Some(writer);
    drop(state);

    loop {
        if core.stopping.load(Ordering::Acquire) {
            break;
        }
        match read_frame(&mut input) {
            Ok(frame) => match frame.kind.as_str() {
                "input" if frame.data.len() <= INPUT_LIMIT => {
                    if !core
                        .pty
                        .lock()
                        .is_ok_and(|mut pty| pty.write_input(&frame.data).is_ok())
                    {
                        break;
                    }
                }
                "resize" => match (frame.rows, frame.cols) {
                    (Some(rows), Some(cols))
                        if (1..=1000).contains(&rows) && (1..=1000).contains(&cols) =>
                    {
                        if !core
                            .pty
                            .lock()
                            .is_ok_and(|mut pty| pty.resize(rows, cols).is_ok())
                        {
                            break;
                        }
                    }
                    _ => break,
                },
                "sample_foreground" if frame.data.is_empty() => {
                    if send_foreground_sample(core, client_id).is_err() {
                        break;
                    }
                }
                _ => break,
            },
            Err(_) => break,
        }
    }
    if let Ok(mut state) = core.state.lock() {
        if state.active_client == Some(client_id) {
            state.active_client = None;
            state.output_stream = None;
        }
    }
}

fn send_foreground_sample(core: &Arc<BrokerCore>, client_id: u64) -> io::Result<()> {
    let (sample, process_details) = {
        let pty = core.pty.lock().map_err(|_| poisoned())?;
        (
            pty.foreground_process_name().unwrap_or_default(),
            pty.foreground_process_group_details().unwrap_or_default(),
        )
    };
    let mut processes = process_details
        .iter()
        .map(|process| process.name.clone())
        .collect::<Vec<_>>();
    processes.sort();
    processes.dedup();
    let mut state = core.state.lock().map_err(|_| poisoned())?;
    if state.active_client != Some(client_id) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session client no longer owns the writer slot",
        ));
    }
    let output = state
        .output_stream
        .as_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "session output is closed"))?;
    write_frame(
        output,
        &SessionFrame {
            kind: "foreground_sample".to_owned(),
            data: sample.into_bytes(),
            processes,
            process_details,
            rows: None,
            cols: None,
            error: None,
        },
    )
}

fn output_loop(core: &Arc<BrokerCore>, mut reader: Box<dyn Read + Send>) {
    let mut buffer = [0; OUTPUT_CHUNK];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let Ok(mut state) = core.state.lock() else {
                    break;
                };
                retain_history(&mut state.history, &buffer[..count]);
                if let Some(stream) = state.output_stream.as_mut() {
                    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                    let frame = SessionFrame {
                        kind: "output".to_owned(),
                        data: buffer[..count].to_vec(),
                        processes: Vec::new(),
                        process_details: Vec::new(),
                        rows: None,
                        cols: None,
                        error: None,
                    };
                    if write_frame(stream, &frame).is_err() {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        state.output_stream = None;
                        state.active_client = None;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if is_retryable_read_error(&error) => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
    core.running.store(false, Ordering::Release);
    if let Ok(mut state) = core.state.lock() {
        if let Some(stream) = state.output_stream.as_mut() {
            let _ = write_frame(stream, &SessionFrame::new("exit"));
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        state.output_stream = None;
        state.active_client = None;
    }
    if let Ok(mut pty) = core.pty.lock() {
        let _ = pty.close();
    }
}

fn retain_history(history: &mut VecDeque<u8>, bytes: &[u8]) {
    history.extend(bytes);
    while history.len() > HISTORY_LIMIT {
        history.pop_front();
    }
}

fn validate_input(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > INPUT_LIMIT {
        return Err(invalid_input("session input exceeds 16 KiB"));
    }
    Ok(())
}

fn is_retryable_read_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || error
            .raw_os_error()
            .is_some_and(|code| code == rustix::io::Errno::AGAIN.raw_os_error())
}

fn read_frame(reader: &mut impl BufRead) -> io::Result<SessionFrame> {
    let mut line = Vec::with_capacity(256);
    let count = reader
        .take((FRAME_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    if count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "session disconnected",
        ));
    }
    if count > FRAME_LIMIT {
        return Err(invalid_input("session frame exceeds 64 KiB"));
    }
    serde_json::from_slice(&line).map_err(|error| invalid_data(error.to_string()))
}

fn write_frame(writer: &mut impl Write, frame: &SessionFrame) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, frame).map_err(|error| invalid_data(error.to_string()))?;
    writer.write_all(b"\n")
}

fn join_thread(handle: &Mutex<Option<JoinHandle<()>>>) -> io::Result<()> {
    let worker = handle.lock().map_err(|_| poisoned())?.take();
    if worker.is_some_and(|worker| worker.join().is_err()) {
        return Err(io::Error::other("session worker thread panicked"));
    }
    Ok(())
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn poisoned() -> io::Error {
    io::Error::other("session server lock is poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn fixture_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        PathBuf::from(format!(".rover-session-test-{nonce}"))
    }

    fn pty_options(command: &str) -> PtyOptions {
        pty_argv(&["/bin/sh", "-c", command])
    }

    fn pty_argv(argv: &[&str]) -> PtyOptions {
        let mut env = BTreeMap::new();
        env.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
        PtyOptions {
            argv: argv.iter().map(|argument| (*argument).to_owned()).collect(),
            cwd: std::env::current_dir().expect("cwd"),
            env,
            rows: 24,
            cols: 80,
        }
    }

    #[test]
    fn frame_bytes_match_go_json_base64_encoding() {
        let frame = SessionFrame {
            kind: "output".to_owned(),
            data: vec![0, 1, 2],
            processes: Vec::new(),
            process_details: Vec::new(),
            rows: None,
            cols: None,
            error: None,
        };
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"type":"output","data":"AAEC"}"#
        );
        assert_eq!(
            serde_json::from_str::<SessionFrame>(r#"{"type":"input","data":"AAEC"}"#)
                .unwrap()
                .data,
            vec![0, 1, 2]
        );
    }

    #[test]
    fn foreground_process_group_field_is_additive_and_round_trips() {
        let frame = SessionFrame {
            kind: "foreground_sample".to_owned(),
            data: b"sh".to_vec(),
            processes: vec!["amp".to_owned(), "sh".to_owned()],
            process_details: vec![crate::pty::ForegroundProcessDetails {
                name: "node".to_owned(),
                argv: Some(vec![
                    "/usr/bin/node".to_owned(),
                    "node_modules/@letta-ai/letta-code/letta".to_owned(),
                ]),
            }],
            rows: None,
            cols: None,
            error: None,
        };
        let encoded = serde_json::to_string(&frame).unwrap();
        assert!(encoded.contains(r#""processes":["amp","sh"]"#));
        assert!(encoded.contains(r#""process_details":[{"name":"node","argv":["/usr/bin/node","node_modules/@letta-ai/letta-code/letta"]}]"#));
        let decoded: SessionFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, frame);
        let legacy: SessionFrame =
            serde_json::from_str(r#"{"type":"output","data":"AAEC"}"#).unwrap();
        assert!(legacy.processes.is_empty());
    }

    #[test]
    fn replay_history_is_capped_and_preserves_the_newest_bytes() {
        let mut history = VecDeque::new();
        retain_history(&mut history, &vec![b'a'; HISTORY_LIMIT]);
        retain_history(&mut history, b"tail");
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(
            history
                .iter()
                .skip(HISTORY_LIMIT - 4)
                .copied()
                .collect::<Vec<_>>(),
            b"tail"
        );
    }

    #[test]
    fn input_frame_limit_is_enforced_before_writing() {
        assert_eq!(
            validate_input(&vec![b'x'; INPUT_LIMIT + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    fn wait_for_output(client: &mut SessionClient, needle: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            match client.read_frame() {
                Ok(frame) if frame.kind == "output" => {
                    output.extend(frame.data);
                    if output.windows(needle.len()).any(|window| window == needle) {
                        return output;
                    }
                }
                Ok(frame) if frame.kind == "error" => panic!("server error: {:?}", frame.error),
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => panic!("session read failed while waiting for {needle:?}: {error}"),
            }
        }
        panic!("timed out waiting for session output: {output:?}");
    }

    #[test]
    fn named_sessions_are_namespace_scoped_and_duplicate_names_fail() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key_a = SessionKey::new("project-a", "shell").unwrap();
        let key_b = SessionKey::new("project-b", "shell").unwrap();
        let info_a = server
            .spawn(key_a.clone(), &pty_options("sleep 30"))
            .unwrap();
        let info_b = server
            .spawn(key_b.clone(), &pty_options("sleep 30"))
            .unwrap();
        assert_eq!(server.list().unwrap().len(), 2);
        assert_eq!(
            server
                .spawn(key_a.clone(), &pty_options("true"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        server.stop(&key_a).unwrap();
        assert_eq!(server.list().unwrap().len(), 1);
        drop(server);
        assert!(!info_a.socket_path.exists());
        assert!(!info_b.socket_path.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn detach_preserves_process_and_reconnect_replays_bounded_output() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key = SessionKey::new("project", "shell").unwrap();
        let info = server
            .spawn(key.clone(), &pty_argv(&["/bin/sleep", "30"]))
            .expect("spawn terminal");
        assert_eq!(
            fs::metadata(&info.socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let mut first = server.connect(&key).unwrap();
        first.send_input(b"READY\n").unwrap();
        let first_output = wait_for_output(&mut first, b"READY");
        assert!(first_output.windows(5).any(|window| window == b"READY"));
        assert!(server.list().unwrap()[0].running);
        first.detach().unwrap();
        drop(first);
        std::thread::sleep(Duration::from_millis(40));
        assert!(server.list().unwrap()[0].running);

        let mut second = server.connect(&key).unwrap();
        let replay = second.read_frame().unwrap();
        assert_eq!(replay.kind, "output");
        assert!(replay.data.windows(5).any(|window| window == b"READY"));
        second.send_input(b"again\n").unwrap();
        let second_output = wait_for_output(&mut second, b"again");
        assert!(second_output.windows(5).any(|window| window == b"again"));
        assert!(server.list().unwrap()[0].running);
        server.stop(&key).unwrap();
        assert!(!info.socket_path.exists());
        drop(server);
        let _ = fs::remove_dir(directory);
    }

    #[test]
    fn attached_client_receives_only_owned_pty_foreground_samples() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key = SessionKey::new("project", "sample").unwrap();
        server
            .spawn(key.clone(), &pty_argv(&["/bin/sleep", "30"]))
            .expect("spawn PTY child");
        let mut client = server.connect(&key).expect("attach session client");
        assert_eq!(client.read_frame().unwrap().kind, "output");
        client.request_foreground_sample().unwrap();
        let sample = client.read_frame().expect("foreground sample reply");
        assert_eq!(sample.kind, "foreground_sample");
        assert!(sample.error.is_none());
        #[cfg(target_os = "linux")]
        assert!(!sample.processes.is_empty());
        #[cfg(target_os = "linux")]
        assert!(sample.process_details.iter().any(|process| {
            process.name == "sleep"
                && process
                    .argv
                    .as_ref()
                    .is_some_and(|argv| argv.iter().any(|argument| argument == "30"))
        }));
        #[cfg(target_os = "macos")]
        assert!(sample
            .process_details
            .iter()
            .any(|process| process.name == "sleep"));
        let name = std::str::from_utf8(&sample.data).expect("UTF-8 process basename");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_eq!(name, "sleep");
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        assert!(name.is_empty());
        server.stop(&key).unwrap();
        drop(client);
        drop(server);
        let _ = fs::remove_dir(directory);
    }

    #[test]
    fn split_reader_writer_halves_keep_one_session_connection_live() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key = SessionKey::new("project", "shell").unwrap();
        server
            .spawn(
                key.clone(),
                &pty_options("IFS= read -r line; printf '<%s>' \"$line\"; sleep 1"),
            )
            .expect("spawn terminal");
        let client = server.connect(&key).unwrap();
        let (mut reader, mut writer) = client.into_parts();
        writer.send_input(b"TUI-bridge\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let frame = reader.read_frame().expect("read session output");
            if frame.kind == "output" {
                output.extend(frame.data);
                if output.windows(12).any(|window| window == b"<TUI-bridge>") {
                    break;
                }
            }
        }
        assert!(output.windows(12).any(|window| window == b"<TUI-bridge>"));
        writer.detach().unwrap();
        drop(server);
        let _ = fs::remove_dir(directory);
    }

    #[test]
    fn client_connects_by_namespace_and_name_and_replays_after_detach() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key = SessionKey::new("project", "shell").unwrap();
        let info = server
            .spawn(
                key.clone(),
                &pty_options("IFS= read -r line; printf '<%s>' \"$line\"; sleep 1"),
            )
            .expect("spawn named session");

        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sessions::tests::named_client_subprocess_fixture",
                "--nocapture",
            ])
            .env("ROVER_SESSION_TEST_SOCKET_DIR", &directory)
            .env("ROVER_SESSION_TEST_NAMESPACE", key.namespace())
            .env("ROVER_SESSION_TEST_NAME", key.name())
            .status()
            .expect("run a separate process as the named client");
        assert!(child.success(), "named client subprocess failed: {child}");

        let mut second = SessionClient::connect_named(&directory, &key).unwrap();
        let replay = second.read_frame().unwrap();
        assert_eq!(replay.kind, "output");
        assert!(replay
            .data
            .windows(b"<from-separate-process>".len())
            .any(|window| window == b"<from-separate-process>"));
        second.detach().unwrap();

        server.stop(&key).unwrap();
        assert!(!info.socket_path.exists());
        match SessionClient::connect_named(&directory, &key) {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
            Ok(_) => panic!("stopped session socket must no longer be connectable"),
        }
        drop(server);
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn named_client_subprocess_fixture() {
        let (Ok(socket_dir), Ok(namespace), Ok(name)) = (
            std::env::var("ROVER_SESSION_TEST_SOCKET_DIR"),
            std::env::var("ROVER_SESSION_TEST_NAMESPACE"),
            std::env::var("ROVER_SESSION_TEST_NAME"),
        ) else {
            return;
        };
        let key = SessionKey::new(namespace, name).unwrap();
        let mut client = SessionClient::connect_named(socket_dir, &key).unwrap();
        client.send_input(b"from-separate-process\n").unwrap();
        let response = wait_for_output(&mut client, b"<from-separate-process>");
        assert!(response
            .windows(b"<from-separate-process>".len())
            .any(|window| window == b"<from-separate-process>"));
        client.detach().unwrap();
    }

    #[test]
    fn only_one_client_can_hold_the_writer_slot() {
        let directory = fixture_dir();
        let server = SessionServer::new(&directory).expect("private session directory");
        let key = SessionKey::new("project", "shell").unwrap();
        let info = server
            .spawn(key.clone(), &pty_options("sleep 30"))
            .expect("spawn terminal");
        let first = server.connect(&key).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let mut second = SessionClient::connect(&info.socket_path).unwrap();
        let response = second.read_frame().unwrap();
        let response = if response.kind == "output" {
            second.read_frame().unwrap()
        } else {
            response
        };
        assert_eq!(response.kind, "error");
        assert_eq!(
            response.error.as_deref(),
            Some("session already has an attached writer")
        );
        drop(first);
        drop(server);
        assert!(!info.socket_path.exists());
        let _ = fs::remove_dir(directory);
    }
}
