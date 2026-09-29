//! Bounded evaluator for the audited Herdr screen-manifest format.
//!
//! This module ports the format and decision rules independently. The bundled
//! TOML data is tracked with per-file provenance in `UPSTREAM_SOURCE_AUDIT.md`.

use std::io;
use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;

use crate::detect::{
    AgentState, Confidence, DetectionEvidence, DetectionResult, DetectionVisibility,
};

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_RULES: usize = 128;
const MAX_GATE_DEPTH: usize = 8;
const MAX_TOTAL_GATES: usize = 512;
const MAX_MATCHERS_PER_GATE: usize = 32;
const MAX_TOTAL_MATCHERS: usize = 1024;
const MAX_MATCHER_CHARS: usize = 512;
const MAX_TOML_NESTING: usize = 32;
const MAX_REGEX_SIZE: usize = 1024 * 1024;
const ENGINE_VERSION: u32 = 3;

const BUNDLED: &[(&str, &str)] = &[
    ("amp", include_str!("herdr_manifests/amp.toml")),
    ("agy", include_str!("herdr_manifests/antigravity.toml")),
    ("claude", include_str!("herdr_manifests/claude.toml")),
    ("cline", include_str!("herdr_manifests/cline.toml")),
    ("codex", include_str!("herdr_manifests/codex.toml")),
    ("cursor", include_str!("herdr_manifests/cursor.toml")),
    ("devin", include_str!("herdr_manifests/devin.toml")),
    ("droid", include_str!("herdr_manifests/droid.toml")),
    ("gemini", include_str!("herdr_manifests/gemini.toml")),
    (
        "copilot",
        include_str!("herdr_manifests/github-copilot.toml"),
    ),
    ("grok", include_str!("herdr_manifests/grok.toml")),
    ("hermes", include_str!("herdr_manifests/hermes.toml")),
    ("kilo", include_str!("herdr_manifests/kilo.toml")),
    ("kimi", include_str!("herdr_manifests/kimi.toml")),
    ("kiro", include_str!("herdr_manifests/kiro.toml")),
    ("letta", include_str!("herdr_manifests/letta.toml")),
    ("maki", include_str!("herdr_manifests/maki.toml")),
    ("muse", include_str!("herdr_manifests/muse.toml")),
    ("opencode", include_str!("herdr_manifests/opencode.toml")),
    ("pi", include_str!("herdr_manifests/pi.toml")),
    ("qodercli", include_str!("herdr_manifests/qodercli.toml")),
    ("qwen", include_str!("herdr_manifests/qwen.toml")),
];

/// A validated, compiled Herdr-format detection manifest.
#[derive(Clone, Debug)]
pub struct HerdrManifest {
    /// Canonical manifest identity.
    pub id: String,
    /// Upstream manifest version when present.
    pub version: Option<String>,
    /// Declared aliases, retained for agent resolution.
    pub aliases: Vec<String>,
    rules: Vec<CompiledRule>,
}

#[derive(Clone, Debug)]
// These flags intentionally mirror the pinned upstream rule schema.
#[allow(clippy::struct_excessive_bools)]
struct CompiledRule {
    id: String,
    state: Option<AgentState>,
    priority: i32,
    region: String,
    visible_idle: bool,
    visible_blocker: bool,
    visible_working: bool,
    skip_state_update: bool,
    gate: CompiledGate,
}

