//! Bounded JSONL parsing for the documented Codex and Claude CLI event forms.
//!
//! Provider-authored messages, completion flags, and usage are claims from
//! that process. They are not independent verification evidence.

use std::collections::BTreeMap;
use std::io;

const MAX_TRANSCRIPT_BYTES: usize = 1 << 20;
const MAX_LINE_BYTES: usize = 1 << 20;
const MAX_CLAIMS: usize = 256;
const PROVENANCE: &str = "provider-emitted claims/usage; not independent verification";

/// Parsed provider output with its source clearly identified as a provider
/// claim, not an independently checked result.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct AgentResult {
    /// Profile that produced this output.
    pub adapter: String,
    /// Provider session identity when the transcript supplies one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session: String,
    /// Provider's terminal completion claim.
    pub completed: bool,
    /// Provider failure detail, if present.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Provider-authored response claims.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<String>,
    /// Nonnegative integer usage fields supplied by the provider.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub usage: BTreeMap<String, i64>,
    /// Provider-reported cost estimate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    /// Count of event types the adapter did not interpret.
    pub unknown_events: usize,
    /// Provenance boundary for returned values.
    pub provenance: String,
}

/// Parse one bounded native CLI JSONL transcript.
///
/// Codex requires a stable `thread.started` identity before `turn.completed`;
/// Claude requires a stable `session_id` and a boolean `is_error` in its
/// terminal result. Unknown adapter names intentionally return an empty result
/// as the Go compatibility contract does; adapter selection itself must use
/// [`crate::profile`] to fail closed before process dispatch.
///
/// # Errors
///
/// Returns `InvalidData` for oversized, malformed, conflicting, or incomplete
/// native transcripts, and for invalid terminal event ordering.
pub fn parse_transcript(adapter: &str, bytes: &[u8]) -> io::Result<AgentResult> {
    let mut result = AgentResult {
        adapter: adapter.to_owned(),
        provenance: PROVENANCE.to_owned(),
        ..AgentResult::default()
    };
    if adapter != "codex-exec" && adapter != "claude-print" {
        return Ok(result);
    }
    if bytes.len() > MAX_TRANSCRIPT_BYTES {
        return Err(invalid_data("transcript exceeds limit"));
    }

    let mut terminal = false;
    let mut seen = BTreeMap::<String, Vec<u8>>::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.len() > MAX_LINE_BYTES {
            return Err(invalid_data("malformed native event"));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let event: serde_json::Value =
            serde_json::from_slice(line).map_err(|_| invalid_data("malformed native event"))?;
        let Some(object) = event.as_object() else {
            if event.is_null() {
                return Err(invalid_data("event missing type"));
            }
            return Err(invalid_data("malformed native event"));
        };
        let kind = object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if kind.is_empty() {
            return Err(invalid_data("event missing type"));
        }
        if let Some(uuid) = object
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            if let Some(previous) = seen.get(uuid) {
                if previous.as_slice() != line {
                    return Err(invalid_data("conflicting duplicate event"));
                }
                continue;
            }
            seen.insert(uuid.to_owned(), line.to_vec());
        }
        if terminal {
            return Err(invalid_data("event after terminal result"));
        }

        if adapter == "codex-exec" {
            parse_codex_event(object, kind, &mut result, &mut terminal)?;
        } else {
            parse_claude_event(object, kind, &mut result, &mut terminal)?;
        }
        if result.claims.len() > MAX_CLAIMS {
            return Err(invalid_data("claim capture limit exceeded"));
        }
    }
    if !terminal {
        return Err(invalid_data("terminal result missing"));
    }
    Ok(result)
}

