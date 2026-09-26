# Contributing to Rover

Start with README, STATUS, the architecture, and the security model. Preserve the full
terminal-first ten-layer scope, but do not claim unimplemented integrations work.

## Local workflow

1. Build the Go product with Go 1.26.6 or newer, a C compiler, system SQLite
   development headers, and Git.
2. Run `make check`, `make race`, `make version-check`, and `make demo` on owned
   fixtures.
3. Run `make manifest` after adding, removing, or editing tracked files, then
   `make manifest-check`. CI fails when `SOURCE_MANIFEST.json` is stale.
4. When a change touches the Rust port (`crates/`, `Cargo.toml`, `Cargo.lock`,
   `licenses/`, or `scripts/rust_*`), install the pinned toolchain with
   `rustup toolchain install 1.88.0 --profile minimal --component clippy --component rustfmt`
   and run `make rust-check CARGO='cargo +1.88.0'`. It runs `cargo fmt`, clippy
   with the workspace's pedantic lints as errors, the workspace tests, the script
   unit tests, and the locked license/notice audit. After a reviewed dependency
   change, `make rust-notices CARGO='cargo +1.88.0'` refreshes the bundled notices.
5. Add a focused regression test before changing acceptance semantics.
6. Update STATUS and the acceptance mapping when a capability's guarantees change.
7. Inspect the diff for source/state/credential leakage and unchecked error paths.

A green hosted CI run ([docs/CI.md](docs/CI.md)) is necessary but not sufficient.
Provide the actual command, platform, toolchain, result, and limitations in a
contribution.

## Design discipline

Keep task execution, checks, acceptance, human assertions, and delivery separate. Do not
promote missing verification to passing. Never hide unsupported functionality behind a
no-op adapter. Use explicit argv and context cancellation. Prefer existing standard library
and tools; add dependencies only with an ADR and license/security review. New parser formats
need malformed/empty/truncated/false-result fixtures. New runtimes need lifecycle and
cancellation tests on the claimed platform. Never run broad checks against a user's work
without consent; the development demo owns its disposable repository.

## Contribution certification

Use Developer Certificate of Origin 1.1 sign-off for future contributions (`git commit -s`)
only when you can personally make that certification. Do not fabricate another person's
sign-off. The locally generated initial source is marked as tool-assisted in provenance;
no human DCO certification is inferred. No CLA or relicensing consent is assumed.

Small reviewable contributions are preferred. Report limitations plainly. A verified
failing behavior is more useful than a confident unsupported quality claim.
