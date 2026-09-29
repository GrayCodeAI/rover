# ADR-0009: Rust local process execution

- Status: accepted for the P041 implementation slice
- Date: 2026-09-25
- Owners: `rover-execution` domain; security model remains owned by Rover maintainers

## Context

Rover's current executor starts explicit argv without implicitly inserting a
shell, clears inherited environment except for a documented base and caller
allowlist, creates private HOME/temp/cache paths, bounds retained stdout and
stderr separately to 1 MiB while counting all bytes, and supports timeout and
caller cancellation. On Linux and macOS, timeout/cancellation kills the
executor's process group. This is local advisory execution under the current OS
user, not a sandbox. Existing Go behavior remains authoritative until Rust
acceptance coverage closes the contract.

## Decision

1. Add a `rover-execution` crate as the process execution domain owner. It uses
   `std::process::Command`, passes argv directly, and clears the inherited
   environment before adding the documented environment and explicit pass-env
   names.
2. Use the existing audited `rustix` 1.1.4 dependency on Unix for safe process
   group termination and descriptor-rooted private output files. Enable only
   its already-reviewed `fs`, `std`, and `process` features. No new registry
   package is introduced.
3. Represent user cancellation with a cloneable atomic cancellation token;
   timeout and cancellation are checked while polling the child. Captured
   output retains at most 1 MiB per stream, counts all observed bytes, records
   truncation, hashes retained bytes, and persists exclusive mode-0600 logs.
   Exit codes and process-error text preserve Go's exit/signal cases; executable
   identity hashing reads at most the first 128 MiB, matching Go's bounded
   prefix hash.
4. Do not report a local process as sandboxed. Restricted Docker (tracked in
   P-100), PTY, detached supervision, and Windows process-tree termination
   remain separate contracts and are not claimed by this crate slice.
5. Process-tree cleanup is implemented and tested for Linux/macOS first.
   Unsupported targets must fail explicitly and advertise no process
   cancellation guarantee until an OS-specific adapter and tests exist.

## Alternatives considered

- `std::process::Command::output`: rejected because it buffers unbounded output
  and does not provide bounded live capture or cooperative cancellation.
- Shell-string execution: rejected because it changes argv interpretation and
  inserts an implicit shell.
- Add a new async/process dependency: rejected for this synchronous contract;
  the existing standard library plus audited `rustix` is sufficient on the
  initial Unix targets.
- Claim Windows process-tree cancellation from `Child::kill`: rejected because
  it only terminates the direct child and does not match Rover's timeout
  cleanup semantics.

## Consequences and acceptance

This ADR does not claim process launch is shipped. P041 is complete only after
compatibility tests cover argv validation, clean environment/pass-env, process
results, timeout, cancellation including descendants, output bounds, durable
logs, and unsupported target behavior. The complete Go-to-Rust acceptance map
and platform matrix remain release gates.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Rust dependency audit](RUST_DEPENDENCY_AUDIT.md)
- [Security model](../SECURITY_MODEL.md)
- `internal/execution/process.go`, `process_unix.go`, and `process_test.go`
