# ADR-0014: Rust terminal workspace layout model

Status: accepted for the serializable workspace state model and Unix private
state-file persistence API. Terminal UI framework selection, rendering,
keyboard controls, automatic launch restoration, and golden interaction tests
remain open.

## Context

Rover needs a Rust representation for Herdr-style workspaces, tabs, split
panes, zoom, popup overlays, and scratch terminals. The product boundary is
CLI/TUI only. Layout records must be bounded and validated before they can be
restored or used to resolve live sessions.

## Decision

- Define `rover_execution::layout::Workspace` as versioned JSON state with a
  validated namespace and stable workspace, tab, split, and pane IDs.
- Keep ordered tabs and per-tab active focus, zoom, popup, title, and split
  tree. Splits are binary and retain child order, axis, and a first-child ratio
  in basis points. Closing a pane promotes its sibling subtree.
- A session pane stores a validated name and absolute working directory;
  scratch panes have neither. A pane reference does not assert that the
  session process exists. The session broker resolves live processes.
- Bound state to 128 tabs, 256 panes, 16 split levels, 256-byte titles, and
  ratios strictly between 0 and 10,000 basis points. JSON input is capped at
  4 MiB and pre-scanned for a maximum nesting depth of 96 before recursive
  deserialization. Deserialization then validates schema, IDs, duplicate
  identifiers, tree shape, bounds, and focus/overlay references before
  returning a workspace.
- On Unix, save through `SafeDir::atomic_write` with mode `0600`, and load via
  no-follow bounded reads capped at 4 MiB plus one byte. The caller supplies an
  already-admitted private directory; layout code does not choose a state path.
- `rover_tui::run_persistent` restores a valid saved layout or uses the supplied
  initial workspace only when the file is absent, then saves after each key
  event. Corrupt or unsafe state is reported to the caller.
- Keep this crate module independent of any selected TUI library. Do not treat
  persistence primitives as automatic launch restoration or shipped UI.

## Evidence and remaining limits

Focused unit tests cover nested splitting and subtree collapse, stable IDs,
tab lifecycle, focus, resize, zoom/popup, invalid values, JSON round trips,
corrupt references, raw-size/depth bounds, atomic state-file round trips, exact
file mode, oversize input, malformed JSON, and symlink refusal. P-046 remains
open until a CLI launcher uses the persistent TUI entry point and
terminal-size-aware layout acceptance tests pass. No upstream source is copied.

## References

- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Rust session broker](ADR-0013-rust-session-broker.md)
- [Terminal screen core](ADR-0012-rust-terminal-emulator.md)
