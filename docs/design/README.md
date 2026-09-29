# Design reference

This folder holds two kinds of material. Check which kind a file is before
relying on it. [STATUS.md](../../STATUS.md), the actual commands, and the tests
are authoritative for what Rover does today.

## Current decisions and plans (Rust port, in progress)

These documents govern the unreleased Rust port. Its preview binary is
`rover-rs` ([docs/RUST_PREVIEW.md](../RUST_PREVIEW.md)). Commands the ADRs write as
`rover tui` or `rover agent session ...` run as `rover-rs ...` until the cutover
in parity task P-164. An accepted ADR records a decision; it does not mean the
feature ships in a Rover release.

- [RUST_PARITY_PLAN.md](RUST_PARITY_PLAN.md): the staged port backlog,
  verification ledger, and the gates that must pass before Go is removed.
- [GO_RUST_STATE_MIGRATION.md](GO_RUST_STATE_MIGRATION.md): the opt-in Go-to-Rust
  state import (a store API, not yet a CLI command), source retention, and
  rollback.
- [RUST_COMPATIBILITY_BASELINE.md](RUST_COMPATIBILITY_BASELINE.md): an index of
  the Go command families and behavior the port must match.
- [RUST_DEPENDENCY_AUDIT.md](RUST_DEPENDENCY_AUDIT.md): the dated Rust dependency
  review. `make rust-deps-check` enforces the locked licenses and bundled notices.
- [UPSTREAM_FEATURE_MATRIX.md](UPSTREAM_FEATURE_MATRIX.md) and
  [UPSTREAM_SOURCE_AUDIT.md](UPSTREAM_SOURCE_AUDIT.md): Herdr, Luvus, and Orca
  ADE behavior references, pinned revisions, and exact-file reuse audits.

| ADR | Decision |
|---|---|
| [ADR-0001](ADR-0001-rust-tui-product-boundary.md) | Rust CLI/TUI product boundary |
| [ADR-0002](ADR-0002-rust-workspace-architecture.md) | Rust workspace and process ownership |
| [ADR-0003](ADR-0003-core-identifiers-digests-and-time.md) | Core identifiers, digests, and timestamps |
| [ADR-0004](ADR-0004-rust-sqlite-adapter.md) | Rust SQLite adapter |
| [ADR-0005](ADR-0005-rust-record-json.md) | JSON record encoding for Rust storage |
| [ADR-0006](ADR-0006-stack-safe-json.md) | Stack-safe deep JSON for Rust storage |
| [ADR-0007](ADR-0007-rust-rooted-file-operations.md) | Rooted file operations for Rust storage |
| [ADR-0008](ADR-0008-rust-project-identity.md) | Canonical repository identity and project scope |
| [ADR-0009](ADR-0009-rust-process-execution.md) | Rust local process execution |
| [ADR-0010](ADR-0010-rust-task-supervisor-state.md) | Rust task supervisor state transitions |
| [ADR-0011](ADR-0011-rust-pty-backend.md) | Rust native PTY backend |
| [ADR-0012](ADR-0012-rust-terminal-emulator.md) | Rust terminal screen core |
| [ADR-0013](ADR-0013-rust-session-broker.md) | Rust local terminal session broker |
| [ADR-0014](ADR-0014-rust-workspace-layout-model.md) | Rust terminal workspace layout model |
| [ADR-0015](ADR-0015-rust-tui-framework.md) | Rust TUI framework and terminal backend |
| [ADR-0016](ADR-0016-rust-task-briefing-drafts.md) | Repository-bound Rust task briefing drafts |
| [ADR-0017](ADR-0017-rust-task-brief-contract.md) | Rust task brief and dependency graph contract |
| [ADR-0018](ADR-0018-rust-task-plan-storage.md) | Rust project-bound task-plan storage |
| [ADR-0019](ADR-0019-rust-agent-profile-registry.md) | Rust agent profile registry |
| [ADR-0020](ADR-0020-native-agent-transcripts.md) | Native agent transcript parsing |
| [ADR-0021](ADR-0021-agent-detection-evidence.md) | Agent detection from explicit process and screen evidence |
| [ADR-0022](ADR-0022-agent-state-rollups.md) | Agent state rollups |
| [ADR-0023](ADR-0023-agent-lifecycle-authority.md) | Sequenced agent lifecycle authority |
| [ADR-0024](ADR-0024-agent-session-identity-and-resume.md) | Exact agent session identity and explicit resume |
| [ADR-0025](ADR-0025-agent-session-inventory-cli.md) | Project-scoped agent session inventory |
| [ADR-0026](ADR-0026-owned-pty-process-sampling.md) | Sampling foreground processes from owned PTYs |
| [ADR-0027](ADR-0027-herdr-compatible-screen-manifests.md) | Herdr-compatible screen manifest evaluation |

Each ADR's own "Status" line gives the exact scope that was accepted.

## Historical reference

- [Rover_Ten_Layer_Master_Plan.md](Rover_Ten_Layer_Master_Plan.md) is copied from
  the supplied design draft. It describes the desired complete ten-layer system,
  not features implemented in this alpha. Do not read its future commands,
  roadmap, or proposed acceptance specifications as released support.
