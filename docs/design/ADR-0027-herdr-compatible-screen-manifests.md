# ADR-0027: Herdr-compatible screen manifest evaluation

Status: accepted for bounded bundled manifest parsing and screen-rule evaluation

## Context

The pinned Herdr revision `c411883ec639c9893ed9c33021c890485e91727b` provides
22 bundled TOML screen manifests and a matcher language with nested gates,
regex, line-regex, region selection, priorities, OSC title/progress fields,
visible-state flags, and a skip-state-update flag. Rover's existing literal
matcher cannot execute these rules. The source is Apache-2.0; exact manifest
digests and required attribution are recorded in
`UPSTREAM_SOURCE_AUDIT.md` before those data files are bundled.

## Decision

- Parse the pinned TOML format using exact `toml 0.8.23`, with unknown fields
  rejected, source bytes capped at 64 KiB per manifest, at most 128 rules,
  gate depth 8, 512 gates, 32 matchers per gate, 1,024 total matchers, and
  512 Unicode scalar values per matcher.
- Compile regular expressions with exact `regex 1.12.4`. Apply a bounded
  regex automaton size, screen snapshots capped at 64 KiB, and fixed rule/gate
  counts. This engine uses a bounded, non-backtracking regex implementation.
- Evaluate `contains`, `regex`, and `line_regex` as conjunctions within each
  gate; `all` is conjunctive, a nonempty `any` is disjunctive, and a matching
  `not` gate rejects its parent gate. Recursion only evaluates already
  validated, bounded manifests.
- Preserve upstream rule ordering: the highest priority matching rule wins;
  the first matching rule wins a priority tie. Preserve its region selectors,
  visible flags, `skip_state_update`, and Codex-specific unknown fallback.
- Evaluate only a live visible-screen snapshot and explicitly collected OSC
  title/progress values. Do not use scrollback, arbitrary child guesses, or
  shell history as detection evidence.
- Keep remote update, user override, hot-reload, and integration installation
  outside this slice. Bundled manifests are immutable data from the audited
  commit; no background network fetch is introduced.

## Dependency review

- `regex 1.12.4` declares `MIT OR Apache-2.0`, Rust 1.65, and uses linear-time
  matching with bounded pattern/screen sizes. It supplies the manifest's
  regex/line-regex operators without an ad hoc or backtracking engine.
- `toml 0.8.23` declares `MIT OR Apache-2.0`, Rust 1.66, and supplies strict
  TOML-to-Serde decoding. Parsing is isolated behind input and nesting bounds.
- Both crates are already cached locally, are compatible with Rover's Rust
  1.88 toolchain, and have their resolved transitive notices captured by
  `make rust-notices` and verified by `make rust-check`.
- A custom TOML parser or regex engine was rejected because it would increase
  parser correctness and denial-of-service risk while failing to match the
  upstream rule language.

## Verification and limits

Regression tests cover every gate operator and region selector, rule
priority/tie behavior, invalid TOML/regex, complexity bounds, all 22 pinned
manifests, Codex fallback, OSC regions, skip-state behavior, and unsafe
evidence. The attached TUI now supplies the live viewport, OSC title, and
bounded OSC 9;4 progress payloads to the evaluator for audited direct
executable mappings. Full behavior still requires lifecycle-authority
integration, expanded process-group and wrapper recognition, and the remaining
CLI/TUI agent surfaces. See P-062 and P-166/P-167 in `RUST_PARITY_PLAN.md`.
