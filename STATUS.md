# Rover implementation status — 0.2.0-alpha.1

This is an expanded local OSS implementation, not completion of every capability in
the original ten-layer vision. Working code and independently tested boundaries are
not interchangeable. See the validation report for exact observed tests.

| Layer | Implemented | Remaining / not validated |
|---|---|---|
| L1 interface | CLI/JSON, Linux keyboard TUI, actual PTY attach, setup preview, Python client, reversible agent instructions | Desktop/mobile, signed public installer, platform parity, broad accessibility validation |
| L2 interoperability | Generic headless/PTY adapters; Codex exec and Claude print argument/event adapters; MCP stdio/JSON HTTP; remote CLI client | Live provider certification, App Server/ACP, full MCP transport feature set, plugin marketplace |
| L3 runtime | Detached supervisors, terminal broker, reconnect, resize/input, cancellation, bounded output, resource admission, repair attempts | Cross-host worker recovery/fencing, native conversation restore, host-loss continuation |
| L4 environments | Worktrees, retained exact committed snapshots, disposable checks, explicit overlays, conservative dependency composition, patch export | Atomic mutable-tree capture, symlinks/submodules/LFS, managed development services/ports/databases, live Docker validation |
| L5 orchestration | Persistent/foreground DAG, parallel roots, cycle rejection, reservations, frozen contracts, bounded repairs, idempotency, reviewed/explicit-unreviewed handoffs, verified integration snapshot | Provider-independent external merge queue, schedules/webhooks, external write reconciliation, fleet/global monetary budgets |
| L6 context | Snapshot literal search, bounded context bundles, scoped expiring human/agent notes, stale markers | Semantic/service graph, authenticated business connectors, cross-project retrieval/knowledge system |
| L7 authority | Project/audience/tool-scoped expiring API grants, revocation, TLS admission, browser-Origin rejection, local grants, scrubbed default env, refused Docker fallback | Independent protected publisher, SSO/OAuth, credential broker, verified hostile-code isolation, OS-level tenant enforcement |
| L8 assurance | Go/JUnit/SARIF/exit-code parsers, exact replay, test-change heuristics, explicit counterfactual cases, finite mutation campaigns, required-check integrity, advisory native agent claims | Universal test-weakening semantics, native Lean/Kani/TLA+ scope interpreters, automated fuzz/property/browser/performance packs, LLM interrogation |
| L9 evidence | SQLite/event/artifact records, exact identities, diff/local review, exports, online backup/restore, explicit-key Ed25519 signatures | Protected CI check publisher, real external PR/merge/deploy writes, in-toto/SLSA attestations, full retention/erasure and global quota system |
| L10 improvement | Historical check-order proposals, explicit labels/datasets, disjoint heldout evaluation, proposal/dataset digests, local promotion/revocation, explicit strategy application | Live empirical optimization validation, RL training, autonomous self-play/RSI, independent evaluator deployment |

## Unreleased platform increment — darwin temp-path canonicalization (pending Linux re-verification and review)

The following source changes exist in the working tree on top of
`0.2.0-alpha.1`. They are **not** part of any validated release until they are
re-run on owned Linux/amd64 with a supported toolchain and reviewed:

- `internal/store/sqlite.go`: `canonicalStatePath()` resolves OS-standard
  symlinked ancestors (darwin `/var` -> `private/var`, `/tmp` -> `private/tmp`)
  via the nearest existing ancestor. A symlink as the final state component and
  an existing ancestor that is itself a symlink are still rejected.
- `internal/archive/archive.go`: `resolveBackupDir()` / `resolveExistingDir()`
  apply the same rule to backup/restore paths.
- `internal/testutil/repo.go`, `scripts/demo.py`, `scripts/demo_extended.py`:
  canonicalize temporary roots so repository and state paths share one identity.
- Test fixtures use `/usr/bin/true` instead of `/bin/true` (absent on macOS;
  both exist on Linux; same exit-0 semantics).
- `TestSourceStatesRejected/case-collision` skips with an explicit reason on
  case-insensitive filesystems (default macOS APFS cannot represent the
  scenario); Linux coverage is unchanged.
- New regression tests: `TestIntermediateSymlinkAncestorRejected`,
  `TestUnresolvedTempStateOpensAtResolvedRoot`,
  `TestBackupRejectsSymlinkedParent`. Existing `TestSymlinkStateRejected` and
  `TestBackupRejectsTraversalAndSymlinks` still pass unchanged.

Observed on darwin/arm64, Go 1.26.5 (outside the validated envelope, so this is
a local observation, not a release claim): `make check` green (22 packages, 90
top-level test functions, 5 fuzz functions) twice consecutively; `make race`
green; `make demo` 12/12 scenarios pass; SDK tests 3/3 pass; fuzz smokes
(`FuzzDecode`, `FuzzSafeName`, `FuzzResultParser`, `FuzzTranscript`, 3s x 2
workers) pass with no crashes; 10/10 schema example pairs validate.
`demo_extended` passes 8/16 scenarios then stops at the designed refusal
`interactive tasks currently require Linux` (PTY remains Linux-only by design).

Still required before these changes mean anything: owned-Linux `make check`,
`make race`, `make demo`, `make demo-extended` re-runs, plus independent review
of the trust-adjacent canonicalization. The acceptance counts below are
unchanged by this increment.

## Acceptance counts (from `docs/acceptance/implementation-map.json`, 40 scenarios)

- Tested in local scope (`tested_local` 11, `tested_linux` 1,
  `tested_admission` 1, `tested_rejection` 1): **14**.
- `partial`: **19** — usable subset with documented limits.
- `not_implemented`: **6** (A02 installer trust, A26 formal scope, A28 external
  uncertainty, A29 stale remote worker, A31 CI publisher, A39 destructive
  delivery). `not_validated`: **1** (A18 worker credentials).

## What was and was not exercised

- Real Linux processes, PTYs, worktrees, DAGs, repair runs, loopback TCP/HTTPS,
  token grants, signature validation and SQLite backup/restore were exercised.
- Codex/Claude event profiles were tested with owned JSONL and process fixtures;
  **no real model account was used**.
- Docker command construction/admission is tested; **no Docker daemon was available**.
- CI configuration is included; **no hosted GitHub workflow or public release was run**.
- Remote control was exercised against a local server; **no separate remote host or
  distributed worker fleet was validated**.
- Learning evaluation uses explicit synthetic fixtures and timing assumptions; no
  real-world speedup or quality improvement is claimed.
- Current validated build is Go 1.23.2 + system SQLite/cgo on Linux/amd64.
  Supported-current-Go, native macOS and Windows validation remain pending; the
  unreleased increment above has darwin-only observations that do not replace
  Linux validation or the pending review.

## Preserved constraints

Same-user local administration is not an independent acceptance authority. Commands
and tests can access host resources in local mode. Tests can be incomplete. Passing
structured reports are not universal correctness proofs. Content hashes and local
signatures do not turn the host into a trusted remote verifier.

Snapshot limits remain 8 MiB/file, 128 MiB/source and 10,000 files; special source
states are rejected. Backup retains at most 10,000 files / 1 GiB in the current
format and does not include raw running-session logs or mutable worktrees. Deleting
notes or records does not retroactively erase audit payloads or old backups.

All ten layers have usable implemented components. This does **not** mean the full
roadmap, all named integrations, security certification, or production readiness is done.
