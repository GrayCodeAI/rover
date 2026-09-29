# ADR-0013: Rust local terminal session broker

Status: accepted for the Unix server-owned session slice, stable named client
discovery, and initial CLI daemon integration. Windows server transport,
persisted restart metadata, and process-tree ownership remain open.

## Context

Herdr's local broker keeps a PTY process alive when its attached client detaches,
replays a bounded output tail after reconnect, rejects a second attached writer,
and ends the process when the server stops. Named sessions must be isolated by
namespace. Rover's Rust PTY wrapper previously owned its process only for the
lifetime of one client call.

## Decision

- Add `rover_execution::sessions::SessionServer` on Unix. The server process
  owns each `PtySession`; a client disconnect releases its writer slot without
  stopping the PTY. Explicit session stop or server drop closes/reaps the direct
  child. Process-tree termination is not yet guaranteed.
- Key sessions by validated `(namespace, name)` tuples. A name may be reused in
  another namespace, while replacement in the same namespace is refused.
  Derive socket basenames from the first 128 bits of SHA-256 over the
  namespace/NUL/name tuple, so a client in another process can resolve the
  endpoint without exposing names or requiring the server's in-memory map.
- Require a private state directory, bind a Unix socket with mode 0600, refuse
  stale socket takeover, and enforce the platform socket path limit. This is a
  same-user local interface, not a remote or multi-user service.
- The initial `rover tui` launcher stores session sockets in a mode-0700
  per-user directory under `/tmp`, keyed by the admitted state-root path, to
  stay within the Unix socket path limit. The canonical state path still owns
  durable layout and database data; socket state is transient and stale
  sockets are not removed or taken over automatically.
- Keep one writer attached per session. Input is capped at 16 KiB, terminal
  dimensions at 1,000 by 1,000, each wire frame at 64 KiB, and replay history at
  32 KiB. A slow client is disconnected when output cannot drain within the
  bounded write timeout; reconnect receives the retained tail.
- Encode byte payloads as standard base64 JSON, matching Go's `[]byte` frame
  encoding. No Herdr, Luvus, or Orca implementation source is copied.
- Session registry metadata exists only while the server process is alive.
  `SessionClient::connect_named` supports attach/reconnect by key while that
  process remains alive. Reopening sessions after server restart and rendering
  PTY bytes through Rover's terminal screen model are later contracts. Raw
  session bytes must not be rendered as trusted terminal text without the
  P-044 screen model.

## Evidence and remaining limits

Focused macOS tests cover namespace isolation and duplicate rejection, client
detach/reconnect by session key with continued process input, an actual child
process attaching by key without the server's in-memory map, one-writer
ownership, base64 wire compatibility, replay capacity, and input bounds. The
socket tests need permission to create/chmod local Unix-domain sockets in the
test sandbox; they pass on the native macOS host when run with that capability.
They do not establish Linux runtime behavior. The implementation is currently
Unix-only and has no Windows named-pipe server.

## References

- [Herdr PTY broker and attach implementation](../../internal/execution/pty_darwin.go)
- [Herdr interactive attach client](../../internal/execution/attach.go)
- [PTY backend decision](ADR-0011-rust-pty-backend.md)
- [Terminal screen core decision](ADR-0012-rust-terminal-emulator.md)
- [Rust dependency and notice audit](RUST_DEPENDENCY_AUDIT.md)
- [Rust parity plan](RUST_PARITY_PLAN.md)
