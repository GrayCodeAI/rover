# Rust port preview (`rover-rs`)

> **This is not the Rover product.** The released product is the Go `rover`
> binary described in the [README](../README.md). The Rust workspace under
> `crates/` is an unreleased port. No release ships it, and its behaviour and
> state format may change without notice. Go remains authoritative until the
> removal gates in the [Rust parity plan](design/RUST_PARITY_PLAN.md) pass.
> Progress is tracked in [STATUS.md](../STATUS.md#rust-port-preview-unreleased).

## Build and run

Requirements: Linux or macOS, `rustup` with the pinned 1.88.0 toolchain
(`rust-toolchain.toml` selects it), and a C compiler. `rusqlite` builds a bundled
SQLite, so no system SQLite headers are needed for the Rust build.

```sh
rustup toolchain install 1.88.0 --profile minimal --component clippy --component rustfmt
cargo +1.88.0 build -p rover-cli --locked
target/rust-1.88.0/debug/rover-rs --version
target/rust-1.88.0/debug/rover-rs tui --repo /path/to/repo
```

The executable is named `rover-rs` so it can never shadow the Go `rover` on
`PATH`. `rover-rs --version` labels itself as an unreleased preview. The name
changes to `rover` only at the final cutover (parity plan task P-164).

Platforms: interactive use has been observed on macOS only. The CI `rust` job
runs the Rust test suite on Linux (ubuntu-24.04). Windows is **not supported**:
`rover-rs tui` session hosting needs Unix, and nothing in CI compiles the
workspace for a Windows target.

Contributor gates are `make rust-check CARGO='cargo +1.88.0'` and
`make rust-sbom` (see [CI reference](CI.md)).

## State

The Rust preview keeps its own state, separate from the Go product:
`--state PATH`, else `$ROVER_RUST_HOME`, else `$XDG_STATE_HOME/rover-rust`,
else `~/.local/state/rover-rust`. The Go product uses `--state`, `$ROVER_HOME`
or `~/.config/rover`. An opt-in importer (a store API; there is no CLI command
for it yet) copies Go state into a separate Rust state root:
it keeps the source tree, requires explicit operator assertions, excludes runtime
task/check trees, and sanitizes grants and execution process state. Repeat
imports verify copied rows, events, request keys, receipt counts, and object
digests. See the [migration and rollback guide](design/GO_RUST_STATE_MIGRATION.md).

## What the workspace implements so far

- `rover-core`: model and repository identity primitives, digests, and time.
- `rover-store`: versioned SQLite migrations; transactional record, event and
  idempotency APIs; a Unix state-root and blob/file layer; store-level agent and
  named-resource reservations; local budget accounting; read-only legacy-state
  inventory and the opt-in Go state importer.
- `rover-access`: locally issued bearer grants bound to an exact project and
  state root.
- `rover-tasks`: project-bound durable task plans, kept separate from task runs
  and never dispatched on their own.
- `rover-source`: captures committed Git blobs and working-tree bytes with
  explicit consistency labels (persisted through the Unix state-root adapter).
- `rover-execution`: a tested Unix runner for argv-only commands with scrubbed
  environments, bounded live output logs, timeouts, and process-group
  cancellation; a task supervisor; PTY sessions; the workspace layout model; a
  bounded terminal screen model.
- `rover-agents`: agent profiles, native transcripts, detection evidence, state
  rollups, lifecycle authority, saved session bindings, and a bounded evaluator
  for the audited Herdr detection manifests.
- `rover-tui`: the Ratatui workspace renderer and attached-session bridge.
- `rover-cli`: the `rover-rs` executable.

## `rover-rs tui`

`rover-rs tui [--state PATH] [--repo PATH]` starts or reattaches to a
project-scoped login shell owned by a detached local process. It streams the
shell's output through the bounded screen model and saves the workspace layout.
Ctrl-] detaches. While attached, workspace shortcuts use the Ctrl-B prefix.
Press `?` for help in the standalone workspace, or Ctrl-B then `?` while
attached. The help overlay keeps keys such as `q` from reaching the shell.

