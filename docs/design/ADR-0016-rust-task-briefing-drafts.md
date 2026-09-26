# ADR-0016: Repository-bound Rust task briefing drafts

- Status: accepted for the local draft slice
- Date: 2026-09-26
- Owners: `rover-tui` owns draft editing; `rover-cli` owns persistence

## Context

The Luvus orchestration form documents a task title, path globs, dependencies,
quality gate and multiline prompt. Rover's Rust binary currently has a PTY and
task supervisor, but it has no Rust task-contract dispatcher or agent capability
registry. A draft editor must not imply that saving a draft creates a queued
task, starts a worker, validates path leases, or runs the quality gate.

## Decision

1. A draft stores title, newline-separated path globs, newline-separated
   dependency identifiers, quality-gate text, and a multiline prompt.
2. `Ctrl-B d` opens repository-local create/edit/delete. Saving is explicit;
   deletion and discarding edits require confirmation.
3. Drafts remain inert local data. They are not converted to task records,
   dispatched to a worker, interpreted as shell commands, or submitted to an
   agent. The future task service must validate paths, dependency identities,
   worker mode, agent capabilities and gate argv before launch.
4. The record is capped at 32 drafts; UTF-8 byte caps are 120 for title, 8 KiB
   for paths, 4 KiB for dependencies, 1 KiB for quality-gate text, and 16 KiB
   for prompt. Line breaks are allowed in the four multiline fields. Other
   control characters and bidi format controls are refused.
5. Schema v1 (`title`, `prompt`) loads with empty values for the new fields.
   Saves write schema v2 with all five fields. Repository path and digest-keyed
   record identity must match the active canonical repository; malformed or
   cross-project records fail closed.
6. `Manual`/`Now` start mode and agent choice are intentionally absent until
   worker dispatch and the agent capability registry exist. They are not
   represented by placeholder choices.

## License and provenance

No upstream source or asset is copied. The field set and interaction contract
are independently implemented from the documented Luvus orchestration form;
no upstream dependency or license notice is introduced by this slice.

## Acceptance

TUI tests cover create/edit/delete, multiline input, save and discard behavior,
and verify drafts do not create tasks. CLI tests cover schema v1 migration to
v2, round-trip equality, project binding, supported text, bidi/control
rejection, count limits and each field's UTF-8 byte limit. `make rust-check`
must pass. Full task creation, task state transitions, path leases, quality-gate
execution, `Manual`/`Now`, agent selection and worker dispatch remain in P-061,
P-071 through P-074.
