# Upstream feature-to-Rover task matrix

Status: source and claim inventory in progress; reviewed 2026-09-26. Repository
trees below are read from the immutable commits in
`UPSTREAM_SOURCE_AUDIT.md`; the documented live sites remain mutable. The
register at the end maps capability groups to paths present in those trees and
separates repository evidence from documentation or availability claims. A path
in a tree proves only that source exists at that commit; it does not prove that
the source builds, ships, is enabled, or is reachable as a service. P-003,
P-004, and P-006 remain open until each row has an observable acceptance case
and its source/claim class has been reviewed. Source reuse separately requires
the exact-file license/attribution gate.

Evidence labels used below:

- `SRC`: path exists in the pinned Git tree; source behavior/build is not
  independently asserted by this label.
- `DOC`: behavior is stated in the linked documentation or pinned README.
- `OPT`: documentation says the behavior is optional, configuration-gated, or
  integration-dependent.
- `BETA` / `EXPERIMENTAL`: documentation explicitly marks maturity or outcome
  as such.
- `CLOUD SRC` / `MOBILE SRC`: corresponding Orca source subtree exists; this
  does not prove a currently hosted service or a store-distributed app works.
- `CLAIM ONLY`: README, website, or release links state availability/scope that
  this source-tree inventory does not independently verify.