| Keys (attached) | Opens | Keys inside the view and limits |
|---|---|---|
| Ctrl-B `t` | Read-only snapshot of this project's stored tasks | Arrows browse and Enter shows recorded task/process metadata. `r` refreshes and keeps the selection where possible. `c` then `y` requests cancellation; prior external actions are not undone. `o`/`e` show stdout/stderr after checking the full bytes against the recorded SHA-256; output is control-sanitized and clipped to a 64 KiB window (arrows, Page Up/Down, Esc to return). `i` shows the linked investigation's stored decision, checks, unknowns, and findings as recorded evidence, not a re-run; evidence for another repository or candidate is rejected. `d` lists added/changed/deleted paths between the verified base and candidate snapshots; use the Go `rover diff` for the full patch. `/` searches ID, status, objective, candidate, and error text (Enter applies, Esc discards). `s` cycles Updated, Status, and Objective order. The query and order are saved per repository. Esc or `t` closes. |
| Ctrl-B `p` | Searchable command palette | Task, tab, help, and file-browser actions that the current provider supports. |
| Ctrl-B `a` | The exact saved Codex or Claude session bound to the active pane | `r` reviews the native interactive resume command and `y` sends it to the attached shell. `d` removes Rover's binding after confirmation without stopping a running agent. Resume uses the provider's own permissions and sandbox; Rover does not mediate them. |
| Ctrl-B `n` | Repository-scoped notes for reusable prompt/context snippets | `n` creates, Enter edits, `d` deletes after confirmation. The first line labels each note. Editor: Left/Right, Home/End, Backspace, Delete; Ctrl-S saves; Esc asks before discarding. At most 32 notes, 32 KiB each, 128 KiB total. Notes stay local and are never sent to an agent automatically. |
| Ctrl-B `d` | Local task briefing drafts | `n`, Enter, `d` as for notes. A draft holds a title, path globs, dependencies, quality-gate text, and a multiline prompt. Up to 32 drafts per repository (120-byte title, 8 KiB paths, 4 KiB dependencies, 1 KiB gate, 16 KiB prompt). Drafts are not tasks: globs and gate text are not interpreted and nothing is sent to a worker. |
| Ctrl-B `k` | Repository-scoped saved shell commands | `n`, Enter, `d` as above. `r` shows the full command and an unsandboxed-execution warning; `y` sends it once to the attached shell. Single lines of at most 256 bytes, 32 entries. Commands run with Rover's OS-user authority and are not sandboxed. |
| Ctrl-B `f` | Repository file tree with Git status labels and colours | Arrows move, Enter opens a directory or file preview, Backspace goes to the parent, `h` toggles hidden entries, `r` refreshes. |
| Ctrl-B `.` | Quick open | Also searches hidden repository files. |
| Ctrl-B `?` | Keyboard help | |

File previews refuse symlinks and special files, sanitize terminal controls, and
show at most 256 KiB. `.git` metadata is excluded from both browsers. Press `e` on
a text preview to edit files up to 8 MiB. In the editor, Ctrl-S saves through an
atomic replacement with a stale-content check, Ctrl-D shows the unsaved unified
diff with three context lines, and Esc asks before discarding edits. Ctrl-E opens
a private working copy in the executable named by `VISUAL` or `EDITOR` (default
`vi`). That setting is one executable path, invoked without a shell. Its changes
return to the TUI buffer and still need Ctrl-S to save. The
[security model](SECURITY_MODEL.md) describes these trust boundaries.

## Other `rover-rs` commands

```sh
rover-rs agents [--json]
rover-rs agent list
rover-rs agent capabilities <adapter>
rover-rs agent session bind   --repo PATH --pane active|ID --adapter codex-exec|claude-print --id EXACT_ID [--state PATH]
rover-rs agent session list   --repo PATH [--state PATH]
rover-rs agent session status --repo PATH --pane ID [--state PATH]
rover-rs agent session clear  --repo PATH --pane ID [--state PATH]
```

`agent session bind` records a user-selected session identity for a pane in the
saved project workspace (`--pane active` or a saved pane ID). `list` and `status`
distinguish a saved identity from live agent status. Live status stays unknown
until Rover has process or lifecycle evidence. `clear` removes Rover's binding
only. The Go `rover` has none of the `agent session` commands.

## Known gaps

Session-owner crash/restart recovery, the broader product CLI (parity tasks P-131
and P-132), full keyboard/layout interaction, lifecycle-event transport, native
Linux runtime validation of the interactive TUI, and any Windows support remain
open. Architecture decisions are indexed in [design/README.md](design/README.md).
