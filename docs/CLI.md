# Command reference — 0.2.0-alpha.1

Build first. All examples use `./bin/rover --state /private/state COMMAND ...`.
`--state` is global and must precede the command. IDs come from JSON output, not
from filenames. Advanced commands generally produce JSON; basic commands provide
human text unless `--json`. Errors do not silently become acceptance.

## Installation and diagnostics

`make build`, `make install`, `help`, `version`, `doctor --json`.
No published package manager, signing service, or automatic updater is implied.
`init --repo REPO` previews. `--apply` writes a new `.rover/config.json` only when
absent. Review/commit this config before using it as the approved baseline.

## Inspect / verify

```
inspect --repo REPO --base REF [--worktree --include-untracked] [--config TRUSTED_FILE]
verify  --repo REPO --base REF [--worktree --include-untracked] [--config TRUSTED_FILE]
        --mode local-advisory --allow-local [--strategy PROMOTED_ID] --json
```

Do not assume `main` exists; the default is explicit `HEAD`. Without `--worktree`,
source is the selected committed state. Config is taken from the base unless an
explicit trusted config file is selected. Local execution is not a sandbox.

Verification codes: 0 accepted under supplied policy, 1 required check failed,
2 error/inconclusive, 3 checks satisfied but review required. Report/read commands
return read-operation status instead; always inspect the JSON `decision` field.

## Tasks / sessions

```
agent list
agent capabilities generic-pty
task run --file TASK_JSON --allow-local [--key IDEMPOTENCY_KEY]
status [--id TASK]
logs --id TASK [--follow]
cancel --id TASK
ui [--watch]
tui
attach --id TASK
limits [--max-agents N]
```

Interactive commands require an actual Linux terminal. `attach` is for interactive PTY tasks;
Ctrl-] detaches. A client detach is not task cancellation. `tui` supports j/k or
arrows, views for logs/evidence/diff, attach, refresh, q, and confirmed local
review/cancellation. A text dashboard remains available without a raw terminal.

Task JSON adds `agent`, `agent_options`, `interactive`, `max_attempts`, `repair_argv`,
`reservations`, `initial_snapshot`. Generic adapters require explicit argv. Native
profiles use an empty `argv` and build their own reviewed CLI arguments. `max_attempts`
defaults to one and is bounded. Repair is only attempted for failed acceptance checks,
not arbitrary tool/parser errors. Failed attempts are retained in `attempts`.

`agent_options`: optional `executable`, `model`, `max_turns`, `write`, `allowed_tools`.
Do not put provider secrets in arguments or committed task files. Explicit inherited
environment settings and adapter configuration may still grant sensitive access.

## Workflows

```
workflow run --file WORKFLOW_JSON --allow-local [--key KEY] [--foreground]
workflow status [--id WORKFLOW]
workflow cancel --id WORKFLOW
```

Workflow fields: schema, objective, repository, base, nodes, max_parallel, timeout,
config_path, allow_unreviewed_handoffs. Node tasks remain local-advisory. A node has id, depends_on, task.
Cycles and unknown dependencies fail validation. Leaf changes compose conservatively;
conflicting file edits stop integration. Final composed source is verified again.
Native sessions do not migrate between tasks. Unreviewed handoffs are explicit policy
exceptions; they do not create human approval records.

## Review / source

```
report --id INVESTIGATION --json
review --id INVESTIGATION --note TEXT --json
outcome --id INVESTIGATION --label LABEL --note TEXT
export --kind investigation --json  # bounded metadata page; use backup for retained objects
replay --id INVESTIGATION --allow-local [--mode local-advisory|restricted-docker --image DIGEST]
diff --id INVESTIGATION [--output NEW_PATCH_FILE | --json]
diff --base SNAPSHOT --candidate SNAPSHOT [--output NEW_PATCH_FILE | --json]
```

Review is a same-user assertion tied to candidate/config, not team authentication or
a merge. Text diff output sanitizes control characters; use `--output`/JSON for
exact patch content. Replays create new evidence without overwriting the old attempt.

## Counterfactuals / mutation

```
prove regression --repo REPO --base-snapshot BASE_ID --candidate CANDIDATE_ID \
  --file REGRESSION_JSON --allow-local
mutate --repo REPO --base-snapshot BASE_ID --candidate CANDIDATE_ID \
  --file MUTATION_JSON --allow-local
```

