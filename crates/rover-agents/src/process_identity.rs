//! Bounded foreground process-group identity resolution.
//!
//! Wrapper rules were traced against the pinned Herdr `src/detect/mod.rs` in
//! `docs/design/UPSTREAM_SOURCE_AUDIT.md`. Only argv observed from Rover's
//! owned PTY sampler is accepted; caller-supplied command text is not used.

use crate::bundled_manifest_for_executable;

/// Process executable and bounded argv evidence from an owned PTY sampler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessArgvSample {
    /// Executable basename obtained from the platform process record.
    pub executable: String,
    /// Argument vector, when the platform could read it safely.
    pub argv: Option<Vec<String>>,
}

/// Result of resolving the group identity by Herdr-compatible priority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGroupIdentity {
    /// Canonical executable label for the selected agent.
    pub executable: String,
    /// True when equally ranked processes resolve to different agents.
    pub ambiguous: bool,
    /// Every equally ranked candidate when `ambiguous`, starting with
    /// `executable`. Empty otherwise. Lets a caller report *which* agents
    /// disagreed instead of only that they did.
    pub conflicting: Vec<String>,
}

/// Resolve direct agent executables and audited runtime/shell wrappers.
///
/// A recognized leader wins. Otherwise wrapper candidates outrank direct
/// executable names; equal-ranked disagreement is marked ambiguous so callers
/// can preserve an unknown identity instead of selecting arbitrarily.
#[must_use]
pub fn identify_foreground_process_group(
    leader: Option<&ProcessArgvSample>,
    processes: &[ProcessArgvSample],
) -> Option<ProcessGroupIdentity> {
    if let Some(leader) = leader {
        if let Some(candidate) = normalized_process_name(leader) {
            if candidate != "letta" || letta_is_interactive(leader) {
                return Some(ProcessGroupIdentity {
                    executable: candidate,
                    ambiguous: false,
                    conflicting: Vec::new(),
                });
            }
        }
    }

    let mut best: Option<(u8, String, Vec<String>)> = None;
    for process in processes {
        let Some(candidate) = normalized_process_name(process) else {
            continue;
        };
        if candidate == "letta" && !letta_is_interactive(process) {
            continue;
        }
        let score = process_priority(process, &candidate);
        match &mut best {
            Some((best_score, _, _)) if *best_score > score => {}
            Some((best_score, best_name, tied)) if *best_score == score => {
                if best_name != &candidate && !tied.iter().any(|held| held == &candidate) {
                    tied.push(candidate);
                }
            }
            _ => best = Some((score, candidate, Vec::new())),
        }
    }
    let (_, executable, tied) = best?;
    let ambiguous = !tied.is_empty();
    let conflicting = if ambiguous {
        let mut all = Vec::with_capacity(tied.len() + 1);
        all.push(executable.clone());
        all.extend(tied);
        all
    } else {
        Vec::new()
    };
    Some(ProcessGroupIdentity {
        executable,
        ambiguous,
        conflicting,
    })
}

fn normalized_process_name(process: &ProcessArgvSample) -> Option<String> {
    let effective = process
        .argv
        .as_ref()
        .and_then(|argv| argv.first())
        .map_or(process.executable.as_str(), String::as_str);
    let effective_name = lookup_name(effective);

    if is_generic_runtime_or_shell(&effective_name) {
        if let Some(candidate) =
            wrapped_agent_name_from_runtime_argv(&effective_name, process.argv.as_deref())
        {
            return Some(candidate);
        }
    }
    if let Some(candidate) = agent_name_from_path_token(effective) {
        return Some(candidate);
    }

    if let Some(argv) = process.argv.as_deref() {
        if let Some(runtime) = argv.first() {
            let runtime_name = lookup_name(runtime);
            if matches!(runtime_name.as_str(), "node" | "bun") {
                if let Some(candidate) = wrapped_agent_name_from_runtime_argv(runtime, Some(argv)) {
                    if matches!(candidate.as_str(), "qwen" | "cline" | "letta") {
                        return Some(candidate);
                    }
                }
            }
        }
        if let Some(candidate) = argv.first().and_then(|arg| agent_name_from_path_token(arg)) {
            return Some(candidate);
        }
    }
    None
}

