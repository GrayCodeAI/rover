# ADR-0024: Exact agent session identity and explicit resume

Status: accepted for the bounded Rust session-binding slice

## Context

Herdr documents native session references from integrations and opt-in
automatic restart restore, gated by valid supported references and a
configuration setting. Luvus documents exact session IDs where discovery by
project directory is unsafe, and resumes through provider-native commands.
The current Rover agent registry only has reviewed Codex and Claude CLI
invocation profiles. Transcript identities are provider-emitted claims, not
independent verification.

## Decision

The Rust session binding stores an exact ID, adapter, evidence origin,
canonical project identity, repository path, and pane ID in Rover's local
record store. It supports only codex-exec and claude-print. Loading verifies
record schema, project digest/path, pane key, adapter, and bounded ID
characters. Resume argv is built only when a caller explicitly requests it
with the same adapter. The builder preserves the existing read-only or
workspace-write Codex sandbox and Claude permission mode. A caller can
explicitly clear a pane's binding; deletion is recorded in the event store.

Rover does not scan provider databases, select latest sessions, infer a
binding from directory similarity, or restart an agent automatically.
Unrecognized providers and path-valued session references fail closed. This
keeps uncertain ownership out of the persisted pane binding.

## Consequences and limits

The attached TUI offers the active pane's binding through Ctrl-B then `a`.
It displays the exact session identity and requires a second confirmation
before sending the provider's interactive resume command to Rover's persistent
project shell. The user can also confirm removal of the Rover binding; this
does not stop an already-running agent. Shell arguments are quoted as separate
POSIX shell words, and control characters are rejected. The provider still
applies its own permission and sandbox configuration; Rover does not mediate
those settings. This is not an installed hook, automatic restore scheduler,
duplicate-session detector, or live provider test. The pinned upstream source snapshots are partial clones;
their session-resume blobs could not be retrieved because network access to
the promisor remote was unavailable. No upstream source was copied. Behavioral
references are the current published [Herdr session restore
documentation](https://herdr.dev/docs/session-state/), [Luvus session resume
guide](https://luvus.dev/docs/guides/agents/), [Codex CLI resume
examples](https://github.com/openai/codex/issues/14343), and [Claude CLI
reference](https://docs.anthropic.com/en/docs/claude-code/cli-usage).
