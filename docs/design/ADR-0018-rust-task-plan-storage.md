# ADR-0018: Rust project-bound task-plan storage

- Status: accepted for durable planning and manual lifecycle tracking
- Date: 2026-09-26
- Owner: `rover-tasks` owns task-plan behavior; `rover-store` owns atomic
  record/event persistence; `rover-execution` owns runnable process records.

## Context

Rover's saved task briefings need a durable destination and dependency state.
The existing `task` record is a Go-compatible runnable execution record and
must not be filled with incomplete argv/agent settings just to make a TUI
draft appear executable. A task plan should preserve intent independently
until Rover can construct and validate an actual worker launch contract.

## Decision

1. Store plans as `task_plan` records using schema `rover/task-plan/v1`. Every
   record includes the canonical repository path and project digest; list and
   get operations enforce both values.
2. Create validates the `TaskBrief`, requires every dependency to exist in the
   same project, rejects malformed stored graphs, and stores the record and
   creation event through `Store::create_once`.
3. Status is `READY` when every prerequisite succeeded, otherwise `WAITING`.
   Readiness is derived from current durable states, so a waiting plan becomes
   startable after prerequisites succeed without relying on a stale cached
   flag. Failed or cancelled prerequisites report `BLOCKED` readiness.
4. Manual start, retry, and finish transitions are durable and append events.
   The attempt history has a ten-attempt bound. Manual outcome notes are
   stored with `authority: manual`; they are user assertions, never provider,
   process, test, or quality-gate evidence.
5. This service does not launch workers, execute quality gates, mutate source
   worktrees, or choose an agent. The TUI can explicitly convert a saved draft
   into a plan and list plans; it does not expose start/retry/finish actions or
   imply that any worker ran. P-071/P-072 and the execution-backed TUI task
   workflow remain open. Capabilities are not advertised as running or
   dispatched by this service.

## Compatibility and OSS provenance

The plan record uses Rover's generic SQLite record/event store without
changing its schema migration version or Go-compatible `task` records. Serde,
serde_json, rusqlite, and Rover store/core are already reviewed workspace
dependencies. No upstream code or assets were copied. Their existing locked
license and bundled-notice audit remains authoritative.

## Acceptance

`rover-tasks` tests cover project isolation, task creation, missing dependency
rejection, waiting/readiness changes, prerequisite failure/cancellation,
manual lifecycle events, retries, and the ten-attempt bound. Full workspace
clippy, tests, dependency auditing, and license verification are required by
`make rust-check CARGO='cargo +1.88.0'`. Native Linux execution remains open.
