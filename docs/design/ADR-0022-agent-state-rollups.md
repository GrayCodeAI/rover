# ADR-0022: Agent state rollups

Status: accepted for the Rust CLI/TUI implementation

## Context

Herdr documents pane, tab, and workspace status rollups where blocked takes
priority, working indicates active work, and an unviewed completion remains
visible as done. Luvus documents blocked, working, done, and idle states, with
completion derived separately from current activity. Rover also needs to show
stale observations without presenting them as current state.

## Decision

`rover-agents` provides pure pane, tab, and workspace aggregation functions.
Inputs are explicit snapshots with Unix-millisecond observation times and an
unviewed-completion bit owned by the event/view state layer. Freshness is
calculated with a caller-supplied positive window; future timestamps are
treated as age zero to tolerate wall-clock skew.

For fresh observations, state precedence is blocked, working, done, idle, then
unknown. A completion is done only when an idle observation explicitly carries
`done_unseen`; viewing is the responsibility of the caller, which clears that
bit. Stale observations are excluded from precedence and counted. The rollup
state is stale only when every observation in scope is stale; otherwise the
fresh state is shown and stale counts remain available. Empty scopes are
unknown. Duplicate pane snapshots and invalid completion/state combinations
are rejected.

## Consequences and limits

This is aggregation logic, not lifecycle authority, event persistence, native
sampling, or TUI presentation. No duration, status, or completion is inferred
from silence. The caller must choose a freshness window and supply consistent
timestamps. The implementation intentionally does not copy upstream source;
the behavior is based on documented user-visible semantics. See the [Herdr
agent status documentation](https://herdr.dev/docs/agents/) and [Luvus agent
guide](https://luvus.dev/docs/guides/agents/).