fn parse_codex_event(
    event: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
    result: &mut AgentResult,
    terminal: &mut bool,
) -> io::Result<()> {
    match kind {
        "thread.started" => {
            let identity = string_field(event, "thread_id");
            if identity.is_empty() || (!result.session.is_empty() && result.session != identity) {
                return Err(invalid_data("invalid thread identity"));
            }
            identity.clone_into(&mut result.session);
        }
        "turn.started" | "item.started" | "item.updated" => {}
        "item.completed" => {
            let Some(item) = event.get("item") else {
                return Err(invalid_data("invalid item"));
            };
            if !item.is_null() && !item.is_object() {
                return Err(invalid_data("invalid item"));
            }
            let item_type = item
                .get("type")
                .map_or(Ok(""), optional_json_string)
                .map_err(|()| invalid_data("invalid item"))?;
            let text = item
                .get("text")
                .map_or(Ok(""), optional_json_string)
                .map_err(|()| invalid_data("invalid item"))?;
            if item_type == "agent_message" {
                result.claims.push(text.to_owned());
            }
        }
        "turn.completed" => {
            if result.session.is_empty() {
                return Err(invalid_data("completion without thread"));
            }
            *terminal = true;
            result.completed = true;
            read_usage(event.get("usage"), &mut result.usage);
        }
        "turn.failed" | "error" => {
            *terminal = true;
            result.error = event
                .get("error")
                .map_or_else(String::new, serde_json::Value::to_string);
            if result.error.is_empty() {
                "provider error".clone_into(&mut result.error);
            }
        }
        _ => result.unknown_events += 1,
    }
    Ok(())
}

fn optional_json_string(value: &serde_json::Value) -> Result<&str, ()> {
    match value {
        serde_json::Value::Null => Ok(""),
        serde_json::Value::String(value) => Ok(value),
        _ => Err(()),
    }
}

fn parse_claude_event(
    event: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
    result: &mut AgentResult,
    terminal: &mut bool,
) -> io::Result<()> {
    if let Some(identity) = event
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        if !result.session.is_empty() && result.session != identity {
            return Err(invalid_data("session changed"));
        }
        identity.clone_into(&mut result.session);
    }
    match kind {
        "system" | "assistant" | "user" | "stream_event" | "tool_progress" | "tool_use_summary" => {
        }
        "result" => {
            let Some(failed) = event.get("is_error").and_then(serde_json::Value::as_bool) else {
                return Err(invalid_data(
                    "terminal result requires session and is_error boolean",
                ));
            };
            if result.session.is_empty() {
                return Err(invalid_data(
                    "terminal result requires session and is_error boolean",
                ));
            }
            *terminal = true;
            result.completed = !failed;
            if failed {
                string_field(event, "result").clone_into(&mut result.error);
                if result.error.is_empty() {
                    "provider failed".clone_into(&mut result.error);
                }
            }
            let claim = string_field(event, "result");
            if !claim.is_empty() {
                result.claims.push(claim.to_owned());
            }
            read_usage(event.get("usage"), &mut result.usage);
            result.estimated_cost_usd = event
                .get("total_cost_usd")
                .and_then(serde_json::Value::as_f64)
                .filter(|cost| *cost >= 0.0);
        }
        _ => result.unknown_events += 1,
    }
    Ok(())
}

fn read_usage(value: Option<&serde_json::Value>, destination: &mut BTreeMap<String, i64>) {
    let Some(usage) = value.and_then(serde_json::Value::as_object) else {
        return;
    };
    for (key, value) in usage {
        if let Some(number) = value.as_i64().filter(|number| *number >= 0) {
            destination.insert(key.clone(), number);
        }
    }
}

