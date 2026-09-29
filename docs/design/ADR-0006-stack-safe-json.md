# ADR-0006: stack-safe deep JSON for Rust storage

- Status: accepted for P-023 implementation
- Date: 2026-09-25
- Owners: Rover maintainers; implementation owner is `rover-store`

## Context

Rover's Go `encoding/json` accepts JSON nesting through 10,000 levels. The
default `serde_json` recursion limit is 128, which would reject valid existing
records. Simply disabling that limit can overflow the native stack while
parsing, serializing, formatting, cloning, or dropping a deeply nested value.
The store needs an explicit depth bound matching Go plus stack-safe operations.

## Decision

- Keep exact `serde_json` 1.0.151 and enable its `unbounded_depth` feature only
  together with exact `serde_stacker` 0.1.14. Use the adapter for both
  deserialization and serialization; independently scan nesting and reject
  beyond Go's 10,000-container bound before recursive serde work begins.
- Use the standard Serde `Serialize` and `DeserializeOwned` contracts for
  typed records. Also provide raw JSON access for callers that need to preserve
  lexical number representation or decode into their own target types.
- Return parsed dynamic JSON through a Rover wrapper with iterative destruction.
  Avoid recursive `Debug`, `Clone`, and implicit conversion APIs on that
  wrapper. Typed user-defined values retain their type's own drop behavior.
- Preserve Go's 16 MiB post-HTML-escape byte cap and deterministic key order.
  Bound intermediate JSON serialization to 16 MiB because HTML escaping never
  shrinks the serialized form.
- `serde_stacker` grows the call stack dynamically. Its `stacker` dependency
  uses the `psm` assembly implementation where available; validate the project
  host and do not claim runtime stack growth on unsupported targets. The
  resolved lock uses `stacker` 0.1.25 and `psm` 0.1.32; PSM's build graph uses
  `ar_archive_writer` 0.5.3 (`Apache-2.0 WITH LLVM-exception`). These packages
  and their complete declared license files are included in the audited notice
  bundle. A C compiler is required to build the stack-switching assembly.
- Keep typed numeric decoding type-directed. A Rust caller can request integer,
  floating-point, or another declared target type; raw access preserves the
  original JSON token. Do not claim that decoding a generic Go `map[string]any`
  into Rust `serde_json::Value` has identical `float64` coercion semantics.

## Alternatives considered

- Retain the 128-level cap: rejected because it excludes valid persisted Go
  records.
- Disable `serde_json` recursion checks without a stack adapter: rejected
  because malformed or deeply nested stored JSON could overflow the process.
- Keep JSON as raw strings only: rejected as the only interface because
  mutation and typed store reads need structured deserialization; raw methods
  remain available for exact-token handling.

## Acceptance evidence

Rust regression tests encode, parse, inspect, and safely drop 10,000 nested
arrays and objects; reject level 10,001; check malformed input and the 16 MiB
encoded limit; and verify typed and raw numeric access. Go 1.26.6's pinned
`maxNestingDepth` source constant and a local `encoding/json` probe confirm the
same inclusive 10,000-container boundary. The audit copies and verifies each
locked license file byte-for-byte.

## Primary references

- [Go 1.26.6 JSON nesting limit](https://github.com/golang/go/blob/go1.26.6/src/encoding/json/scanner.go#L148)
- [`serde_json` recursion-limit API](https://docs.rs/serde_json/1.0.151/serde_json/struct.Deserializer.html#method.disable_recursion_limit)
- [`serde_stacker` 0.1.14 stack-safe Serde adapters](https://docs.rs/serde_stacker/0.1.14/serde_stacker/)
- [`serde_stacker` upstream source and dual license](https://github.com/dtolnay/serde-stacker)
- [`stacker` 0.1.25 dependency and platform notes](https://docs.rs/crate/stacker/0.1.25)
- [`psm` 0.1.32 target support and license](https://docs.rs/crate/psm/0.1.32)
