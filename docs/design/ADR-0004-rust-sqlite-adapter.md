# ADR-0004: Rust SQLite adapter

- Status: accepted for the Rust storage foundation
- Date: 2026-09-25
- Owners: Rover maintainers; implementation owner is `internal/store` / `rover-store`

## Context

Rover's Go store uses SQLite for controller-owned JSON records, append-only
events, and idempotency keys. It sets WAL, `synchronous=FULL`, foreign keys,
and a 5-second busy timeout. Schema version 1 consists of `records`, `events`,
and `request_keys`. `Open` rejects a future `PRAGMA user_version`; the Rust
port must preserve that fail-closed behavior and support a later explicit
read-only Go-state migration. See `internal/store/sqlite.go` and
`internal/store/sqlite_test.go`.

## Decision

- Use exact `rusqlite` 0.40.2 with default features disabled and only
  `bundled` enabled. It provides typed parameters and errors, prepared SQL,
  explicit transaction behavior, and SQLite's online-backup interface for a
  later task. `bundled` uses the crate's bundled SQLite source rather than a
  host-installed SQLite library.
- Keep database connections synchronous and owned by the store. Concurrency,
  lock ownership, and write transaction semantics will be specified and tested
  in P-023; do not add a pool or async runtime here.
- Use `PRAGMA user_version` as the compatibility version shared with current
  Rover. Add a `schema_migrations` ledger with version, stable migration name,
  SHA-256 of immutable SQL, and UTC application time. Never rewrite an applied
  migration.
- On a legacy v1 database without the ledger, validate the known tables and
  columns before inserting the ledger entry. Refuse unknown tables, malformed
  schema, unversioned nonempty databases, mismatched ledger hashes, and
  versions newer than the binary.
- `rover-store` now configures caller-supplied connections and provides
  transactional record, event, mutation, idempotency, delete, and bounded list
  APIs. Rooted Unix file operations, blob storage, and state-root admission
  are implemented under P-024; database backup remains separate. JSON
  stack-growth and typed/raw access contracts are recorded in
  ADR-0005 and ADR-0006; their regression evidence is part of P-023.

## Dependency and build contract

The upstream crate metadata declares MIT. The SQLite amalgamation included by
its `bundled` feature is public domain according to rusqlite's bundling
documentation. Preserve rusqlite/libsqlite3-sys license files and the bundled
SQLite notice/source attribution found in the resolved crate archive. The
project's pinned Rust 1.88.0 toolchain is the build gate. Bundled SQLite
requires a C compiler. Do not advertise a supported target until native CI
build and migration tests pass for it.

Only the `bundled` feature is enabled. Do not enable SQLCipher, loadable
extensions, wasm support, or optional integrations without a new review. No
upstream Herdr, Luvus, or Orca application source is copied.

## Alternatives considered

- System SQLite via rusqlite defaults: rejected for the Rust product because
  the artifact would depend on host-installed SQLite and versions would vary.
- SQLx: rejected for this synchronous local database foundation because the
  current store has no async database contract and a pool adds lifecycle and
  configuration beyond the migration requirement.
- Hand-written SQLite FFI: rejected because it would recreate parameter
  binding, row conversion, error, transaction, and platform-linking code
  already maintained by rusqlite.

## Acceptance evidence

`rover-store` tests cover migration and transaction behavior, matching
idempotency across independent connections, event append, callback rollback,
payload limits, and connection reopen. CI checks the selected Rust MSRV.
Integration with a complete Rust state-root opener and Go-state import remains
pending.

## Primary references

- [rusqlite 0.40.2 manifest](https://github.com/rusqlite/rusqlite/blob/v0.40.2/Cargo.toml)
- [rusqlite 0.40.2 SQLite bundling and license guidance](https://github.com/rusqlite/rusqlite/tree/v0.40.2)
- [rusqlite 0.40.2 transactions](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html)
- [SQLite copyright and public-domain notice](https://www.sqlite.org/copyright.html)