#[derive(Clone, Debug)]
struct CompiledGate {
    all: Vec<Self>,
    any: Vec<Self>,
    not: Vec<Self>,
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDoc {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    min_engine_version: Option<u32>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    rules: Vec<RuleDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
// These booleans are distinct, documented fields in the pinned TOML schema.
#[allow(clippy::struct_excessive_bools)]
struct RuleDoc {
    id: String,
    #[serde(default)]
    state: Option<AgentState>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_region")]
    region: String,
    #[serde(default)]
    visible_idle: bool,
    #[serde(default)]
    visible_blocker: bool,
    #[serde(default)]
    visible_working: bool,
    #[serde(default)]
    skip_state_update: bool,
    #[serde(default)]
    all: Vec<GateDoc>,
    #[serde(default)]
    any: Vec<GateDoc>,
    #[serde(default, rename = "not")]
    not: Vec<GateDoc>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateDoc {
    #[serde(default)]
    all: Vec<Self>,
    #[serde(default)]
    any: Vec<Self>,
    #[serde(default, rename = "not")]
    not: Vec<Self>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

#[derive(Default)]
struct Complexity {
    gates: usize,
    matchers: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RuleOutcomeFlags {
    visibility: DetectionVisibility,
    skip_state_update: bool,
}

fn default_region() -> String {
    "whole_recent".to_owned()
}

/// Parse and compile one Herdr-format manifest under explicit input and
/// complexity limits.
///
/// # Errors
///
/// Returns `InvalidInput` for malformed TOML, unsupported engine versions,
/// unknown fields, invalid regions/regex, or exceeded bounds.
pub fn parse_herdr_manifest(source: &str) -> io::Result<HerdrManifest> {
    if source.len() > MAX_MANIFEST_BYTES || !toml_nesting_is_bounded(source) {
        return Err(invalid_input("agent manifest exceeds parser bounds"));
    }
    let doc: ManifestDoc = toml::from_str(source)
        .map_err(|error| invalid_input_owned(format!("invalid agent manifest TOML: {error}")))?;
    validate_doc(&doc)?;
    let mut rules = Vec::with_capacity(doc.rules.len());
    for rule in doc.rules {
        let gate = GateDoc {
            all: rule.all,
            any: rule.any,
            not: rule.not,
            contains: rule.contains,
            regex: rule.regex,
            line_regex: rule.line_regex,
        };
        let gate = compile_gate(&gate)?;
        rules.push(CompiledRule {
            id: rule.id,
            state: rule.state,
            priority: rule.priority,
            region: rule.region,
            visible_idle: rule.visible_idle,
            visible_blocker: rule.visible_blocker,
            visible_working: rule.visible_working,
            skip_state_update: rule.skip_state_update,
            gate,
        });
    }
    Ok(HerdrManifest {
        id: doc.id,
        version: doc.version,
        aliases: doc.aliases,
        rules,
    })
}

/// Return all 22 immutable bundled manifests after validating the complete
/// set on first use.
///
/// # Errors
///
/// Returns an error if any compiled-in manifest violates the parser contract.
pub fn bundled_herdr_manifests() -> io::Result<&'static [HerdrManifest]> {
    static MANIFESTS: OnceLock<Result<Vec<HerdrManifest>, String>> = OnceLock::new();
    match MANIFESTS.get_or_init(|| {
        BUNDLED
            .iter()
            .map(|(_, source)| parse_herdr_manifest(source).map_err(|error| error.to_string()))
            .collect()
    }) {
        Ok(manifests) => Ok(manifests),
        Err(error) => Err(invalid_input_owned(format!(
            "bundled agent manifest set is invalid: {error}"
        ))),
    }
}

/// Resolve an audited direct interactive-agent executable to its bundled
/// screen manifest. Wrapper and child-process inference are not performed.
///
/// # Errors
///
/// Returns an error if the compiled-in manifest set fails validation.
pub fn bundled_manifest_for_executable(
    executable: &str,
) -> io::Result<Option<&'static HerdrManifest>> {
    let basename = executable.rsplit(['/', '\\']).next().unwrap_or_default();
    if basename.is_empty()
        || basename.len() > 128
        || basename
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Ok(None);
    }
    let normalized = basename.to_ascii_lowercase();
    let normalized = normalized.strip_suffix(".exe").unwrap_or(&normalized);
    let normalized = normalized.strip_suffix(".cmd").unwrap_or(normalized);
    let Some(id) = manifest_id_for_executable(normalized) else {
        return Ok(None);
    };
    Ok(bundled_herdr_manifests()?
        .iter()
        .find(|manifest| manifest.id == id))
}

fn manifest_id_for_executable(executable: &str) -> Option<&'static str> {
    Some(match executable {
        "pi" => "pi",
        "claude" => "claude",
        "codex" => "codex",
        "gemini" => "gemini",
        "cursor-agent" => "cursor",
        "devin" => "devin",
        "agy" => "agy",
        "cline" => "cline",
        "opencode" => "opencode",
        "copilot" => "copilot",
        "kimi" => "kimi",
        "kiro-cli" => "kiro",
        "droid" => "droid",
        "amp" => "amp",
        "grok" => "grok",
        "hermes" => "hermes",
        "kilo" => "kilo",
        "qodercli" => "qodercli",
        "qwen" => "qwen",
        "letta" => "letta",
        "maki" => "maki",
        "muse" => "muse",
        _ => return None,
    })
}

/// Evaluate one validated manifest against a live screen and explicit OSC
/// title/progress evidence.
#[must_use]
pub fn evaluate_herdr_manifest(
    manifest: &HerdrManifest,
    screen: &str,
    osc_title: &str,
    osc_progress: &str,
) -> DetectionResult {
    if screen.len() > 64 * 1024 {
        return unknown_result(&manifest.id, "live_screen_unavailable", None);
    }
    if osc_title.len() > 4096 || osc_progress.len() > 4096 {
        return unknown_result(&manifest.id, "osc_evidence_exceeds_limit", None);
    }
    if !screen_text_is_safe(screen)
        || !screen_text_is_safe(osc_title)
        || !screen_text_is_safe(osc_progress)
    {
        return unknown_result(&manifest.id, "unsafe_terminal_evidence", None);
    }
    let input = MatchInput {
        screen,
        osc_title,
        osc_progress,
    };
    let mut selected: Option<(&CompiledRule, String)> = None;
    for rule in &manifest.rules {
        let region = select_region(input, &rule.region);
        if !gate_matches(&rule.gate, region) {
            continue;
        }
        match selected {
            Some((previous, _)) if previous.priority >= rule.priority => {}
            _ => selected = Some((rule, region.to_owned())),
        }
    }
    let Some((rule, _matched_region)) = selected else {
        let state = if manifest.id == "codex" {
            AgentState::Unknown
        } else {
            AgentState::Idle
        };
        let reason = if manifest.id == "codex" {
            "codex_state_ambiguous"
        } else {
            "default_known_agent_idle_fallback"
        };
        return result_with_rule(manifest, state, reason, None, RuleOutcomeFlags::default());
    };
    let state = rule.state.unwrap_or(AgentState::Unknown);
    result_with_rule(
        manifest,
        state,
        "manifest_rule_matched",
        Some(&rule.id),
        RuleOutcomeFlags {
            visibility: DetectionVisibility {
                idle: rule.visible_idle && state == AgentState::Idle,
                blocker: rule.visible_blocker && state == AgentState::Blocked,
                working: rule.visible_working && state == AgentState::Working,
            },
            skip_state_update: rule.skip_state_update,
        },
    )
}

fn result_with_rule(
    manifest: &HerdrManifest,
    state: AgentState,
    reason: &'static str,
    rule_id: Option<&str>,
    flags: RuleOutcomeFlags,
) -> DetectionResult {
    let mut evidence = Vec::new();
    if let Some(rule_id) = rule_id {
        evidence.push(DetectionEvidence {
            source: "screen_manifest",
            manifest_source: Some("herdr-bundled".to_owned()),
            manifest_version: manifest.version.clone(),
            rule_id: Some(rule_id.to_owned()),
            value: "manifest rule matched".to_owned(),
        });
    }
    DetectionResult {
        agent: Some(manifest.id.clone()),
        state,
        confidence: if rule_id.is_some() {
            Confidence::High
        } else if state == AgentState::Idle {
            Confidence::Medium
        } else {
            Confidence::None
        },
        reason,
        evidence,
        visibility: flags.visibility,
        skip_state_update: flags.skip_state_update,
    }
}

fn unknown_result(
    manifest_id: &str,
    reason: &'static str,
    rule_id: Option<&str>,
) -> DetectionResult {
    DetectionResult {
        agent: Some(manifest_id.to_owned()),
        state: AgentState::Unknown,
        confidence: Confidence::None,
        reason,
        evidence: rule_id
            .map(|id| DetectionEvidence {
                source: "screen_manifest",
                manifest_source: Some("herdr-bundled".to_owned()),
                manifest_version: None,
                rule_id: Some(id.to_owned()),
                value: "manifest rule matched".to_owned(),
            })
            .into_iter()
            .collect(),
        visibility: DetectionVisibility::default(),
        skip_state_update: false,
    }
}

fn validate_doc(doc: &ManifestDoc) -> io::Result<()> {
    if !valid_metadata(&doc.id, 64)
        || doc
            .version
            .as_ref()
            .is_some_and(|value| !valid_metadata(value, 64))
        || doc
            .updated_at
            .as_ref()
            .is_some_and(|value| !valid_metadata(value, 64))
        || doc.aliases.len() > 32
        || doc.aliases.iter().any(|value| !valid_metadata(value, 64))
    {
        return Err(invalid_input("invalid agent manifest metadata"));
    }
    if doc
        .min_engine_version
        .is_some_and(|required| required > ENGINE_VERSION)
    {
        return Err(invalid_input("agent manifest requires a newer engine"));
    }
    if doc.rules.is_empty() || doc.rules.len() > MAX_RULES {
        return Err(invalid_input("invalid agent manifest rule count"));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut complexity = Complexity::default();
    for rule in &doc.rules {
        if !valid_metadata(&rule.id, 128) || !ids.insert(rule.id.as_str()) {
            return Err(invalid_input("invalid or duplicate agent manifest rule id"));
        }
        if rule.skip_state_update
            && (rule.state != Some(AgentState::Unknown)
                || rule.visible_idle
                || rule.visible_blocker
                || rule.visible_working)
        {
            return Err(invalid_input("invalid skip_state_update rule"));
        }
        validate_region(&rule.region, doc.min_engine_version)?;
        let gate = GateDoc {
            all: rule.all.clone(),
            any: rule.any.clone(),
            not: rule.not.clone(),
            contains: rule.contains.clone(),
            regex: rule.regex.clone(),
            line_regex: rule.line_regex.clone(),
        };
        validate_gate(&gate, 0, true, &mut complexity)?;
    }
    Ok(())
}

fn validate_gate(
    gate: &GateDoc,
    depth: usize,
    require_positive: bool,
    complexity: &mut Complexity,
) -> io::Result<()> {
    if depth > MAX_GATE_DEPTH {
        return Err(invalid_input("agent manifest gate nesting exceeds limit"));
    }
    complexity.gates += 1;
    if complexity.gates > MAX_TOTAL_GATES {
        return Err(invalid_input("agent manifest gate count exceeds limit"));
    }
    let count = gate.contains.len() + gate.regex.len() + gate.line_regex.len();
    if count > MAX_MATCHERS_PER_GATE {
        return Err(invalid_input("agent manifest matcher count exceeds limit"));
    }
    complexity.matchers += count;
    if complexity.matchers > MAX_TOTAL_MATCHERS {
        return Err(invalid_input(
            "agent manifest total matcher count exceeds limit",
        ));
    }
    if gate
        .contains
        .iter()
        .chain(&gate.regex)
        .chain(&gate.line_regex)
        .any(|value| value.chars().count() > MAX_MATCHER_CHARS)
    {
        return Err(invalid_input("agent manifest matcher length exceeds limit"));
    }
    let positive = count > 0 || !gate.all.is_empty() || !gate.any.is_empty();
    let has_any = positive || !gate.not.is_empty();
    if (require_positive && !positive) || (!require_positive && !has_any) {
        return Err(invalid_input("agent manifest gate has no positive matcher"));
    }
    for pattern in gate.regex.iter().chain(&gate.line_regex) {
        compile_regex(pattern)?;
    }
    for nested in &gate.all {
        validate_gate(nested, depth + 1, true, complexity)?;
    }
    for nested in &gate.any {
        validate_gate(nested, depth + 1, true, complexity)?;
    }
    for nested in &gate.not {
        validate_gate(nested, depth + 1, false, complexity)?;
    }
    Ok(())
}

fn compile_gate(gate: &GateDoc) -> io::Result<CompiledGate> {
    Ok(CompiledGate {
        all: gate
            .all
            .iter()
            .map(compile_gate)
            .collect::<io::Result<_>>()?,
        any: gate
            .any
            .iter()
            .map(compile_gate)
            .collect::<io::Result<_>>()?,
        not: gate
            .not
            .iter()
            .map(compile_gate)
            .collect::<io::Result<_>>()?,
        contains: gate
            .contains
            .iter()
            .map(|text| text.to_lowercase())
            .collect(),
        regex: gate
            .regex
            .iter()
            .map(|pattern| compile_regex(pattern))
            .collect::<io::Result<_>>()?,
        line_regex: gate
            .line_regex
            .iter()
            .map(|pattern| compile_regex(pattern))
            .collect::<io::Result<_>>()?,
    })
}

fn compile_regex(pattern: &str) -> io::Result<Regex> {
    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE)
        .dfa_size_limit(MAX_REGEX_SIZE)
        .build()
        .map_err(|_| invalid_input("invalid or oversized agent manifest regex"))
}

fn gate_matches(gate: &CompiledGate, text: &str) -> bool {
    let lower = text.to_lowercase();
    gate.contains.iter().all(|needle| lower.contains(needle))
        && gate.regex.iter().all(|regex| regex.is_match(text))
        && gate
            .line_regex
            .iter()
            .all(|regex| text.lines().any(|line| regex.is_match(line)))
        && gate.all.iter().all(|nested| gate_matches(nested, text))
        && (gate.any.is_empty() || gate.any.iter().any(|nested| gate_matches(nested, text)))
        && !gate.not.iter().any(|nested| gate_matches(nested, text))
}

#[derive(Clone, Copy)]
struct MatchInput<'a> {
    screen: &'a str,
    osc_title: &'a str,
    osc_progress: &'a str,
}

