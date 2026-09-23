# Detailed plan to implement all remaining gaps — honest, end-to-end, verifiable

This plan covers every one of the 40 acceptance IDs and the 9 ROADMAP gates.
It does not invent live provider, Docker daemon, remote-host, SSO, or hosted-CI
results. For each gap it states: domain owner, reviewed contract, regression
test, and exact limitation that remains until the external dependency is supplied.

## Baseline (already done, verified on darwin/arm64 go1.26.5)

- Increment A: darwin temp-path canonicalization (`store`/`archive` + harness)
- Increment B: darwin PTY/terminal (`posix_openpt`, `termios`) — `demo_extended`
  now 16/16 on darwin (was 8/16 Linux-only)
- Increment C: local gaps — `doctor --verify` (A02), `publish --id/--to` (A28/A31),
  local fencing docs (A29), formal-scope labelling (A26), absence-by-design (A39)
- `make check` green (24 pkgs, 90 top funcs, 5 fuzz), `make race` green,
  `make fuzz`/`make sdk-test` green; demo 12/12 and demo_extended 16/16 via
  `GOFLAGS=-buildvcs=false make demo` (the TCC-blocked `.git` under `~/Desktop`
  breaks VCS stamping on the plain `make build` path)
- `STATUS.md` updated per increment with honest Linux-reverification gate

## Acceptance matrix → implementation work

| ID | Status now | Plan (owner) | Contract | Test | Honest limit |
|---|---|---|---|---|---|
| A02 installer trust | **done** `tested_local` | `doctor --verify` checks `SOURCE_MANIFEST.json` hash/size (owner: `internal/install`, `internal/cli`) | No network download; verifies local artifact only | `install/verify_test.go` + `cli_test.go` | Not a signed public installer/update channel |
| A03/A04 capability/protocol | **done** `tested_local` | Adapter negotiation harded: unknown profile → explicit error, malformed JSONL / events-after-terminal → failed attempt (owner: `internal/agents`) | No live provider | `agents_test.go` (`TestUnknownProfileRejected`, `TestMalformedNativeEventRejected`) + `FuzzTranscript` | Live-provider certification still absent |
| A06 host loss | **done** `tested_local` | Stale-heartbeat local fencing via `Reconcile` (`DefinitelyGone`, marks LOST; no implicit replay) | No host migration | `tasks_test.go` (`TestLostWorkerReconciliation`, `TestQueuedLaunchedProcessCanBeReconciled`) | No cross-host failover |
| A08/A09 budgets/pressure | **done** `tested_local` | Local global budget ledger: `SetBudgetCap`, atomic `Charge`, `BudgetReset` (owner: `internal/store`) | Local only | `store/budget_test.go` (`TestGlobalBudgetAccounting`, `TestBudgetRefusesInvalidInput`) | Not bound to task execution; no fleet-wide billing |
| A10/A11 snapshot | **done** `tested_local` | Double identical reads frozen; consistency label explicit; post-capture mutation detected | — | `source_test.go` (`TestCaptureConsistencyLabels`, `TestMutationDetectedAfterCapture`) | Not filesystem-atomic TX |
| A12 isolation | **done** `tested_local` | Port/db named-reservation coverage (`port-8080`, `db-analytics`) in `Acquire`/`Release` (owner: `internal/store`) | Local | `resources_test.go` (`TestPortReservation`, `TestDatabaseReservation`) | No OS-level binding or container isolation |
| A14-A15 contracts/dup | **done** `tested_local` | Run pins loaded plan (`TestFrozenConfigBinding`); same-key different-contract is an error; concurrent `CreateOnce` idempotency | — | `workflow_test.go`, `sqlite_test.go` | No schedules/webhook integration |
| A18 worker creds | **done** `tested_local` | `TestDockerRequiresDaemon`: refuses cleanly (no fallback) when daemon absent; `TestDockerFunctionalSmokeLocalDaemon`: real pinned container on local daemon from Linux | No cred broker | `execution/process_test.go` | No trusted worker, no independent-host/security certification |
| A26 formal | **done** `partial` | `formal` parser labels exit-code as advisory; SARIF/JSON required for stronger claims (owner: `internal/assurance`) | No Lean/Kani | `TestFormalScopeHeuristic` | No proof replay |
| A28 delivery | **done** `partial` | `publish --id --to DIR` local advisory publisher (owner: `internal/publish`, `internal/cli`) — no push/merge | No external write | `publish/publish_test.go` | No real PR/merge/deploy |
| A29 stale worker | **done** `tested_local` | Local fencing: heartbeat timeout + `Reconcile` marks `LOST`, revokes leases (owner: `internal/tasks`) | Local | `TestStaleWorkerFencing` | No remote host fencing |
| A31 CI publisher | **done** `tested_local` | `publish` local advisory publisher (owner: `internal/publish`) | Not protected CI | `publish/publish_test.go` | Not independent CI |
| A38 cache reuse | **done** `tested_local` | Outcomes credited only against exact stored candidate evidence; no acceptance cache | — | `learning_test.go` (`TestNoAcceptanceCacheReuse`) | Full retention/RL still future |
| A39 destructive | **done** `tested_local` | Absent by design; `cli_test.go` asserts no merge/deploy tool exists | — | `cli_test.go` | Correct absence |
| A32-A37,A40 | partial | Local retention/learning/evidence coverage exists; external components (retention service, auto-updater, independent evaluator, dataset governance, security certification, full installer) need infra | — | existing | Full retention/RL/installer still future |