fn string_field<'a>(event: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> &'a str {
    event
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_events_keep_claims_usage_and_session_provenance() {
        let transcript = br#"{"type":"thread.started","thread_id":"t1"}
{"type":"item.completed","item":{"type":"agent_message","text":"provider says fixed"}}
{"type":"turn.completed","usage":{"input_tokens":8,"output_tokens":4,"invalid":-1,"fractional":1.5}}"#;
        let result = parse_transcript("codex-exec", transcript).unwrap();
        assert!(result.completed);
        assert_eq!(result.session, "t1");
        assert_eq!(result.claims, ["provider says fixed"]);
        assert_eq!(result.usage.get("input_tokens"), Some(&8));
        assert_eq!(result.usage.get("output_tokens"), Some(&4));
        assert!(!result.usage.contains_key("invalid"));
        assert!(!result.usage.contains_key("fractional"));
        assert_eq!(result.provenance, PROVENANCE);
    }

    #[test]
    fn claude_terminal_result_captures_claim_cost_usage_and_failure() {
        let success = br#"{"type":"result","session_id":"s1","is_error":false,"result":"provider says done","total_cost_usd":0.04,"usage":{"input_tokens":9}}"#;
        let result = parse_transcript("claude-print", success).unwrap();
        assert!(result.completed);
        assert_eq!(result.session, "s1");
        assert_eq!(result.claims, ["provider says done"]);
        assert_eq!(result.estimated_cost_usd, Some(0.04));
        assert_eq!(result.usage.get("input_tokens"), Some(&9));

        let failure =
            br#"{"type":"result","session_id":"s2","is_error":true,"result":"provider error"}"#;
        let result = parse_transcript("claude-print", failure).unwrap();
        assert!(!result.completed);
        assert_eq!(result.error, "provider error");
    }

    #[test]
    fn duplicate_event_identity_is_idempotent_but_conflicts_fail() {
        let event = br#"{"type":"thread.started","thread_id":"thread","uuid":"event-1"}"#;
        let mut transcript = event.to_vec();
        transcript.push(b'\n');
        transcript.extend_from_slice(event);
        transcript.extend_from_slice(b"\n{\"type\":\"turn.completed\"}");
        let result = parse_transcript("codex-exec", &transcript).unwrap();
        assert!(result.completed);

        let conflict = br#"{"type":"thread.started","thread_id":"thread","uuid":"event-1"}
{"type":"thread.started","thread_id":"other","uuid":"event-1"}"#;
        assert_eq!(
            parse_transcript("codex-exec", conflict)
                .unwrap_err()
                .to_string(),
            "conflicting duplicate event"
        );
    }

    #[test]
    fn nullable_codex_item_fields_match_go_zero_value_decoding() {
        let transcript = br#"{"type":"thread.started","thread_id":"t"}
{"type":"item.completed","item":{"type":null,"text":null}}
{"type":"turn.completed"}"#;
        let result = parse_transcript("codex-exec", transcript).unwrap();
        assert!(result.completed);
        assert!(result.claims.is_empty());
    }

    #[test]
    fn malformed_missing_terminal_and_event_order_fail_closed() {
        for (transcript, expected) in [
            (&b"not-json"[..], "malformed native event"),
            (&b"{}"[..], "event missing type"),
            (&b"null"[..], "event missing type"),
            (
                &br#"{"type":"turn.completed"}"#[..],
                "completion without thread",
            ),
            (
                &br#"{"type":"result","session_id":"s","is_error":null}"#[..],
                "terminal result requires session and is_error boolean",
            ),
            (
                &br#"{"type":"result","session_id":"s","is_error":false}
{"type":"system"}"#[..],
                "event after terminal result",
            ),
            (
                &br#"{"type":"thread.started","thread_id":"a"}
{"type":"thread.started","thread_id":"b"}"#[..],
                "invalid thread identity",
            ),
        ] {
            let result = if expected.contains("terminal result")
                || expected == "event after terminal result"
            {
                parse_transcript("claude-print", transcript)
            } else {
                parse_transcript("codex-exec", transcript)
            };
            assert_eq!(result.unwrap_err().to_string(), expected, "{transcript:?}");
        }
    }

    #[test]
    fn size_claim_and_unknown_adapter_bounds_are_enforced() {
        assert_eq!(
            parse_transcript("codex-exec", &vec![b' '; MAX_TRANSCRIPT_BYTES + 1])
                .unwrap_err()
                .to_string(),
            "transcript exceeds limit"
        );
        let mut transcript = String::from("{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n");
        for _ in 0..=MAX_CLAIMS {
            transcript.push_str("{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"x\"}}\n");
        }
        assert_eq!(
            parse_transcript("codex-exec", transcript.as_bytes())
                .unwrap_err()
                .to_string(),
            "claim capture limit exceeded"
        );
        let unknown = parse_transcript("generic-headless", b"not-json").unwrap();
        assert_eq!(unknown.adapter, "generic-headless");
        assert!(unknown.usage.is_empty());
        assert!(!unknown.completed);
    }
}