fn select_region<'a>(input: MatchInput<'a>, name: &str) -> &'a str {
    let region = name.trim();
    match region {
        "whole_recent" => input.screen,
        "after_last_prompt_marker" => after_last_prompt_marker(input.screen),
        "before_current_prompt_marker" => before_current_prompt_marker(input.screen),
        "whole_recent_without_current_prompt_marker" => {
            whole_recent_without_current_prompt_marker(input.screen)
        }
        "current_prompt_block_marker" => current_prompt_block_marker(input.screen).unwrap_or(""),
        "after_current_prompt_block_marker" => {
            after_current_prompt_block_marker(input.screen).unwrap_or("")
        }
        "prompt_box_body" => prompt_box_body(input.screen).unwrap_or(""),
        "above_prompt_box" => above_prompt_box(input.screen),
        "last_non_empty_above_prompt_box" => last_non_empty_line(above_prompt_box(input.screen)),
        "after_last_horizontal_rule" => after_last_horizontal_rule(input.screen),
        "osc_title" => input.osc_title,
        "osc_progress" => input.osc_progress,
        _ => match parse_count(region, "bottom_lines") {
            Some(count) => bottom_lines(input.screen, count),
            None => match parse_count(region, "bottom_non_empty_lines") {
                Some(count) => bottom_non_empty_lines(input.screen, count),
                None => parse_top_count(region)
                    .map_or("", |count| top_non_empty_lines(input.screen, count)),
            },
        },
    }
}

