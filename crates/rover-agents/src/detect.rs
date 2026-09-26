//! Evidence-based process and bottom-buffer agent detection.
//!
//! The caller supplies the foreground executable and the live bottom-buffer
//! screen snapshot. A scrolled viewport or historical text is not a valid
//! substitute for that snapshot.

use std::collections::BTreeSet;
use std::io;

use crate::profiles;

const MAX_MANIFESTS: usize = 128;
const MAX_PROCESS_NAMES: usize = 32;
const MAX_RULES: usize = 256;
const MAX_SCREEN_BYTES: usize = 64 * 1024;
const MAX_MARKERS_PER_RULE: usize = 16;
const MAX_MARKER_BYTES: usize = 256;

/// State derived from process and screen evidence.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// A known foreground agent has not been classified from available input.
    Unknown,
    /// The screen did not match an active/blocked rule for a known agent.
    Idle,
    /// A screen rule indicates ongoing work.
    Working,
    /// A screen rule indicates a visible request for human input.
    Blocked,
}

/// Confidence for the state conclusion, separate from whether identity exists.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// No state conclusion is supported.
    None,
    /// A documented fallback was used without a matching state rule.
    Medium,
    /// One unambiguous status conclusion has explicit evidence.
    High,
}

/// A group of literal markers that can classify a live screen snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenRule {
    /// Stable manifest-local rule identifier.
    pub id: String,
    /// State assigned when the rule matches.
    pub state: AgentState,
    /// Higher-priority rules take precedence; conflicting top-priority matches
    /// produce unknown rather than an arbitrary state.
    pub priority: u16,
    /// Any one marker may match. Values are literal substrings, not regex.
    pub any_of: Vec<String>,
}

/// Local, declarative process/screen manifest for an agent kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenManifest {
    /// Stable agent kind or user-defined agent label.
    pub agent: String,
    /// Manifest origin shown in explain evidence (for example `local`).
    pub source: String,
    /// Human-readable manifest version.
    pub version: String,
    /// Exact executable basenames that identify this agent.
    pub process_names: Vec<String>,
    /// Rules for classifying the live bottom-buffer snapshot.
    pub rules: Vec<ScreenRule>,
}

impl ScreenManifest {
    /// Validate a manifest before it participates in detection.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for empty, oversized, duplicate, or terminal-
    /// control-bearing metadata and rules.
    pub fn validate(&self) -> io::Result<()> {
        if !valid_display_text(&self.agent, 128)
            || !valid_display_text(&self.source, 256)
            || !valid_display_text(&self.version, 64)
        {
            return Err(invalid_input("invalid agent detection manifest metadata"));
        }
        if self.process_names.is_empty() || self.process_names.len() > MAX_PROCESS_NAMES {
            return Err(invalid_input("invalid agent detection process names"));
        }
        let mut process_names = BTreeSet::new();
        for name in &self.process_names {
            if !valid_process_name(name) || !process_names.insert(normalize_process_name(name)) {
                return Err(invalid_input("invalid or duplicate agent process name"));
            }
        }
        if self.rules.len() > MAX_RULES {
            return Err(invalid_input("agent detection manifest has too many rules"));
        }
        let mut rule_ids = BTreeSet::new();
        for rule in &self.rules {
            if !valid_rule_id(&rule.id) || !rule_ids.insert(rule.id.as_str()) {
                return Err(invalid_input(
                    "invalid or duplicate agent detection rule id",
                ));
            }
            if rule.state == AgentState::Unknown
                || rule.any_of.is_empty()
                || rule.any_of.len() > MAX_MARKERS_PER_RULE
                || rule.any_of.iter().any(|marker| {
                    !valid_display_text(marker, MAX_MARKER_BYTES) || marker.trim().is_empty()
                })
            {
                return Err(invalid_input("invalid agent detection rule"));
            }
        }
        Ok(())
    }
}

/// Foreground process evidence supplied by a platform adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessSample {
    /// Executable path or basename for the current foreground process.
    pub executable: String,
}

/// Inputs for one detection evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DetectionRequest {
    /// Foreground process sample. Child-process guesses are not accepted here.
    pub foreground: Option<ProcessSample>,
    /// Current live bottom-buffer snapshot, not scrollback or a historical view.
    pub screen: Option<String>,
    /// Validated local or bundled manifests available to this evaluation.
    pub manifests: Vec<ScreenManifest>,
}

