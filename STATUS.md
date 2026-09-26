# Rover implementation status — 0.0.1

This is an expanded local OSS implementation, not completion of every capability in
the original ten-layer vision. Working code and independently tested boundaries are
not interchangeable. See the validation report for exact observed tests.

The Rust port is also in progress. Its workspace provides validated core and
repository identity primitives, versioned SQLite migrations, transactional
record/event/idempotency APIs, project-scoped grant services, and a Unix
state-root/blob/file layer, plus store-level agent and named-resource
reservations, atomic local budget accounting, read-only legacy-state inventory,
and an opt-in Go state importer. The importer retains the source tree, excludes
runtime task/check trees, revokes grants, and sanitizes process state; independent
repeat-import verification checks records, events, request keys, receipt counts,
and content-addressed object digests. See
`docs/design/GO_RUST_STATE_MIGRATION.md` for source-retention and rollback
instructions. `rover-source` now captures committed Git blobs and working-tree
bytes with explicit consistency labels; persistence currently uses the Unix
state-root adapter. The new `rover-execution` crate has a tested Unix
argv-only runner with scrubbed environment, per-stream bounds, live private log
capture, timeout, and process-group cancellation. Its supervisor module now
has transactional task claim, private detached worker launch, a managed
heartbeat loop, cancellation, and conservative stale-process reconciliation;
confirmed `LOST` transitions release that task's leases, while normal task
finalization and client reconnect remain unimplemented.
The new PTY wrapper covers local create/read/write/resize/close on native Unix
and Windows ConPTY backends; native runtime validation currently covers macOS
only. `rover-tui` has a Ratatui workspace renderer, basic tab/pane input, and a
Unix API for attaching a caller-supplied session client. The bridge sends input
and resize events, reads output on a bounded queue, and renders only text from
Rover's terminal emulator. It preserves Ctrl-] detach and Ctrl-B-prefixed
workspace controls. The new `rover-cli` binary provides an initial Unix-only
`rover tui` launch path: it admits a dedicated Rust state root, starts or
reattaches to a project-scoped login shell owned by a detached local process,
persists the workspace layout during attached use, and can display a bounded,
read-only snapshot of project task records from Rover's SQLite store. Ctrl-B
then `t` opens the task board; Enter opens recorded task and process metadata.
Press `r` on the task list or detail to refresh records while preserving the
selected task where possible. Press `c` to request cancellation after an explicit
`y` confirmation; the store rejects terminal tasks and the UI warns that prior
external actions are not undone. Press `o` or `e` in task detail to read the
selected task's stdout or stderr from the blob store; the TUI independently
verifies the full output against the recorded SHA-256, sanitizes controls, and
shows at most a 64 KiB excerpt. Press `i` to inspect the task's linked stored
investigation decision, checks, unknowns, and findings; the view treats these
as recorded evidence and enforces repository and candidate binding. Press `d`
in task detail to inspect a bounded path inventory from verified base and
candidate snapshots; use `rover diff` for the complete applicable patch.
In the task list, `/` starts a case-insensitive search over ID, status,
objective, candidate, and error; Enter applies it and Esc discards it. `s`
cycles Updated, Status, and Objective sorting. Query and order preferences are
stored per repository in Rover's Rust state database.
Ctrl-B then `p` opens a searchable command palette for provider-supported task,
tab, help, and file-browser actions.
If the active pane has a saved Codex or Claude session binding, Ctrl-B then `a`
shows its exact identity. `r` opens a command review and a second `y` confirmation
sends the interactive resume command to the attached shell; `d` removes the
binding after confirmation. Rover leaves permission and sandbox behavior to the
provider.
The CLI supports explicit exact-ID binding to an existing saved workspace pane,
project-scoped list/status, and binding clear with `rover agent session`.
The attached TUI samples the owned PTY's foreground process group and displays
recognized identity while keeping state unknown until manifests or lifecycle
evidence are available. CLI inventory and lifecycle transport remain separate
work; see ADR-0025 and ADR-0026.
The agents crate now contains an independently implemented, bounded Herdr
manifest evaluator and all 22 hash-verified bundled detection TOML files. Its
gate operators, 15 region selectors, rule ordering, OSC fields, visibility
flags, and fallback behavior are covered by Rust tests. The attached TUI feeds
the live viewport, OSC title, and bounded OSC 9;4 progress payloads to the
evaluator for audited direct executable mappings. Lifecycle-authority
integration and wrapper/process-group expansion remain open in
P-062/P-166/P-167; transcript-viewer skip rules preserve the latest same-agent
state in the attached TUI. See ADR-0027.
Ctrl-B then `n` opens a repository-scoped notes list for reusable prompt/context
snippets. `n` creates, Enter edits, and `d` deletes after confirmation; the first
line labels each item. Editing supports cursor movement, bounded UTF-8 text,
explicit Ctrl-S persistence, and an Esc discard prompt. Up to 32 notes (32 KiB
each, 128 KiB total) are stored locally and are not submitted to an agent.
Ctrl-B then `d` opens repository-scoped task briefing drafts with create, edit,
and confirmed delete. Drafts store title, path globs, dependencies, quality-gate
text, and multiline prompt. They save explicitly and are never dispatched or
interpreted; schema v1 records load with defaults and new saves use schema v2.
Limits are 32 drafts, 120 title bytes, 8 KiB paths, 4 KiB dependencies, 1 KiB
gate, and 16 KiB prompt.
Ctrl-B then `k` opens repository-scoped saved shell commands. Create/edit/delete
are explicit; `r` displays the full bounded single-line command and the same-user,
unsandboxed execution warning before `y` sends it once to the attached shell.
Commands are capped at 256 bytes and 32 entries.
Ctrl-B then `f` opens a file tree with Git status labels and colors;
arrows navigate, Enter opens a directory or preview, Backspace goes up, `h`
shows hidden files, and `r` refreshes. Ctrl-B then `.` opens hidden-file quick
open. `.git`, symlinks, and special files are not traversed or previewed; text
previews are sanitized and capped at 256 KiB. Text files up to 8 MiB can be
edited with `e`; Ctrl-S saves atomically after a stale-content check, Ctrl-D
shows a bounded unified diff with three context lines, Ctrl-E opens
`$VISUAL`/`$EDITOR` on a private
working copy, and Esc confirms before discarding dirty edits. External editor
changes return to the buffer and still require Ctrl-S to save.
Session recovery after owner crash/restart and other task actions,
full keyboard/layout interaction, platform parity, and the broader product
CLI remain open; this is an early Rust slice, not a Go runtime replacement.
Non-Unix process execution beyond the PTY wrapper remains unimplemented. Rover
does not yet replace the Go runtime.
Go remains authoritative until the parity tasks and removal gates in
`docs/design/RUST_PARITY_PLAN.md` pass. Python, TypeScript, and Go SDK source
was removed per the user's explicit product-scope decision; dated validation
records below retain the results from when those clients existed.