fn validate_region(region: &str, min_engine: Option<u32>) -> io::Result<()> {
    let valid = matches!(
        region.trim(),
        "whole_recent"
            | "after_last_prompt_marker"
            | "before_current_prompt_marker"
            | "whole_recent_without_current_prompt_marker"
            | "current_prompt_block_marker"
            | "after_current_prompt_block_marker"
            | "prompt_box_body"
            | "above_prompt_box"
            | "last_non_empty_above_prompt_box"
            | "after_last_horizontal_rule"
            | "osc_title"
            | "osc_progress"
    ) || parse_count(region.trim(), "bottom_lines").is_some()
        || parse_count(region.trim(), "bottom_non_empty_lines").is_some()
        || parse_top_count(region.trim()).is_some();
    if !valid
        || (region.trim().starts_with("top_non_empty_lines(")
            && min_engine.is_some_and(|version| version < 3))
    {
        return Err(invalid_input(
            "invalid or unsupported agent manifest region",
        ));
    }
    Ok(())
}

fn parse_count(spec: &str, prefix: &str) -> Option<usize> {
    let value = spec
        .strip_prefix(prefix)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    value.parse().ok()
}

fn parse_top_count(spec: &str) -> Option<usize> {
    let value = spec
        .strip_prefix("top_non_empty_lines(")?
        .strip_suffix(')')?;
    if value.starts_with('0') || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<usize>()
        .ok()
        .filter(|count| u16::try_from(*count).is_ok())
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

fn slice_from<'a>(text: &'a str, rows: &[&str], index: usize) -> &'a str {
    let offset = rows[..index.min(rows.len())]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len());
    &text[offset..]
}

