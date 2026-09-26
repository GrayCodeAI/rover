# Go to Rust State Import and Rollback

The Rust migration importer is currently a store API, not a CLI command or a
runtime cutover. The Go CLI/TUI remains authoritative. Importing creates a
separate Rust state root and does not edit, rename, or delete the Go source
root.

## Before import

1. Stop Rover and every worker that can write to the Go state root. The
   `source_quiesced` option is an operator assertion; the importer cannot prove
   that no external writer exists.
2. Make a separate filesystem backup of the Go state root and record its
   location. Keep the original root in place and writable by the current Go
   version.
3. Run `LegacyStateInventory::inspect` and retain its report. Resolve every
   database or object blocker before proceeding.
4. Review the listed task/check runtime trees and unmanaged root entries. The
   importer excludes those files. Set `accept_excluded_runtime_data` only after
   deciding that their continued retention in the original root and omission
   from the Rust destination are acceptable.
5. Choose a distinct, private destination path. The importer rejects source
   and destination paths that overlap and refuses unrelated populated
   destinations.

## Import and verification

Call `LegacyStateInventory::import_to` with both explicit options set only
after the preceding review. Keep the returned receipt with the inventory
report. A repeated call for an unchanged source and destination verifies the
receipt, imported records, events, request keys, and content-addressed objects
and returns `already_imported`. It refuses a changed source snapshot or altered
destination data.

The source remains the rollback copy. Runtime files under `tasks/` and
`checks/`, plus unmanaged root entries, remain only in that original source;
the importer does not copy them. Grants are revoked in the destination, leases
are released, and task/workflow process state is removed or marked `LOST` so
that import cannot resume an old process automatically.

## Rollback

1. Stop every Rust process using the destination state root.
2. Restore the application's state-root configuration to the original Go
   state path. Do not point Go at the Rust destination.
3. Start the Go Rover version that owns the source schema and confirm it opens
   the original state. Do not run Go and Rust writers against the same root.
4. Keep the Rust destination and migration receipt for investigation until the
   operator decides whether to archive or remove them. The importer performs
   no automatic rollback deletion.

If verification fails, do not cut over. Preserve both roots and the error
details. Since import does not modify the source, a retry can use a new,
separate destination after the cause is resolved; do not overwrite an unrelated
or partially modified destination.

This procedure documents state handling only. A future product cutover still
needs an explicit CLI/TUI workflow, compatibility policy, and release gate.