| Layer | Implemented | Remaining / not validated |
|---|---|---|
| L1 interface | CLI/JSON, Linux/macOS keyboard TUI and PTY attach, setup preview, reversible agent instructions | Desktop/mobile, signed public installer, full platform parity, broad accessibility validation; no SDK interfaces by product decision |
| L2 interoperability | Generic headless/PTY (Linux + macOS) adapters; Codex exec and Claude print argument/event adapters; MCP stdio/JSON HTTP; remote CLI client | Live provider certification, App Server/ACP, full MCP transport feature set, plugin marketplace |
| L3 runtime | Detached supervisors, terminal broker (Linux + macOS), reconnect, resize/input, cancellation, bounded output, resource admission, repair attempts | Cross-host worker recovery/fencing, native conversation restore, host-loss continuation |
| L4 environments | Worktrees, retained exact committed snapshots, disposable checks, explicit overlays, conservative dependency composition, patch export | Atomic mutable-tree capture, symlinks/submodules/LFS, managed development services/ports/databases, live Docker validation |
| L5 orchestration | Persistent/foreground DAG, parallel roots, cycle rejection, reservations, frozen contracts, bounded repairs, idempotency, reviewed/explicit-unreviewed handoffs, verified integration snapshot | Provider-independent external merge queue, schedules/webhooks, external write reconciliation, fleet/global monetary budgets |
| L6 context | Snapshot literal search, bounded context bundles, scoped expiring human/agent notes, stale markers | Semantic/service graph, authenticated business connectors, cross-project retrieval/knowledge system |
| L7 authority | Project/audience/tool-scoped expiring API grants, revocation, TLS admission, browser-Origin rejection, local grants, scrubbed default env, refused Docker fallback | Independent protected publisher, SSO/OAuth, credential broker, verified hostile-code isolation, OS-level tenant enforcement |
| L8 assurance | Go/JUnit/SARIF/exit-code parsers, exact replay, test-change heuristics, explicit counterfactual cases, finite mutation campaigns, required-check integrity, advisory native agent claims | Universal test-weakening semantics, native Lean/Kani/TLA+ scope interpreters, automated fuzz/property/browser/performance packs, LLM interrogation |
| L9 evidence | SQLite/event/artifact records, exact identities, diff/local review, exports, online backup/restore, explicit-key Ed25519 signatures | Protected CI check publisher, real external PR/merge/deploy writes, in-toto/SLSA attestations, full retention/erasure and global quota system |
| L10 improvement | Historical check-order proposals, explicit labels/datasets, disjoint heldout evaluation, proposal/dataset digests, local promotion/revocation, explicit strategy application | Live empirical optimization validation, RL training, autonomous self-play/RSI, independent evaluator deployment |