fn bottom_lines(text: &str, count: usize) -> &str {
    let rows = lines(text);
    slice_from(text, &rows, rows.len().saturating_sub(count))
}

fn bottom_non_empty_lines(text: &str, count: usize) -> &str {
    let rows = lines(text);
    let Some(start) = rows
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
        .last()
        .map(|(index, _)| index)
    else {
        return "";
    };
    slice_from(text, &rows, start)
}

fn top_non_empty_lines(text: &str, count: usize) -> &str {
    let rows = lines(text);
    let Some(end) = rows
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
        .last()
        .map(|(index, _)| index + 1)
    else {
        return "";
    };
    let offset = rows[..end]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len());
    &text[..offset]
}

fn after_last_prompt_marker(text: &str) -> &str {
    let rows = lines(text);
    rows.iter()
        .rposition(|line| codex_prompt_line(line))
        .map_or(text, |index| slice_from(text, &rows, index + 1))
}

fn before_current_prompt_marker(text: &str) -> &str {
    let rows = lines(text);
    let Some(index) = current_codex_prompt_index(&rows) else {
        return text;
    };
    let offset = rows[..index]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len());
    &text[..offset]
}

fn whole_recent_without_current_prompt_marker(text: &str) -> &str {
    if current_codex_prompt_index(&lines(text)).is_some() {
        ""
    } else {
        text
    }
}

