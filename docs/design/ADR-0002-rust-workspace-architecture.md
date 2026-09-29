# ADR-0002: Rust workspace and process ownership

- Status: accepted for implementation foundation; dependency choices remain open
- Date: 2026-09-25
- Owners: Rover maintainers

## Context

Rover must retain its current CLI, JSON, exit-code, state, process supervision,
and evidence contracts while adding terminal workspaces and agent workflows.
The upstreams use multiple processes, protocols, and extension points. The
first Rust foundation must compile without implying that those features exist.
The typed core dependencies are reviewed in ADR-0003. SQLite and JSON record
encoding are reviewed separately in ADR-0004 and ADR-0005; other adapter
dependencies remain gated.

## Decisions

1. Use a Cargo workspace organized by domain boundaries. `rover-core` owns
   versioned domain types, validation, and canonical repository identity.
   `rover-store` owns database migrations, transactional record APIs, atomic
   agent/resource reservations, atomic local budget accounting, and rooted Unix
   blob/file operations.
   `rover-access` owns project and state
   scoped bearer grant issuance, authentication, and revocation. These are
   library foundations; CLI/TUI wiring and the remaining capabilities remain
   later work. `rover-execution` owns argv-only bounded local process
   execution under ADR-0009. Planned crate boundaries include
   `store`,
   `source`, `execution`, `terminal`, `agents`, `assurance`, `workflow`,
   `remote`, `plugins`, `tui`, and `cli`. Add each crate when its public
   contract and tests are ready; do not create capability stubs.
2. The eventual installed product remains one Rust `rover` executable. An
   internal supervisor role owns detached worker and terminal process
   lifetimes; CLI and TUI clients request operations over owner-private local
   IPC. The supervisor records ambiguous process loss as `LOST` rather than
   claiming recovery. Network listeners and remote execution are separate,
   explicit capabilities governed by the existing security model.
3. Keep CLI, TUI, and supervisor orchestration in separate crates/modules even
   if initially linked into one binary. This keeps rendering and input handling
   from becoming process or policy owners.
4. Keep the domain core small. Ratatui/Crossterm, a PTY and terminal
   emulator, SQLite, async runtime, crypto, SSH, and browser
   libraries each require a separate reviewed contract, supported-platform
   statement, license/dependency audit, and tests before addition. rusqlite is
   approved separately in ADR-0004.
5. The first compatibility slice ports Rover's ASCII identifier validation as
   a typed core value. This does not claim full model, JSON, hash, timestamp,
   or command parity.

## Alternatives considered

- A single crate for all behavior: rejected because terminal rendering, process
  lifetime, storage, and policy need separate owners and test surfaces.
- A library-only Rust replacement with no supervisor role: rejected because
  detached tasks and persistent terminal sessions need an explicit lifetime
  owner.
- Add the upstream dependency sets wholesale: rejected because dependency
  licenses, maintenance, platform scope, features, and Rover acceptance
  contracts have not been audited.

## Consequences

- Existing Go behavior remains authoritative until an individually mapped Rust
  capability passes its compatibility and security acceptance cases.
- There is no Rust CLI/TUI feature claim from the core crate alone.
- Future crates and dependencies require explicit review; unknown dependency
  licenses and unsupported platform claims fail closed.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Core model dependency decision](ADR-0003-core-identifiers-digests-and-time.md)
- [Current Rust dependency audit](RUST_DEPENDENCY_AUDIT.md)
- [Rust process execution decision](ADR-0009-rust-process-execution.md)
- [Compatibility baseline](RUST_COMPATIBILITY_BASELINE.md)
- [Security model](../SECURITY_MODEL.md)