## ROADMAP gates → same mapping

1. Toolchain/OS: darwin PTY done; **Linux/amd64 re-run DONE** (`go1.23.12
   linux/amd64` in `golang:1.23-bookworm` on OrbStack VM: check/race/demo
   12/demo-extended 16/sdk 3/fuzz all green — see `verification_log` in the
   implementation map); Windows stub remains a stub
2. Restricted executor + CI publisher: Docker flags done; **functional smoke ran
   a real pinned container on the local daemon from Linux** (same machine, not
   certified); protected CI + independent host still need infra
3. Credential broker/SSO: interface stub only
4. Recovery/fencing: local `Reconcile` done; remote-host still needs infra
5. Schedules/webhooks/external delivery: local idempotency done; webhooks still need infra
6. Verifier packs: exit-code/SARIF/JUnit done; Lean/Kani still need tools
7. Connectors: literal search/bundles done; semantic connectors still need auth
8. Release signing/retention: Ed25519 + manifest done; public installer still needs infra
9. Evaluation: learning holdout done; live workflow eval still needs data

## Execution order

1. (**done**) **A02 + A31** — smallest honest verticals that produce visible evidence
2. (**done**) **A29 + A28** — fencing + local publish (no external host/network)
3. (**done**) **A26 + remaining assurance** — formal-scope labelling
4. (**done**) **A39** — absence-by-design regression test
5. (**done**) **A08/A09/A12** — local global budget ledger + port/db reservation coverage
6. (**done**) **A03/A04/A06/A10/A11/A14/A15/A38** — adapter negotiation, snapshot
   consistency/frozen-bytes, local fencing, frozen config binding, no-cache reuse
7. (**done**) **A18 + Increment G** — Docker admission gate + owned-Linux/amd64
   re-verification (container on same machine; go1.23.12, not an independent host)
8. **Remaining 11 partials (A16/A17/A23/A26/A28/A32/A33/A34/A36/A37/A40)** —
   every one needs external infrastructure (host, provider, CI, tools, installers,
   independent evaluator, dataset governance, security certification); local code
   and tests exist, live claims stay explicitly not validated until supplied
9. After each: `gofmt`, `go vet`, `go test ./...` (and `-race` where relevant), `python3 scripts/demo*`, schema check; update `STATUS.md` only with observed numbers

## Verification (3× as requested)

After all code: run in this environment (darwin/arm64, go1.26.6, no external Go modules):
- `make build` (or `GOFLAGS=-buildvcs=false make build` when checkout VCS metadata is unavailable)
- `make check` (fmt-check + vet + test)
- `make race` (`go test -race`)
- `GOFLAGS=-buildvcs=false make demo` (12 scenarios)
- `GOFLAGS=-buildvcs=false make demo-extended` (16 scenarios)
- `make sdk-test` (`python3 -m unittest discover -s sdk/python`)
- `make fuzz` (3s × 2: `FuzzDecode`, `FuzzSafeName`, `FuzzResultParser`, `FuzzTranscript`, `FuzzEnvelope`)
- schema validation 10/10
Each run reports exact pass/fail; no invented CI/provider results.

## What "done" means here

Local code for all 40 IDs will exist with tests and honest limit docs, but
production claims (live provider, Docker daemon, remote hosts, SSO, hosted CI,
SLSA, RL) remain explicitly **not validated** until the external dependency is
supplied. This matches `AGENTS.md`: no weakening of tests, no stub advertising
unsupported capabilities, no invented coverage/certification.
