# Rover implementation architecture

This release keeps the original ten logical capability layers and a modular codebase.
They are not ten independent services. Public commands and protocol tools call the
same task, snapshot, assurance and storage implementations.

```text
CLI / keyboard TUI / Python CLI client / MCP clients
                        |
             local operator or scoped grant
                        |
       application workflows and fixed project service
          /             |                   \
 tasks + PTY     dependency scheduler     read/search/report
 worktrees         snapshot handoffs            |
       \                |                       /
                exact candidate
                       |
        baseline-approved verification config
                       |
  execution -> structured interpretation -> evidence -> local review
                       |
  backup/signatures/context/outcomes/evaluation (explicit operations)
```

## Source ownership

- `internal/cli`: human/JSON interfaces and command routing.
- `internal/agents`: declared profiles, native argv and bounded JSONL parsing.
- `internal/tasks`, `execution`, `terminal`: detached runs, repair attempts, process
  groups, PTYs and same-user terminal brokers.
- `internal/workflow`: approved dependency graphs and integrated snapshot checking.
- `internal/source`: safe Git capture, materialization, overlays, composition, diffs.
- `internal/config`, `wire`: strict validated configuration and protocol decoding.
- `internal/assurance`: check interpreters, counterfactuals, mutation, replay, decisions.
- `internal/contextstore`: bounded snapshot context and explicit expiring notes.
- `internal/access`, `service`, `mcp`: grants, project-scoped API and transport clients.
- `internal/archive`, `attestation`, `integration`: maintenance and integrity workflows.
- `internal/learning`: bounded check-order proposals and explicit local promotion.
- `internal/store`, `model`: typed identities, SQLite transactions/events and artifacts.

## Invariants

Execution completion, provider assertions, test outcomes, policy acceptance and human
review remain distinct. Verification sees retained source bytes, not an actively
edited directory. Candidate configuration cannot silently replace a frozen baseline
check plan. Results reference attempts and candidates. Repair attempts keep earlier
failures. Dependency branches are composed conservatively and rechecked together.

API grants scope resources and tools. They are not OS isolation: explicitly enabled
local execution carries the server user's permissions. Native CLI arguments are not
proof that a real provider will behave exactly like a fixture. Same-user artifacts
and signatures are not independently administered acceptance evidence.

## Persistence and failure handling

SQLite owns metadata with transactional events. Task and workflow admission use
idempotency keys. Ambiguous supervisor failure is marked LOST rather than blindly
resubmitted. No arbitrary external side-effect replay is implemented. A local PTY
session may outlive a client while its supervisor/host lives; no host-loss continuity
or native conversation migration is promised.

Large outputs use bounded storage and record truncation. Artifact paths and hashes
are checked. There is no full global disk quota or evidence garbage collector; use
external disk/resource limits. Online backups combine a consistent SQLite copy with
retained objects. Restores revoke grants/leases and do not restart old processes.

## Remote and model interfaces

MCP is pinned to protocol 2025-11-25. The HTTP implementation intentionally supports
JSON responses, not SSE/OAuth/browser transports. It rejects browser Origins,
requires grants, bounds concurrent work, supports request cancellation, and requires
TLS for nonloopback listeners. Remote client retries no operations automatically.
Full remote worker execution/fencing and independently trusted publication remain
unimplemented. Configure a real TLS certificate and protected host for deployment.

## Learning authority

Only a complete permutation of checks can be proposed. Dataset/proposal digests bind
an evaluation; overlapping training/holdout candidates are refused. Promotion needs
an explicit local note and never activates itself. `verify --strategy` is required.
This is a small evaluable scheduling mechanism, not a trained RL model, proof of
improved defect detection, or self-authorizing RSI system.
