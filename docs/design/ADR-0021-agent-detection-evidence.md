# ADR-0021: Agent detection from explicit process and screen evidence

Status: accepted for the bounded library contract and owned-PTY sampling slice.
Bundled manifests, lifecycle authority, and complete Herdr matcher parity
remain open.

## Evidence and scope

Herdr's current [agent documentation](https://herdr.dev/docs/agents/) says it
identifies the foreground process first, then evaluates a live bottom-buffer
snapshot with detection manifests. It describes blocked as a match against a
known visible approval/question/permission prompt; a known agent with no screen
rule match falls back to idle. This Rust slice implements that documented
boundary independently; it does not claim source-level equivalence to the
pinned Herdr commit. The local upstream repository is a partial clone whose
detection blobs were unavailable when inspected offline, so exact matcher
syntax and bundled per-agent patterns have not been independently verified.

## Decision

- Accept platform-supplied foreground executable identity and a live screen
  snapshot; do not infer identity from arbitrary substrings, child-process
  guesses, or historical/scrollback text.
- Built-in identity mappings are restricted to declared native profiles whose
  executable names are known (`codex` and `claude`). Other process names need a
  valid local manifest with exact basename aliases.
- Screen manifests use bounded literal substring rules with explicit
  priorities. Same-priority rules that disagree produce `unknown`; lower
  priority matches cannot override the highest priority. This is intentionally
  not a port of Herdr's full TOML matcher language.
- A state fallback to `idle` requires both a recognized process and a valid
  manifest plus a supplied live screen with no matching rule. Missing process,
  screen, or manifest, invalid screen controls, and ambiguous mappings remain
  `unknown`.
- Return confidence, stable reason codes, process basename, manifest source and
  version, and rule IDs. Never copy raw screen contents into explain evidence.
- Bound screen input to 64 KiB; manifests to 128, process names to 32 per
  manifest, rules to 256, and literal markers to 256 bytes each. Reject control
  and bidirectional formatting characters in metadata and markers.
- On Unix, sample only the foreground process-group leader reported by the
  Rover-owned PTY. Do not enumerate child processes or infer a wrapper's agent.
  Linux binds `/proc/<pid>/exe` to the kernel boot ID, PID, and process start
  time before and after reading it. macOS makes two bounded `/bin/ps` queries
  for start time, process group, and command; mismatches, command failures,
  oversized output, and unsupported platforms produce no sample.
- The local session protocol accepts `sample_foreground` only from its active
  attached writer and returns `foreground_sample` with a basename or empty
  evidence. The attached TUI evaluates process identity but has no bundled
  screen manifests yet, so recognized agents display with state `unknown`.
  Empty samples also remain unknown.

## Verification and limits

Fixtures cover process basenames and Windows `.exe` normalization, unknown and
ambiguous identity, explicit screen matches, priority/conflict handling,
documented idle fallback, absent evidence, hostile controls, metadata
validation, bounds, safe explanation output, and request/reply over an owned
PTY session. Native sampling has been exercised on macOS; Linux sampling still
requires a Linux verification run. No full agent manifest set, lifecycle
hooks, wrapper/child detection, or complete Herdr matcher port is included.
