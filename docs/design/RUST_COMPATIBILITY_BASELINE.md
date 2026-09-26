# Rover compatibility baseline for Rust

Date: 2026-09-25. The Go implementation remains the production source of truth.
The Rust core has begun implementation; this index does not claim CLI, TUI,
storage, or feature parity.

## Public CLI command families

Preserve the command spellings and aliases below until Rust golden-contract
tests establish an approved change. The detailed flags and examples are in
[docs/CLI.md](../CLI.md); dispatch is in `internal/cli/cli.go` and
`internal/cli/advanced.go`.

| Family | Commands / aliases |
|---|---|
| Setup and diagnostics | `version`, `doctor`, `init`, `help` |
| Inspect and verify | `inspect`, `verify`, `check` |
| Tasks and sessions | `task run`, `do`, `status`/`st`, `logs`/`lg`, `cancel`, `ui`, `tui`, `attach`, `limits`, `agent list`, `agent capabilities` |
| Workflows | `workflow`/`wf` (`run`, `status`, `cancel`) |
| Review and records | `report`/`rep`, `review`, `outcome`, `events`, `export`, `publish` |
| Source and assurance | `replay`, `diff`, `prove regression`, `mutate` |
| Context and instructions | `context search|bundle`, `memory put|list|delete`, `integrate` |
| Protocol and access | `mcp`, `serve`, `grant create|list|revoke` |
| Remote CLI | `remote node-add|node-list|node-delete|tools|call` |
| Maintenance and evidence | `backup`, `backup-check`, `restore`, `attest keygen|sign|verify` |
| Learning | `learn recommend|dataset|evaluate|promote|show|revoke` |

The current CLI also accepts `__workflow` and `__worker` internal dispatch
commands. They are implementation details, not stable user commands; audit any
Rust equivalents against supervisor launch behavior before removing them.

## Serialized contracts

| Contract | Current source of truth | Compatibility requirement |
|---|---|---|
| Common Rover records | `model.Schema` (`rover/v1alpha1`), `docs/API.md` | Preserve field names/types and explicit unknown/error semantics; version intentional changes. |
| Project check configuration | `schemas/config.schema.json`, `internal/config` | Strict JSON; reject unknown/duplicate fields, multiple documents, malformed/oversized/deep input. |
| Task request | `schemas/task.schema.json`, `internal/model`, `docs/CLI.md` | Preserve argv, timeouts, attempts, reservations, interactive/session and agent-option semantics. |
| Workflow request | `schemas/workflow.schema.json`, `internal/workflow`, `docs/CLI.md` | Preserve graph validation, idempotency, dependencies, bounded parallelism, frozen config and conservative integration. |
| Counterfactual request | `schemas/regression.schema.json` | Preserve exact base/candidate and named-test semantics; ambiguity stays inconclusive. |
| Mutation request | `schemas/mutation.schema.json` | Preserve finite, uniquely matched mutations and explicit per-mutant outcomes. |
| MCP | `docs/CLI.md`, `internal/mcp`, `internal/service` | Current documented protocol is MCP 2025-11-25; stdio line framing and JSON HTTP subset, no SSE/OAuth/browser CORS claim. |
| Acceptance inventory | `docs/acceptance/implementation-map.json` | Preserve A01-A40 IDs and honest per-platform scope until each has a Rust mapping. |

## Current Rust coverage

`crates/rover-core` currently implements the `rover/v1alpha1` marker, validated
IDs, fallible legacy-compatible ID generation, typed SHA-256 digests, and typed
UTC timestamps with RFC3339Nano-compatible formatting. Its 13 unit tests cover
ID boundaries, digest vectors and validation, timestamp normalization, and
fraction trimming. The crate does not yet serialize Rover records or expose
the existing Go CLI commands. See [ADR-0003](ADR-0003-core-identifiers-digests-and-time.md)
and [the parity plan](RUST_PARITY_PLAN.md).

## Current package disposition

This is the initial disposition for Rover-owned packages. "Port" means preserve
observable contracts in Rust, not translate every Go file line for line.
Implementation starts only after each domain's dependency/security contract is
recorded and its acceptance owner is known.