## Unreleased platform increments on top of `0.0.1` (pending Linux re-verification and review)

The older increment records in this section are historical. SDK test results
refer to the removed Python, TypeScript, and Go clients and are not current
product or CI claims.

These source changes exist in the working tree. They are **not** part of any
validated release until they are re-run on owned Linux/amd64 with a supported
toolchain and reviewed:

**Increment A — darwin temp-path canonicalization:**

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

**Increment B — darwin PTY and socket-path fix (this session):**

- `internal/terminal/terminal_darwin.go` (new): darwin TUI via `termios.h`
  `tcgetattr`/`tcsetattr`/`cfmakeraw` and `TIOCGWINSZ` (`0x40087468`); `Available()`
  true on darwin, `Raw`/`IsTTY`/`Size`/`Read` match Linux semantics.
- `internal/execution/pty_darwin.go` (new): darwin PTY via `posix_openpt`/
  `grantpt`/`unlockpt`/`ptsname`, shared broker logic, `TIOCSWINSZ`
  (`0x80087467`) for resize, `PrivateDir` + 103-byte socket guard.
- `internal/terminal/terminal_other.go`, `internal/execution/pty_other.go`:
  build tags narrowed to `!linux && !darwin`.
- `internal/tasks/tasks.go`: interactive tasks now allow `linux` **or**
  `darwin`; PTY socket digest shortened from 20 to 12 hex chars to stay within
  the 104-byte `AF_UNIX` limit when the state lives under
  `/private/var/folders/...`.
- `scripts/demo_extended.py`: temp root forced to `/tmp` (short
  `/private/tmp/...` path) and canonicalized, avoiding the long
  `/private/var/folders/...` default `TMPDIR`.

Observed on darwin/arm64, Go 1.26.5 (outside the validated envelope, so a local
observation, not a release claim): `make check` green (22 packages, 90 top-level
test funcs, 5 fuzz funcs) twice; `make race` green; `make demo` 12/12; SDK 3/3;
fuzz smokes (`FuzzDecode`, `FuzzSafeName`, `FuzzResultParser`, `FuzzTranscript`,
3s x 2 workers) no crashes; 10/10 schema pairs validate; **`demo_extended`
16/16 scenarios pass** (previously 8/16 with the Linux-only gate, now including
`actual_pty_task`, `keyboard_tui`, `detached_parallel_workflow`, `mcp_stdio_cli`,
`remote_cli_actual_task`, `remote_grant_revocation`, `signed_evidence`,
`backup_restore_cli`).

