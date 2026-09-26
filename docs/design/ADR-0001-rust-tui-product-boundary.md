# ADR-0001: Rust CLI/TUI product boundary

- Status: accepted; SDK removal implemented, Rust port in progress
- Date: 2026-09-24
- Owners: Rover maintainers

## Context

The requested destination is a Rust implementation of Rover that adopts the
capabilities of Herdr, Luvus, and Orca ADE. The selected product interfaces are
the command-line interface (CLI) and terminal user interface (TUI). The user
explicitly requested removal of Rover SDKs and clarified: port all behavior to
Rust CLI/TUI, with upstream source reused only after audit.

Rover is currently implemented in Go, has Python, TypeScript, and Go CLI SDK
directories, and documents security/acceptance contracts that must survive a
rewrite. The upstreams have different languages, surfaces, revisions, and
license histories. Earlier indexed Luvus README and LICENSE snapshots differed;
the pinned `63cb482…` revision audited for this plan has Apache-2.0 in both.
Herdr's current license does not automatically apply to older tagged releases.

## Decisions

1. The product surface is CLI and keyboard-operated TUI. Do not add a separate
   desktop GUI, web UI, or mobile application. A capability can be represented
   through a CLI/TUI workflow only when its acceptance case documents the
   equivalent interaction and resulting behavior.
2. The Rust implementation is the target runtime. Rust crates, schemas, and
   protocols must be selected through recorded decisions and tested contracts;
   language choice alone does not establish compatibility or security.
3. Python, TypeScript, and Go SDK removal is authorized and should be completed
   as a separate bounded change. It does not authorize removal of the existing
   Go CLI/TUI runtime; that remains gated on Rust parity and migration.
4. Preserve existing Rover CLI, JSON, exit-code, configuration, acceptance,
   and security behavior unless a later, explicit compatibility decision names
   the change and its migration.
5. Upstream code may be reused only from an immutable, audited revision after
   license, copyright, NOTICE, asset, and dependency review. Public availability
   does not resolve contradictory license statements. Until resolved, use
   independently implemented behavior based on documented interfaces and do
   not import disputed source.
6. Port every in-scope behavior into Rust CLI/TUI. Read non-Rust GUI/mobile/cloud
   code when it is the authoritative behavior source, but do not add those
   applications to Rover. Reuse only individual eligible modules or assets after
   recording source commit/path/blob digest, license/NOTICE obligations and a
   Rust acceptance test. Implement independently whenever that evidence is not
   complete. Do not vendor whole repositories into the product.

## Consequences

- All feature claims require a traceable CLI/TUI acceptance case.
- GUI-only behaviors must have a validated terminal equivalent or be listed as
  unsupported; they must not be represented by a stub.
- SDK removal and Go runtime removal are separate gated changes.
- License ambiguity blocks source import, but does not block an independent
  implementation of the required behavior.
- This ADR does not approve dependency or license changes, external writes, or
  releases.

## References

- [Rust feature parity plan](RUST_PARITY_PLAN.md)
- [Rover command reference](../CLI.md)
- [Rover security model](../SECURITY_MODEL.md)
- [Rover acceptance map](../acceptance/implementation-map.json)
