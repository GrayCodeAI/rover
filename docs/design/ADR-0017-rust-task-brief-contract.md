# ADR-0017: Rust task brief and dependency graph contract

- Status: accepted for validation and graph-planning foundation
- Date: 2026-09-26
- Owner: `rover-core` owns portable task-domain validation; `rover-store` and
  `rover-execution` continue to own persistence and process lifecycle.

## Context

The TUI persists repository-bound task briefing drafts with a title, path
globs, dependencies, quality-gate text, and prompt. Those drafts are editable
and may be incomplete. Rover's existing execution record and supervisor own a
separate lifecycle contract. Go's current task validator rejects nonempty
`depends_on` because its alpha scheduler does not implement dependency
scheduling. A Rust task feature must validate user input and graph structure
without silently claiming that text is executable or that a graph has been
scheduled.

## Decision

1. `rover_core::TaskBrief` is the execution-neutral form contract. Draft
   persistence may hold incomplete values; task creation must call
   `TaskBrief::validate`.
2. Titles are nonblank and capped at 120 bytes. Path globs are newline
   separated, capped at 8 KiB and 128 entries, relative, traversal-free, and
   limited to `*`, `**`, and `?` wildcard behavior. Dependencies are
   newline-separated Rover IDs, capped at 4 KiB and 128 IDs; duplicates and
   invalid IDs fail validation. Quality-gate text remains opaque and capped
   at 1 KiB. Prompts are nonblank and capped at 16 KiB. Controls other than
   line feed in multiline fields and bidi formatting characters are rejected.
3. `validate_task_graph` resolves a proposed in-memory graph against the task
   set supplied by its caller. Missing IDs, duplicate task/dependency IDs,
   self-dependencies, cycles, and graphs above 10,000 nodes fail closed. A
   valid graph returns deterministic lexical tie-broken topological order.
4. Validation is not scheduling or dispatch. This ADR does not add task-record
   persistence, readiness transitions, worker execution, retry history, path
   leases, or quality-gate execution. Those remain explicit P-071 through
   P-074 implementation work. The existing Go-compatible supervisor schema
   remains unchanged.
5. Quality-gate text is not parsed, invoked, or represented as a verified
   check. Future gate execution requires its own typed argv/timeout/output
   contract and acceptance tests.

## Compatibility and OSS provenance

The brief mirrors the fields in the current Rover TUI draft. Relative glob
constraints follow Rover's documented existing config path safety rules.
Graph validation supplies a Rust capability missing from the current Go
alpha; it is a Rover implementation, not a claim about current Go behavior.
No Herdr, Luvus, or Orca source or assets were copied. Serde was already a
locked Rover workspace dependency and its existing license bundle remains
covered by `make rust-check`'s lockfile notice audit.

## Acceptance evidence

`rover-core` regression tests cover valid globs, traversal/absolute/unsupported
patterns, duplicate and malformed dependency IDs, stable topological order,
missing and self dependencies, duplicate task/dependency IDs, and cycles.
The focused task tests passed three consecutive runs. The full
`make rust-check CARGO='cargo +1.88.0'` gate passed, including clippy, all Rust
workspace tests, dependency audit tests, and dependency license checks. Native
Linux execution remains a separate platform requirement.
