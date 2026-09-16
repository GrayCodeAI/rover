# Rover 0.2.0-alpha.1 — observed validation

This release extends the previously delivered 0.1 alpha. It is **not** a completed
production implementation of every requirement in the ten-layer blueprint.
The results below are observations on owned local fixtures, not certification of
live coding providers, containers, remote hosts, or organizational deployment.

## Executed checks

| Check | Observed result |
|---|---|
| Build | `make build` succeeded |
| Formatting/static analysis | `gofmt` clean; `go vet ./...` passed |
| Race-enabled Go suite | **87 top-level tests, 56 subtests, and 13 fuzz seed cases** passed; 22 test packages passed |
| Go statement coverage | **53.9%**; coverage is not correctness or security assurance |
| Original CLI demonstration | **12 scenarios passed** |
| Extended CLI demonstration | **16 scenarios passed**, with actual pipes, PTYs, subprocesses, Git worktrees, SQLite, local networking, signing and restore |
| Repeated extended demonstration | Three consecutive retained runs passed; a later final run also passed |
| Python CLI client | **3 unit tests passed** |
| Schema examples | **12 examples validated** against supplied JSON Schemas; runtime validation adds constraints |
| Configuration fuzz smoke | **84,656** reported executions; passed |
| Source-path fuzz smoke | **31,613** reported executions; passed |
| Result-parser fuzz smoke | **72,670** reported executions; passed |
| Agent-transcript fuzz smoke | **81,116** reported executions; passed |

The four fuzz campaigns requested three seconds each with two workers. These are
short smoke campaigns, not exhaustive robustness checks. Fuzz executions are not
inflated into the ordinary test count. The 13 fuzz seeds are separately identified
from the five fuzz parent functions in the JSON test output.

## End-to-end scope

`scripts/demo_extended.py` exercised bounded repairs with retained failed attempts,
applicable snapshot diffs, replay, named counterfactual regression checks, explicit
mutation campaigns, context search, scoped notes, reversible agent instructions,
real interactive terminal input/output, keyboard-TUI exit, a detached parallel DAG,
MCP stdio, a separate remote CLI authenticated to a **localhost** HTTP server,
duplicate-key handling, grant revocation, explicit-key Ed25519 verification, and
SQLite/artifact backup and new-state restore.

The remote CLI submitted an actual detached fixture task. This is not a validation
of a cross-host worker fleet, SSH migration, WAN behavior, or remote-host fencing.
TLS, rejected origins, wrong/revoked tokens, cancellation, redirect refusal and
project scoping also have local automated tests.

All “agents” used in execution tests were deterministic owned fixtures. Native
Codex/Claude profiles have argv/event-parser tests only. No provider account, model
API key or real coding model was used. A fixture cannot certify provider behavior.

## Regressions found during this implementation

- Advanced CLI handlers initially were not connected to command dispatch; wiring
  and compile checks caught this before packaging.
- Overlapping flag setup registered `--json` twice; integration tests exposed the
  panic and duplicate registration was removed.
- Workflow duplicate handling misread the store's “already exists” return value.
  Real workflow tests caught it and idempotent submission was corrected.
- Detached workflows failed when HOME was absent: default state resolution happened
  before the explicit `--state` flag was read. This was corrected and covered by
  `TestExplicitStateDoesNotRequireHome`.
- Learning promotion initially did not bind both the proposal and dataset to the
  evaluated content. Digest checks and tests now reject those changes and training
  candidate reuse in a claimed holdout.
- The first extended run had an unclassified mutation-result assertion failure.
  Its log is retained as `extended-0.log`; the cause was not conclusively isolated.
  Repeated later runs passed. This is not evidence that the workflow can never be flaky.

One combined validation shell exceeded the execution tool's time limit while
starting a fuzz campaign. Completed demo/config/source results were retained; the
interrupted parser campaign and agent campaign were rerun separately to completion.

## Environment and limitations

Observed environment: Go **1.23.2**, Linux/amd64, system SQLite through cgo, Git,
a C compiler, Python 3. No external Go module downloads are required.

This compiler was available in the build environment; it is not a recommendation
for a new production deployment. Validate with a currently supported toolchain.
The supplied GitHub workflow selects `stable`, but hosted CI has **not** executed.
macOS/Windows runtime compatibility has **not** been established. Docker arguments
and admission have tests; no Docker daemon was available for live validation.

Local execution remains same-user advisory, not a sandbox. The MCP/API grants scope
operations at the API boundary; they do not restrict a malicious process that shares
the controller's OS account. Local evidence signatures establish key-attributed
integrity, not independent acceptance or SLSA/in-toto compliance. Backups are sensitive
plaintext. Restore revokes grants and marks active execution lost; it does not restore
live processes or mutable worktrees.

Full native agent protocols/ACP, remote worker fleet management, a protected CI
publisher, production delivery, broad formal verification adapters, external
connectors and model-training/RL/RSI remain unimplemented or unvalidated. See
`STATUS.md` and the scope-qualified original 40-case acceptance implementation map.

## Artifacts and identity

`race.jsonl`, coverage, static/build logs, fuzz logs, original/extended demo logs,
SDK and schema results are retained beside this report. `summary.json` and
`tested-code-manifest.json` bind the recorded test summary to current Go code.
A separate `package-check.json` records checks performed on a freshly extracted
source package. Documentation/packaging-only edits do not change the tested Go-code
identity. None of these local records establish trust against a malicious host.

## Fresh-package validation

The staged source was archived, extracted to a separate directory without its Git
metadata, built, and tested there. Every one of the 76 Go-source/module identities
matched `tested-code-manifest.json`. The extracted package passed ordinary Go tests,
its build, all 16 extended CLI demonstration scenarios, and the 3 Python client tests.
The final package adds these validation records and documentation corrections only;
its Go source is the same tested content. See `package-check.json`.
