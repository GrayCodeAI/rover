# ADR-0011: Rust native PTY backend

Status: accepted for the local PTY lifecycle slice; multi-client brokering,
session persistence, terminal emulation, and native Windows runtime validation
remain open.

## Context

The pinned Herdr and Orca feature inventories require interactive terminal
processes on supported hosts. Rover's Rust execution crate previously had no
PTY API. The product boundary remains a Rust CLI/TUI; this is not a GUI or a
remote terminal service.

## Decision

- Use `portable-pty` 0.9.0, pinned in `Cargo.lock`. Its MIT license and
  transitive package licenses are included in `licenses/THIRD_PARTY_NOTICES.md`
  and checked by `make rust-check`.
- Use the crate's native Unix PTY implementation and Windows ConPTY backend.
  Rover's wrapper in `rover_execution::pty` exposes argv-only spawn, blocking
  byte input/output, dimension resize, child PID observation, and explicit
  close/reap. It clears the inherited environment, accepts only explicitly
  supplied valid environment pairs, and requires an absolute executable and
  existing absolute working directory.
- Reject zero or greater-than-1000 row/column dimensions. Pixel dimensions are
  not yet part of Rover's interface.
- Dropping or closing a session terminates and reaps the direct child and
  closes the PTY handles. This API does not claim to terminate arbitrary
  descendants, sandbox the child, retain scrollback, reattach a client, or
  persist a terminal across Rover restarts.
- Keep the third-party crate's unsafe platform code behind its audited API;
  Rover's workspace continues to forbid unsafe Rust.

## Evidence and remaining limits

Four native macOS tests exercise validation, child output, input round-trip,
resize, and close/reap. The full workspace cross-compiles for Linux x86_64.
The isolated PTY module plus `portable-pty` dependency cross-compiles for
Windows x86_64; full-workspace Windows compilation is currently blocked by
the macOS host's missing MSVC `lib.exe` and Windows SDK headers required by
unrelated bundled C dependencies. These checks do not prove native runtime
behavior. Native Linux and Windows PTY tests remain required. Windows relies
on the operating-system ConPTY implementation and therefore requires a
Windows version that supports ConPTY.

## References

- [`portable-pty` 0.9.0 API](https://docs.rs/portable-pty/0.9.0/portable_pty/)
- [`PtySystem::openpty`](https://docs.rs/portable-pty/0.9.0/portable_pty/trait.PtySystem.html)
- [`MasterPty` read, write, and resize](https://docs.rs/portable-pty/0.9.0/portable_pty/trait.MasterPty.html)
- [`PtySize`](https://docs.rs/portable-pty/0.9.0/portable_pty/struct.PtySize.html)
- [Rust dependency and notice audit](RUST_DEPENDENCY_AUDIT.md)
- [Rust parity plan](RUST_PARITY_PLAN.md)
