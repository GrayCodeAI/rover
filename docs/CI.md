# CI/CD Reference

## Source validation — `.github/workflows/ci.yml`

Triggers on `push`, `pull_request`, and `workflow_dispatch`. Two independent
jobs run on GitHub-hosted `ubuntu-24.04` runners, so a failure in one never
hides the other's results.

### `go` job — the Rover product (Go 1.26.6, system SQLite/C toolchain, 20 min limit)

| Step | Make target | What it checks |
|------|-------------|----------------|
| Go toolchain | `make toolchain-check` | Go 1.26.6 or newer |
| Version contract | `make version-check` | `VERSION`, runtime, capability, documentation, and release metadata agree; removed SDK tree stays absent |
| Source manifest | `make manifest-check` | Every tracked source file matches its generated size and SHA-256 entry |
| Format, vet, tests | `make check` | `gofmt -l`, `go vet`, `go test ./...` (24 packages, 21 with tests) |
| Race detector | `make race` | `go test -race ./...` — data race detection |
| Fuzz campaigns | `make fuzz` | `FuzzDecode`, `FuzzSafeName`, `FuzzResultParser`, `FuzzTranscript`, `FuzzEnvelope` (3s each) |
| Demo smoke | `make demo` | 12 end-to-end CLI scenarios (no model/network) |
| Extended scenarios | `make demo-extended` | 16 scenarios: PTY, TUI, workflow, MCP, remote, backup |
| Cross-compile | `make cross-build` | linux/amd64 (cgo), darwin/amd64 + darwin/arm64 (cgo-free) |
| Vulnerability scan | `make vulncheck` | Pinned `govulncheck` v1.8.0 — scans for known CVEs |

### `rust` job — unreleased Rust port preview (Rust 1.88.0 MSRV, 30 min limit)

The Rust workspace under `crates/` is a preview gated by
`docs/design/RUST_PARITY_PLAN.md`; it is not part of any release.

| Step | Command | What it checks |
|------|---------|----------------|
| Toolchain | `rustup toolchain install 1.88.0 --profile minimal --component clippy --component rustfmt` | Pinned MSRV from `rust-toolchain.toml` |
| Build cache | `Swatinem/rust-cache` (SHA-pinned) | Restores `~/.cargo` and `target/rust-1.88.0`, keyed on `Cargo.lock`, `rust-toolchain.toml` and `.cargo/config.toml`; only `main` saves |
| Locked fetch | `cargo +1.88.0 fetch --locked` | `Cargo.lock` is complete; later steps run `--offline` |
| Format | `make rust-fmt-check` (via `rust-check`) | `cargo fmt --all -- --check` |
| Lint | `make rust-clippy` (via `rust-check`) | `clippy --workspace --all-targets --locked --offline -- -D warnings` with the workspace's `clippy::pedantic` and `unsafe_code = "forbid"` lints |
| Tests | `make rust-test` (via `rust-check`) | `cargo test --workspace --locked --offline` |
| Dependency audit | `make rust-deps-check` (via `rust-check`) | `scripts/test_*.py` unit tests, then `scripts/rust_dependency_audit.py --check`: every locked crate's SPDX expression and byte-exact bundled notice (SHA-256) in `licenses/` |
| Rust SBOM | `make rust-sbom` | CycloneDX 1.7 inventory of packages active on the supported targets, uploaded as the `rover-rust-sbom` run artifact (30-day retention) |

All Rust steps take `CARGO='cargo +1.88.0'`; the audit script honours the same
`CARGO` value. The Rust SBOM is not attached to releases because releases ship
only the Go binaries. Cold-cache duration on the hosted runner has not been measured
yet; record it here after the first run on `main`.

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
| `version` | Version tag (e.g. `v0.0.1`) | (required) |
| `draft` | Create as draft release? | `true` |

### Release steps

1. Checkout source
2. Setup Go 1.26.6
3. Validate the Go toolchain, release tag, `make version-check`, and `make manifest-check`
4. Build host binary (`make build`)
5. Verify the local source package (`rover doctor --verify`)
6. Build the declared target matrix (`make cross-build`)
7. Generate SBOM (`make sbom`)
8. Run govulncheck (`make vulncheck`)
9. Run all checks (`make check`, `make race`)
10. Compute SHA-256 checksums (`bin/checksums.txt`)
11. Attest build provenance for every checksummed asset with
    `actions/attest-build-provenance` (keyless Sigstore signing through the
    run's OIDC token; needs `id-token: write` and `attestations: write`)
12. Create GitHub release with artifacts

### Verifying a release download

```sh
sha256sum -c checksums.txt --ignore-missing
gh attestation verify rover-linux-amd64 --repo GrayCodeAI/rover
```

The attestation proves which workflow run, commit and repository built the
file. It does not prove the code is correct or secure. The existing `v0.0.1`
release is a source-only tag with no binary assets, so it has no attestations.

**No automatic publication, merge, release, deployment, or credential upload
occurs.** The release workflow requires explicit `workflow_dispatch` trigger.
