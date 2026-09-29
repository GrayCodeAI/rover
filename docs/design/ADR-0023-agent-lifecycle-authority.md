# ADR-0023: Sequenced agent lifecycle authority

Status: accepted for the Rust CLI/TUI implementation

## Context

Herdr documents source-scoped lifecycle reports, optional monotonically
increasing sequence numbers, ignored reports whose sequence is not newer, and
explicit authority release when an agent exits. While lifecycle authority is
active, Herdr does not also use screen-manifest fallback. Luvus documents
hook-reported lifecycle events for supported agents while retaining screen
detection as the fallback.

## Decision

`rover-agents::LifecycleAuthority` is a bounded in-memory registry owned by a
single pane. Rover requires a sequence on every report and release so delayed
events cannot restore state after release. The last sequence remains recorded
after release. Active lifecycle reports for idle, working, and blocked
override screen estimates. Releasing the source makes screen fallback
available again.

Rover accepts at most 32 distinct sources per registry lifetime. If multiple
sources are simultaneously active, the state is unknown and screen detection
remains suppressed; Rover does not compare independent source sequences or
choose an arbitrary winner. Completion/unread state remains owned by the
separate rollup/event-view layer.

## Consequences and limits

This implements only local state ordering and authority resolution. It does
not install hooks, send socket messages, persist events, bind session identity,
or wire the result into a TUI. The mandatory sequence and source cap are Rover
contract choices for bounded, deterministic behavior. The multi-source
conflict result is an explicit fail-closed policy. See [Herdr integration
guidance](https://herdr.dev/docs/integrations/),
[Herdr socket API](https://herdr.dev/docs/socket-api/), and
[Luvus agent guide](https://luvus.dev/docs/guides/agents/).
