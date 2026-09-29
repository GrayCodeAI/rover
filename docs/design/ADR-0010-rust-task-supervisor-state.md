# ADR-0010: Rust task supervisor state transitions

- Status: accepted for the transactional lifecycle slice
- Date: 2026-09-25
- Owners: `rover-execution` owns supervisor state transitions; `rover-store` owns atomic persistence

## Context

Rover's Go task runner records task claim, process ownership, cancellation
intent, periodic heartbeats, and `LOST` reconciliation in the existing task
record and event stream. A stale heartbeat does not prove the worker is gone.
PID reuse, terminal state changes, and a heartbeat racing with reconciliation
must not cause an active task to be marked `LOST` or restarted.

## Decision

1. Preserve the Go task JSON fields and `task.*` event names while mutating the
   existing `task` record. The Rust supervisor layer does not introduce a
   parallel task schema.
2. Claim is a one-way `QUEUED` transition. A pre-existing cancellation request
   makes the task `CANCELLED` before process launch; any other prior status
   rejects a second claim.
3. Heartbeats update `heartbeat` and `updated_at` without appending event rows.
   Heartbeat reads and returns the persisted cancellation request so the worker
   can stop its owned work.
4. Cancellation is durable intent and is rejected after a terminal status.
5. Reconciliation waits at least ten seconds after a valid heartbeat, then
   requires the platform process adapter to prove the recorded PID is gone or
   reused. It rechecks status, PID, and the exact observed heartbeat inside the
   SQLite transaction before writing `LOST` and `task.lost`. This conditional
   mutation prevents a concurrent heartbeat from being overwritten and avoids
   a false event. Ambiguity never triggers restart; workspace and evidence are
   retained.
6. `Store::mutate_with_event` is the shared primitive for conditional state
   changes whose event depends on the record read in the same transaction.
   It emits no event when the callback declines to make a transition.
7. Detached task launch uses an absolute executable and explicit argv, a private
   exclusive `supervisor.log` in the persisted task workspace parent, null
   stdin, a separate process group, and only PATH, HOME, and declared
   `pass_env` values. An in-process reaper thread waits for the direct worker
   while Rover remains alive. A second dispatch cannot replace the log or
   silently start another worker. A post-spawn persistence failure returns the
   PID and warns callers to inspect the task before retrying.
8. `HeartbeatGuard` owns the 250 ms heartbeat loop. It observes durable
   cancellation intent and cancels the worker token on cancellation or store
   failure; stopping the guard joins the thread and reports persistence errors.
9. Confirmed `LOST` transitions release leases owned by that task. Repeated
   reconciliation also retries owner-scoped cleanup for already-LOST tasks,
   covering an interrupted cleanup without taking another task's leases.
   Normal task-finalization release and client reconnect remain open. This
   slice does not provide CLI/TUI commands and is not advertised as shipped.

## Dependency and license review

No registry package or upstream source was added. `rover-execution` uses the
already pinned `serde_json` 1.0.151 and `time` 0.3.55 packages and their existing
MIT OR Apache-2.0 notices. The time feature set is the workspace-reviewed
`formatting`, `parsing`, and `std` set. License and notice checks remain part
of `make rust-check`.

## Acceptance

Tests cover one-way claim, malformed cancellation data failing closed,
pre-launch cancellation, durable cancellation, managed heartbeat cancellation
and error propagation, terminal-state protection, stale/fresh heartbeat
boundaries, ambiguous process presence, conditional `LOST` persistence,
confirmed-loss-only lease release, race-safe dispatch and event behavior,
private exclusive logs, duplicate launch refusal, child survival after launch
returns and after launcher process exit, child reaping, and spawn failure
recording. Nineteen supervisor tests, including the macOS PID-reuse ambiguity
test, pass on macOS. Linux identity and zombie test cases are
present and cross-compile, but a native Linux run remains required because the
current owned host is macOS;
cross-compilation is not runtime evidence.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Dependency and notice audit](RUST_DEPENDENCY_AUDIT.md)
- `internal/tasks/tasks.go`, `internal/tasks/identity_linux.go`, and `internal/tasks/identity_darwin.go`
- `crates/rover-store/src/lib.rs`
