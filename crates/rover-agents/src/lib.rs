//! Declarative agent profile registry.
//!
//! A descriptor reports the adapter surface Rover declares. It does not
//! certify that a provider is installed, authenticated, or live-compatible.

use serde::Serialize;
use std::io;

mod detect;
mod herdr_manifest;
mod lifecycle;
mod process_identity;
mod rollup;
mod sessions;
mod transcript;

pub use detect::{
    detect_agent, AgentState, Confidence, DetectionEvidence, DetectionRequest, DetectionResult,
    DetectionVisibility, ProcessSample, ScreenManifest, ScreenRule,
};
pub use herdr_manifest::{
    bundled_herdr_manifests, bundled_manifest_for_executable, evaluate_herdr_manifest,
    parse_herdr_manifest, HerdrManifest,
};
pub use lifecycle::{AuthorityDecision, LifecycleAuthority, LifecycleEvent, LifecycleResult};
pub use process_identity::{
    identify_foreground_process_group, ProcessArgvSample, ProcessGroupIdentity,
};
pub use rollup::{
    rollup_pane, rollup_tab, rollup_workspace, AgentObservation, DisplayState, Rollup,
};
pub use sessions::{
    clear_session_binding, list_session_bindings, load_session_binding, save_session_binding,
    AgentSessionBinding, AgentSessionSource, SessionBindingError,
};
pub use transcript::{parse_transcript, AgentResult};

/// A declared CLI/PTY adapter and its evidence boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Descriptor {
    /// Stable adapter profile name.
    pub name: &'static str,
    /// Expected executable, or `user-defined` for generic adapters.
    pub executable: &'static str,
    /// The adapter uses a structured output contract.
    pub structured: bool,
    /// The adapter requires interactive terminal input.
    pub interactive: bool,
    /// Whether Rover mediates provider permission prompts.
    pub permission_mediation: bool,
    /// Current validation evidence and limitations.
    pub validation: &'static str,
}

/// Options for constructing one safe, explicit agent process invocation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InvocationOptions {
    /// Optional native CLI executable override.
    pub executable: Option<String>,
    /// Optional provider model name.
    pub model: Option<String>,
    /// Whether the native CLI may make workspace edits.
    pub write: bool,
    /// Claude CLI tool allowlist.
    pub allowed_tools: Vec<String>,
    /// Optional Claude CLI turn limit; nonpositive means unspecified.
    pub max_turns: i32,
    /// Generic profile argv template.
    pub argv: Vec<String>,
    /// Generic profile repair argv template.
    pub repair_argv: Vec<String>,
}

const PROFILES: [Descriptor; 4] = [
    Descriptor {
        name: "generic-headless",
        executable: "user-defined",
        structured: false,
        interactive: false,
        permission_mediation: false,
        validation: "local command integration",
    },
    Descriptor {
        name: "generic-pty",
        executable: "user-defined",
        structured: false,
        interactive: true,
        permission_mediation: false,
        validation: "Linux PTY; local advisory only",
    },
    Descriptor {
        name: "codex-exec",
        executable: "codex",
        structured: true,
        interactive: false,
        permission_mediation: false,
        validation: "documented CLI; fixture conformance; live-account testing required",
    },
    Descriptor {
        name: "claude-print",
        executable: "claude",
        structured: true,
        interactive: false,
        permission_mediation: false,
        validation: "documented CLI; fixture conformance; live-account testing required",
    },
];

/// Return every declared adapter profile in stable order.
#[must_use]
pub const fn profiles() -> &'static [Descriptor] {
    &PROFILES
}

/// Resolve one declared profile, failing closed for unknown names.
///
/// # Errors
///
/// Returns `NotFound` when `name` is not a declared adapter profile.
pub fn profile(name: &str) -> io::Result<&'static Descriptor> {
    PROFILES
        .iter()
        .find(|descriptor| descriptor.name == name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown agent adapter"))
}

/// Build argv for one declared profile without invoking the executable.
///
/// The returned vector includes the executable as element zero. Generic
/// profiles use the caller-provided argv and replace exact `{{objective}}`
/// arguments. Native adapters emit only reviewed flags.
///
/// # Errors
///
/// Returns `NotFound` for unknown profiles and `InvalidInput` for missing
/// generic argv or malformed native executable, model, or tool values.
pub fn argv(
    profile_name: &str,
    options: &InvocationOptions,
    prompt: &str,
    repair: bool,
) -> io::Result<Vec<String>> {
    match profile_name {
        "generic-headless" | "generic-pty" => {
            let template = if repair {
                &options.repair_argv
            } else {
                &options.argv
            };
            if template.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "executable required",
                ));
            }
            Ok(template
                .iter()
                .map(|argument| {
                    if argument == "{{objective}}" {
                        prompt.to_owned()
                    } else {
                        argument.clone()
                    }
                })
                .collect())
        }
        "codex-exec" => {
            let executable = native_executable(options.executable.as_deref(), "codex")?;
            validate_model(options.model.as_deref())?;
            let sandbox = if options.write {
                "workspace-write"
            } else {
                "read-only"
            };
            let mut result = vec![executable, "exec".to_owned(), "--json".to_owned()];
            result.push("--sandbox".to_owned());
            result.push(sandbox.to_owned());
            if let Some(model) = nonempty(options.model.as_deref()) {
                result.push("--model".to_owned());
                result.push(model.to_owned());
            }
            result.push("--".to_owned());
            result.push(prompt.to_owned());
            Ok(result)
        }
        "claude-print" => {
            let executable = native_executable(options.executable.as_deref(), "claude")?;
            validate_model(options.model.as_deref())?;
            for tool in &options.allowed_tools {
                if !valid_tool(tool) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid allowed tool",
                    ));
                }
            }
            let mode = if options.write { "acceptEdits" } else { "plan" };
            let mut result = vec![
                executable,
                "--print".to_owned(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
                "--verbose".to_owned(),
                "--permission-mode".to_owned(),
                mode.to_owned(),
            ];
            if options.max_turns > 0 {
                result.push("--max-turns".to_owned());
                result.push(options.max_turns.to_string());
            }
            if let Some(model) = nonempty(options.model.as_deref()) {
                result.push("--model".to_owned());
                result.push(model.to_owned());
            }
            if !options.allowed_tools.is_empty() {
                result.push("--allowedTools".to_owned());
                result.push(options.allowed_tools.join(","));
            }
            result.push("--".to_owned());
            result.push(prompt.to_owned());
            Ok(result)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "unknown agent adapter",
        )),
    }
}

