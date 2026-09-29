# Changelog

## Unreleased

- Track the Rust port workspace (nine crates, Rust 1.88.0 MSRV) as an
  unreleased preview. Its executable is `rover-rs`, not `rover`, so it cannot
  shadow the Go product binary, and `rover-rs --version` labels it a preview.
  Build and usage notes are in `docs/RUST_PREVIEW.md`.
- CI runs the Go product gates and the Rust preview gates as independent jobs.
  The Rust job is cached and keeps its CycloneDX SBOM as a run artifact.
- The release workflow attests build provenance (GitHub artifact attestations,
  keyless Sigstore signing) for every checksummed asset; verify with
  `gh attestation verify`.
- `make manifest-check` names the paths that make `SOURCE_MANIFEST.json` stale.
  The Rust dependency audit runs with the same `CARGO` as the Makefile.
  `.gitattributes` keeps the hash-verified license notices byte-exact.
- README, STATUS, SECURITY, SUPPORT, and GOVERNANCE now describe the public
  repository, hosted CI, the source-only `v0.0.1` release, and one platform
  matrix. `docs/validation/v0.0.1/REPORT.md` records the tag's hosted CI run.
- Remove the Python, TypeScript, and Go SDK source trees and their CI/Make
  targets per product direction. Historical validation records remain dated.
- `ProcessGroupIdentity` gains a `conflicting` field listing every equally
  ranked candidate when the group is ambiguous (additive; empty otherwise). The
  TUI now emits one `foreground_process` evidence entry per conflicting
  candidate instead of a single generic message, so an operator can see which
  agents disagreed rather than only that they did. Identity selection is
  unchanged: an ambiguous group still resolves to unknown.
- `rover-execution`: `parse_nul_terminated_argv` and the `MAX_PROCESS_ARG_*`
  bounds are now `cfg(target_os = "linux")`. They only ever served the Linux
  `/proc` reader — the macOS backend deliberately reports unknown argv rather
  than parse a lossy `ps` command line — so the wider gate left them dead on
  macOS.
- `rover-execution`: reject a negative pid parsed out of `ps` output instead of
  casting it to `u32`, and drop the unused `&self` from
  `foreground_process_group_details_for`.

## 0.0.1

- Establish the current public release identity and fail-closed version checks.

## 0.2.0-alpha.1

- Extend the existing alpha rather than replacing its history.
- Add Linux PTY input/attach/reconnect/resize and keyboard TUI.
- Add Codex exec / Claude print CLI adapters with bounded transcript interpretation;
  fixture-tested only, no real model execution claim.
- Add bounded repair attempts, named resource reservations and store admission limits.
- Add detached DAG workflows, exact dependency handoffs, conflict detection and
  reverified integration snapshots.
- Add SARIF, explicit counterfactual testcase verification, finite textual mutation
  campaigns, heuristic test-change detection and exact evidence replay.
- Add applicable snapshot patches, literal context search, bundles and expiring notes.
- Add MCP stdio plus scoped JSON-only HTTP, cancellation, TLS/grants and remote CLI.
- Add reversible, previewed agent instructions and a standard-library Python client.
- Add online SQLite backup/restore with artifact validation and grant revocation.
- Add Ed25519 evidence signatures against explicit trusted keys (not certification).
- Add check-order recommendation, disjoint holdout evaluation, explicit promotion,
  revocation, frozen evaluation inputs and opt-in strategy application.
- Fix duplicate workflow dispatch, HOME-free supervisor startup, stale workflow
  state on restore, duplicate CLI flag wiring and mutable evaluation promotion.
- Retain explicit Linux/toolchain/provider/Docker/CI/security limitations.

## 0.1.0-alpha.1

Initial local CLI, headless detached tasks, worktrees, snapshots, structured Go/JUnit
verification, SQLite evidence and same-user review. Historical validation is retained.