fn current_prompt_block_marker(text: &str) -> Option<&str> {
    let rows = lines(text);
    let prompt = current_codex_prompt_index(&rows)?;
    rows[..prompt]
        .iter()
        .rev()
        .find(|line| codex_block_marker_line(line))
        .copied()
}

fn after_current_prompt_block_marker(text: &str) -> Option<&str> {
    let rows = lines(text);
    let prompt = current_codex_prompt_index(&rows)?;
    let block = rows[..prompt]
        .iter()
        .rposition(|line| codex_block_marker_line(line))?;
    Some(slice_from(text, &rows, block))
}

fn current_codex_prompt_index(rows: &[&str]) -> Option<usize> {
    let prompt = rows.iter().rposition(|line| codex_prompt_line(line))?;
    (!rows[prompt + 1..]
        .iter()
        .any(|line| codex_block_marker_line(line)))
    .then_some(prompt)
}

fn codex_prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn codex_block_marker_line(line: &str) -> bool {
    line.starts_with(['•', '■', '✗', '✓'])
}

fn prompt_box_body(text: &str) -> Option<&str> {
    let rows = lines(text);
    let top = prompt_box_top_border_index(&rows)?;
    let start = rows[..=top]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let end_index = rows[top + 1..]
        .iter()
        .position(|line| is_horizontal_rule(line))
        .map_or(rows.len(), |relative| top + 1 + relative);
    let end = rows[..end_index]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len());
    Some(&text[start.min(text.len())..end])
}

fn above_prompt_box(text: &str) -> &str {
    let rows = lines(text);
    let Some(top) = prompt_box_top_border_index(&rows) else {
        return text;
    };
    let end = rows[..top]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(text.len());
    &text[..end]
}

fn after_last_horizontal_rule(text: &str) -> &str {
    let mut offset = 0usize;
    let mut end = 0usize;
    for line in text.lines() {
        offset = (offset + line.len() + 1).min(text.len());
        if is_horizontal_rule(line) {
            end = offset;
        }
    }
    &text[end..]
}

fn last_non_empty_line(text: &str) -> &str {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
}

fn prompt_box_top_border_index(rows: &[&str]) -> Option<usize> {
    let mut count = 0;
    for index in (0..rows.len()).rev() {
        if is_horizontal_rule(rows[index]) {
            count += 1;
            if count == 2 {
                return Some(index);
            }
        }
    }
    None
}

fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let count = trimmed
        .chars()
        .take_while(|character| *character == '─')
        .count();
    if count == 0 {
        return false;
    }
    let suffix = trimmed.chars().skip(count).collect::<String>();
    suffix.trim().is_empty() || count >= 3
}

fn valid_metadata(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
}

fn screen_text_is_safe(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
        && !value.chars().any(is_bidi_control)
}