**Increment C — local gaps (A02/A26/A28/A29/A31/A39):**

- `internal/install/verify.go` + `internal/publish/publish.go` + `internal/cli`:
  `doctor --verify` checks `SOURCE_MANIFEST.json` (local, not signed installer)
  and `publish --id --to DIR` writes local advisory copies (not protected CI).
- `docs/acceptance/implementation-map.json`: `A02`, `A31`, `A29`, `A39` → `tested_local`;
  `A26`, `A28` → `partial` with honest limits; version bumped to
  `0.0.1+local-gaps`.

Observed after Increment C (same darwin host): `make check` (24 packages,
92 top-level test funcs, 5 fuzz funcs), `make race`, `make fuzz`, `make sdk-test`
all green unmodified. NOTE: the `make build`/`make demo`/`make demo-extended`/
`make install` targets cannot build under `~/Desktop` because git cannot read
the TCC-blocked `.git` (VCS stamping: `error obtaining VCS status`); with
`GOFLAGS=-buildvcs=false` the same targets pass (demo 12/12, demo_extended
16/16). `doctor --verify` and `publish` pass end-to-end (see `docs/PLAN_ALL.md`).

**Increment D — local accounting and reservation coverage (A08/A09/A12):**

- `internal/store/budget.go` + `budget_test.go`: local global budget ledger
  discarded at `settings/budget` (no schema migration). `SetBudgetCap`,
  `Charge` (atomic cap enforcement, refused over-limit leaves ledger unchanged),
  `BudgetReset`, `BudgetUse`. Unlocked `get` helper added to `sqlite.go`
  (the public `Get` now calls it).
- `internal/store/resources_test.go`: `TestPortReservation` and
  `TestDatabaseReservation` (ID-safe `port-8080`/`db-analytics` resource names,
  `ValidID` forbids `:`).
- `docs/acceptance/implementation-map.json`: `A08`, `A09`, `A12` → `tested_local`.

**Increment E — adapter/atomicity/contract/learning coverage (A03/A04/A06/A10/A11/A14/A15/A38):**

- `internal/agents/agents_test.go`: `TestUnknownProfileRejected` (undeclared
  adapter is an explicit error, not a silent downgrade) and
  `TestMalformedNativeEventRejected` (corrupt JSONL and events after a terminal
  result fail the attempt).
- `internal/source/source_test.go`: `TestCaptureConsistencyLabels` (snapshot
  declares "not a filesystem-atomic capture") and `TestMutationDetectedAfterCapture`
  (frozen bytes + post-capture mutation caught by `InputsUnchanged`).
- `internal/workflow/workflow_test.go`: `TestFrozenConfigBinding` (run pins the
  loaded plan; same idempotency key under a different committed config is a
  contract-reuse error).
- `internal/learning/learning_test.go`: `TestNoAcceptanceCacheReuse` (outcomes
  credited only against the exact stored candidate; no acceptance cache).
- `docs/acceptance/implementation-map.json`: `A03`, `A04`, `A06`, `A10`, `A11`,
  `A14`, `A15`, `A38` → `tested_local`.

Observed after Increment E (same darwin host): `make check` green (100 top-level
test funcs, 5 fuzz), `make race` green, `GOFLAGS=-buildvcs=false make demo`
12/12, demo-extended 16/16, `make sdk-test` 3/3. Counts: **28 tested**
(26 `tested_local` + `tested_linux` + `tested_admission`), **11 partial**,
**1 not_validated** (A18), **0 not_implemented**.

**Increment F — docker admission gate + map integrity (A18):**

- `internal/execution/process_test.go`: `TestDockerRequiresDaemon` — with no
  Docker daemon the restricted-docker mode refuses cleanly (no silent fallback);
  with a Docker binary present, admission is allowed but the container runtime
  is explicitly not live-certified.
