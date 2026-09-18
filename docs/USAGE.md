# Usage Guide

## Getting started

```bash
make build          # builds ./bin/rover
make install        # installs to ~/.local/bin/rover (refuses overwrite)
./bin/rover version --json
./bin/rover doctor --json
```

## Verifying a repository

```bash
# Inspect the current state against a known-base
./bin/rover inspect --repo . --base HEAD --json

# Run acceptance checks with local execution
./bin/rover verify --repo . --base HEAD --mode local-advisory --allow-local --json
```

Verification exit codes: 0 accepted, 1 check failed, 2 error/inconclusive,
3 checks passed but review required.

## Running a task

```bash
# Create a task file (see examples/task.generic.json)
./bin/rover task run --file task.json --allow-local --json

# Monitor progress
./bin/rover status --id <task_id> --json
./bin/rover logs --id <task_id>
./bin/rover logs --id <task_id> --follow

# Cancel if needed
./bin/rover cancel --id <task_id>
```

### Interactive PTY tasks

Set `"interactive": true` in the task spec. The task exposes a Unix socket
for terminal I/O. Use `attach --id <task_id>` to connect, or pipe frames
directly:

```bash
./bin/rover attach --id <task_id>
```

Ctrl-] detaches the terminal session — it does **not** cancel the task.

### Human TUI

```bash
./bin/rover --state /path/to/rover tui
```

Interactive terminal UI with j/k or arrow navigation, q to quit, and
confirmed local review/cancellation. A text dashboard is available without
a raw terminal.

## Workflows

```bash
./bin/rover workflow run --file workflow.json --allow-local --json
./bin/rover workflow status --id <workflow_id> --json
./bin/rover workflow cancel --id <workflow_id>
```

Workflows execute task nodes with dependency ordering (`depends_on`). Cycles
and unknown dependencies fail validation. Conflicting file edits stop
integration, and the composed result is verified again before final acceptance.

## Reviewing proposals

```bash
# After a verification, review the investigation
./bin/rover review --id <investigation_id> --note "Looks good" --json

# Approve or reject
./bin/rover outcome --id <investigation_id> --label accept --note "Approved" --json
```

## Managing remote nodes

```bash
./bin/rover remote node-add --node worker1 --endpoint https://10.0.0.1:8765 --token-file ~/.ssh/rover-token
./bin/rover remote node-list
./rover remote node-delete --node worker1
```

Node records store credential file paths, not token bytes. Token files must
be owner-readable only.

## Backup and restore

```bash
./bin/rover backup --to /mnt/backup/rover-$(date +%F)
./bin/rover backup-check --from /mnt/backup/rover-2024-01-01
./bin/rover restore --from /mnt/backup/rover-2024-01-01 --to /new/state
```

Restore revokes all active grants and leases, marking running work as `LOST`.
Backups include online metadata and retained objects — not mutable worktrees
or running session log directories.

## Signing evidence

```bash
# Generate a key pair
./bin/rover attest keygen --key ~/.rover/keys/sign.pem --public-key ~/.rover/keys/sign.pub

# Sign an investigation
./bin/rover attest sign --id <investigation_id> --key ~/.rover/keys/sign.pem

# Verify
./bin/rover attest verify --file envelope.json --public-key ~/.rover/keys/sign.pub
```

Signatures prove origin relative to the trusted key — they do not certify
code correctness or independently administered CI. Protect keys separately
from workers.

## Using as an MCP server

```bash
./bin/rover mcp --repo . --base HEAD --enable-execution --allow-local
```

Run with `--enable-execution --allow-local` to expose task execution tools.
Without these flags, the server is read-only (inspect, status, report, etc.).

## CLI flags

### Global flags
- `--state PATH` — state directory (default: `$ROVER_HOME` or OS config dir)
- `--json` — output JSON (strip before subcommand dispatch)
- `--no-color` — disable colored output

### Per-command flags
See `docs/CLI.md` for the full reference.

## Error handling

Rover never silently turns errors into acceptance. All CLI commands return:
- Exit 0: success
- Exit 1: check failure (acceptance policy)
- Exit 2: error/inconclusive
- Exit 3+: CLI or infrastructure errors

When using `--json`, always inspect the `decision` field in investigation
output, not just the exit code.
