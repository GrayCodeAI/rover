# ADR-0007: rooted file operations for Rust storage

- Status: accepted; Unix implementation and focused macOS tests are in place
- Date: 2026-09-25
- Owners: Rover maintainers; Go behavior owner is `internal/store`, Rust owner is `rover-store`

## Context

The Go store keeps content-addressed blobs below `objects/`, verifies SHA-256
on read, refuses final-component symlinks, bounds reads to 16 MiB, writes
objects and state files through synced temporary files, and applies private
permissions. Path helpers also provide lexical root containment. See
`internal/store/sqlite.go` and `internal/store/sqlite_test.go`.

The Rust store supports caller-supplied SQLite connections as well as an
explicit Unix `StateRoot` opener. Root admission and opened directory handles
must remain visible contracts; file operations must not accept arbitrary
absolute paths as if containment were established.

## Decision

- Add a directory-handle based file API. It opens a caller-selected directory
  without following a symlink in its final component and anchors subsequent
  operations to that handle. File names accepted by these APIs are single
  normal path components; `.`/`..`, separators, and absolute paths are
  rejected. The opened directory must have exact mode `0700`. `StateRoot::open`
  creates/adopts a dedicated state path, rejects filesystem roots, home/current
  directories, user symlink ancestors and unrelated nonempty directories,
  creates private `objects`, `tasks`, and `checks` areas, and opens SQLite with
  its no-follow flag. The Go-state inventory/import remains separate work.
- On Unix, use exact `rustix` filesystem wrappers for `openat`-style
  operations, no-follow opens, exclusive temporary creation, link/rename,
  metadata, permission changes, and `fsync`. No raw unsafe syscall code is
  introduced in Rover. The reviewed crate version and complete license notice
  are recorded in `RUST_DEPENDENCY_AUDIT.md`.
- Content-addressed writes use lowercase SHA-256 names, temporary files opened
  exclusively with mode `0600`, file sync before publication, no-replace link
  semantics, verification of an already-existing object, temporary cleanup,
  and directory sync. Writes preserve Go's current lack of an input-size cap;
  reads reject non-regular files, files larger than 16 MiB, and digest
  mismatches. The separate state-root admission contract will define how
  large-file creation is bounded by its callers.
- Atomic file replacement creates a same-directory temporary file, applies
  the caller-selected mode to the open handle, writes and syncs it, renames it
  over the target entry, and syncs the containing directory. Bounded reads
  reject non-regular files and no-follow open the final component. Root
  containment is a lexical path-component check; it is not represented as a
  substitute for directory-handle anchoring.
- Implement and validate Unix behavior first, matching the existing Go
  `O_NOFOLLOW` contract. Do not advertise file-store support on other targets
  until a native implementation and tests exist; platform support is tracked
  by P-060.
- Temporary names contain a process identifier and monotonic counter and are
  created with exclusive semantics. Cleanup is best effort after an error;
  successful publication always includes the required directory sync.

## Alternatives considered

- Path-based check-then-open: rejected because a symlink can be swapped between
  validation and use.
- Raw libc/OS FFI in Rover: rejected because it adds unsafe code and
  platform-specific syscall maintenance.
- A new filesystem abstraction that follows links: rejected because its
  default path behavior would make the no-follow contract less explicit.
- An input-size cap on blob writes: rejected for this compatibility slice
  because Go `Blob` currently accepts any slice size while `ReadBlob` enforces
  16 MiB. A future explicit schema/contract change may tighten this.

## Acceptance evidence

Focused Rust tests cover valid digest round-trip, duplicate concurrent
creation, digest mismatch, invalid digest, symlink and non-regular rejection,
16 MiB read boundary, containment including sibling-prefix confusion, atomic
replacement, exact file and directory modes, descriptor anchoring, state-root
reopen, rejection-before-mutation, symlink safety, and bounded-read behavior.
They pass on the current macOS/arm64 host. `cargo check --all-targets` also
type-checks `rover-store` for `x86_64-unknown-linux-gnu`; because the machine has
no Linux C toolchain, that check used the host SQLite pkg-config metadata and is
compile evidence only. Native Linux execution and wider target coverage remain
subject to P-060; Windows support is not advertised.

## Primary references

- `internal/store/sqlite.go`: `Blob`, `ReadBlob`, `AtomicFile`, `BoundedFile`,
  `IsWithin`, and `PrivateDir` behavior.
- `internal/store/sqlite_test.go`: blob tamper, symlink/path, and permission
  regression contracts.
- [`rustix` filesystem API](https://docs.rs/rustix/latest/rustix/fs/): safe
  wrappers for descriptor-relative Unix filesystem operations.