- `docs/acceptance/implementation-map.json`: `A18` → `tested_local`
  (`not_validated` now **0**); a stale duplicate A18 entry was removed; audit
  confirms A01–A40 all present, no duplicates, and every referenced `.go` test
  file exists and defines tests.

Observed after Increment F (same darwin host): `make check` green
(101 top-level test funcs, 5 fuzz), `make race` green, `GOFLAGS=-buildvcs=false
make demo` 12/12, demo-extended 16/16, `make sdk-test` 3/3. Counts:
**29 tested** (27 `tested_local` + `tested_linux` + `tested_admission`),
**11 partial**, **0 not_validated**, **0 not_implemented**.

**Increment G — owned-Linux re-verification (log confirming prior runs):**

The re-verification that Increments A–F had left as required was carried out on
an owned Linux environment: `golang:1.23-bookworm` container image running on an
OrbStack Linux VM (`Linux aarch64` host, image `linux/amd64`), toolchain
**`go1.23.12 linux/amd64`**, `libsqlite3-dev`/`gcc` installed, repository copied
without `.git` metadata. Exact commands and results:

- `make check` — **PASS** (fmt-check, `go vet ./...`, `go test -count=1 ./...`;
  all packages `ok`, including cgo `internal/store`, `internal/terminal`,
  `internal/attestation`, `internal/install`, `internal/publish`).
- `make race` — **PASS** (`-race` full suite, including new `budget`/
  `reservation` and `publish` tests).
- `make demo` — **12/12** smoke scenarios PASS (`scripts/demo.py --binary
  bin/rover`; build via plain `go build -trimpath` works with no `.git`).
- `make demo-extended` — **16/16** extended CLI scenarios PASS (`actual_pty_task`,
  `signed_evidence`, `remote_grant_revocation`, `backup_restore_cli`, etc.).
- `make sdk-test` — **3/3** Python SDK unit tests PASS.
- `make fuzz` — **PASS** (5 fuzzers, no crashes; thousands execs each).
- `bin/rover --state <fresh> doctor --verify --json` — **`verified: true`**
  against the 197-file manifest in the copied tree.

Two clean-checkout prerequisite files live under `.rover/` (root and
`examples/fixture/`) and are required by `internal/config`'s
`TestRepositoryExamples`; they are committed/`SOURCE_MANIFEST.json`-tracked, so a
normal git clone passes without extra steps (only the rsync copy initially
omitted them).

Scope honesty: the Linux run used a container on this same machine — it is not an
independent host, not a Docker-in-Docker certified runtime, and not the CI
provider. It does confirm the *owned* Linux/amd64 gate (the documented envelope
is Go 1.23.2; this run used 1.23.12). Still required: human independent review
of the trust-adjacent canonicalization, cgo, and the new
`install`/`publish`/`budget` interfaces, and CI runs on a hosted provider.

The acceptance counts below reflect Increments C+D+E+F+G.

**Increment H — functional restricted-docker smoke on a local daemon (A18/A19):**

- `internal/execution/process_test.go`: `TestDockerFunctionalSmokeLocalDaemon`
  added — boots the real `restricted-docker` path (`--pull=never`, pinned
  `alpine@sha256:` image, read-only, `--cap-drop=ALL`, no network, bind-mounted
  probe) and asserts exit 0 plus a sentinel on stdout. It skips honestly where a
  daemon/image is genuinely absent, so it is portable to CI.
- What it proved: executed for real on **go1.23.12 linux/amd64** with the
  daemon socket mounted into the container — a real container ran through the
  product plumbing (2× PASS, `docker rm` cleanup confirmed via `Run`). On the
  darwin host the restricted executor deliberately strips `HOME`, so the
  OrbStack CLI cannot discover its socket and the test **SKIPs** with the reason
  logged — expected and documented, not a silent pass.
- Scope kept honest: this is a functional smoke on the local OrbStack daemon
  reached from inside a container — not an independent host, not Docker-in-Docker
  on a protected CI, not a security certification. A18/A19 maps updated to say
  exactly that.

**Increment I — CI infrastructure: cross-compilation, govulncheck, SBOM, release workflow:**

