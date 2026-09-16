# Observed validation report — Rover 0.1.0-alpha.1

This is the record of actual local commands, not a claim that the entire blueprint is complete.

## Results

| Check | Observed result |
|---|---|
| Build | `go build -trimpath -o bin/rover ./cmd/rover` succeeded |
| Formatting | `gofmt -l cmd internal` empty |
| Static checks | `go vet ./...` succeeded |
| Race suite | **50 top-level Test functions**, 56 subtests, 9 fuzz seed cases; no failed events across 7 tested packages |
| Statement coverage | **64.0%** across all Go packages; not a correctness or security score |
| End-to-end CLI smoke | **12 scenarios passed**, including a real detached subprocess and Git worktree |
| Bounded fuzz smoke | Three targets, 3 seconds requested per target, two workers each; no counterexample in these short campaigns |
| Example schemas | Two configuration and three task files validated against the supplied JSON Schemas |
| CI workflow | YAML syntax checked; NOT executed on GitHub |

The ordinary Go test run includes fuzz seed cases; campaign-generated inputs are counted
separately below, not inflated into the ordinary unit-test count.

- `config`: 28,964 reported executions; passed.
- `source`: 90,266 reported executions; passed.
- `assurance`: 84,025 reported executions; passed.

## Environment and limitations

- go version go1.23.2 linux/amd64
- git version 2.47.3
- System SQLite 3.46.1; cgo and system C library required.
- Python 3.13.5, Linux amd64.
- Toolchain/module downloads unavailable. Go 1.23.2 was the actual available compiler,
  not a recommendation for production. Supported-current-toolchain validation is pending.
- No Docker daemon, provider credentials, interactive PTY, hosted CI run, or production
  environment was used. Codex/Claude templates are not certified adapters.

The demo uses a deterministic owned fixture that repairs a seeded authorization defect.
It is not a model benchmark. The 40 original blueprint acceptance scenarios remain a
specification inventory; `docs/acceptance/implementation-map.json` records limited scope.

## Review and regression record

During hardening, an added directory-safety guard broke detached supervisor startup.
The full CLI demo found it (worker stayed QUEUED). The worker startup directory was
corrected, launch diagnostics and launched-PID recording were added, and the full demo
was rerun successfully. This is why unit/race success alone is not end-to-end completion.

## Tested code identity

Manifest SHA-256: `bc0abb7e2166633178631ab8b54acd6b559a735cd5332e8b1bc32c5a08bd6ca6`.

`summary.json` includes per-file hashes of all Go source and go.mod. Documentation-only
packaging changes do not alter that tested-code manifest. The raw JSON race-test log,
coverage files, fuzz logs, and smoke report are included alongside this document.

No source hashes or log files establish trust against a malicious same-user host.
No security certification or universal correctness claim is made.

## Rover verifying Rover

The compiled Rover executable captured its own committed source, loaded the committed
baseline configuration, and ran both checks in fresh materialized source copies.

Source commit: `7114d8daed5d093e2c4f067d332d277c610b6812`.

`unit`: PASS, 118 non-skipped Go test events (including subtests and fuzz seed/parent events).
`vet`: PASS, interpreted only as successful command exit.
Decision: **REVIEW_REQUIRED**, exit 3. No automatic acceptance, merge, or approval occurred.
The exact investigation is in `self-verify.json`. This self-check is an integration
demonstration; it does not constitute an independently administered trust boundary.