| Current Go package/path | Planned disposition |
|---|---|
| `cmd/rover`, `internal/cli` | Port public CLI/TUI entry points and JSON/exit behavior; retain old binary during parity period. |
| `internal/model`, `internal/wire` | Port typed records, IDs/digests, strict wire parsing and versioned serialized contracts. |
| `internal/store`, `internal/archive` | Port transactional persistence, artifacts, budget/reservations, backup and restore with migrations. |
| `internal/source` | Port safe Git capture, immutable snapshots, diffs, candidate integration and path restrictions. |
| `internal/execution`, `internal/terminal`, `internal/tasks` | Port bounded process/PTY/session supervision, cancellation, task lifecycle and recovery. |
| `internal/agents`, `internal/integration` | Port declared adapters, event handling, native session metadata and reversible integration management. |
| `internal/workflow` | Port DAG validation, idempotency, concurrency, handoffs, repair bounds and final integration verification. |
| `internal/assurance` | Port strict check interpretation, replay, counterfactuals, mutation, decision semantics and evidence binding. |
| `internal/access`, `internal/service`, `internal/mcp` | Port local/API authority, grants, project boundaries, MCP contracts and remote CLI protocol. |
| `internal/config`, `schemas/` | Preserve strict config/task/workflow/regression/mutation schemas; version reviewed changes. |
| `internal/contextstore` | Port bounded snapshot search/bundles and expiring scoped notes. |
| `internal/attestation` | Port explicit-key signatures and verification; retain current limitations. |
| `internal/learning` | Port bounded evaluation/proposal behavior only with preserved dataset separation and explicit promotion. |
| `internal/install`, `internal/publish` | Re-evaluate as CLI diagnostics/local advisory output; do not imply signed installer or protected CI. |
| `internal/testutil` | Replace with Rust fixture builders and deterministic owned workers; not shipped as runtime. |
| `sdk/python`, `sdk/typescript`, `sdk/go`, `sdk/README.md` | Removed by explicit user direction; historical results remain dated evidence. |
| `scripts/demo.py`, `scripts/demo_extended.py` | Port to deterministic Rust CLI integration/demo harnesses; retain scripts only until equivalent coverage runs. |
| `.github/workflows`, `Makefile`, dependency manifests | Keep the existing Go runtime gates while adapting them for Rust CI, license, SBOM, and release gates; SDK targets have been removed. |
| Current docs and acceptance records | Update to Rust only as each capability is verified; retain dated Go results as historical. |

This disposition does not mark any Rust port complete.

## Exit and decision codes

Documented verification exit codes are 0 accepted under supplied policy, 1
required check failed, 2 error/inconclusive, and 3 checks satisfied but human
review remains required. Read/report commands return their own operation status;
callers must inspect the serialized `decision`, not infer acceptance from a
zero exit alone. Rust contract tests must capture command-specific errors,
cancellation, malformed input, and JSON output, not only the four verification
codes.

## Security and data invariants to port

- Local execution is not a sandbox and carries the host user's authority.
- A failed, missing, malformed, truncated, or zero-test required result cannot
  silently count as a pass; an empty verification plan is inconclusive.
- Verification binds retained source/config/tool identities; candidate edits do
  not silently replace baseline checks.
- Ambiguous worker loss is `LOST`; do not blindly re-submit side effects.
- Grants are scoped/expiring/revocable; secrets are not serialized into logs or
  repository task files. HTTP non-loopback requires TLS and browser Origins are
  rejected.
- Paths, object hashes, resource limits, SQLite transactions, backup integrity,
  and private file permissions remain enforced.
- Human review, provider assertion, command completion, and evidence acceptance
  are separate facts.

## Acceptance baseline

`docs/acceptance/implementation-map.json` contains A01-A40. Its rows and test
references were mechanically checked on 2026-09-25: all 40 IDs occur exactly
once and every referenced path exists. Existing Go tests are evidence about the
current Go implementation, not proof that a future Rust port meets those rows.
Rust work must create a per-row Rust test mapping before Go removal.

## Validation boundary

This document was checked against `README.md`, `STATUS.md`, `docs/CLI.md`,
`docs/API.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY_MODEL.md`, the five JSON
schemas, `internal/cli` dispatch, and the acceptance map. The check is an
inventory, not a test run or a claim of Rust parity.