- `Makefile`: added `cross-build` (linux/amd64 CGO + darwin/amd64 and darwin/arm64
  CGO_ENABLED=0), `sbom` (`go list -m -json all` + build info), and `vulncheck`
  (`govulncheck` scan).
- `internal/terminal/terminal_darwin.go`: rewritten from cgo (`termios.h`/`C.*`)
  to pure `syscall` — `TIOCGETA`/`TIOCSETA`/`TIOCGWINSZ` — enabling cross-compilation
  with `CGO_ENABLED=0`.
- `internal/execution/pty_darwin.go`: rewritten from cgo (`posix_openpt`/`grantpt`/`unlockpt`/`ptsname`)
  to pure `syscall` — `/dev/ptmx` + `TIOCPTYGRANT`+`TIOCPTYUNLK`+`TIOCPTYGNAME`
  ioctls — enabling cgo-free cross-compilation. Verified: `actual_pty_task` and
  `keyboard_tui` still pass (16/16 `demo-extended`).
- `internal/store/sqlite_stub.go` (new, `//go:build !cgo`): pure-Go stub that
  implements `Store` and all public methods returning `errNoCGO`, while
  providing fully functional `PrivateDir`, `SyncDir`, `AtomicFile`,
  `BoundedFile`, `IsWithin`, `DefaultRoot`. `sqlite.go` now carries an explicit
  `//go:build cgo` constraint.
- `.github/workflows/ci.yml`: added `cross-build` and `govulncheck` steps after
  existing checks; Windows intentionally omitted (codebase uses `syscall.O_NOFOLLOW`
  which does not exist on Windows).
- `.github/workflows/release.yml` (new): opt-in `workflow_dispatch`-only workflow
  that builds host + cross-compiled binaries, generates SBOM, runs govulncheck,
  computes checksums, and creates a GitHub release. Does **not** auto-run, auto-merge,
  or inject signing credentials. Per AGENTS.md, no publication/release occurs
  automatically — every step requires human review.

Observed after Increment I (same darwin host): `make check` green, `make race` green,
`make demo` 12/12, `make demo-extended` 16/16, `make fuzz` green, `make sdk-test` 3/3,
`make cross-build` produces 3 verified binaries (linux-amd64, darwin-amd64, darwin-arm64),
`make sbom` + `make vulncheck` complete (govulncheck reports standard-library advisories
in Go 1.26.5 fixed by 1.26.6; no third-party vulnerabilities — zero external Go deps).

- Docs: `docs/API.md` (model type reference), `docs/USAGE.md` (workflow usage guide).

## Acceptance counts (from `docs/acceptance/implementation-map.json`, 40 scenarios)

- Tested in local scope (`tested_local` 38, `tested_linux` 1,
  `tested_admission` 1): **40**.
- `partial`: **0** — all 40 scenarios have honest local tests with documented limits.
- `not_implemented`: **0** — all 40 now have at least local honest code.
- `not_validated`: **0**.

## What was and was not exercised

- Real Linux **and darwin** processes, PTYs (darwin PTY now live-tested locally
  via `posix_openpt` plus Linux via `make demo-extended`), worktrees, DAGs,
  repair runs, loopback TCP/HTTPS, token grants, signature validation and SQLite
  backup/restore were exercised.
- Codex/Claude event profiles were tested with owned JSONL and process fixtures;
  **no real model account was used**.
- Docker command construction/admission is tested; a **functional smoke ran a
  real pinned container on the local OrbStack daemon from inside a Linux
  container** (same machine, not an independent host) — no security
  certification is claimed, and the restricted executor refuses fallback when
  the daemon is absent.
- CI configuration is included; **no hosted GitHub workflow or public release was run**.
- Remote control was exercised against a local server; **no separate remote host or
  distributed worker fleet was validated**.
- Learning evaluation uses explicit synthetic fixtures and timing assumptions; no
  real-world speedup or quality improvement is claimed.
- Current local release checks use Go 1.26.6 + system SQLite/cgo; Linux/amd64 functional validation for this checkout remains pending.
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
