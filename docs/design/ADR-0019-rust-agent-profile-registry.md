# ADR-0019: Rust agent profile registry

Status: accepted for the declarative profile, invocation-builder, and read-only
listing slice. Process launch, provider protocol parsing, discovery, permission
mediation, native session recovery, and live-provider validation remain
separate work.

## Context

Rover has four declared Go profiles: generic headless, generic PTY, Codex CLI,
and Claude CLI. Listing a profile must describe the integration surface without
implying provider availability, live conformance, or permission mediation.
Unknown names must fail closed instead of falling back to a generic process.

## Decision

- Add `rover-agents` as a domain crate containing the four Go-compatible
  descriptors and strict profile lookup.
- Preserve the Go validation labels verbatim, including its Linux PTY
  qualification and live-account testing requirement for native CLIs.
- Port Go's exact generic objective substitution and native Codex/Claude argv
  templates. Keep prompts after `--`, validate executable/model/tool values,
  and never select Claude's bypass permission mode.
- Keep every descriptor's permission mediation false; the registry does not
  add any provider execution or permission-control behavior.
- Expose read-only profile listing and exact descriptor lookup through the Rust
  `agents` and `agent capabilities` CLI commands. The command does not launch
  providers or inspect credentials.
- Do not add third-party dependencies beyond workspace `serde`, already present
  in the audited lockfile.

## Verification and limits

Crate tests cover the four descriptors, disclosure values, unknown-profile
failure, generic objective substitution, native argv, prompt argument
separation, and malformed option rejection. CLI tests cover JSON shapes,
schema envelope, successful lookup, and unknown lookup. Bounded native
transcript parsing is tracked separately in ADR-0020. Process discovery, live
validation, and user-selectable TUI worker dispatch remain in
P-062/P-070/P-072.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Upstream feature matrix](UPSTREAM_FEATURE_MATRIX.md)
- Go reference owner: `internal/agents/agents.go`
