# Rust dependency snapshot

Audit date: 2026-09-25. This snapshot covers the current Rust workspace,
including the `rover-store` transactional JSON record adapter and its bundled
SQLite amalgamation. It is not a complete Rover product SBOM while Go remains the
published CLI/TUI implementation, and it is not a vulnerability audit. The
locked package license/author files are bundled in
[`licenses/THIRD_PARTY_NOTICES.md`](../../licenses/THIRD_PARTY_NOTICES.md), but
the bundle is byte-checked by `scripts/rust_dependency_audit.py`. CI now pins
the Rust 1.88.0 MSRV toolchain for the workspace gates. `time` 0.3.55 declares
this minimum Rust version; the `rusqlite` release was compiled and tested with
the same pinned toolchain. The stack-safe JSON adapter also uses `psm` and
`ar_archive_writer`, which declare Rust 1.88.0 and are covered by this pin.

## Direct dependencies

| Crate | Locked version | License expression | Use |
|---|---:|---|---|
| `alacritty_terminal` | 0.26.0 | Apache-2.0 | VT terminal state machine and bounded scrollback grid; default features disabled. OSC 52 clipboard handling and outward events are disabled in Rover. ADR-0012 records the security limits. |
| `base64` | 0.22.1 | MIT OR Apache-2.0 | Standard base64 encoding for the local terminal socket frames; matches Go `[]byte` JSON wire format. Default features disabled. |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 | OS-backed cryptographic entropy for IDs; no `wasm_js` feature. |
| `portable-pty` | 0.9.0 | MIT | Native Unix PTY and Windows ConPTY create/read/write/resize/close backend; dependency contract and scope are in ADR-0011. |
| `regex` | 1.12.4 | MIT OR Apache-2.0 | Bounded, non-backtracking Herdr-compatible screen-manifest regex and line-regex matching; default Unicode/performance features. ADR-0027 bounds input, matcher sizes, and automaton size. |
| `rusqlite` | 0.40.2 | MIT | Synchronous SQLite connection, schema migration, and record APIs; default features disabled, only `bundled` enabled. |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Safe Unix descriptor-relative file operations in `rover-store` and process-group termination in `rover-execution`; default features disabled, only `fs`, `process`, and `std` enabled. |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Typed record and session-frame serialization/deserialization contracts; default features disabled, `std` and `derive` enabled. |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | JSON parsing/encoding; only `std` and `unbounded_depth` enabled, with `serde_stacker` and an explicit 10,000-level limit. `rover-execution` uses the same locked package for transactional task record transitions. |
| `toml` | 0.8.23 | MIT OR Apache-2.0 | Strict bounded deserialization of audited Herdr detection manifests; default features disabled, `parse` only. ADR-0027 records why a general parser is required. |
| `serde_stacker` | 0.1.14 | MIT OR Apache-2.0 | Dynamically growing stack adapter for deep typed JSON operations. |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 | SHA-256 implementation; pure Rust. |
| `time` | 0.3.55 | MIT OR Apache-2.0 | UTC clock and RFC3339 formatting/parsing for task heartbeat reconciliation; only `formatting`, `parsing`, and `std` enabled. |

## Locked package license metadata

`cargo metadata --format-version 1 --locked --offline` reports 163 registry
packages, all with a declared license field, plus six local workspace crates.
Ten crates use legacy `MIT/Apache-2.0` or `Apache-2.0/MIT` labels; their
license alternatives are normalized to SPDX `OR` expressions. The terminal emulator adds 38 locked registry packages, including target-specific
Windows support packages. The PTY backend previously added 19 packages, including target-specific Windows ConPTY
dependencies. Two small Windows GNU import-library packages omit license files
from their crate archives; the audit takes the exact MIT and Apache license
texts from the `winapi` crate at the same upstream repository and matching
SPDX expression, then records and byte-checks those bundled files for each
locked package. Cargo metadata includes target-specific and optional packages;
the active normal/build dependency graphs are in the verification record
below.

The Ratatui/Crossterm TUI dependency set adds the renderer, widget, and event
backend packages. Its reviewed SPDX expressions include `MIT OR Apache-2.0 OR
CC0-1.0` (`fast-srgb8`), `Zlib` (`foldhash`), and `Apache-2.0 OR BSL-1.0`
(`ryu`). The exact CC0, Zlib, Apache, and Boost Software License files are
retained in the per-package bundle; `BSL-1.0` here is the SPDX identifier for
Boost Software License 1.0. Crossterm's OSC 52 feature is disabled in Rover.