fn is_bidi_control(character: char) -> bool {
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

fn toml_nesting_is_bounded(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut multiline = false;
    let mut escaped = false;
    let mut comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if comment {
            if byte == b'\n' {
                comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(delimiter) = quote {
            if delimiter == b'"' && !multiline && escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if delimiter == b'"' && !multiline && byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == delimiter {
                if multiline {
                    if bytes.get(index..index + 3) == Some(&[delimiter; 3]) {
                        quote = None;
                        multiline = false;
                        index += 3;
                        continue;
                    }
                } else {
                    quote = None;
                }
            }
            index += 1;
            continue;
        }
        if byte == b'#' {
            comment = true;
            index += 1;
            continue;
        }
        if matches!(byte, b'"' | b'\'') {
            quote = Some(byte);
            multiline = bytes.get(index..index + 3) == Some(&[byte; 3]);
            index += if multiline { 3 } else { 1 };
            continue;
        }
        if matches!(byte, b'[' | b'{') {
            depth += 1;
            if depth > MAX_TOML_NESTING {
                return false;
            }
        } else if matches!(byte, b']' | b'}') {
            depth = depth.saturating_sub(1);
        }
        index += 1;
    }
    true
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_input_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(rules: &str, id: &str) -> String {
        format!("id = \"{id}\"\nversion = \"1\"\n{rules}")
    }

    #[test]
    fn every_pinned_bundled_manifest_parses_and_compiles() {
        let manifests = bundled_herdr_manifests().expect("all audited manifests parse");
        assert_eq!(manifests.len(), 22);
        let ids = manifests
            .iter()
            .map(|value| value.id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"copilot"));
    }

    #[test]
    fn gate_operators_and_priority_match_the_pinned_manifest_contract() {
        let manifest = parse_herdr_manifest(&manifest(
            r#"
[[rules]]
id = "lower"
state = "working"
priority = 10
contains = ["active"]

[[rules]]
id = "upper"
state = "blocked"
priority = 20
all = [{ contains = ["active"], any = [{ regex = ["approve\\s+now"] }, { line_regex = ["^allow$"] }] }]
not = [{ contains = ["cancelled"] }]
"#,
            "sample",
        ))
        .unwrap();
        let result = evaluate_herdr_manifest(&manifest, "active\napprove now", "", "");
        assert_eq!(result.state, AgentState::Blocked);
        assert_eq!(result.evidence[0].rule_id.as_deref(), Some("upper"));

        let excluded = evaluate_herdr_manifest(&manifest, "active\napprove now\ncancelled", "", "");
        assert_eq!(excluded.state, AgentState::Working);
        assert_eq!(excluded.evidence[0].rule_id.as_deref(), Some("lower"));
    }

    #[test]
    fn ties_keep_the_first_rule_and_regions_select_only_the_requested_text() {
        let manifest = parse_herdr_manifest(&manifest(
            r#"
[[rules]]
id = "first"
state = "working"
priority = 1
region = "bottom_non_empty_lines(1)"
contains = ["active"]

[[rules]]
id = "tie"
state = "blocked"
priority = 1
region = "bottom_non_empty_lines(1)"
contains = ["active"]
"#,
            "sample",
        ))
        .unwrap();
        let result = evaluate_herdr_manifest(&manifest, "active\n\nidle", "", "");
        assert_eq!(result.state, AgentState::Idle);
        let result = evaluate_herdr_manifest(&manifest, "old\n\nactive", "", "");
        assert_eq!(result.state, AgentState::Working);
        assert_eq!(result.evidence[0].rule_id.as_deref(), Some("first"));
    }

    #[test]
    fn osc_regions_skip_state_flags_and_codex_fallback_are_preserved() {
        let codex = parse_herdr_manifest(include_str!("herdr_manifests/codex.toml")).unwrap();
        let fallback = evaluate_herdr_manifest(&codex, "ordinary", "", "");
        assert_eq!(fallback.state, AgentState::Unknown);
        assert_eq!(fallback.reason, "codex_state_ambiguous");

        let amp = parse_herdr_manifest(include_str!("herdr_manifests/amp.toml")).unwrap();
        let title = evaluate_herdr_manifest(&amp, "", "Amp: Plugin confirmation needed", "");
        assert_eq!(title.state, AgentState::Blocked);
        assert!(title.visibility.blocker);

        let viewer = evaluate_herdr_manifest(
            &codex,
            "↑/↓ to scroll\npgup/pgdn to move\nhome/end to jump\nq to quit\nesc to edit prev",
            "",
            "",
        );
        assert!(viewer.skip_state_update);
    }

    #[test]
    fn malformed_unknown_or_excessively_nested_manifests_fail_closed() {
        let bad_regex = manifest(
            "[[rules]]\nid = \"bad\"\nstate = \"working\"\nregex = [\"(\"]\n",
            "bad",
        );
        assert!(parse_herdr_manifest(&bad_regex).is_err());
        let unknown = manifest(
            "[[rules]]\nid = \"bad\"\nstate = \"working\"\nextra = 1\ncontains = [\"x\"]\n",
            "bad",
        );
        assert!(parse_herdr_manifest(&unknown).is_err());
        let nested = format!(
            "id = \"bad\"\nrules = {}{}{}",
            "[".repeat(40),
            "0",
            "]".repeat(40)
        );
        assert!(parse_herdr_manifest(&nested).is_err());
    }

    #[test]
    fn top_non_empty_region_without_minimum_engine_version_matches_upstream() {
        let source = manifest(
            "[[rules]]\nid = \"top\"\nstate = \"working\"\nregion = \"top_non_empty_lines(1)\"\ncontains = [\"active\"]\n",
            "sample",
        );
        let parsed = parse_herdr_manifest(&source).expect("minimum engine version is optional");
        let result = evaluate_herdr_manifest(&parsed, "active\ninactive", "", "");
        assert_eq!(result.state, AgentState::Working);

        let old_engine =
            source.replace("id = \"sample\"", "id = \"sample\"\nmin_engine_version = 2");
        assert!(parse_herdr_manifest(&old_engine).is_err());
    }

    #[test]
    fn every_upstream_region_selector_is_accepted_and_slices_screen_as_expected() {
        let screen = "top\n• current block\n────────\nbody\n────────\nlast\n› run\n";
        let input = MatchInput {
            screen,
            osc_title: "title evidence",
            osc_progress: "progress evidence",
        };
        let cases = [
            ("whole_recent", screen),
            ("after_last_prompt_marker", ""),
            (
                "before_current_prompt_marker",
                "top\n• current block\n────────\nbody\n────────\nlast\n",
            ),
            ("whole_recent_without_current_prompt_marker", ""),
            ("current_prompt_block_marker", "• current block"),
            (
                "after_current_prompt_block_marker",
                "• current block\n────────\nbody\n────────\nlast\n› run\n",
            ),
            ("prompt_box_body", "body\n"),
            ("above_prompt_box", "top\n• current block\n"),
            ("last_non_empty_above_prompt_box", "• current block"),
            ("after_last_horizontal_rule", "last\n› run\n"),
            ("osc_title", "title evidence"),
            ("osc_progress", "progress evidence"),
            ("bottom_lines(1)", "› run\n"),
            ("bottom_non_empty_lines(1)", "› run\n"),
            ("top_non_empty_lines(1)", "top\n"),
        ];
        for (selector, expected) in cases {
            validate_region(selector, None).unwrap_or_else(|error| panic!("{selector}: {error}"));
            assert_eq!(select_region(input, selector), expected, "{selector}");
        }
    }

    #[test]
    fn unsafe_osc_terminal_controls_fail_closed() {
        let manifest = parse_herdr_manifest(include_str!("herdr_manifests/amp.toml")).unwrap();
        let result = evaluate_herdr_manifest(&manifest, "", "title\u{001b}", "");
        assert_eq!(result.state, AgentState::Unknown);
        assert_eq!(result.reason, "unsafe_terminal_evidence");
    }

    #[test]
    fn audited_direct_interactive_executables_resolve_to_bundled_manifests() {
        for (executable, id) in [
            ("pi", "pi"),
            ("claude", "claude"),
            ("codex", "codex"),
            ("gemini", "gemini"),
            ("cursor-agent", "cursor"),
            ("devin", "devin"),
            ("agy", "agy"),
            ("cline", "cline"),
            ("opencode", "opencode"),
            ("copilot", "copilot"),
            ("kimi", "kimi"),
            ("kiro-cli", "kiro"),
            ("droid", "droid"),
            ("amp", "amp"),
            ("grok", "grok"),
            ("hermes", "hermes"),
            ("kilo", "kilo"),
            ("qodercli", "qodercli"),
            ("qwen", "qwen"),
            ("letta", "letta"),
            ("maki", "maki"),
            ("muse", "muse"),
        ] {
            assert_eq!(
                bundled_manifest_for_executable(executable)
                    .unwrap()
                    .map(|manifest| manifest.id.as_str()),
                Some(id),
                "{executable}"
            );
        }
        assert!(bundled_manifest_for_executable("custom-wrapper")
            .unwrap()
            .is_none());
        assert!(bundled_manifest_for_executable("codex\nmalicious")
            .unwrap()
            .is_none());
    }
}
