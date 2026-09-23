# Rover

[![CI](https://github.com/GrayCodeAI/rover/actions/workflows/ci.yml/badge.svg)](https://github.com/GrayCodeAI/rover/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE) [![Version](https://img.shields.io/badge/version-0.0.1-blue.svg)](https://github.com/GrayCodeAI/rover/releases/tag/v0.0.1) [![Go](https://img.shields.io/badge/Go-1.26.6%2B-00ADD8?logo=go)](go.mod)

**Terminal-first agent workspaces, persistent sessions, parallel workflows,
verification, review, and evidence.**

`0.0.1` is the initial public OSS release — terminal-first, agent-neutral, monorepo (Go + Python + TypeScript + Go SDKs). See [STATUS.md](STATUS.md) for explicit boundaries.

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
headers/library**. Python 3 runs the demonstrations and optional SDK. There are no
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
- [Command reference](docs/CLI.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Security and limitations](docs/SECURITY_MODEL.md)
- [Current validation](docs/validation/v0.2.0/REPORT.md)
- [Python CLI client](sdk/python/README.md) · [TypeScript CLI client](sdk/typescript/README.md) · [Go CLI client](sdk/go/README.md)
- [Original ten-layer plan and acceptance inventory](docs/acceptance/)

MIT. No telemetry, model training, automatic upload, push, merge, or deployment
is enabled by default. Source and user-owned Git history remain independent of Rover.