| Package | Version | License expression |
|---|---:|---|
| `aho-corasick` | 1.1.5 | Unlicense OR MIT |
| `alacritty_terminal` | 0.26.0 | Apache-2.0 |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 |
| `ar_archive_writer` | 0.5.3 | Apache-2.0 WITH LLVM-exception |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 |
| `atomic-waker` | 1.1.2 | Apache-2.0 OR MIT |
| `base64` | 0.22.1 | MIT OR Apache-2.0 |
| `bitflags` | 1.3.2 | MIT OR Apache-2.0 |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 |
| `block-buffer` | 0.12.1 | MIT OR Apache-2.0 |
| `cc` | 1.4.7 | MIT OR Apache-2.0 |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 |
| `cfg_aliases` | 0.1.1 | MIT |
| `concurrent-queue` | 2.5.0 | Apache-2.0 OR MIT |
| `const-oid` | 0.10.2 | Apache-2.0 OR MIT |
| `cpufeatures` | 0.3.1 | MIT OR Apache-2.0 |
| `crossbeam-utils` | 0.8.23 | MIT OR Apache-2.0 |
| `crypto-common` | 0.2.2 | MIT OR Apache-2.0 |
| `cursor-icon` | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| `deranged` | 0.5.8 | MIT OR Apache-2.0 |
| `digest` | 0.11.3 | MIT OR Apache-2.0 |
| `downcast-rs` | 1.2.1 | MIT OR Apache-2.0 |
| `errno` | 0.3.14 | MIT OR Apache-2.0 |
| `fallible-iterator` | 0.3.0 | MIT OR Apache-2.0 |
| `fallible-streaming-iterator` | 0.1.9 | MIT OR Apache-2.0 |
| `fastrand` | 2.5.0 | Apache-2.0 OR MIT |
| `filedescriptor` | 0.8.3 | MIT |
| `find-msvc-tools` | 0.1.13 | MIT OR Apache-2.0 |
| `futures-io` | 0.3.34 | MIT OR Apache-2.0 |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 |
| `hermit-abi` | 0.5.3 | MIT OR Apache-2.0 |
| `home` | 0.5.12 | MIT OR Apache-2.0 |
| `hybrid-array` | 0.4.15 | MIT OR Apache-2.0 |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 |
| `lazy_static` | 1.5.0 | MIT OR Apache-2.0 |
| `libc` | 0.2.189 | MIT OR Apache-2.0 |
| `libsqlite3-sys` | 0.38.2 | MIT |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 |
| `log` | 0.4.34 | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Unlicense OR MIT |
| `miow` | 0.6.1 | MIT OR Apache-2.0 |
| `nix` | 0.28.0 | MIT |
| `num-conv` | 0.2.2 | MIT OR Apache-2.0 |
| `object` | 0.39.1 | Apache-2.0 OR MIT |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT |
| `piper` | 0.2.5 | MIT OR Apache-2.0 |
| `pkg-config` | 0.3.34 | MIT OR Apache-2.0 |
| `polling` | 3.11.0 | Apache-2.0 OR MIT |
| `portable-pty` | 0.9.0 | MIT |
| `powerfmt` | 0.2.0 | MIT OR Apache-2.0 |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 |
| `psm` | 0.1.32 | MIT OR Apache-2.0 |
| `quote` | 1.0.47 | MIT OR Apache-2.0 |
| `r-efi` | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| `redox_syscall` | 0.5.18 | MIT |
| `regex-automata` | 0.4.18 | MIT OR Apache-2.0 |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 |
| `rusqlite` | 0.40.2 | MIT |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `rustix-openpty` | 0.2.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 |
| `serde_stacker` | 0.1.14 | MIT OR Apache-2.0 |
| `serial2` | 0.2.38 | BSD-2-Clause OR Apache-2.0 |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 |
| `shared_library` | 0.1.9 | Apache-2.0 OR MIT |
| `shell-words` | 1.1.1 | MIT OR Apache-2.0 |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 |
| `signal-hook` | 0.4.4 | MIT OR Apache-2.0 |
| `signal-hook-registry` | 1.4.8 | MIT OR Apache-2.0 |
| `smallvec` | 1.16.1 | MIT OR Apache-2.0 |
| `stacker` | 0.1.25 | MIT OR Apache-2.0 |
| `syn` | 2.0.119 | MIT OR Apache-2.0 |
| `syn` | 3.0.6 | MIT OR Apache-2.0 |
| `thiserror` | 1.0.69 | MIT OR Apache-2.0 |
| `thiserror-impl` | 1.0.69 | MIT OR Apache-2.0 |
| `time` | 0.3.55 | MIT OR Apache-2.0 |
| `time-core` | 0.1.9 | MIT OR Apache-2.0 |
| `time-macros` | 0.2.32 | MIT OR Apache-2.0 |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 |
| `unicode-ident` | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `unicode-width` | 0.2.2 | MIT OR Apache-2.0 |
| `vcpkg` | 0.2.15 | MIT OR Apache-2.0 |
| `vte` | 0.15.0 | Apache-2.0 OR MIT |
| `winapi` | 0.3.9 | MIT OR Apache-2.0 |
| `winapi-i686-pc-windows-gnu` | 0.4.0 | MIT OR Apache-2.0 |
| `winapi-x86_64-pc-windows-gnu` | 0.4.0 | MIT OR Apache-2.0 |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_aarch64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_i686_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnu` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 |
| `winreg` | 0.10.1 | MIT |
| `zmij` | 1.0.23 | MIT |

The locked `r-efi` package is a target-specific `getrandom` dependency for UEFI,
not part of Rover's terminal OS targets. Where it is distributed, the declared
license expression permits MIT or Apache-2.0 as alternatives to LGPL-2.1-or-
later. `unicode-ident` declares Unicode-3.0 in addition to its MIT/Apache
choice. Preserve the notices for the selected licenses in any binary/source
release. The rusqlite `bundled` feature compiles SQLite 3.53.2 from the
`libsqlite3-sys` crate into the binary; its exact public-domain blessing is
retained at `licenses/third-party/libsqlite3-sys-0.38.2/SQLITE_BLESSING.txt`.
The SQLite amalgamation is an indirect bundled dependency, not copied into the
Rover source tree. No Herdr, Luvus, or Orca application source is copied.

## Verification and remaining gates

- `cargo tree --locked --offline --target <triple> -e normal,build` checks
  Linux, macOS, and Windows x86_64 dependency edges. These are inventory
  targets, not a claim that Rover has passed native build/behavior tests on all
  three operating systems.
- Cargo metadata returned no packages with a missing declared license field.
- All 163 locked registry packages have declared licenses; 304 license/author
  files and the bundled SQLite blessing are included with SHA-256 records.
  `make rust-notices` refreshes files from resolved crate sources, or from a
  same-repository matching-license fallback when a target-only archive omits
  them; `make rust-check` verifies the bundled bytes.
- The dependency set was fetched from crates.io and compiled from the lockfile;
  SHA-256 known-answer and timestamp compatibility tests pass.
- `make rust-check` runs the unit tests for the audit tool, verifies all locked
  package license expressions against an explicit allowlist, compares every
  bundled license/author file byte-for-byte with the locked crate source, and
  checks `cargo tree` for the configured Linux, macOS, and Windows x86_64 target
  triples. It also verifies the lockfile/metadata package set matches.
- `make rust-sbom` emits CycloneDX 1.7 JSON with lockfile checksums, normalized
  SPDX expressions, the package inventory, embedded SQLite source checksum,
  and dependency relationships for the configured target triples. SBOM generation uses the official
  [CycloneDX 1.7 JSON schema](https://cyclonedx.org/schema/bom-1.7.schema.json)
  identifier; this script does not yet download or execute JSON Schema
  validation.
- P-019 remains open because no Rust vulnerability scanner is wired yet.
  Hosted CI has not run for this change, so the pinned 1.88.0 result is pending
  hosted confirmation.

## Primary references

- [`sha2` 0.11.0](https://docs.rs/sha2/0.11.0/sha2/)
- [`getrandom` 0.4.3](https://docs.rs/getrandom/0.4.3/getrandom/)
- [`time` 0.3.55](https://docs.rs/time/0.3.55/time/)
- [`r-efi` 6.0.0 license metadata](https://docs.rs/crate/r-efi/6.0.0)
- [`rusqlite` 0.40.2](https://docs.rs/rusqlite/0.40.2/rusqlite/)
- [`rustix` 1.1.4](https://docs.rs/rustix/1.1.4/rustix/)
- [`rusqlite` 0.40.2 SQLite bundling and licensing](https://github.com/rusqlite/rusqlite/tree/v0.40.2)
- [`serde_stacker` 0.1.14](https://docs.rs/serde_stacker/0.1.14/serde_stacker/)
- [`stacker` 0.1.25](https://docs.rs/crate/stacker/0.1.25)
- [SQLite public-domain notice](https://www.sqlite.org/copyright.html)