/// Explainable result of one detection evaluation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DetectionResult {
    /// Recognized agent kind, if foreground-process evidence resolved one.
    pub agent: Option<String>,
    /// Classified state, or `unknown` when evidence is insufficient/conflicted.
    pub state: AgentState,
    /// Confidence in the state conclusion (identity may still be known).
    pub confidence: Confidence,
    /// Stable reason code for diagnostics and tests.
    pub reason: &'static str,
    /// Bounded evidence details; raw screen contents are never copied here.
    pub evidence: Vec<DetectionEvidence>,
    /// Manifest metadata indicating visible idle prompt chrome.
    pub visibility: DetectionVisibility,
    /// A transcript/history viewer should not update the authoritative state.
    pub skip_state_update: bool,
}

/// Screen chrome flags from the matched manifest rule.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct DetectionVisibility {
    /// Manifest metadata indicating a visible idle prompt.
    pub idle: bool,
    /// Manifest metadata indicating a visible human-input blocker.
    pub blocker: bool,
    /// Manifest metadata indicating visible working UI chrome.
    pub working: bool,
}

/// One safe-to-display detection observation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DetectionEvidence {
    /// Signal class (`foreground_process` or `screen_manifest`).
    pub source: &'static str,
    /// Manifest origin, when this observation came from a screen rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_source: Option<String>,
    /// Manifest version, when this observation came from a screen rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_version: Option<String>,
    /// Rule identifier, when this observation came from a screen rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// Safe process basename or literal rule marker, never the full screen.
    pub value: String,
}

/// Detect an agent using exact foreground executable matches and an optional
/// live screen manifest. A known process with no matching rule falls back to
/// idle only when a valid live screen snapshot and manifest were supplied.
#[must_use]
pub fn detect_agent(request: &DetectionRequest) -> DetectionResult {
    let Some(foreground) = request.foreground.as_ref() else {
        return result(
            None,
            AgentState::Unknown,
            Confidence::None,
            "foreground_process_unavailable",
            Vec::new(),
        );
    };
    let executable = basename(&foreground.executable);
    if !valid_process_name(executable) {
        return result(
            None,
            AgentState::Unknown,
            Confidence::None,
            "invalid_foreground_process",
            Vec::new(),
        );
    }
    let process_evidence = || DetectionEvidence {
        source: "foreground_process",
        manifest_source: None,
        manifest_version: None,
        rule_id: None,
        value: executable.to_owned(),
    };
    let agent = match identify_foreground(request, executable) {
        Ok(agent) => agent,
        Err(reason) => {
            return result(
                None,
                AgentState::Unknown,
                Confidence::None,
                reason,
                vec![process_evidence()],
            );
        }
    };
    let evidence = vec![process_evidence()];
    let Some(screen) = request.screen.as_deref() else {
        return result(
            Some(agent),
            AgentState::Unknown,
            Confidence::None,
            "live_screen_unavailable",
            evidence,
        );
    };
    if screen.len() > MAX_SCREEN_BYTES {
        return result(
            Some(agent),
            AgentState::Unknown,
            Confidence::None,
            "live_screen_exceeds_limit",
            evidence,
        );
    }
    if !valid_screen_text(screen) {
        return result(
            Some(agent),
            AgentState::Unknown,
            Confidence::None,
            "live_screen_contains_unsafe_controls",
            evidence,
        );
    }
    evaluate_screen(&agent, screen, &request.manifests, evidence)
}

fn identify_foreground(
    request: &DetectionRequest,
    executable: &str,
) -> Result<String, &'static str> {
    if request.manifests.len() > MAX_MANIFESTS {
        return Err("too_many_agent_manifests");
    }
    let normalized = normalize_process_name(executable);
    let mut candidates = BTreeSet::<String>::new();
    for descriptor in profiles() {
        if descriptor.executable != "user-defined"
            && normalize_process_name(descriptor.executable) == normalized
        {
            candidates.insert(descriptor.name.to_owned());
        }
    }
    for manifest in request
        .manifests
        .iter()
        .filter(|manifest| manifest.validate().is_ok())
    {
        if manifest
            .process_names
            .iter()
            .any(|name| normalize_process_name(name) == normalized)
        {
            candidates.insert(manifest.agent.clone());
        }
    }
    match candidates.len() {
        0 => Err("foreground_process_unrecognized"),
        1 => Ok(candidates.into_iter().next().unwrap_or_default()),
        _ => Err("foreground_process_ambiguous"),
    }
}

