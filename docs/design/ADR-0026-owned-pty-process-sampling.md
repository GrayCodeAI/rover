# ADR-0026: Sampling foreground processes from owned PTYs

Status: accepted for Unix attached-session identity display

## Context

P-062 needs live foreground-process evidence for agent identity. Sampling an
arbitrary PID supplied by a UI client would cross the session boundary and
could inspect a process unrelated to the attached pane. A saved session ID
also does not prove that the session is currently running.

## Decision

- The session server reads the foreground process-group leader from the master
  PTY it owns. It does not accept a PID from the client. It samples executable
  basenames for members of that foreground group; this does not enumerate
  descendants in other groups or infer wrappers from argv.
- Linux reads `/proc/<pid>/exe` between two process identity checks. The
  identity is bound to boot ID, PID, and `/proc/<pid>/stat` start time.
- macOS runs `/bin/ps` directly with a numeric PID, a cleared environment,
  fixed `LC_ALL=C`, no shell, null stdin/stderr, and bounded stdout. Two
  observations must agree on process start time, process group, and command;
  the reported group must be the sampled PTY foreground leader.
- Linux enumerates numeric `/proc` entries with a 4096-entry scan limit and a
  64-distinct-name limit. It binds each selected process to `/proc` start time
  and verifies the group again after reading `/proc/<pid>/exe`. macOS uses a
  cleared-environment `/bin/ps -A` listing capped at 256 KiB, then requires two
  matching start-time/group/command observations for each selected PID. Both
  samplers return no group list when a bound is exceeded or selected evidence
  is unstable. Unsupported systems, denied inspection, malformed output, and
  oversized output produce an empty sample. Empty means unknown; it does not
  mean idle or stopped.
- Only the currently attached single writer may request a sample. The reply
  uses the existing bounded JSON/base64 session frame. Its existing `data`
  field remains the group leader basename; an additive `processes` field
  carries distinct validated group-member basenames, never PIDs or full
  command lines.
- The TUI evaluates group member names against the bounded Herdr-compatible
  manifest matcher and visible screen evidence. A recognized group leader has
  priority; conflicting recognized non-leader identities remain unknown. A
  known manifest identity may be detected; this sampler does not confer
  lifecycle authority.

## Verification and limits

Regression tests cover the owned PTY sampler, session request/reply framing,
foreground group detection, leader priority, and ambiguous non-leader
identity. These PTY tests passed on the current macOS host; Linux runtime
verification remains pending. This does not provide cross-session process
inventory, wrapper argv inference, Letta-specific filtering, wait/prompt/read/
attach operations, or lifecycle authority. See ADR-0021, ADR-0025, and
P-062/P-067/P-167 in
`RUST_PARITY_PLAN.md`.