fn wrapped_agent_name_from_runtime_argv(runtime: &str, argv: Option<&[String]>) -> Option<String> {
    let argv = argv?;
    match lookup_name(runtime).as_str() {
        "node" => cursor_agent_from_node_argv(argv)
            .or_else(|| script_agent_from_argv(argv, &["-e", "--eval", "-p", "--print"], &[])),
        "bun" => script_agent_from_argv(argv, &["-e", "--eval", "-p", "--print"], &[]),
        name if is_python_runtime(name) => script_agent_from_argv(argv, &["-c"], &["-m"]),
        "sh" | "bash" | "zsh" | "fish" => shell_command_agent_from_argv(argv),
        "cmd" => command_agent_from_argv(argv),
        "powershell" | "pwsh" => powershell_agent_from_argv(argv),
        _ => None,
    }
}

fn cursor_agent_from_node_argv(argv: &[String]) -> Option<String> {
    let runtime = argv.first()?;
    let script = argv.get(1)?;
    let runtime_parent = parent_and_basename(runtime)?;
    let script_parent = parent_and_basename(script)?;
    if !runtime_parent.1.eq_ignore_ascii_case("node.exe")
        || !script_parent.1.eq_ignore_ascii_case("index.js")
        || !runtime_parent.0.eq_ignore_ascii_case(script_parent.0)
    {
        return None;
    }
    let parts = path_components(runtime_parent.0);
    (parts.len() >= 3
        && parts[parts.len() - 3].eq_ignore_ascii_case("cursor-agent")
        && parts[parts.len() - 2].eq_ignore_ascii_case("versions")
        && parts[parts.len() - 1].chars().any(|ch| ch.is_ascii_digit()))
    .then(|| "cursor-agent".to_owned())
}

fn parent_and_basename(path: &str) -> Option<(&str, &str)> {
    let split = path.rfind(['/', '\\'])?;
    let parent = path[..split].trim_end_matches(['/', '\\']);
    let basename = &path[split + 1..];
    (!parent.is_empty() && !basename.is_empty()).then_some((parent, basename))
}

fn script_agent_from_argv(
    argv: &[String],
    eval_flags: &[&str],
    module_flags: &[&str],
) -> Option<String> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == "--" {
            return rest
                .next()
                .and_then(|token| agent_name_from_path_token(token));
        }
        if flag_matches(arg, eval_flags) || flag_matches(arg, module_flags) {
            return None;
        }
        if arg.starts_with('-') {
            if option_takes_value(arg) {
                let _ = rest.next();
            }
            continue;
        }
        return agent_name_from_path_token(arg);
    }
    None
}

fn shell_command_agent_from_argv(argv: &[String]) -> Option<String> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == "-c" {
            return rest.next().and_then(|command| command_text_agent(command));
        }
        if arg.starts_with('-') {
            continue;
        }
        break;
    }
    None
}

fn command_agent_from_argv(argv: &[String]) -> Option<String> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.trim_matches('"').to_ascii_lowercase().as_str() {
            "/c" | "/k" => return rest.next().and_then(|command| command_text_agent(command)),
            _ => {}
        }
    }
    None
}

fn powershell_agent_from_argv(argv: &[String]) -> Option<String> {
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.trim_matches('"').to_ascii_lowercase().as_str() {
            "-file" | "-f" | "/file" => {
                return rest
                    .next()
                    .and_then(|path| agent_name_from_path_token(path));
            }
            "-command" | "-c" | "/command" | "/c" => {
                return rest.next().and_then(|command| command_text_agent(command));
            }
            "-encodedcommand" | "-enc" | "/encodedcommand" | "/enc" => return None,
            _ => {}
        }
    }
    None
}

fn command_text_agent(command: &str) -> Option<String> {
    let mut rest = command;
    while let Some((token, next)) = command_text_token(rest) {
        let token = token.trim();
        if ["&", ".", "call"]
            .iter()
            .any(|prefix| token.eq_ignore_ascii_case(prefix))
        {
            rest = next;
            continue;
        }
        return agent_name_from_path_token(token);
    }
    None
}