fn evaluate_screen(
    agent: &str,
    screen: &str,
    manifests: &[ScreenManifest],
    evidence: Vec<DetectionEvidence>,
) -> DetectionResult {
    let matching: Vec<_> = manifests
        .iter()
        .filter(|manifest| manifest.agent == agent && manifest.validate().is_ok())
        .collect();
    let Some(manifest) = matching.first().copied() else {
        return result(
            Some(agent.to_owned()),
            AgentState::Unknown,
            Confidence::None,
            "agent_manifest_unavailable",
            evidence,
        );
    };
    if matching.len() > 1 {
        return result(
            Some(agent.to_owned()),
            AgentState::Unknown,
            Confidence::None,
            "agent_manifest_ambiguous",
            evidence,
        );
    }
    classify_screen(agent, screen, manifest, evidence)
}

fn classify_screen(
    agent: &str,
    screen: &str,
    manifest: &ScreenManifest,
    mut evidence: Vec<DetectionEvidence>,
) -> DetectionResult {
    let screen_lower = screen.to_lowercase();
    let mut matches = Vec::new();
    for rule in &manifest.rules {
        let matched_marker = rule
            .any_of
            .iter()
            .find(|marker| screen_lower.contains(&marker.to_lowercase()));
        if matched_marker.is_some() {
            matches.push((rule.priority, rule.state, rule.id.as_str()));
        }
    }
    let Some(priority) = matches.iter().map(|entry| entry.0).max() else {
        return result(
            Some(agent.to_owned()),
            AgentState::Idle,
            Confidence::Medium,
            "known_agent_no_screen_rule_defaults_idle",
            evidence,
        );
    };
    matches.retain(|entry| entry.0 == priority);
    matches.sort_by(|left, right| left.2.cmp(right.2));
    let states = matches.iter().map(|entry| entry.1).collect::<BTreeSet<_>>();
    for (_, _, rule_id) in &matches {
        evidence.push(DetectionEvidence {
            source: "screen_manifest",
            manifest_source: Some(manifest.source.clone()),
            manifest_version: Some(manifest.version.clone()),
            rule_id: Some((*rule_id).to_owned()),
            value: "literal marker matched".to_owned(),
        });
    }
    if states.len() != 1 {
        return result(
            Some(agent.to_owned()),
            AgentState::Unknown,
            Confidence::None,
            "conflicting_screen_rules",
            evidence,
        );
    }
    result(
        Some(agent.to_owned()),
        states.iter().next().copied().unwrap_or(AgentState::Unknown),
        Confidence::High,
        "screen_rule_matched",
        evidence,
    )
}

fn result(
    agent: Option<String>,
    state: AgentState,
    confidence: Confidence,
    reason: &'static str,
    evidence: Vec<DetectionEvidence>,
) -> DetectionResult {
    DetectionResult {
        agent,
        state,
        confidence,
        reason,
        evidence,
        visibility: DetectionVisibility::default(),
        skip_state_update: false,
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or_default()
}

fn normalize_process_name(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_owned()
}

fn valid_process_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+'))
}

fn valid_rule_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_display_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(is_unsafe_text_character)
}

fn valid_screen_text(value: &str) -> bool {
    value.len() <= MAX_SCREEN_BYTES
        && !value.chars().any(|character| {
            character != '\n' && character != '\t' && is_unsafe_text_character(character)
        })
}

