# Rover 0.0.1 — hosted CI validation record

`v0.0.1` is an annotated tag on commit
`158d9d5eb79fe2763806c7a5f87021c1919d2d47` (tag object `efd450d6`). The GitHub
release of the same name, published 2026-09-16, is source-only: it has **no
binary assets**, checksums, or attestations.

This record copies what GitHub Actions reported. It is not a certification of
live coding providers, containers, remote hosts, or production readiness. Earlier
local validation of the same code under its former `0.2.0-alpha.1` label is kept
as history in [../v0.2.0/REPORT.md](../v0.2.0/REPORT.md).

## Tag run

- Workflow: `source-validation` (`.github/workflows/ci.yml` at the tag), event
  `push` of tag `v0.0.1`
- Run: <https://github.com/GrayCodeAI/rover/actions/runs/35097554082>
- Runner: GitHub-hosted `ubuntu-24.04` (linux/amd64), system `libsqlite3-dev`,
  Go `stable` via `actions/setup-go`, which resolved to **go1.27.1** (the run
  log prints `go version go1.27.1 linux/amd64`). The tag workflow did not pin a
  toolchain; later commits pin Go 1.26.6 as the minimum.
- Job `linux`: started 2026-09-16T12:44:08Z, completed 2026-09-16T12:46:23Z,
  conclusion **success**

| Step at the tag | Command | Result |
|---|---|---|
| Format, vet, tests | `make check` | success |
| Race detector | `make race` | success |
| Short parser fuzz campaigns | `make fuzz` | success |
| Real detached fixture workflow | `make demo` | success |
| Extended terminal, workflow and protocol scenarios | `make demo-extended` | success |
| Python CLI client | `make sdk-test` | success, "Ran 3 tests" (SDK since removed) |
| TypeScript CLI client | `node --test sdk/typescript/test/*.test.ts` | success (SDK since removed) |
| Go CLI client | `go test ./sdk/go` | success (SDK since removed) |

The version-surface, source-manifest, cross-build and `govulncheck` gates did not
exist in the workflow at the tag, so this run does not cover them.

## Later runs on `main` (unreleased)

The tip of `main` before the Rust workspace landed, commit
`2cd2337f685e14fe161a6e7d68296fb84cb594ec`, passed on 2026-09-23 in run
<https://github.com/GrayCodeAI/rover/actions/runs/35936166378> on `ubuntu-24.04`
with pinned go1.26.6. That run also ran `make toolchain-check`,
`make version-check`, `make manifest-check`, `make cross-build` and
`make vulncheck`, all **success**. Those commits are not part of the `0.0.1`
release.