fn command_text_token(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let quote = input.chars().next()?;
    if quote == '"' || quote == '\'' {
        let start = quote.len_utf8();
        if let Some(end) = input[start..].find(quote) {
            let end = start + end;
            return Some((&input[start..end], &input[end + quote.len_utf8()..]));
        }
        return Some((&input[start..], ""));
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    Some((&input[..end], &input[end..]))
}

fn letta_is_interactive(process: &ProcessArgvSample) -> bool {
    let Some(argv) = process.argv.as_deref() else {
        return true;
    };
    let Some(entrypoint) = letta_entrypoint_index(argv) else {
        return true;
    };
    let rest = &argv[entrypoint + 1..];
    if rest.iter().any(|arg| {
        let option = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        matches!(
            option,
            "-p" | "--print"
                | "--prompt"
                | "--json"
                | "--stream-json"
                | "--run"
                | "--disable-memory-guard"
                | "--output-format"
                | "--input-format"
                | "--include-partial-messages"
                | "--from-agent"
                | "--environment"
                | "--env"
                | "--pre-load-skills"
                | "--tags"
                | "--ephemeral"
                | "--stateless"
                | "--max-turns"
                | "--memfs-startup"
                | "-h"
                | "--help"
                | "-v"
                | "--version"
                | "--info"
                | "--update"
                | "--upgrade"
        )
    }) {
        return false;
    }
    first_letta_cli_argument(rest).is_none_or(|argument| argument.starts_with('-'))
}

fn letta_entrypoint_index(argv: &[String]) -> Option<usize> {
    if argv
        .first()
        .is_some_and(|arg| agent_name_from_path_token(arg).as_deref() == Some("letta"))
    {
        return Some(0);
    }
    let runtime = lookup_name(argv.first()?);
    if !matches!(runtime.as_str(), "node" | "bun") {
        return None;
    }
    let mut index = 1;
    while let Some(arg) = argv.get(index) {
        if arg == "--" {
            return argv
                .get(index + 1)
                .is_some_and(|arg| agent_name_from_path_token(arg).as_deref() == Some("letta"))
                .then_some(index + 1);
        }
        if flag_matches(arg, &["-e", "--eval", "-p", "--print"]) {
            return None;
        }
        if arg.starts_with('-') {
            index += if option_takes_value(arg) { 2 } else { 1 };
            continue;
        }
        return (agent_name_from_path_token(arg).as_deref() == Some("letta")).then_some(index);
    }
    None
}

fn first_letta_cli_argument(args: &[String]) -> Option<&str> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--backend" {
            let _ = args.next();
            continue;
        }
        if arg.starts_with("--backend=") {
            continue;
        }
        return Some(arg);
    }
    None
}

fn agent_name_from_path_token(token: &str) -> Option<String> {
    let token = token.trim_matches(['"', '\'']);
    if token.is_empty() || token.starts_with('-') {
        return None;
    }
    let base = lookup_name(token);
    if let Some(agent) = canonical_manifest_agent(&base) {
        return Some(agent);
    }
    if base
        .strip_prefix("muse-bin-")
        .is_some_and(|suffix| suffix.starts_with(|ch: char| ch.is_ascii_digit()))
    {
        return Some("muse".to_owned());
    }
    let components = path_components(token)
        .into_iter()
        .map(lookup_name)
        .collect::<Vec<_>>();
    let suffixes: &[(&[&str], &str)] = &[
        (
            &[
                "node_modules",
                "@earendil-works",
                "pi-coding-agent",
                "dist",
                "cli",
            ],
            "pi",
        ),
        (
            &[
                "node_modules",
                "@earendil-works",
                "pi-coding-agent",
                "dist",
                "bundle",
                "cli",
            ],
            "pi",
        ),
        (
            &[
                "node_modules",
                "@oh-my-pi",
                "pi-coding-agent",
                "dist",
                "cli",
            ],
            "omp",
        ),
        (
            &[
                "node_modules",
                "@moonshot-ai",
                "kimi-code",
                "dist",
                "main.mjs",
            ],
            "kimi",
        ),
        (
            &["node_modules", "@qwen-code", "qwen-code", "dist", "index"],
            "qwen",
        ),
        (&["node_modules", "mastracode", "dist", "cli"], "mastracode"),
        (
            &["node_modules", "@letta-ai", "letta-code", "letta"],
            "letta",
        ),
    ];
    suffixes.iter().find_map(|(suffix, agent)| {
        (components.len() >= suffix.len()
            && components[components.len() - suffix.len()..]
                .iter()
                .map(String::as_str)
                .eq(suffix.iter().copied()))
        .then(|| (*agent).to_owned())
    })
}

fn canonical_manifest_agent(name: &str) -> Option<String> {
    let alias = match name {
        "claude-code" => "claude",
        "cursor" => "cursor-agent",
        "devin-cli" | "devin cli" => "devin",
        "antigravity" | "antigravity-cli" => "agy",
        ".cline" => "cline",
        "open-code" | "opencode2" => "opencode",
        "github-copilot" | "ghcs" => "copilot",
        "kimi-code" | "kimi code" => "kimi",
        "kiro" => "kiro-cli",
        "amp-local" => "amp",
        "grok-build" => "grok",
        "hermes-agent" => "hermes",
        "kilo-code" | "kilo code" => "kilo",
        "qoderclicn" | "qoder" | "qodercn" => "qodercli",
        "qwen-code" | "qwen code" => "qwen",
        "letta-code" | "letta code" => "letta",
        "muse-code" | "muse-cli" => "muse",
        _ => name,
    };
    bundled_manifest_for_executable(alias)
        .ok()
        .flatten()
        .map(|manifest| manifest.id.clone())
        .or_else(|| matches!(alias, "omp" | "mastracode").then(|| alias.to_owned()))
}