These also accept `--base REF` and `--worktree --include-untracked`, with an optional
explicit config. Regression JSON: `schema`, `check_id`, `test_paths`, `test_id`,
`failure_contains`. The named testcase must reproduce the intended failure on base
and pass on candidate; ambiguous/setup/import failures are inconclusive.
Mutation JSON: `schema`, `check_ids`, `mutations:[{id,path,before,after}]`. Each
replacement must be unambiguous and occur once. Outcomes remain per-mutant and
explicitly synthetic; this is not AST mutation or unlimited fuzzing.

## Context / memory / agent instructions

```
context search --snapshot ID --query TEXT [--limit N]
context search --repo REPO --base REF --query TEXT
context bundle --snapshot ID --path FILE [--path FILE ...]
memory put --snapshot ID --text TEXT --origin human|agent --ttl 24h
memory list --snapshot ID
memory delete --id NOTE
integrate --repo REPO --agent generic|codex|claude|gemini|opencode
integrate --repo REPO --agent generic --apply [--expected-before SHA]
integrate --undo RECEIPT
```

Search is literal and bounded, not a semantic graph. Notes are assertions with scope
and expiry. Deletion does not erase historical audit payloads/backups. Instructions
are an approved, reversible managed block, not enforcement. Undo refuses to overwrite
new user edits. No package install or repository hook executes during preview.

## MCP / scoped HTTP

```
mcp --repo REPO --base REF [--enable-execution --allow-local]
serve --repo REPO --base REF --listen 127.0.0.1:8765 [--ready-file NEW_JSON]
      [--tls-cert CERT --tls-key KEY] [--enable-execution --allow-local]
grant create --repo REPO --tool rover_status --tool rover_report --ttl 1h --note REASON
grant list --repo REPO
grant revoke --repo REPO --id GRANT
```

Protocol: 2025-11-25, JSON-RPC 2.0. Stdio uses one JSON message per line. HTTP uses
POST /mcp, bearer authorization and the MCP-Protocol-Version header after initialize.
No SSE, OAuth, web UI, or permissive browser CORS. Initialization is negotiated;
unknown versions/capabilities are not claimed. Read-only by default. Local execution
requires explicit enablement and still carries the server user's host authority.

Tool schemas are discoverable through `tools/list`. Read operations cover agent
capabilities, inspect, status, report, diff, context search. Explicit execution adds
verify/task-run/task-cancel. No grant, review, key, merge or deployment tool is exposed.

## Remote CLI

```
remote node-add --node NAME --endpoint URL --token-file PRIVATE_FILE [--ca-file PEM]
remote node-list
remote node-delete --node NAME
remote tools --node NAME
remote call --node NAME --tool rover_status [--arguments JSON_FILE] [--timeout 30s]
```

Inline endpoint/token-file/CA flags can replace a stored node. Node records store
credential paths, not token bytes. Owner-only token files are required. No TLS bypass,
redirect following, arbitrary environment proxy or automatic write retry. A transport
timeout may be an unknown operation outcome; reconcile before reissuing work.

## Maintenance / signatures

```
backup --to NEW_DIRECTORY
backup-check --from BACKUP_DIRECTORY
restore --from BACKUP_DIRECTORY --to NEW_STATE_DIRECTORY
attest keygen --key NEW_PRIVATE_PEM --public-key NEW_PUBLIC_PEM
attest sign --id INVESTIGATION --key PRIVATE_PEM [--out NEW_ENVELOPE]
attest verify --file ENVELOPE --public-key EXPECTED_PUBLIC_PEM
```

Backup includes online metadata and retained objects, not mutable worktrees or running
session log directories. Restore revokes grants/leases and marks active work LOST.
Data is not encrypted. Signatures prove origin relative to the trusted key, not code
correctness or independently administered CI. Protect keys separately from workers.

## Learning / evaluation

```
learn recommend --repo REPO --base REF
learn dataset --file DATASET_JSON
learn evaluate --id PROPOSAL --dataset DATASET
learn promote --id EVALUATION --note EXPLICIT_REVIEW
learn show --id ID
learn revoke --id PROMOTION --note REASON
verify ... --strategy PROMOTION
```

Dataset JSON has schema, description, partition(training|holdout), and cases:
`{investigation,label(confirmed-defect|confirmed-clean|unknown),attribution,synthetic}`.
This is local check-order experimentation. All mandatory checks stay present. Proposal
and dataset digests bind evaluation. Training candidate overlap is rejected. Promotion
never happens automatically; local review is not enterprise-separated evaluator trust.
