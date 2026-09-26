# ADR-0025: Project-scoped agent session inventory

Status: accepted for the stored-identity inventory slice

## Context

Herdr and Luvus list live agents and expose status, read, prompt, wait, and
attach operations. Rover stores exact Codex/Claude session identities for
explicitly bound project panes. The attached TUI now receives a foreground
process sample from its owned PTY, but the generic CLI inventory does not yet
join those observations and lifecycle-event transport is not present.
Treating a saved identity as a live agent or reporting its state as
idle/working would invent evidence.

## Decision

- Add `rover agent session bind --repo PATH --pane ID --adapter ADAPTER
  --id EXACT_ID [--state PATH]` to bind a user-selected exact ID to a pane
  present in that project's persisted Rover workspace. The pane may be the
  literal `active`, which resolves to the workspace's current active pane.
  This records `ExplicitUser` provenance and never scans provider databases.
- Add `rover agent session list --repo PATH [--state PATH]` to list only exact
  stored bindings for the selected canonical repository.
- Add `rover agent session status --repo PATH --pane ID [--state PATH]` to
  retrieve one project's exact pane binding.
- Sort by stable pane ID. Refuse malformed matching records and collections
  larger than the store query bound rather than returning partial data.
- Return `binding_status: bound` separately from `agent_state: unknown`, with a
  reason that no live process or lifecycle observation is available.
- Add `rover agent session clear --repo PATH --pane ID [--state PATH]` to remove
  only Rover's identity association. The command does not stop an agent.
- Do not expose prompt, wait, read, or attach as generic agent operations yet.
  The attached TUI's explicit interactive resume flow remains a provider
  session operation, not proof of live-agent discovery.

## Consequences and limits

The CLI gives users and scripts explicit bind/clear operations plus an exact,
project-scoped inventory of persisted session identities without reading
provider databases or claiming that those sessions are running. The store
query is capped at 1,000 session records across the state root, so an oversized
global collection fails closed. Attached TUI process identity is a separate
live observation and its state remains unknown. Live list/status,
prompt, wait, terminal read, and direct attach still require authority-aware
lifecycle transport and pane I/O contracts. See P-062, P-064, and P-067 in
`RUST_PARITY_PLAN.md` and the sampling contract in ADR-0026.
