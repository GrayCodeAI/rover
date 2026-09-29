# ADR-0005: JSON record encoding for Rust storage

- Status: accepted for the P-023 storage API
- Date: 2026-09-25
- Owners: Rover maintainers; implementation owner is `rover-store`

## Context

The Go record store accepts values, serializes each to JSON, rejects payloads
over 16 MiB, and stores the JSON text in `records.payload`. Mutations read a
record and atomically replace it with a new JSON value and an event. The Rust
store needs validated JSON values and byte-size enforcement without introducing
an unbounded parser or silently changing object order.

## Decision

- Use exact `serde_json` 1.0.151, with default features disabled and only
  `std` and `unbounded_depth` enabled alongside the stack adapter in
  ADR-0006. It provides the `Value` representation used by the current
  store API. The package declares `MIT OR Apache-2.0`; Rust 1.88 exceeds the
  release's declared Rust 1.71 minimum.
- Leave `preserve_order` disabled. `serde_json::Value` then stores object
  members in sorted key order, matching Go's deterministic key ordering for
  ordinary string-keyed JSON objects.
- Serialize and size-check before writing. Retain Go's 16 MiB maximum and HTML
  escaping for `<`, `>`, `&`, U+2028, and U+2029. Store update plus optional
  event append remains one SQLite transaction.
- Bound values to Go's 10,000 nested-container maximum on both write and read;
  stack growth and safe destruction are specified in ADR-0006.
- Parse stored JSON when returning a stack-safe `RecordValue` or caller-selected
  typed value; invalid or oversized
  database payloads are errors. Reads never repair or rewrite malformed rows.
- Keep `preserve_order` and arbitrary precision numbers disabled. Typed reads
  choose the caller's numeric representation; raw list and mutation APIs retain
  the original JSON number tokens. Dynamic Rust `Value` does not coerce all
  numbers to `float64` like Go's untyped map decoder.

## Alternatives considered

- Raw JSON text only: rejected because the current `Mutate` behavior parses,
  transforms, and replaces a record as JSON data.
- Preserve source object key order: rejected because Go's encoder sorts map
  keys and order is not semantically meaningful in JSON objects.
- Reject all deep JSON beyond the parser default: rejected because Go accepts
  valid records with up to 10,000 nested containers.

## Acceptance evidence

Storage regression tests cover typed and raw access, JSON round trips,
deterministic escaping and encoding, Go's nesting bound, payload size, atomic
mutate/event behavior, and rollback/reopen recovery. Exact dependency license
files are checked from locked crate sources. Dynamic Rust `Value` retains its
native number representation; callers needing Go-style or typed numeric
semantics choose a target type or use the raw APIs.

## Primary references

- [serde_json 1.0.151 manifest and license/MSRV metadata](https://docs.rs/crate/serde_json/1.0.151/source/Cargo.toml)
- [serde_json 1.0.151 `Value` object ordering](https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html)
