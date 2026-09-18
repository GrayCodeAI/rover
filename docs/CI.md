# CI/CD Reference

## Source validation — `.github/workflows/ci.yml`

Triggers on `push`, `pull_request`, and `workflow_dispatch`. Runs on
`ubuntu-24.04` with Go stable and system SQLite/C toolchain installed.

### Steps

| Step | Make target | What it checks |
|------|-------------|----------------|
| Format, vet, tests | `make check` | `gofmt -l`, `go vet`, `go test ./...` (22 packages) |
| Race detector | `make race` | `go test -race ./...` — data race detection |
| Fuzz campaigns | `make fuzz` | `FuzzDecode`, `FuzzSafeName`, `FuzzResultParser` (3s each) |
| Demo smoke | `make demo` | 12 end-to-end CLI scenarios (no model/network) |
| Extended scenarios | `make demo-extended` | 16 scenarios: PTY, TUI, workflow, MCP, remote, backup |
| SDK tests | `make sdk-test` + inline | Python 3/3, TypeScript 3/3, Go 2/2 SDK tests |
| Cross-compile | `make cross-build` | linux/amd64 (cgo), darwin/amd64 + darwin/arm64 (cgo-free) |
| Vulnerability scan | `make vulncheck` | `govulncheck` — scans for known CVEs |

### Cross-compilation matrix

| Target | CGO | SQLite | Notes |
|--------|-----|--------|-------|
| linux/amd64 | Enabled | System libsqlite3 | Host build, full functionality |
| darwin/amd64 | Disabled (CGO_ENABLED=0) | Stub (non-functional store) | Pure-Go, compiles and links |
| darwin/arm64 | Disabled (CGO_ENABLED=0) | Stub (non-functional store) | Pure-Go, compiles and links |
| windows/amd64 | — | — | **Not supported** — uses `syscall.O_NOFOLLOW` |

## Release — `.github/workflows/release.yml`

Opt-in via `workflow_dispatch` only. Does **not** trigger on push or PR.
Requires manual review of all build artifacts and vulnerability scan results
before publishing.

### Release inputs

| Input | Description | Default |
|-------|-------------|---------|
| `version` | Version tag (e.g. `v0.2.0-alpha.2`) | (required) |
| `draft` | Create as draft release? | `true` |

### Release steps

1. Checkout source
2. Setup Go stable
3. Build host binary (`make build`)
4. Cross-compile darwin binaries
5. Generate SBOM (`make sbom`)
6. Run govulncheck (`make vulncheck`)
7. Run all checks (`make check`, `make race`)
8. Compute SHA-256 checksums
9. Create GitHub release with artifacts

**No automatic publication, merge, release, deployment, or credential upload
occurs.** The release workflow requires explicit `workflow_dispatch` trigger.
