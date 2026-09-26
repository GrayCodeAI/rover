# ADR-0020: Native agent transcript parsing

Status: accepted for bounded Codex/Claude JSONL fixture parsing. Live provider
conformance and process integration remain open.

## Context

Codex and Claude CLI adapters emit structured JSONL with session IDs, terminal
events, provider-authored content, and optional usage. Corrupt, conflicting, or
incomplete streams must not look successful. Provider output remains an
unverified provider claim, never observed Rover verification evidence.

## Decision

- Parse only `codex-exec` and `claude-print`; unknown adapters retain the Go
  compatibility behavior of returning an empty, provenance-labelled result.
  Profile selection separately fails closed before dispatch.
- Bound transcripts and individual event lines to 1 MiB and captured claims to
  256. Ignore blank lines, count unknown event types, and accept only exact
  byte-identical duplicates carrying the same nonempty UUID.
- Require Codex thread identity before terminal completion and stable identity
  through its events. Require Claude session identity and a boolean
  `is_error` on its terminal result.
- Reject malformed JSON, missing terminal results, identity changes,
  conflicting event IDs, and any nonduplicate event after terminal status.
- Retain provider usage only for nonnegative signed 64-bit integer fields and
  retain nonnegative provider-reported Claude cost as an estimate.
- Preserve the source label `provider-emitted claims/usage; not independent
  verification` on results.
- Use already-locked `serde_json`; no new third-party package is introduced.

## Verification and limits

Fixture tests cover both adapters' success/failure, identity, duplicate event,
malformed JSON, event ordering, size and claim bounds, usage/cost filtering,
and provenance. They do not establish live provider compatibility. Rust rejects
invalid UTF-8 JSON bytes; the Go JSON decoder may replace invalid bytes inside
JSON strings, so that malformed-input edge is intentionally stricter and must
not be treated as fixture conformance evidence.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Agent profile registry](ADR-0019-rust-agent-profile-registry.md)
- Go reference owner: `internal/agents/agents.go`