fn lookup_name(token: &str) -> String {
    let basename = token
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or(token);
    let mut name = basename.to_ascii_lowercase();
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_owned();
            break;
        }
    }
    name
}

fn path_components(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect()
}

fn is_generic_runtime_or_shell(name: &str) -> bool {
    is_python_runtime(name)
        || matches!(
            name,
            "sh" | "bash"
                | "zsh"
                | "fish"
                | "tmux"
                | "node"
                | "bun"
                | "cmd"
                | "powershell"
                | "pwsh"
        )
}

fn is_python_runtime(name: &str) -> bool {
    name == "python"
        || name.strip_prefix("python").is_some_and(|version| {
            !version.is_empty()
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
        })
}

fn process_priority(process: &ProcessArgvSample, candidate: &str) -> u8 {
    if candidate != process.executable.to_ascii_lowercase() {
        3
    } else if is_generic_runtime_or_shell(candidate) {
        1
    } else {
        2
    }
}

fn flag_matches(arg: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| {
        arg == *flag
            || (flag.starts_with('-')
                && !flag.starts_with("--")
                && arg.starts_with(flag)
                && arg.len() > flag.len())
            || (flag.starts_with("--")
                && arg
                    .strip_prefix(flag)
                    .is_some_and(|rest| rest.starts_with('=')))
    })
}

fn option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-r" | "--require"
            | "--loader"
            | "--import"
            | "--experimental-loader"
            | "--inspect-port"
            | "-W"
            | "-X"
            | "-S"
            | "-L"
            | "-o"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, argv: &[&str]) -> ProcessArgvSample {
        ProcessArgvSample {
            executable: name.to_owned(),
            argv: Some(argv.iter().map(|arg| (*arg).to_owned()).collect()),
        }
    }

    #[test]
    fn node_package_and_python_script_wrappers_resolve_agent_names() {
        let node = process(
            "node",
            &[
                "/usr/bin/node",
                "/work/node_modules/@qwen-code/qwen-code/dist/index.js",
            ],
        );
        assert_eq!(
            identify_foreground_process_group(None, &[node]).map(|identity| identity.executable),
            Some("qwen".to_owned())
        );
        let python = process("python3.12", &["python3.12", "/usr/local/bin/claude"]);
        assert_eq!(
            identify_foreground_process_group(None, &[python]).map(|identity| identity.executable),
            Some("claude".to_owned())
        );
    }

    #[test]
    fn shell_commands_and_unrelated_script_mentions_do_not_false_match() {
        let shell = process("bash", &["bash", "-c", "codex --help"]);
        assert_eq!(
            identify_foreground_process_group(None, &[shell]).map(|identity| identity.executable),
            Some("codex".to_owned())
        );
        let unrelated = process("node", &["node", "-e", "console.log('codex')"]);
        assert_eq!(identify_foreground_process_group(None, &[unrelated]), None);
    }

    #[test]
    fn letta_noninteractive_flags_are_filtered_but_interactive_is_retained() {
        let interactive = process(
            "node",
            &[
                "node",
                "node_modules/@letta-ai/letta-code/letta",
                "--backend",
                "local",
            ],
        );
        assert_eq!(
            identify_foreground_process_group(None, &[interactive])
                .map(|identity| identity.executable),
            Some("letta".to_owned())
        );
        let print = process("letta", &["letta", "--print", "hello"]);
        assert_eq!(identify_foreground_process_group(None, &[print]), None);
    }

    #[test]
    fn leader_priority_and_equal_rank_disagreement_are_explicit() {
        let leader = process("amp", &["amp"]);
        let other = process("codex", &["codex"]);
        assert_eq!(
            identify_foreground_process_group(Some(&leader), &[other.clone()]),
            Some(ProcessGroupIdentity {
                executable: "amp".to_owned(),
                ambiguous: false,
                conflicting: Vec::new()
            })
        );
        let tie = identify_foreground_process_group(None, &[other, process("claude", &["claude"])])
            .expect("two recognized direct names");
        assert!(tie.ambiguous);
        assert_eq!(tie.executable, "codex");
        assert_eq!(
            tie.conflicting,
            vec!["codex".to_owned(), "claude".to_owned()]
        );
    }
}
