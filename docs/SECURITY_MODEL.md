# Security model — development release

## Supported local threat model

Rover resists accidental candidate drift, missing/malformed required check output,
unsafe file paths, stale instruction undo, duplicate dispatch, cross-project API
requests, token replay after revocation, report corruption and invalid signatures
within the tested local environment. These tests are not a hostile-code security audit.

**Local execution and local administration share the OS user's authority.** A worker
may access same-user host resources. Worktrees, scrubbed environments, process names
and bearer grants do not sandbox arbitrary executable code. Do not expose execution
on a host holding unrelated credentials or run hostile repositories there.

## Authority boundaries

- Read-only MCP is default. Execution requires explicit server flags and a grant for
  the tool. Tool grants do not promise that arbitrary local code cannot access the host.
- Bearer tokens are stored as hashes in controller state; client tokens are private
  files. Node records reference paths instead of storing raw tokens. Audience binds
  to the service's state/project identity. Revocation affects future authorization,
  not undo of already authorized effects.
- No API tool can create grants, approve reviews, sign results, merge or deploy.
- Nonloopback HTTP requires TLS; redirects and browser Origins are refused. JSON-only
  clients are supported; do not advertise browser/OAuth/SSE capabilities.
- Docker has restrictive generated arguments and no local fallback, but the backend
  is not live-tested. Container identity/configuration and host permissions matter.
- PTY sockets are same-user and private. Full interactive terminals inherently expose
  untrusted terminal output; use trusted code. Noninteractive views sanitize control
  characters; use `diff --output` or JSON for exact raw patch data.

## Evidence limitations

A zero command exit is not proof that expected tests ran. Go/JUnit/SARIF interpreters
have explicit semantics and errors. Candidate-generated tests/reports may still be
wrong or dishonest. Native agent claims remain assertions. No generic command result
should be described as Lean proof, Kani universality, secure code, or all edge cases.

Ed25519 verifies origin relative to the supplied public key. The local owner controls
both signer and store, so this is not independent acceptance, a transparency log,
SLSA compliance or universal correctness. Do not expose private signing keys to agents.

## Data and maintenance

Backups contain plaintext source/evidence/context. They are not encrypted. Restores
require a new directory, verify hashes, revoke grants, clear resource leases and mark
active work LOST. They do not restore mutable workspaces or live sessions. Evidence
and memory may contain sensitive content; no automatic network upload is enabled.
Record deletion does not erase past audit event payloads or already exported backups.
Redaction and filename exclusions are best effort, not full data-loss prevention.

No private inputs go to shared learning automatically. Synthetic and human-confirmed
labels remain distinguished. Local evaluation/promotion is not independent team RBAC.
A malicious local administrator can change code, tests, labels, keys and policy.

## Unsupported guarantees

Independent CI publisher, credential brokerage, IdP/SSO, remote worker fencing,
strong tenant isolation, secure auto-update distribution, live sandbox validation,
complete retention erasure, and production deployment recovery are still future work.
SECURITY.md describes how to report issues without posting live credentials or secrets.
