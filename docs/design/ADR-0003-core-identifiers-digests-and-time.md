# ADR-0003: Core identifiers, digests, and timestamps

- Status: accepted for the P-021 compatibility slice
- Date: 2026-09-25
- Owners: Rover maintainers

## Context

Rover's Go model creates IDs from 16 operating-system random bytes, renders
SHA-256 digests as lowercase hexadecimal, and formats UTC timestamps as
RFC3339Nano. Rust's standard library does not provide a cross-platform secure
random API, SHA-256, or RFC3339 formatting. Implementing cryptography or
platform entropy access in Rover would add avoidable correctness and security
risk.

## Decisions

1. Use `getrandom` 0.4.3 for operating-system cryptographic random bytes. Do
   not enable its browser/WASM backend. ID creation is fallible; entropy-source
   failure must reach the caller, never produce a predictable fallback.
2. Use RustCrypto `sha2` 0.11.0 for SHA-256. Return the existing lowercase
   64-character hex representation. Do not add a separate hex dependency.
3. Use `time` 0.3.55 with only the `formatting` feature (which enables `std`)
   for UTC clock reads and RFC3339 formatting. Do not enable local-offset,
   serde, parsing, macros, or unrelated features. Preserve fractional-second
   trimming and UTC `Z` output in regression tests.
4. Pin these direct versions exactly in the workspace manifest and retain the
   Cargo lockfile. All three direct crates declare MIT OR Apache-2.0. Their
   resolved transitive crates must also be inventoried; an unknown license
   blocks builds and reuse until resolved.
5. Raise the Rover Rust MSRV to 1.88 because the current `time` release
   requires Rust 1.88. This is a published build requirement for the Rust
   workspace; it does not change the existing Go toolchain requirement.

## Alternatives considered

- Implement SHA-256 in Rover: rejected because cryptographic code should use a
  maintained implementation with its known-answer tests.
- Read `/dev/urandom` directly: rejected because it is not a cross-platform
  operating-system RNG contract.
- Use wall-clock formatting by hand: rejected because calendar, offset, and
  fractional-second formatting require tricky edge-case handling.
- Pin older library releases solely to retain Rust 1.75: rejected because that
  would trade away current fixes without an established Rust MSRV policy.

## Verification requirements

- SHA-256 known-answer vectors, including empty input and `abc`.
- Generated IDs have the requested valid prefix, 32 lowercase hex entropy
  characters, and pass the same validator; generation errors are propagated.
- Timestamp fixtures prove UTC `Z`, RFC3339 validity, nanosecond precision,
  and Go-compatible trimming of trailing fractional zeros.
- Inspect `cargo tree` and resolved package metadata for the exact dependency
  graph, licenses, and target-specific packages before closing P-019.

## References

- [`sha2` 0.11 documentation](https://docs.rs/sha2/0.11.0/sha2/)
- [`getrandom` 0.4 documentation](https://docs.rs/getrandom/0.4.3/getrandom/)
- [`time` 0.3 documentation](https://docs.rs/time/0.3.55/time/)
- [Rust parity plan](RUST_PARITY_PLAN.md)
- [Rust dependency audit](RUST_DEPENDENCY_AUDIT.md)
- [Bundled third-party notices](../../licenses/THIRD_PARTY_NOTICES.md)
