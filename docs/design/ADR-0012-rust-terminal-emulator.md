# ADR-0012: Rust terminal screen core

Status: accepted for the bounded local screen-model slice. Rendering, interactive
input routing, durable sessions, and native Linux/Windows runtime behavior remain
open.

## Context

Rover needs to interpret untrusted PTY bytes before the TUI renders or exports
terminal content. Passing escape sequences straight through would let a child
process control the user's terminal. The model must also bound screen and history
memory, handle Unicode cell widths and resizing, and suppress clipboard or other
outward terminal actions.

## Decision

- Use `alacritty_terminal` 0.26.0, pinned in `Cargo.lock`, under its declared
  Apache-2.0 license. Default features are disabled. Exact package notices and
  the 107-package locked inventory are included in the Rust dependency audit.
- Keep the dependency behind `rover_execution::terminal::TerminalScreen`. Rover
  does not copy terminal-emulator source from Herdr, Luvus, or Orca.
- Limit visible dimensions to 1,000 rows and 1,000 columns, with a 250,000-cell
  viewport budget. Cap scrollback at 10,000 lines and 1,000,000 cells; reduce
  the history line limit for wider viewports. Limit each parser call to 64 KiB.
- Disable OSC 52 clipboard handling. The screen model captures only terminal
  bell events through a coalescing flag; all other outward terminal events are
  discarded. The TUI relays a bell to the host terminal only while that pane is
  unfocused. Expose filtered cell text only; replace control and bidirectional
  formatting characters before the caller renders or exports it.
- Treat this as an emulator state model, not a renderer or complete terminal
  application. Selection, copy/paste, key and mouse input, remote transport,
  session persistence, and full terminal protocol compatibility are separate
  work with their own acceptance contracts.

## Evidence and remaining limits

Eight focused native macOS unit tests cover bounded input and dimensions,
scrollback caps, resize trimming, wide-character resize and erase handling,
control-sequence suppression, styling, bidi filtering, and bell coalescing.
The TUI separately tests active-pane suppression. These tests establish
the current wrapper's behavior on this host; they are not a complete VT
conformance suite or security certification. Native Linux and Windows runtime
checks and integration with Rover's eventual renderer remain required.

## References

- [`alacritty_terminal` 0.26.0 API](https://docs.rs/alacritty_terminal/0.26.0/alacritty_terminal/)
- [Rust dependency and notice audit](RUST_DEPENDENCY_AUDIT.md)
- [Rust parity plan](RUST_PARITY_PLAN.md)
