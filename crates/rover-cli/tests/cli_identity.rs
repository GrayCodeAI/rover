//! The Rust preview binary must identify itself as a preview and must never
//! present itself as the Go `rover` product binary.

use std::process::Command;

fn preview() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rover-rs"))
}

#[test]
fn version_identifies_the_unreleased_rust_preview() {
    for flag in ["--version", "version"] {
        let output = preview().arg(flag).output().expect("run rover-rs");
        assert!(output.status.success(), "{flag} exited {:?}", output.status);
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 version output");
        assert!(
            stdout.starts_with(&format!("rover-rs {} ", env!("CARGO_PKG_VERSION"))),
            "{stdout}"
        );
        assert!(stdout.contains("Rust port preview"), "{stdout}");
        assert!(stdout.contains("not the Rover product binary"), "{stdout}");
    }
}

#[test]
fn usage_and_errors_name_the_preview_binary() {
    let output = preview().output().expect("run rover-rs");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 usage output");
    assert!(stderr.starts_with("usage: rover-rs tui "), "{stderr}");
    assert!(stderr.contains("rover-rs: unknown command"), "{stderr}");
    assert!(!stderr.contains("usage: rover tui"), "{stderr}");
}