fn is_unsafe_text_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(agent: &str, process: &str, rules: Vec<ScreenRule>) -> ScreenManifest {
        ScreenManifest {
            agent: agent.to_owned(),
            source: "local-test".to_owned(),
            version: "1".to_owned(),
            process_names: vec![process.to_owned()],
            rules,
        }
    }

    fn rule(id: &str, state: AgentState, priority: u16, marker: &str) -> ScreenRule {
        ScreenRule {
            id: id.to_owned(),
            state,
            priority,
            any_of: vec![marker.to_owned()],
        }
    }

    #[test]
    fn exact_foreground_process_and_screen_rule_supply_explainable_evidence() {
        let manifest = manifest(
            "review-bot",
            "review-bot",
            vec![rule(
                "approval",
                AgentState::Blocked,
                10,
                "Allow this action?",
            )],
        );
        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "/usr/local/bin/review-bot".to_owned(),
            }),
            screen: Some("Tests done.\nAllow this action?".to_owned()),
            manifests: vec![manifest],
        });
        assert_eq!(result.agent.as_deref(), Some("review-bot"));
        assert_eq!(result.state, AgentState::Blocked);
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.reason, "screen_rule_matched");
        assert_eq!(result.evidence.len(), 2);
        assert_eq!(result.evidence[1].rule_id.as_deref(), Some("approval"));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("Tests done"));
    }

    #[test]
    fn built_in_process_identity_never_implies_a_status() {
        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "C:\\tools\\CODEX.EXE".to_owned(),
            }),
            screen: None,
            manifests: Vec::new(),
        });
        assert_eq!(result.agent.as_deref(), Some("codex-exec"));
        assert_eq!(result.state, AgentState::Unknown);
        assert_eq!(result.confidence, Confidence::None);
        assert_eq!(result.reason, "live_screen_unavailable");
    }

    #[test]
    fn idle_fallback_requires_valid_manifest_and_live_snapshot() {
        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "claude".to_owned(),
            }),
            screen: Some("ordinary agent screen".to_owned()),
            manifests: Vec::new(),
        });
        assert_eq!(result.state, AgentState::Unknown);
        assert_eq!(result.confidence, Confidence::None);
        assert_eq!(result.reason, "agent_manifest_unavailable");

        let manifest = manifest("claude-print", "claude", Vec::new());
        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "claude".to_owned(),
            }),
            screen: Some("ordinary agent screen".to_owned()),
            manifests: vec![manifest],
        });
        assert_eq!(result.state, AgentState::Idle);
        assert_eq!(result.confidence, Confidence::Medium);
        assert_eq!(result.reason, "known_agent_no_screen_rule_defaults_idle");
    }

    #[test]
    fn process_names_are_exact_and_manifest_collisions_are_unknown() {
        let unrecognized = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "codex-wrapper".to_owned(),
            }),
            screen: Some("working".to_owned()),
            manifests: Vec::new(),
        });
        assert_eq!(unrecognized.agent, None);
        assert_eq!(unrecognized.reason, "foreground_process_unrecognized");

        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "worker".to_owned(),
            }),
            screen: Some("working".to_owned()),
            manifests: vec![
                manifest("one", "worker", Vec::new()),
                manifest("two", "worker", Vec::new()),
            ],
        });
        assert_eq!(result.agent, None);
        assert_eq!(result.reason, "foreground_process_ambiguous");
    }

    #[test]
    fn priority_resolves_rules_but_conflicting_ties_remain_unknown() {
        let detect = |manifest| {
            detect_agent(&DetectionRequest {
                foreground: Some(ProcessSample {
                    executable: "worker".to_owned(),
                }),
                screen: Some("running; approve".to_owned()),
                manifests: vec![manifest],
            })
        };
        let higher_priority = manifest(
            "worker",
            "worker",
            vec![
                rule("working", AgentState::Working, 1, "running"),
                rule("blocked", AgentState::Blocked, 2, "approve"),
            ],
        );
        assert_eq!(detect(higher_priority).state, AgentState::Blocked);
        let tied = manifest(
            "worker",
            "worker",
            vec![
                rule("working", AgentState::Working, 2, "running"),
                rule("blocked", AgentState::Blocked, 2, "approve"),
            ],
        );
        let result = detect(tied);
        assert_eq!(result.state, AgentState::Unknown);
        assert_eq!(result.confidence, Confidence::None);
        assert_eq!(result.reason, "conflicting_screen_rules");
    }

    #[test]
    fn invalid_manifests_and_hostile_screen_fail_closed() {
        let mut invalid = manifest(
            "worker",
            "worker",
            vec![rule("bad\u{202e}rule", AgentState::Blocked, 1, "approve")],
        );
        assert!(invalid.validate().is_err());
        invalid.rules[0].id = "approval".to_owned();
        assert!(invalid.validate().is_ok());
        assert!(manifest(
            "worker",
            "worker",
            vec![
                rule("same", AgentState::Idle, 1, "a"),
                rule("same", AgentState::Working, 1, "b")
            ]
        )
        .validate()
        .is_err());

        let result = detect_agent(&DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "worker".to_owned(),
            }),
            screen: Some("unsafe\u{202e}screen".to_owned()),
            manifests: vec![invalid],
        });
        assert_eq!(result.state, AgentState::Unknown);
        assert_eq!(result.reason, "live_screen_contains_unsafe_controls");

        let request = DetectionRequest {
            foreground: Some(ProcessSample {
                executable: "worker".to_owned(),
            }),
            screen: Some("idle".to_owned()),
            manifests: (0..=MAX_MANIFESTS)
                .map(|index| manifest(&format!("worker-{index}"), "worker", Vec::new()))
                .collect(),
        };
        assert_eq!(detect_agent(&request).reason, "too_many_agent_manifests");
    }
}