fn native_executable(value: Option<&str>, default: &str) -> io::Result<String> {
    let executable = value.filter(|value| !value.is_empty()).unwrap_or(default);
    if executable.len() > 1024
        || executable.starts_with('-')
        || executable == "."
        || executable == ".."
        || executable.contains('\0')
        || executable.split('/').any(|component| component == "..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid agent executable",
        ));
    }
    Ok(executable.to_owned())
}

fn validate_model(model: Option<&str>) -> io::Result<()> {
    if let Some(model) = nonempty(model) {
        if model.len() > 256
            || model.starts_with('-')
            || !model
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid agent model",
            ));
        }
    }
    Ok(())
}

fn valid_tool(tool: &str) -> bool {
    !tool.is_empty()
        && tool.len() <= 128
        && !tool.starts_with('-')
        && tool
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_go_compatible_profiles_and_evidence_labels() {
        assert_eq!(profiles().len(), 4);
        assert_eq!(
            profile("generic-headless").unwrap().executable,
            "user-defined"
        );
        assert!(profile("generic-pty").unwrap().interactive);
        assert_eq!(profile("codex-exec").unwrap().executable, "codex");
        assert_eq!(profile("claude-print").unwrap().executable, "claude");
        for descriptor in profiles() {
            assert!(!descriptor.permission_mediation);
        }
        assert!(profile("codex-exec")
            .unwrap()
            .validation
            .contains("live-account testing required"));
    }

    #[test]
    fn unknown_profile_is_not_downgraded_to_a_generic_adapter() {
        assert_eq!(
            profile("custom-unknown").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            profile("custom-unknown").unwrap_err().to_string(),
            "unknown agent adapter"
        );
    }

    #[test]
    fn generic_profiles_replace_only_exact_objective_arguments() {
        let options = InvocationOptions {
            argv: vec!["agent".to_owned(), "{{objective}}".to_owned()],
            repair_argv: vec!["agent".to_owned(), "repair".to_owned()],
            ..InvocationOptions::default()
        };
        assert_eq!(
            argv("generic-headless", &options, "fix issue", false).unwrap(),
            ["agent", "fix issue"]
        );
        assert_eq!(
            argv("generic-pty", &options, "unused", true).unwrap(),
            ["agent", "repair"]
        );
        assert!(argv(
            "generic-headless",
            &InvocationOptions::default(),
            "x",
            false
        )
        .is_err());
    }

    #[test]
    fn native_argv_matches_documented_modes_and_keeps_prompt_after_separator() {
        let codex = argv(
            "codex-exec",
            &InvocationOptions {
                model: Some("gpt-5.4".to_owned()),
                ..InvocationOptions::default()
            },
            "--dangerous prompt",
            false,
        )
        .unwrap();
        assert_eq!(
            codex,
            [
                "codex",
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "--model",
                "gpt-5.4",
                "--",
                "--dangerous prompt"
            ]
        );

        let claude = argv(
            "claude-print",
            &InvocationOptions {
                write: true,
                allowed_tools: vec!["Read".to_owned(), "Edit".to_owned()],
                max_turns: 4,
                ..InvocationOptions::default()
            },
            "update file",
            false,
        )
        .unwrap();
        assert!(claude
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "acceptEdits"]));
        assert!(claude
            .windows(2)
            .any(|pair| pair == ["--allowedTools", "Read,Edit"]));
        assert!(!claude.iter().any(|argument| argument.contains("bypass")));
        assert_eq!(claude.last().map(String::as_str), Some("update file"));
    }

    #[test]
    fn native_argv_rejects_option_injection_and_malformed_values() {
        for executable in ["-evil", "../evil", "dir/../evil", ".", "bad\0path"] {
            assert!(argv(
                "codex-exec",
                &InvocationOptions {
                    executable: Some(executable.to_owned()),
                    ..InvocationOptions::default()
                },
                "prompt",
                false
            )
            .is_err());
        }
        for model in ["-evil", "model with spaces", "m,evil"] {
            assert!(argv(
                "codex-exec",
                &InvocationOptions {
                    model: Some(model.to_owned()),
                    ..InvocationOptions::default()
                },
                "prompt",
                false
            )
            .is_err());
        }
        assert!(argv(
            "claude-print",
            &InvocationOptions {
                allowed_tools: vec!["--evil".to_owned()],
                ..InvocationOptions::default()
            },
            "prompt",
            false
        )
        .is_err());
    }
}
