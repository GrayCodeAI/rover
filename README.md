# Rover

[![CI](https://github.com/GrayCodeAI/rover/actions/workflows/ci.yml/badge.svg)](https://github.com/GrayCodeAI/rover/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE) [![Version](https://img.shields.io/badge/version-0.0.1-blue.svg)](https://github.com/GrayCodeAI/rover/releases/tag/v0.0.1) [![Go](https://img.shields.io/badge/Go-1.26.6%2B-00ADD8?logo=go)](go.mod)

**Terminal-first agent workspaces, persistent sessions, parallel workflows,
verification, review, and evidence.**

`0.0.1` is the initial public OSS release — terminal-first and agent-neutral. See [STATUS.md](STATUS.md) for explicit boundaries.

The Rust migration is in progress. The current published CLI and TUI remain Go;
the Rust workspace now contains core model and repository identity primitives,
versioned SQLite migrations, transactional record/event/idempotency APIs,
project-scoped grant services, and a Unix state-root and blob/file layer. It is
not yet a product replacement. Store-level agent/resource reservations,
local budget accounting, read-only legacy-state inventory, and an opt-in Go
state importer are also ported. The importer preserves the source tree, requires
explicit operator assertions, excludes runtime task/check trees, and sanitizes
grants and execution process state. Repeat imports verify copied rows, events,
request keys, receipt counts, and object digests. See the
[migration and rollback guide](docs/design/GO_RUST_STATE_MIGRATION.md).
The `rover-source` crate captures committed Git blobs and working-tree bytes
with explicit consistency labels; persistence currently uses the Unix
state-root adapter. The `rover-execution` crate has a tested Unix runner for
argv-only commands, scrubbed environments, bounded live output logs, timeouts,
and process-group cancellation. A Rust `rover tui` executable now provides an
initial Unix path that starts or reattaches to a project-scoped login shell,
streams its terminal output through the bounded screen model, and saves layout
state. Session owner crash recovery, broader CLI behavior, and native runtime
validation on Linux and Windows remain open. A separate `rover-tui` library
renders validated workspace layouts and handles basic
tab/pane keys, and can display filtered text supplied by Rover's terminal
screen model. On Unix it can attach through a caller-supplied session client,
stream output through a bounded queue, send child input/resize events, and
detach with Ctrl-]. Workspace shortcuts use Ctrl-B as a prefix while attached.
Press `?` for keyboard help in the standalone workspace, or Ctrl-B then `?`
while attached; the overlay keeps keys such as `q` from reaching the shell.
Ctrl-B then `t` opens a read-only snapshot of this project's stored tasks;
use the arrow keys to browse, Enter to inspect recorded task/process metadata,
`r` refreshes the project task snapshot while preserving the selected task when
possible. `c` requests task cancellation after a `y` confirmation; it does not
undo prior external actions. `o` or `e` opens the selected task's stdout or
stderr after checking its bytes against the recorded SHA-256. Output is
control-character sanitized and clipped to a 64 KiB display window; use the
arrow keys or Page Up/Page Down to scroll, and Esc to return to task details.
When a task links an investigation, `i` opens its stored decision, checks,
unknowns, and findings. This view labels the material as recorded assessment
evidence; it does not independently verify or rerun those checks. The loader
rejects evidence whose repository or candidate does not match the selected
task.
In task detail, `d` displays the bounded list of added, changed, and deleted
paths between the task's verified base and candidate snapshots. The full
applicable patch remains available through `rover diff`.
In the task list, `/` opens a case-insensitive search over task ID, status,
objective, candidate, and error text; Enter applies it and Esc discards the
draft. `s` cycles deterministic Updated, Status, and Objective ordering. Rover
saves the query and ordering per canonical repository in its Rust state store.
Esc or `t` closes the list.
Ctrl-B then `p` opens a searchable command palette with task, tab, help, and
file-browser actions supported by the current TUI provider.
When this project pane has an explicitly bound Codex or Claude session,
Ctrl-B then `a` shows the exact saved identity. Press `r` to review its native
interactive resume command, then `y` to send it to the attached project shell;
`d` removes Rover's binding after confirmation without stopping a running
agent. Resume uses the provider's configured permissions and sandbox; Rover
does not mediate them.
For scripts, `rover agent session bind --repo PATH --pane active --adapter codex-exec
--id EXACT_ID` records a user-selected identity for a pane in the saved project
workspace (`--pane ID` selects another saved pane). `rover agent session list --repo PATH` lists bindings;
`rover agent session status --repo PATH --pane ID` inspects one; and
`rover agent session clear --repo PATH --pane ID` removes Rover's binding only.
List and status distinguish a saved identity from live agent status, which
remains unknown until Rover has process or lifecycle evidence. Use
`--adapter claude-print` for a Claude session.
Ctrl-B then `n` opens repository-scoped local notes for reusable prompt/context
snippets. Press `n` to create, Enter to edit, or `d` to delete after confirmation;
the first line labels each snippet. In the editor, Left/Right, Home/End,
Backspace, and Delete move or change text; Ctrl-S saves and Esc asks before
discarding edits. Rover stores at most 32 notes, 32 KiB each and 128 KiB total.
Notes remain local and are not submitted to an agent automatically.
Ctrl-B then `d` opens local task briefing drafts. `n` creates a draft, Enter edits,
and `d` deletes after confirmation. Drafts hold a title, path globs, dependencies,
quality-gate text, and multiline prompt; Ctrl-S saves and Esc confirms before
discarding edits. Up to 32 drafts are stored per repository (120-byte title,
8 KiB paths, 4 KiB dependencies, 1 KiB gate, and 16 KiB prompt). Drafts are not
tasks, path globs and gate text are not interpreted, and nothing is sent to a worker.
Ctrl-B then `k` opens repository-scoped saved shell commands. Use `n` to create,
Enter to edit, `d` to delete after confirmation, and `r` to review a command
before sending it once to the attached shell. Commands are single lines, limited
to 256 bytes, and run with Rover's OS-user authority; they are not sandboxed.
Ctrl-B then `f` opens the repository file tree; arrows move, Enter
opens directories or a file preview, Backspace goes to the parent, `h` toggles
hidden entries, and `r` refreshes. Git changes receive status labels and colors.
Ctrl-B then `.` opens quick open, which searches hidden repository files too.
Previews refuse symlinks and special files, sanitize terminal controls, and show
at most 256 KiB. Press `e` on a text preview to edit files up to 8 MiB; Ctrl-S
saves with an atomic replacement and stale-content check, Ctrl-D shows the
unsaved unified diff with three context lines. Ctrl-E opens a private working
copy in the executable named by `VISUAL` or `EDITOR` (default `vi`), and Esc
prompts before discarding edits.
The editor setting is one executable path, invoked without a shell; its changes
return to the TUI buffer and still require Ctrl-S to save. `.git` metadata is
excluded from both browsers.
This initial executable path is not a product replacement; Go remains
authoritative until the Rust parity gates pass.
Track the staged port in [the Rust parity
plan](docs/design/RUST_PARITY_PLAN.md).

## Run the complete demonstrations

```sh
make build
make version-check
make manifest-check
make check
make demo
make demo-extended
./bin/rover help
```

The demonstrations use **owned deterministic fixture workers**, not model accounts.
They exercise detached tasks, Linux PTYs, repair attempts, parallel dependencies,
candidate integration, verification, counterfactual tests, finite mutations,
context, reversible agent instructions, MCP, remote CLI control, revocation,
backup/restore, and signed evidence. No automatic GitHub push, merge, or deployment.

Build prerequisites: **Linux, Git, Go 1.26.6 or newer, a C compiler and system SQLite development
headers/library**. Python 3 runs the deterministic demonstrations. There are no
external Go modules. This release still uses cgo/system SQLite, not a dependency-free
static binary. The current release gate requires the patched Go 1.26.6 line; validate
with a supported toolchain.

## Two supported workflow shapes

1. Keep using your coding agent and invoke Rover to inspect and verify its output.
2. Let Rover supervise a command/agent session in its own worktree and verify the
   resulting candidate, optionally repairing or coordinating dependent tasks.

The global `--state` option **must precede the command**. It defaults to
`$ROVER_HOME` or `~/.config/rover`; pass `--state` to override. Choose a
private state directory outside all repositories. Do not replace another
installed `rover` binary.

Shortcuts: `check --repo . --worktree --allow-local` runs inspect + verify +
decision in one step; `do --file task.json --allow-local` runs a task in the
foreground instead of polling `status`/`logs`. Aliases: `st`, `lg`, `wf`, `rep`.

```sh
ROVER="$PWD/bin/rover"
STATE="$HOME/.local/state/rover-dev"
"$ROVER" --state "$STATE" init --repo /path/to/repo --json         # preview
"$ROVER" --state "$STATE" init --repo /path/to/repo --apply --json # explicit write
"$ROVER" --state "$STATE" inspect --repo /path/to/repo --base HEAD --worktree --json
"$ROVER" --state "$STATE" verify --repo /path/to/repo --base HEAD --worktree --allow-local --json
```

Review and commit `.rover/config.json` yourself. Strict JSON is used in this alpha.
Rover does not silently overwrite instructions, install dependencies, fetch provider
credentials, or upload repository data. An empty verification plan is inconclusive.

### Tasks and agent profiles

```sh
"$ROVER" --state "$STATE" agent list --json
"$ROVER" --state "$STATE" task run --file task.json --allow-local --key issue-123 --json
"$ROVER" --state "$STATE" status --json
"$ROVER" --state "$STATE" tui
"$ROVER" --state "$STATE" attach --id TASK_ID # interactive PTY tasks; Ctrl-] detaches
```

`generic-headless` and Linux `generic-pty` execute reviewed argv. `codex-exec` and
`claude-print` generate native CLI arguments and interpret JSONL completion/failure
and usage events. **These native profiles are transcript/subprocess-fixture tested,
not live-provider-certified.** They do not implement full Codex App Server, ACP, or
native conversation migration. Authentication requires explicit approved configuration.
Never use them to bypass provider permissions, plans, or account restrictions.

```json
{
  "schema": "rover/v1alpha1",
  "objective": "Implement the approved task without weakening checks",
  "repository": "/absolute/path/to/repo",
  "base": "HEAD",
  "agent": "generic-headless",
  "argv": ["your-installed-agent", "{{objective}}"],
  "timeout": "20m",
  "max_attempts": 1,
  "auto_verify": true
}
```

Replace the executable with an approved installed command. A submitted task is not
accepted software. `logs --follow` is a log stream, not terminal attachment.
Repair loops require explicit limits; all failed attempts remain in the evidence.

### Parallel workflows

`workflow run --file workflow.json --allow-local --key KEY` starts a detached DAG
supervisor. `--foreground` uses the caller lifetime instead. Nodes get separate
worktrees, named reservations and exact dependency snapshots. Disjoint changes can
be composed; conflicting edits stop rather than being guessed. Final integration is
verified again. Unreviewed handoffs require an explicit opt-in; they are not approvals.

### Review and deeper checks

```sh
"$ROVER" --state "$STATE" report --id INVESTIGATION_ID --json
"$ROVER" --state "$STATE" diff --id INVESTIGATION_ID --output change.patch
"$ROVER" --state "$STATE" review --id INVESTIGATION_ID --note "Reviewed this candidate" --json
"$ROVER" --state "$STATE" replay --id INVESTIGATION_ID --allow-local --json
```

Checks include command exit, structured Go tests, JUnit and SARIF. Required missing
or malformed results, no expected tests, timeouts, truncation and modified inputs do
not become passing checks. `prove regression` compares a named intended failure on
the base with a passing candidate; import/setup failures are not regression proof.
`mutate` executes a bounded, explicitly supplied set of text mutations. It is not a
universal language-aware mutation engine. Test-change detectors are labeled heuristics.
Existing formal/fuzz/security tools can be invoked as approved commands; native deep
verifier interpretation must not be inferred from a generic command's exit code.

### Tool and remote interfaces

`mcp --repo ...` provides newline-delimited stdio MCP. `serve --repo ...` provides a
**JSON-only, stateless Streamable HTTP subset pinned to MCP 2025-11-25**. No SSE,
OAuth discovery or ACP implementation is claimed. Read-only by default. Execution
requires `--enable-execution --allow-local`; it carries the server user's authority.

HTTP requires project/audience-scoped bearer grants. Plain HTTP is restricted to
numeric loopback; nonloopback requires TLS. Browser Origins are rejected. Requests
are bounded, cancellation is supported, and duplicate in-flight IDs are refused.

```sh
"$ROVER" --state "$STATE" serve --repo /repo --listen 127.0.0.1:8765
"$ROVER" --state "$STATE" grant create --repo /repo --tool rover_status --note "Personal read access" --json
# Store the returned token in a private, owner-only file; never commit it.
"$ROVER" --state /private/client-state remote node-add --node workstation \
  --endpoint http://127.0.0.1:8765/mcp --token-file /private/token --json
"$ROVER" --state /private/client-state remote tools --node workstation --json
```

Real local TCP/TLS tests and remote-CLI-to-local-server task submission were run.
This is remote **control**, not a validated multi-host worker fleet, tunnel service,
or cross-machine conversation migration. See [docs/CLI.md](docs/CLI.md).

### Context, maintenance, and learning

Snapshot search/bundles and explicit expiring memory are repository scoped. Memory
is a recorded assertion, not current evidence. `integrate` previews reversible
instruction-file changes; instructions do not enforce agent behavior.

`backup` uses SQLite's online backup API plus retained objects. `restore` requires a
new destination, revokes grants and marks active work lost. It does not restore live
processes or mutable worktrees. Backups are sensitive plaintext.

`attest` signs completed retained investigations with explicit Ed25519 keys and
verifies against an explicitly trusted public key. This proves signature validity,
not correctness, independent administration, SLSA level, or in-toto compliance.

`learn` implements historical check-order recommendations, disjoint holdout datasets,
offline evaluation and explicit local promotion/revocation. A promoted strategy can
only reorder the complete approved check list and is used only with
`verify --strategy ID`. It cannot remove checks. This is not an RL-trained model,
autonomous self-modifying product, or enterprise-separated evaluator.

## Trust and support

**Local execution is NOT a sandbox.** A same-user worker can potentially access or
modify same-user host resources. Scoped HTTP grants restrict API operations, not
arbitrary code once local execution is authorized. Keep untrusted code away from
credential-bearing hosts. The restricted Docker adapter has argument/admission tests
but has not been executed against a Docker daemon here. No protected CI publisher,
team SSO, remote worker fencing or hostile-code security certification is claimed.

Source capture still rejects symlinks, submodules, unresolved LFS pointers, unsafe or
case-colliding paths and oversized source states. Working-tree capture uses repeated
reads, not an atomic filesystem transaction. Prefer committed snapshots.

Linux/amd64 is the validated envelope. darwin/arm64 is observed locally (PTY/TUI
supported via posix_openpt); native macOS and Windows full validation remain
pending. The repository and package name
are proposed: no public GitHub repo, package-manager release or hosted CI execution
is claimed. The public naming collision noted in the design remains unresolved.

## Documentation

- [Implementation status](STATUS.md)
- [Rust parity plan](docs/design/RUST_PARITY_PLAN.md) · [Rust third-party notices](licenses/THIRD_PARTY_NOTICES.md)
- [Command reference](docs/CLI.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Security and limitations](docs/SECURITY_MODEL.md)
- [Current validation](docs/validation/v0.2.0/REPORT.md)
- [Original ten-layer plan and acceptance inventory](docs/acceptance/)

MIT. No telemetry, model training, automatic upload, push, merge, or deployment
is enabled by default. Source and user-owned Git history remain independent of Rover.
