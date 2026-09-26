#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rover_execution::sessions::{SessionClient, SessionKey};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_session_daemon_accepts_named_client_and_exits_with_shell() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = PathBuf::from("/tmp").join(format!("rvcli-{unique}"));
    fs::create_dir(&root).expect("create scratch root");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private scratch root");
    let _scratch = Scratch(root.clone());
    let sessions = root.join("sessions");
    let cwd = root.join("work");
    let shell = root.join("controlled-shell");
    fs::create_dir(&cwd).expect("create work directory");
    fs::write(
        &shell,
        b"#!/bin/sh\nwhile IFS= read -r line; do\n  [ \"$line\" = exit ] && exit 0\ndone\n",
    )
    .expect("write controlled shell fixture");
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o700))
        .expect("make controlled shell executable");
    let key = SessionKey::new("integration_project", "shell").expect("valid session key");

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_rover"))
        .args([
            "__session_daemon",
            "--sessions",
            sessions.to_str().expect("UTF-8 scratch path"),
            "--namespace",
            key.namespace(),
            "--name",
            key.name(),
            "--cwd",
            cwd.to_str().expect("UTF-8 working path"),
            "--shell",
            shell.to_str().expect("UTF-8 shell path"),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &root)
        .env("SHELL", &shell)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start detached session owner");

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut client = loop {
        match SessionClient::connect_named(&sessions, &key) {
            Ok(client) => break client,
            Err(_) if Instant::now() < deadline => {
                if let Some(status) = daemon.try_wait().expect("check owner process") {
                    panic!("session owner exited before accepting a client: {status}");
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("session owner did not accept a client: {error}"),
        }
    };
    client
        .send_input(b"exit\n")
        .expect("send controlled shell exit");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = daemon.try_wait().expect("wait for session owner") {
            assert!(status.success(), "owner exits successfully after its shell");
            break;
        }
        if Instant::now() >= deadline {
            let _ = daemon.kill();
            panic!("controlled shell did not end the session owner");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