For current Herdr and Luvus documentation reviewed on 2026-09-26, see their
[quick start](https://herdr.dev/docs/quick-start/),
[agent state documentation](https://herdr.dev/docs/agents/),
[restore documentation](https://herdr.dev/docs/session-state/),
[Luvus orchestration guide](https://luvus.dev/docs/guides/orchestration/), and
[module guide](https://luvus.dev/docs/extend/using-modules/). Those URLs are
live documentation, not immutable commit evidence.

## Herdr

Evidence: [quick start](https://herdr.dev/docs/quick-start/),
[agents](https://herdr.dev/docs/agents/),
[integrations](https://herdr.dev/docs/integrations/),
[session state and restore](https://herdr.dev/docs/session-state/),
[persistence and remote access](https://herdr.dev/docs/persistence-remote/),
[connecting machines](https://herdr.dev/docs/connecting-machines/),
[configuration](https://herdr.dev/docs/configuration/),
[plugins](https://herdr.dev/docs/plugins/),
[CLI reference](https://herdr.dev/docs/cli/),
[socket API](https://herdr.dev/docs/socket-api/).

| Capability evidenced in current docs | Rover coverage task(s) | Acceptance condition to write before implementation |
|---|---|---|
| Persistent server/client session; client detach leaves pane processes alive; server stop ends them | P-042, P-045, P-047 | Detach/reconnect and explicit stop tests prove process/session ownership and restore outcome. |
| Project workspaces containing tabs, panes, and agents | P-046, P-051 | Create/list/switch/close operations preserve workspace isolation and ordering after restart. |
| Split panes, resizing, focus, zoom, popup/scratch layouts, prefix-mode navigation | P-046, P-049, P-053 | Keyboard-only golden interaction tests cover supported layouts and focus ownership. |
| Terminal mouse selection, context menu, links, wheel and mouse reporting | P-049, P-050, P-146 | Supported terminal input events have deterministic behavior; passthrough/input ownership is explicit; unsupported events are reported. |
| Direct attach to one agent/terminal; takeover semantics | P-048, P-049 | Concurrent-input ownership and explicit takeover are enforced and tested. |
| Scrollback copy mode, search, selection, export and history replay | P-044, P-047, P-050 | Bounded replay/search and safe rendering tests include hostile terminal sequences. |
| Session snapshot/restart restore and native agent session identity/resume | P-047, P-065 | Rover now persists explicit Codex/Claude session IDs bound to project and pane and can construct opt-in resume argv. It does not yet launch, restore after restart, integrate provider hooks, or support provider store discovery; see ADR-0024. |
| Agent detection by foreground process and screen manifests | P-062, P-166, P-167 | Rust has owned-PTY foreground-group leader sampling, exact executable evidence, and a bounded Herdr-format TOML/regex evaluator with all 22 pinned manifests and selector/operator tests. The attached TUI evaluates the live viewport, OSC title, and bounded OSC 9;4 progress payloads for audited direct executable mappings. Lifecycle-authority integration, process-group enumeration, and wrapper/child identification remain open. |
| Codex/Claude structured JSONL session events and provider usage claims | P-061, P-066, P-070 | Bounded fixtures prove event parsing and failure handling; provider claims remain distinct from verification evidence, and fixture results are not live-provider certification. |
| Lifecycle-hook authority, screen fallback, no double authority | P-063, P-064 | Rust currently has a per-pane authority registry with source sequence ordering, explicit release, and screen suppression; transport, event persistence, lifecycle expiry, and TUI integration remain open (ADR-0023). |
| State rollups across pane/tab/workspace and priority/unread behavior | P-051, P-063, P-125 | Rust now has pure pane/tab/workspace aggregation with explicit freshness, done-unseen input, state precedence, and blocked-pane IDs. Lifecycle-event persistence, viewed/unread transitions, and TUI display remain open; see ADR-0022. |
| Agent integration install/status/update/uninstall; local and remote manifests | P-069 | Preview and explicit apply/undo; updates are pinned and invalid manifests fail safely. |
| Agent explain/debug evidence and custom labels/metadata | P-062, P-067 | Explain output names evidence/rule/source and keeps semantic state distinct from display metadata. |
| Agent automation/socket API and direct control | P-067, P-068, P-091 | Rover can explicitly bind, list, inspect, and clear project-bound native session identities; attached TUI sessions can receive owned-PTY foreground samples and display known identity with unknown state. Live inventory, wait, prompt, read, and direct attach require lifecycle evidence and explicit pane input authority; protocol operations still need shared schemas, authorization, and audit records. |
| Worktree commands/provenance in workspace responses | P-035, P-036, P-101 | Worktree ancestry and dirty state remain visible and removal is guarded. |
| SSH remote sessions, saved machine profiles, reconnect and multi-machine visibility | P-095, P-096, P-097 | Host keys, session identity, reconnect and non-replayed input are covered on a real SSH fixture. |
| Themes, keybindings, sidebar/status configuration | P-054, P-126 | Strict schema, conflict resolution, atomic save and terminal color snapshots. |
| Plugins, event hooks and marketplace-delivered extensions | P-121, P-122, P-123 | Manifest, digest, authority disclosure and removal; unsupported sandbox claims absent. |
| Notifications and sound customization | P-059, P-125, P-128 | Rust currently captures terminal BEL and relays it to the host only for an unfocused pane. Configurable sound/notification policy, quiet hours, persistent unread actions and opt-out remain required; suppress active-pane noise. |
| Platform behavior, including Windows-specific support | P-043, P-060, P-156 | Each advertised target passes native build and terminal behavior tests on an owned host. |

## Luvus

Pinned source: `RizRiyz/luvus` at `63cb48263f347093f952dc43bbe0c2d468609567`
(see [source and license audit](UPSTREAM_SOURCE_AUDIT.md)). Evidence:
[README](https://github.com/RizRiyz/luvus),
[orchestration guide](https://luvus.dev/docs/guides/orchestration/),
[installation](https://luvus.dev/docs/getting-started/installation/),
[modules](https://luvus.dev/docs/extend/using-modules/),
[module authoring](https://luvus.dev/docs/extend/writing-modules/).

| Capability evidenced in current docs | Rover coverage task(s) | Acceptance condition to write before implementation |
|---|---|---|
| Persistent panes/tabs/layout and server-client model | P-045, P-046, P-047, P-051 | Restart and reconnect restore stable IDs, layout, cwd and explicit child-process outcomes. |
| Live agent status sidebar across projects; jump to blocked agent | P-051, P-062, P-063, P-067 | State shows evidence/freshness and navigation lands on correct pane. |
| Native conversation resume where adapter supports it; fork a session | P-065, P-071 | Resume/fork is opt-in, identity-bound, and fails visibly when unsupported. |
| Worktree-isolated workers and explicit shared-workspace workers | P-035, P-072 | Isolation mode is visible; shared mode warns; dirty worktree cleanup is safe. |
| Task board, drafts, dependencies, status, retries/history | P-052, P-055, P-058, P-071 | Acceptance covers repository-bound drafts with title, path globs, dependencies, quality-gate text, and multiline prompt; saving a draft never dispatches it. rover-tasks persists project-bound plans, derives transitive prerequisite readiness, and records bounded manual attempt history. The TUI confirms draft-to-plan creation and lists plans, while keeping them separate from runnable tasks. Full task forms still require Manual/Now start choice, validated worker selection, process-backed execution, and gate evaluation. The current Rust TUI slice adds repository-scoped search and deterministic sort preferences. |
| Glob path leases and conflict prevention | P-073 | Atomic admission, overlap rules, expiry, owner check and crash recovery tests. |
| Quality gates with retained output and retries | P-074 | Required check status is strict, bounded, retained and never replaced by task success. |
| Serialized merge/integration gate | P-075, P-076 | Concurrent branch completions serialize; conflicts abort without guessed merge. |
| Schedules, one-shot and recurring automation, misfire handling | P-078 | Timezone/DST, idempotency, missed-run policy and restart tests. |
| Explicit reviewed unattended profiles and read/workspace/full access | P-079 | Policy decision is visible, scoped and logged; default is not elevated. |
| Git branches/commits/diffs and PR/issue integrations | P-101, P-102, P-103, P-104 | Read/write capabilities are distinct; external writes require explicit confirmation and reconciliation. |
| File tree, Git coloring, viewer, editor handoff and diffs | P-056, P-057, P-107 | Path containment, size bounds, atomic writes and dirty-change confirmation. |
| SSH remote attach and compact changed-cell transport | P-095, P-096 | Host identity and framing are authenticated; disconnect never replays terminal input. |
| Local socket/CLI with UI actions scriptable | P-016, P-067, P-091 | Every exposed action has a versioned schema and identical CLI/TUI authorization. |
| Modules in multiple languages through manifest/actions/hooks/panes/settings | P-121, P-122, P-123 | Versioned manifest, argument-safe launch, hook bounds, state ownership and honest unsandboxed notice. |
| Themes, remappable keys, sidebar docks, localization | P-054, P-126, P-127 | Keyboard reachability, persistent settings and translation-key completeness. |
| Notifications/unread and phone-sized remote view | P-051, P-125, P-095 | TUI inbox actions work; responsive behavior is represented by narrow terminal tests. |
| Named/moved/swapped/reordered pane and tab operations | P-143 | Stable IDs and layouts survive reordering and restart without changing the wrong session. |
| Agent-to-agent messaging, key input, semantic waits and supported session forks | P-145 | Explicit target and adapter capability are required; every input has an audit event. |
| Persistent per-pane scrollback memory and cross-history search | P-144 | Retention limits, restart restore and search scope are visible and tested. |
| Multi-client remote sessions with independent viewports | P-140, P-141 | View state is per client; only the input owner can write to a terminal. |
| Versioned Universal Harness Protocol (UHP) 1.0 method registry, IPC, snapshots, events and semantic waits | P-139 | Version negotiation, owner-only local IPC, bounded stream/cancellation and exact-input fixtures. |
| Agent titles, token/cost/context-use summaries | P-142 | Values appear only from a documented adapter/provider signal and include freshness/unknown behavior. |
| Module startup hooks, sidebar docks and status-bar widgets | P-152 | Lifecycle, bounds, order, config ownership and unsandboxed authority are visible. |
| License statements at the pinned commit: README and root LICENSE both identify Apache-2.0; earlier indexed snapshots differed | P-005, P-007, P-008 | Use only the pinned revision for review; preserve exact notices and audit each reused file/dependency before import. |

## Orca ADE

Evidence: [repository README](https://github.com/stablyai/orca),
[CLI overview](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/cli/overview.mdx),
[SSH docs](https://www.onorca.dev/docs/ssh),
[remote worktrees recipe](https://www.onorca.dev/docs/recipes/remote-worktrees),
[notifications](https://www.onorca.dev/docs/notifications),
[mobile readme](https://github.com/stablyai/orca/blob/main/mobile/README.md).
Current repository root LICENSE says MIT; each copied asset/dependency still
requires a separate audit.

| Capability evidenced in current docs/repository | Rover coverage task(s) | CLI/TUI acceptance condition to write before implementation |
|---|---|---|
| Parallel agents in isolated Git worktrees; fan-out and result comparison | P-035, P-072, P-077 | N-way runs retain separate identities; comparison is inspectable and winner is explicitly selected. |
| Terminal splits, persistent terminals, scrollback | P-043, P-044, P-045, P-046, P-047 | Pane lifecycle and restore are deterministic across client/server restart tests. |
| Task/agent board, profiles, state and account/usage visibility | P-051, P-061, P-063, P-129, P-130 | Rust `rover-agents` now exposes the four declared Go profiles through read-only CLI listing/lookup and fails closed on unknown names. Process launch, TUI selection, state, provider account, and usage visibility remain open; only provider-documented values appear and account switching must be explicit and confirmed. |
| Editor/file browser, image/file handoff, diff comments | P-056, P-057, P-105, P-108 | File handling is bounded and private; annotations bind to exact candidate paths/hashes. |
| GitHub/Linear issues, PRs, boards and CI | P-103, P-104, P-106 | Provider fixtures prove contracts only; real writes are opt-in, confirmed and reconciled. |
| Conflict review, branch/PR status tracking | P-075, P-106 | No automatic merge; conflict evidence and final status bind to branch/base identities. |
| Markdown, image and PDF previews | P-107 | Parsing is size-bounded and hostile content cannot emit terminal control or execute. |
| Embedded browser and dev server | P-111, P-112 | Only owned processes are terminated; allowed origins and ports are explicit. |
| DOM/accessibility inspection, design mode, computer-use controls | P-113, P-114, P-115, P-116, P-117, P-118 | Snapshot provenance is captured; interaction requires explicit authority and audit; no pixel-mode claim from textual output. |
| Local, SSH, self-hosted, ephemeral remote runtime and reconnect | P-094, P-095, P-096, P-097, P-098, P-099 | Each adapter is functional, host-authenticated, capability-reported and tested under disconnect. |
| Port forwarding and local browser to remote dev server | P-098 | Endpoint is user-selected; bind target is visible; no automatic public exposure. |
| Mobile companion over paired desktop runtime | P-095, P-096, P-119 | CLI/TUI remote access provides equivalent supported actions; no mobile app/API is added to Rover. |
| Persistent notification inbox and unread state | P-125 | Inbox survives restart, supports unread/snooze/jump, and records source timestamps. |
| CLI scripting for worktrees, terminals, browser and UI | P-016, P-091, P-102, P-112, P-116, P-131 | Commands have stable schemas, dry-run/confirmation where needed, and TUI parity. |
| Schedules, artifacts and skills | P-078, P-088, P-124 | Schedules are idempotent; artifacts are content-addressed; skills retain provenance and safe rendering. |
| Desktop/mobile/cloud/Electron/React functionality in upstream source | P-006, P-009, P-111..P-120 | Include only behavior with a tested CLI/TUI equivalent; do not copy unsupported UI/runtime code into the Rust product. |
| Browser CLI `snapshot`, `click`, `fill`, design-mode HTML/CSS and cropped screenshot handoff | P-147, P-148 | Actions operate only on a bounded captured page; explicit element/source/artifact IDs are retained. |
| Quick open across files, worktrees, agents, commands and repository context | P-150 | Search is bounded, deterministic for identical input and respects project boundaries. |
| Account switcher and rate-limit reset/usage tracking | P-149 | Read only supported local provider state, require confirmation to switch, and never persist credentials. |
| Rich Markdown/image/PDF and repository document previews | P-151 | Size/type bounds, terminal-safe rendering, and explicit unavailable preview errors. |

## Traceability rule

Each table row is a scope statement, not evidence that Rover already implements
it. Tasks remain pending until the Rust behavior, a focused regression/contract
test, the plan ledger, and capability output agree. Exact per-file upstream
provenance and license records are separate gates from feature behavior.

## Pinned source-path and claim register

These anchors are exact paths at the commits in `UPSTREAM_SOURCE_AUDIT.md`.
Several anchors are directories because a capability spans multiple modules;
they are mapping evidence, not authorization to copy the directory. The
file-level reuse gate still requires each reused file's blob digest and notice
review. On 2026-09-26, every listed anchor was checked against `git ls-tree`
for its pinned commit; all listed paths were present. This check proves path
existence only, not implementation semantics, build status, release inclusion,
or live service availability.

### Herdr

| Capability rows above | Pinned source anchors | Evidence class and qualification |
|---|---|---|
| Persistent server/client sessions; workspaces, tabs, panes and layout | `src/server/`, `src/client/attach.rs`, `src/workspace.rs`, `src/workspace/aggregate.rs`, `src/workspace/tab.rs`, `src/persist/restore.rs`, `src/persist/snapshot.rs` | `SRC + DOC`. Live docs distinguish detach (processes continue) from server restart (processes stop, layout may restore). |
| Split, resize, focus, zoom, popup, keyboard/mouse and copy/search | `src/app/api/panes.rs`, `src/input/keybindings.rs`, `src/input/mouse.rs`, `src/selection.rs`, `src/copy_mode.rs`, `src/client/clipboard_forwarding.rs` | `SRC + DOC`. Mouse passthrough and platform-specific link gestures are terminal/config dependent. |
| Direct terminal/agent attach and input takeover | `src/client/attach.rs`, `src/server/terminal_attach.rs`, `src/input/lease.rs`, `src/cli/agent.rs` | `SRC + DOC`. Takeover is an explicit `--takeover` operation; single-writer/input ownership is part of the acceptance contract. |
| Session screen history and native-agent restore | `src/terminal/history_read.rs`, `src/persist/snapshot.rs`, `src/agent_resume.rs`, `src/app/agent_resume.rs` | `SRC + DOC + OPT`. Pane history is documented off by default; native resume requires integration-reported session identity and supported agent/integration versions. |
| Update handoff and process detection fallback | `src/handoff_runtime.rs`, `src/server/autodetect.rs`, `src/pane/agent_detection.rs`, `src/detect/manifest.rs` | `SRC + DOC + EXPERIMENTAL`. Handoff is documented best-effort; child-process-group detection is an opt-in fallback with possible foreground-process misclassification. |
| Agent lifecycle hooks, manifests, explain output and state rollups | `src/integration/registry.rs`, `src/integration/actions.rs`, `src/detect/manifest_update.rs`, `src/agent_view_eval.rs`, `src/workspace/aggregate.rs`, `src/ui/sidebar.rs` | `SRC + DOC`. Docs describe one lifecycle authority per pane, screen fallback, and a strict known-visible-prompt rule for `blocked`; unseen prompts may be shown `idle`. |
| Integration install/status/update and custom agent metadata | `src/cli/integration.rs`, `src/integration/`, `src/cli/agent.rs`, `src/metadata_tokens.rs` | `SRC + DOC`. Remote manifest updates, local overrides, and display-only labels have separate semantics; display metadata is not lifecycle state. |
| Agent automation, socket/API and wait/control operations | `src/api/client.rs`, `src/api/server.rs`, `src/api/schema/`, `src/api/wait.rs`, `src/cli/api.rs`, `src/cli/agent.rs` | `SRC + DOC`. Each protocol action still needs exact authorization and side-effect classification in Rover. |
| Worktree provenance and machine/SSH sessions | `src/worktree.rs`, `src/cli/worktree.rs`, `src/remote/attach.rs`, `src/remote/host.rs`, `src/remote/saved.rs`, `src/cli/machine.rs` | `SRC + DOC`. Host identity, reconnect, and input replay behavior require separate acceptance tests. |
| Themes, keybindings, sidebar, notifications and sound | `src/config/theme.rs`, `src/config/keybinds.rs`, `src/config/sidebar.rs`, `src/config/sound.rs`, `src/server/notifications.rs`, `src/client/notifications.rs`, `src/terminal_notify.rs` | `SRC + DOC + OPT`. User configuration and notification opt-outs must be represented explicitly. |
| Plugins, commands, platform PTYs and Windows support | `src/plugin_command.rs`, `src/plugin_paths.rs`, `src/persist/plugin_registry.rs`, `src/pty/backend.rs`, `src/platform/`, `src/cli/plugin.rs` | `SRC + DOC`. Repository presence does not establish a plugin's trust isolation or native behavior on every advertised OS. |

### Luvus

| Capability rows above | Pinned source anchors | Evidence class and qualification |
|---|---|---|
| Persistent terminal sessions, panes, tabs and workspace UI | `src/session.rs`, `src/terminal/`, `src/ui/panes.rs`, `src/layout.rs`, `src/persist.rs`, `src/app/persistence.rs` | `SRC + DOC`. Session restore and persistence behavior must be checked against the pinned source, not inferred from the live site alone. |
| Agent detection, state, usage, native session adapters and messaging | `src/agent/registry.rs`, `src/agent/types.rs`, `src/agent/usage.rs`, `src/agent/*/sessions.rs`, `src/app/dispatch/agents.rs`, `src/app/mission.rs` | `SRC + DOC`. Per-agent resume/state is adapter-dependent; generic process support does not prove native resume for every agent. |
| Task board, task drafts, dependencies, retries and worker start | `src/ui/board.rs`, `src/app/board.rs`, `src/app/mission.rs`, `src/orch/mod.rs`, `src/orch/worker.rs`, `src/app/dispatch/orchestration.rs` | `SRC + DOC`. The current orchestration guide specifies project binding, worker modes, task transitions and restart behavior. |
| Path leases, quality gates and serialized integration/merge | `src/app/dispatch/orchestration.rs`, `src/app/dispatch/tests/orchestration.rs`, `src/orch/worker.rs`, `src/worktree.rs`, `src/git/local.rs`, `src/git/github.rs` | `SRC + DOC`. Lease overlap, gate failure, merge conflict and interrupted integration are distinct states, not task success. |
| Scheduled/recurring agent automations and access profiles | `src/automation/model.rs`, `src/automation/persist.rs`, `src/automation/schedule.rs`, `src/automation/worker.rs`, `src/app/automation.rs`, `src/app/automation_persistence.rs` | `SRC + DOC + BETA`. The guide labels the board's Automations view beta; misfire/restart rules and access profiles require independent contract tests. |
| Git, PR review, diffs, files, previews and search | `src/git/`, `src/diff/`, `src/files/`, `src/search/`, `src/ui/git.rs`, `src/ui/diff.rs`, `src/ui/files.rs`, `src/ui/preview.rs`, `src/ui/search.rs` | `SRC + DOC`. Preview parser/security behavior is separate from the fact that a preview UI exists. |
| Remote sessions and multiple machines | `src/machine/ssh.rs`, `src/machine/catalog.rs`, `src/machine/recovery.rs`, `src/machine/api.rs`, `src/terminal/pty/` | `SRC + DOC`. Authentication, host-key policy, reconnect and remote input semantics remain separate acceptance areas. |
| Scriptable CLI, local API and Universal Harness Protocol | `src/cli.rs`, `src/api/`, `src/uhp/`, `protocol/` | `SRC + DOC`. Protocol method/version and authorization claims must be matched to exact schemas before Rover advertises parity. |
| Language-neutral modules, manifest, install, actions, hooks and settings | `src/module/`, `src/app/modules.rs`, `src/app/dispatch/extensions.rs`, `plugins/` | `SRC + DOC`. The current docs explicitly say modules are ordinary programs with no SDK and no plugin runtime; install confirmation can be skipped with `--yes`, so code execution is not sandboxed by that prompt. No marketplace registry is described. |
| Themes, localization, status bar and configuration | `src/theme/`, `src/i18n/`, `src/bar/`, `src/ui/settings.rs`, `src/config.rs` | `SRC + DOC`. Built-in/theme assets also have nested notices recorded in the source audit. |
| Mobile-sized, web and remote-view surfaces | `src/ui/mobile/`, `src/web/`, `src/machine/` | `SRC + DOC`. These are source/documentation indicators; Rover's deliverable maps supported behavior to CLI/TUI and does not add a web or mobile application. |

### Orca ADE

| Capability rows above | Pinned source anchors | Evidence class and qualification |
|---|---|---|
| CLI and local scripting contracts | `src/cli/`, `src/shared/rpc-contract/` | `SRC + DOC`. The pinned README advertises CLI commands including worktree creation and browser snapshot/click/fill; command contract and side effect must be reviewed per command. |
| Persistent terminals, daemon, PTY and terminal rendering | `src/main/daemon/`, `src/main/pty/`, `src/main/ghostty/`, `src/renderer/src/components/terminal/` | `SRC + DOC`. The README's “infinite splits” and restart-surviving scrollback are product claims, not verified here as unbounded capacity or a Rover guarantee. |
| Agent launch, account/usage and task orchestration | `src/main/agent-launch/`, `src/main/claude-accounts/`, `src/main/claude-usage/`, `src/main/codex-accounts/`, `src/main/codex-usage/`, `src/main/automations/` | `SRC + DOC`. Provider-specific account and usage data cannot be generalized to arbitrary agents. |
| Worktrees, GitHub/Linear/GitLab and review annotations | `src/main/git/`, `src/main/ipc/worktrees/`, `src/main/github/`, `src/main/linear/`, `src/main/gitlab/`, `src/renderer/src/components/editor/` | `SRC + DOC`. External provider writes and candidate-bound comments are independent operations requiring their own Rover contracts. |
| File tree, editor handoff, diffs and document previews | `src/main/ipc/filesystem-list-files.ts`, `src/main/ipc/filesystem-mutations.ts`, `src/main/file-transaction-lock.ts`, `src/renderer/src/components/editor/`, `src/main/artifacts/` | `SRC + DOC`. README drag/drop/autosave statements are GUI claims; Rover acceptance maps these to explicit CLI/TUI paths and safe edit/write behavior. |
| Browser, DOM/design inspection and computer-use controls | `src/main/browser/`, `src/main/computer/`, `src/shared/rpc-contract/browser-params.ts`, `src/shared/rpc-contract/computer-params.ts`, `src/cli/handlers/browser-*`, `src/cli/handlers/computer.ts` | `SRC + DOC`. Browser and computer actions require captured-target provenance, explicit authority and auditable effects. |
| SSH, self-hosted/ephemeral runtime, reconnect and port forwarding | `src/main/ssh/`, `src/main/ephemeral-vm-runtime-service.ts`, `src/main/host/`, `src/cli/runtime/` | `SRC + DOC`. These are code paths in an application repository; their presence does not establish a live remote service or provider certification. |
| Notifications, scheduling, artifacts, skills and plugins | `src/main/notifications/`, `src/main/automations/`, `src/main/artifacts/`, `src/main/plugins/`, `src/cli/handlers/automations.ts`, `src/cli/handlers/artifacts.ts`, `src/cli/handlers/skills.ts` | `SRC + DOC`. Inbox state, schedule durability, artifact provenance and extension authority are separate parity contracts. |
| Cloud relay/service and paired mobile companion | `cloud/`, `mobile/`, `src/main/ssh/`, `src/shared/rpc-contract/ssh-params.ts` | `CLOUD SRC + MOBILE SRC + CLAIM ONLY`. The pinned README links iOS/TestFlight/Android distribution and identifies `cloud/` as the pairing relay; source presence/README links do not prove current store availability or hosted-service uptime. Rover maps needed remote-access behavior to CLI/TUI only. |
| Desktop/mobile renderer and native computer-use areas | `src/renderer/`, `mobile/`, `native/`, `src/main/computer/` | `SRC + MOBILE SRC`. These are upstream implementation areas to study for behavior; they are not Rover product surfaces under the CLI/TUI-only decision. |

### Claim disposition

- `SRC` is repository-content evidence only. We have not built or run the three
  upstream products from these snapshots as part of this inventory.
- `DOC` records a published claim and links its source. Live Herdr/Luvus web
  documentation can change independently of the pinned commit; it must not be
  treated as proof of the exact pinned implementation without a path-level
  source check.
- `OPT`, `BETA`, and `EXPERIMENTAL` preserve the upstream qualification in the
  parity requirements. Rover must report its own capability state and must not
  turn these into unconditional guarantees.
- Orca's repository includes `cloud/`, `mobile/`, and `native/` source roots,
  while its README also links external distribution artifacts and describes
  broad compatibility claims. This inventory establishes those source roots
  and README statements only; it does not verify current stores, services,
  external integrations, or “any agent” compatibility.
- No `CLAIM ONLY` item becomes a Rover acceptance requirement until a CLI/TUI
  observable behavior and testable contract are written. No source file is
  authorized for reuse by this register.
