# Coding-agent landscape research for Rover

**Research date:** 2026-09-24

**Scope:** Open-source coding-agent and long-horizon agent systems relevant to Rover, with deep source review of OpenHands, SWE-agent, Aider, OpenCode, and OpenAI Codex.
**Rover comparison target:** a terminal-first, agent-neutral control plane that can support multiple model/tool providers while preserving explicit authority, evidence, and human control ([Rover README](../../README.md#L5-L8), [Rover status](../../STATUS.md#L7-L18), [Rover security model](../../docs/SECURITY_MODEL.md#L3-L13)).

**Evidence labels used below:**

- **Observed** — directly visible in a pinned repository revision, release feed, or local Rover document.
- **Official claim** — stated in project documentation; it is not independently validated here.
- **Rover interpretation** — an engineering inference from the cited evidence, not a project assertion.
- **Gap** — not established by the reviewed evidence.

Repository stars are approximate GitHub HTML values observed on 2026-09-24 and will drift. GitHub's REST API was unavailable during this research, so activity was triangulated from repository pages, Atom release feeds, commit feeds, and local clones rather than API counts.

## 1. Which projects are in scope, and how should Rover prioritize them?

### Takeaway

The five requested projects remain a strong deep-dive set, but they are no longer five versions of the same product. **Codex** is the most complete reference for OS-enforced execution policy, approvals, protocol surfaces, compaction, and multi-agent control. **OpenHands** is the best reference for a conversation-centric server/SDK architecture, repository-aware workspaces, child runs, and extensible context condensation. **OpenCode** is the strongest compact reference for permission UX, session persistence, plugins, and provider-neutral interfaces, while explicitly warning that permissions are not a sandbox. **Aider** remains the clearest terminal-first reference for repository mapping, git-aware editing, recovery, and lint/test reflection. **SWE-agent** is the clearest configuration-driven SWE benchmark harness, but its own project now points users to `mini-swe-agent` for the actively evolving lightweight path, so Rover should treat it as a design reference rather than assume it is the successor to maintain.

For Rover, a broader top 20 should also include PrimeIntellect's Prime Agent, Goose, `mini-swe-agent`, Qwen Code, Cline, Gemini CLI, Roo Code, Open SWE, Kilo Code, and current Open Interpreter. Agentless and AutoCodeRover remain useful historical research baselines but rank lower until current maintenance and comparative evidence are checked. Continue and Amazon Q Developer CLI should be excluded from a current active shortlist because their repositories now explicitly signal end-of-active-development or read-only status.

### Cited Findings

#### Method and ranking rubric

This is an editorial relevance ranking, not a benchmark result. Projects are weighted toward Rover's actual differentiators rather than GitHub popularity:

| Criterion | Weight | Rover-relevant question |
|---|---:|---|
| Trust and execution control | 25% | Are filesystem, process, network, credentials, and external writes bounded and attributable? |
| Rover architecture fit | 20% | Can an agent-neutral control plane coordinate runs without becoming provider-specific? |
| Current maturity and maintenance | 15% | Is the project actively maintained, released, and coherent at the cited revision? |
| Interfaces and extensibility | 15% | Are there stable CLI, SDK, protocol, plugin, MCP, or headless surfaces? |
| Context and orchestration | 15% | Can long runs be resumed, compacted, branched, delegated, and inspected? |
| Privacy and evidence quality | 10% | Are telemetry, trajectory retention, sharing, and acceptance evidence explicit? |

Evidence depth is marked **A** for the five source-audited projects and **B** for a current primary-repository/README screen. A rank is not an assertion that a B-level project is less capable; it reflects lower evidence depth and/or weaker direct fit to Rover.

#### Current identity, activity, and license of the five deep dives

| Project | Current identity and approximate stars | Activity observed by 2026-09-24 | License |
|---|---|---|---|
| **OpenHands** | [`OpenHands/OpenHands`](https://github.com/OpenHands/OpenHands), about **89k stars**. The main repository is now `@openhands/agent-canvas`, a React/TypeScript Agent Canvas; the Python agent runtime and server continue primarily in [`OpenHands/software-agent-sdk`](https://github.com/OpenHands/software-agent-sdk). ([architecture](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/docs/architecture.md#L1-L31), [package identity](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/package.json#L1-L7)) | `v1.23.0` released 2026-09-23; main inspected at `b090680…` on 2026-09-23. ([release feed](https://github.com/OpenHands/OpenHands/releases.atom), [commit](https://github.com/OpenHands/OpenHands/commit/b0906809b3e8777491519c386d55ce32d7f4daa4)) | [MIT](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/LICENSE) |
| **SWE-agent** | [`SWE-agent/SWE-agent`](https://github.com/SWE-agent/SWE-agent), about **20.4k stars**; a configurable, research/evaluation-oriented SWE agent. ([README](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/README.md#L1-L38)) | Latest stable release in the reviewed feed was `v1.1.0` on 2025-05-22; main had changes through 2026-07-16 at `3ea751c…`. The README points to `mini-swe-agent` as the actively evolving lightweight path. ([release feed](https://github.com/SWE-agent/SWE-agent/releases.atom), [commit](https://github.com/SWE-agent/SWE-agent/commit/3ea751c087f32b16e039a2233dd6eefecef325d5), [README successor note](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/README.md#L14-L23)) | [MIT](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/LICENSE) |
| **Aider** | [`Aider-AI/aider`](https://github.com/Aider-AI/aider), about **49.1k stars**; terminal-first pair programming with an optional browser UI. ([README](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/README.md#L1-L36)) | The reviewed feed contained development release `v0.86.3.dev` dated 2026-02-12; main had changes through 2026-05-22 at `5dc9490…`. ([release feed](https://github.com/Aider-AI/aider/releases.atom), [commit](https://github.com/Aider-AI/aider/commit/5dc9490bb35f9729ef2c95d00a19ccd30c26339c)) | [Apache-2.0](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/LICENSE.txt) |
| **OpenCode** | [`anomalyco/opencode`](https://github.com/anomalyco/opencode), about **210k stars**; an agent platform with TUI, desktop, web/IDE clients, server, SDK, and ACP. ([README](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/README.md#L1-L35)) | The feed contained `v2.0.15` on 2026-09-23 while the inspected root package still reported `1.18.32`; the main commit was 2026-09-22. This is a transition signal, not a verified v2 architecture comparison. ([release feed](https://github.com/anomalyco/opencode/releases.atom), [package version](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/package.json#L1-L8), [commit](https://github.com/anomalyco/opencode/commit/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159)) | [MIT](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/LICENSE) |
| **OpenAI Codex** | [`openai/codex`](https://github.com/openai/codex), about **126.2k stars**; a Rust coding-agent runtime distributed through a TUI, `codex exec`, app-server, TypeScript/Python SDKs, MCP, and integrations. ([TypeScript SDK](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/sdk/typescript/README.md#L1-L36), [Python SDK](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/sdk/python/README.md#L1-L29), [app-server binary](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/app-server/src/main.rs#L32-L124)) | The reviewed feeds contained `0.158.0-alpha.4` on 2026-09-23 and a stable-series reference to `0.156.1`; main was inspected at `7e5054d…` from 2026-09-22. ([release feed](https://github.com/openai/codex/releases.atom), [commit](https://github.com/openai/codex/commit/7e5054d32f1dae4f30136f078116100ee9722e5c)) | [Apache-2.0](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/LICENSE) |

**Observed scope correction:** the old `All-Hands-AI/OpenHands` and `sst/opencode` locations redirect to the current repositories. `block/goose` redirects to `aaif-goose/goose`. Current repository identity should be used in future Rover references.

#### Relevance-weighted top 20

| Rank | Project | Primary repository | Why it remains relevant to Rover | Evidence / principal caution |
|---:|---|---|---|---|
| 1 | **OpenAI Codex** | [`openai/codex`](https://github.com/openai/codex) | OS-level sandbox policies, approval policy, app-server protocol, compaction, subagents, MCP, and local trace bundles. | **A.** Most complete directly comparable trust/runtime reference; model-provider and product coupling remain significant. |
| 2 | **OpenHands** | [`OpenHands/OpenHands`](https://github.com/OpenHands/OpenHands) + [`software-agent-sdk`](https://github.com/OpenHands/software-agent-sdk) | Server/SDK/UI separation, event-based conversations, condensers, local and remote workspaces, worktree children, and automation. | **A.** Main/core identity is split across repositories; local execution can be host-authoritative. |
| 3 | **OpenCode** | [`anomalyco/opencode`](https://github.com/anomalyco/opencode) | Provider-neutral client/server model, permission rules, plugins, MCP, subagents, session snapshots, and ACP. | **A.** Explicitly no sandbox; current v1/v2 release transition needs follow-up. |
| 4 | **Prime Agent** | [`PrimeIntellect-ai/prime-agent`](https://github.com/PrimeIntellect-ai/prime-agent) | Persistent programmatic context, recursive subagents, daemon-backed sessions, budgets, quality gates, durable harness state, and direct agent messaging. | **B.** Extremely relevant long-horizon design, but a young 2026 project; benchmark and runtime claims remain official claims until reproduced. |
| 5 | **Goose** | [`aaif-goose/goose`](https://github.com/aaif-goose/goose) | Block's open-source agent runtime, provider-neutral model/tool configuration, MCP extensions, and session-oriented CLI. | **B.** Broad scope; current execution isolation and acceptance semantics need source review. |
| 6 | **mini-swe-agent** | [`SWE-agent/mini-swe-agent`](https://github.com/SWE-agent/mini-swe-agent) | The current lightweight SWE-agent successor named by SWE-agent itself; small configurable shell/agent loop useful for constrained adapters. | **B.** Narrower SWE benchmark focus, but easier to embed and audit than a full product. |
| 7 | **Qwen Code** | [`QwenLM/qwen-code`](https://github.com/QwenLM/qwen-code) | Terminal agent, MCP, headless/CI modes, sandbox-related commands, and provider-specific tool policy worth comparing with Rover adapters. | **B.** Strong Qwen ecosystem coupling; independent execution guarantees need verification. |
| 8 | **Cline** | [`cline/cline`](https://github.com/cline/cline) | Mature editor-agent UX, approval UX, MCP, plan/act modes, checkpoints, and provider-neutral model selection. | **B.** Primarily an extension/client architecture; extension host authority differs from Rover. |
| 9 | **Gemini CLI** | [`google-gemini/gemini-cli`](https://github.com/google-gemini/gemini-cli) | Terminal-first agent, sandboxing, MCP, policy/configuration, extensions, and headless operation. | **B.** First-party and Gemini-centric; compare capability declarations rather than assuming generic parity. |
| 10 | **Roo Code** | [`RooVetGit/Roo-Code`](https://github.com/RooVetGit/Roo-Code) | VS Code-native orchestration, profiles, modes, MCP, and a large extension ecosystem. | **B.** Editor extension lifecycle and trust boundaries differ from a terminal control plane. |
| 11 | **Aider** | [`Aider-AI/aider`](https://github.com/Aider-AI/aider) | Best deep-dive reference for repo maps, edit/lint/test loops, git checkpoints, undo, and human-in-the-loop terminal UX. | **A.** Lower release cadence; no runtime sandbox and largely single-session rather than multi-agent orchestration. |
| 12 | **SWE-agent** | [`SWE-agent/SWE-agent`](https://github.com/SWE-agent/SWE-agent) | Declarative agent/tool/history composition, Docker deployment, SWE-bench evaluation, and reviewer patterns. | **A.** Stable research baseline, but the project points to `mini-swe-agent`; latest stable release is older than the other four deep dives. |
| 13 | **Kilo Code** | [`Kilo-Org/kilocode`](https://github.com/Kilo-Org/kilocode) | Active OpenCode-derived client ecosystem; useful for comparing forks, provider configuration, and hosted/editor boundaries. | **B.** Fork overlap means not all OpenCode lessons transfer independently. |
| 14 | **Open SWE** | [`Open-SWE/Open-SWE`](https://github.com/Open-SWE/Open-SWE) | Open-source software-engineering agent/reference implementation and release artifact. | **B.** Current scope and overlap with Agentless/mini-SWE need deeper review before direct borrowing. |
| 15 | **Open Interpreter** | [`OpenInterpreter/open-interpreter`](https://github.com/OpenInterpreter/open-interpreter) | The current project presents a Rust, Codex-derived, low-cost-model harness rather than the historical Python computer-use project. | **B.** Treat the earlier Python architecture and current repository as separate generations. |
| 16 | **Trae Agent** | [`bytedance/trae-agent`](https://github.com/bytedance/trae-agent) | Open engineering-agent project with trajectory/tooling components useful for SWE research. | **B.** Current product scope, maintenance, and evidence model need source review. |
| 17 | **SWE-Smith** | [`SWE-bench/SWE-smith`](https://github.com/SWE-bench/SWE-smith) | Generates/curates software-engineering tasks and evaluates many agent harnesses; useful for benchmark and evidence-pipeline design. | **B.** Primarily research infrastructure, not a general user-facing agent runtime. |
| 18 | **Plandex** | [`plandex-ai/plandex`](https://github.com/plandex-ai/plandex) | Long-running terminal/planning agent, context management, files, and local-agent integration. | **B.** The hosted service announced a wind-down; separate the still-available local/self-hosted project from cloud roadmap claims. |
| 19 | **Agentless** | [`OpenAutoCoder/Agentless`](https://github.com/OpenAutoCoder/Agentless) | Important localization-first agent design and SWE-bench baseline. | **B.** Ranked as a research reference; current activity and comparative results need revalidation. |
| 20 | **AutoCodeRover** | [`nchuc/autocoderover`](https://github.com/nchuc/autocoderover) | Historical autonomous program-repair agent and SWE-bench baseline. | **B.** Lower current-relevance confidence; retain for comparison, not as a presumed current leader. |

#### Projects deliberately not placed in the current top 20

- **Continue** is excluded from the active ranking because its repository says it is in read-only mode and released a final version on 2026-08-07. ([repository README](https://github.com/continuedev/continue))
- **Amazon Q Developer CLI** is excluded because its repository says it is no longer actively maintained and receives only critical security fixes. ([repository README](https://github.com/aws/amazon-q-developer-cli))
- **Vibe Kanban/Orca and similar worktree/task-control UIs** remain watch-list references for Rover orchestration, but they are agent-workbench platforms rather than primary coding-agent runtimes and should be evaluated in a separate platform category.
- **Closed agents and assistants** are outside an open-source implementation comparison unless their published interfaces are explicitly treated as external-provider contracts.

### Inferences / Rover Implications

1. **Keep five deep dives but change their roles.** Codex should anchor execution-policy and protocol design; OpenHands should anchor conversation/server/context design; OpenCode should anchor permission UX and extensibility; Aider should anchor terminal/git recovery; SWE-agent should anchor configuration and reproducible evaluation.
2. **Do not infer current maintenance from stars.** Aider and SWE-agent remain highly instructive despite lower release cadence, while Prime Agent is much newer and may be more strategically relevant. Rover's shortlist should carry both an evidence score and an activity signal.
3. **Track repositories by identity, not only brand.** OpenHands, Goose, Open Interpreter, and Kilo have redirects, repository splits, forks, or major generational changes. A dependency inventory should record canonical URL, revision, and product generation.
4. **Use a tiered evidence policy.** Source-audited projects can inform contracts and tests; README-only projects should remain candidates until their execution, telemetry, and acceptance paths are inspected.

### Gaps

- GitHub API access returned 403 during research, so star counts, issue counts, contributor distributions, and release comparisons were not independently calculated through the API.
- The fifteen B-level projects were not all source-audited. Their ranks are a screening result, not a claim of equivalent evidence depth.
- Current benchmark results, provider/model quality, and cost were deliberately not scored. They are configuration- and model-dependent and were not reproduced here.
- The OpenCode v2 release was present in the feed, but the inspected revision still identified the root package as v1; no v2-specific architecture conclusions are made.

## 2. How do the five deep-dive coding agents actually work?

### Takeaway

The five systems converge on a basic loop—construct context, call a model, execute tools, observe results, and repeat—but differ sharply in where authority and context live. Codex makes the operating-system sandbox and approval policy first-class. SWE-agent makes the environment and declarative configuration first-class. Aider makes repository state, git checkpoints, and the terminal user's immediate feedback first-class. OpenCode makes permissions, extensible clients, and durable sessions first-class. OpenHands makes a conversation server, event history, pluggable condenser, and local/remote workspace abstraction first-class.

None of these systems, as reviewed, replaces Rover's need for a separate acceptance authority. Tests, lint output, model reviewers, successful tool calls, commits, and telemetry records are useful signals, but none by itself proves that the exact candidate artifact and declared requirement were independently accepted.

### Cited Findings

#### Cross-project comparison

| Dimension | OpenHands | SWE-agent | Aider | OpenCode | Codex |
|---|---|---|---|---|---|
| Primary surface | Agent Canvas + Agent Server + SDK | CLI/config + SWE-ReX environment | CLI, optional web UI, scripts | TUI/desktop/web/IDE, server, SDK, ACP | TUI, `exec`, app-server, TypeScript/Python SDKs, MCP |
| Loop control | SDK `Agent.run`, bounded by iterations/budgets/stuck detection | Model → parser → tool → environment → history processing | Edit → optional shell → lint/test reflection → response | Persisted message loop with retry, compaction, max steps | Rust turn loop with model streaming, tool dispatch, approvals, retries |
| Context strategy | Append-only events + replaceable condensers | Configurable history processors and agent messages | Repo map + chat files + recursive history summary | Messages/snapshots + compaction that preserves recent tokens | Compaction with local/remote strategies, checkpoints, and retained recent history |
| Default execution boundary | Local host workspace by default; remote/container isolation optional | Docker by default, with remote environment providers | Direct subprocess in the user's environment | Direct host process after permission evaluation | OS-native sandbox; workspace-write and restricted network by default on supported platforms |
| Multi-agent/orchestration | Child conversations, worktree/shared isolation, cloud children, automation | Reviewer is a special second agent; no general product-wide swarm shown | Architect/editor models and user-driven sessions; no general swarm | Foreground/background subagents, task/deps/output/parent-child metadata | Multi-agent V2 spawn/message/follow-up/interrupt/wait tools |
| Verification signal | Run events and test/tool output; no independent Rover-style acceptance authority | SWE-bench tests plus optional model reviewer; submission is not acceptance | Lint/test reflection, git commit/undo/diff; commit is not acceptance | Tool/session results; no central acceptance gate | Tool/test results and review surfaces; no independent acceptance authority |
| Telemetry/privacy | Canvas install event is pre-consensus; later events consent-gated; SDK/server has separate consent policy | No runtime analytics client identified; docs enable Google Analytics | PostHog only after opt-in, with system metadata and exception autocapture | No first-party analytics client identified; optional OTel; sharing is manual by default but configurable as auto/disabled | First-party structured analytics plus optional OTLP; rollout traces are separate and local/opt-in |

The table summarizes the cited findings below; it is not a security certification or a performance comparison.

#### OpenHands

**Repository identity and interfaces — Observed**

- The main repository describes Agent Canvas as a web frontend and npm library that talks to an Agent Server over REST and WebSocket; the project also documents local execution and remote/containerized cloud execution. ([architecture](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/docs/architecture.md#L1-L31))
- The SDK provides Python, TypeScript, and REST APIs. Its repository boundary assigns the canonical Python SDK, Agent Server, agents, tools, conversations, workspaces, events, and REST/WebSocket API to this repository; Agent Canvas consumes the typed client. ([SDK interfaces and execution modes](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/README.md#L30-L43), [repository boundary](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/README.md#L83-L87))
- The system exposes more than a terminal interface: Agent Server APIs, SDK clients, the Canvas frontend, and an ACP integration path. ([architecture](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/docs/architecture.md#L1-L23), [ACP live-test contract](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/tests/e2e/live-acp/README.md#L1-L35))

**Agent loop and context — Observed**

- `LocalConversation.run()` repeatedly calls `Agent.step`, handles finish/pause/stuck/error states, and enforces model-budget and maximum-iteration limits. The persisted state default is 500 iterations. ([conversation run loop](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py#L1902-L2042), [state defaults](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/conversation/state.py#L108-L127))
- `EventLog` persists typed events; a `View` derives the LLM-visible history by applying append-only condensation events. The base `Agent` condenser is optional and defaults to `None`, while agent profiles default their condenser settings to `LLMSummarizingCondenserSettings`. ([persistent event log](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/conversation/event_store.py#L34-L61), [condenser model](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/context/condenser/README.md#L13-L29), [Agent default](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/agent/base.py#L266-L280), [profile default](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/profiles/agent_profile.py#L193-L203))
- Event types include messages, actions, observations, errors, pause/interrupt controls, hook execution, condensation, and state updates. This gives a richer audit substrate than retaining only rendered chat text. ([event exports](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/event/__init__.py#L1-L57))
- Tool execution is sequential by default. Parallel execution can be enabled, but the SDK warns that concurrent tools share the conversation, filesystem, and working directory, so mutations may race. ([parallel tools](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/agent/base.py#L293-L301))

**Execution and authority — Observed**

- `LocalWorkspace` intentionally runs commands and filesystem operations in the current working directory with the host process's authority. The SDK explicitly offers either the local machine or an ephemeral Docker/Kubernetes workspace through Agent Server. ([local workspace](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/workspace/local.py#L17-L29), [SDK execution modes](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/README.md#L30-L43))
- `ConversationState` defaults to `confirmation_policy=NeverConfirm()` and `security_analyzer=None`. A local Agent Server can therefore have broad host authority unless the integrator changes the policy and execution boundary. ([state defaults](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-sdk/openhands/sdk/conversation/state.py#L108-L127))
- The self-hosting guide states that anyone able to reach Agent Server should be treated as having the authorized agent's capabilities. This is an explicit deployment trust warning, not an incidental security finding. ([self-hosting warning](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/docs/SELF_HOSTING.md#L6-L19))

**Orchestration — Observed**

- Agent Canvas can launch local or cloud child conversations. Local children can use a Git worktree, which the server creates to reduce sibling collisions, or share the parent's directory; cloud children are described as isolated sandboxes. ([child validation and isolation](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/src/services/child-conversation-launch.ts#L126-L192), [worktree rationale](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/src/services/child-conversation-launch.ts#L250-L289))
- The launch service validates target, task, repository, branch, and isolation before network work, and uses a browser-side handled-call ledger to avoid duplicate launches on replay. This is a useful idempotency pattern for a control plane. ([launch validation](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/src/services/child-conversation-launch.ts#L100-L193), [replay ledger](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/src/services/child-conversation-launch.ts#L196-L227))
- Agent Canvas supports an optional Automation Server for scheduled or event-triggered runs. The SDK repository assigns automation lifecycle behavior to the separate `OpenHands/automation` repository; SDK/Agent Server executes the dispatched conversations. ([Canvas automation service](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/docs/architecture.md#L25-L31), [repository boundary](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/README.md#L83-L87))

**Telemetry and privacy — Observed**

- Agent Canvas sends an anonymous install event on first use regardless of consent. Other Canvas events require the consent modal. Do Not Track, `VITE_DO_NOT_TRACK=1`, or the runtime disable flag prevents PostHog initialization entirely. ([Canvas telemetry policy](https://github.com/OpenHands/OpenHands/blob/b0906809b3e8777491519c386d55ce32d7f4daa4/src/services/telemetry.ts#L4-L31))
- The SDK's Agent Server has a separate consent contract: unset effective consent is not consent, delivery also requires a configured exporter, and the diagnostic-event schema uses constrained scalar fields rather than unrestricted message/tool payloads. Its analytics identity passes through a deployment-supplied `user_id`; otherwise it uses an ephemeral per-process anonymous ID. ([Agent Server consent policy](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-agent-server/openhands/agent_server/telemetry/policy.py#L1-L15), [event contract](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-agent-server/openhands/agent_server/telemetry/models.py#L1-L16), [identity rules](https://github.com/OpenHands/software-agent-sdk/blob/5b36cacccc2bbe6f8fbce9e1d3ff4b0a3dcddadb/openhands-agent-server/openhands/agent_server/telemetry/factory.py#L126-L137))
- **Rover interpretation:** Canvas install telemetry and SDK operational telemetry are separate contracts. Rover should expose an explicit machine-readable telemetry policy and require separate consent for local diagnostics, optional analytics, and any remote sharing.

**Rover lessons**

- Borrow the event vocabulary and replaceable condenser boundary; make summaries/checkpoints addressable artifacts rather than opaque chat truncation.
- Keep worktree-backed child execution, but make fallback to shared writable state visible and require an explicit policy decision.
- Do not copy `NeverConfirm` as a platform default. Confirmation, authority, and isolation must be adapter capabilities evaluated by Rover before a run starts.
- Treat a reachable Agent Server as an agent-capability endpoint and bind it to an authenticated, least-authority execution identity.

#### SWE-agent

**Architecture and loop — Observed**

- `SWEEnv` is a thin adapter around SWE-ReX. The architecture starts a local Docker container by default or a container on a remote system such as Modal or AWS, then executes commands through a shell session inside that deployment. ([environment architecture](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/docs/background/architecture.md#L7-L17))
- The outer run loop repeatedly calls `step`, saves a trajectory, feeds each submission into the retry/reviewer loop, and can start another attempt. A step samples the model, parses the action, checks the blocklist, executes it in the environment, records the observation, and retries selected format/blocklist/syntax failures. ([outer loop](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/agents.py#L390-L440), [step and retry handling](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/agents.py#L936-L1159))
- YAML configuration composes instructions, model, environment, tools, history processors, retry limits, and reviewer settings. This makes experiments reproducible and the harness replaceable. ([configuration example](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/README.md#L88-L126))
- Tool execution is mediated by a tool registry and a `ToolHandler` that enforces the configured blocklist before invoking environment commands. This constrains agent behavior but is not equivalent to an OS sandbox. ([tool interfaces](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/tools/tools.py#L29-L54), [blocklist handler](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/tools/tools.py#L353-L388))

**Context, reviewer, and evidence — Observed**

- History processors transform observations, tag tool calls, collapse stale file windows, and add provider-specific cache-control marks. They are configured independently of the environment. ([processor protocol](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/history_processors.py#L13-L16), [observation and file-window processors](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/history_processors.py#L160-L218), [cache-control processor](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/history_processors.py#L261-L284))
- The reviewer runs after submissions and can retry the agent up to configured attempt/budget limits. Its chooser retry loop sends the collected submissions to a model and returns the selected attempt index. This is model-mediated selection, not independent verification. ([reviewer interfaces](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/reviewer.py#L30-L116), [chooser retry and selection](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/reviewer.py#L499-L555))
- Trajectories persist the full agent trajectory, message history, environment name, replay configuration, and model/run information to JSON, making experiments inspectable. ([trajectory serialization](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/agent/agents.py#L762-L787))
- PR creation is an external write and is therefore explicitly gated by a user flag rather than a normal completion action. ([PR submission flag](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/sweagent/run/hooks/open_pr.py#L36-L48))

**Telemetry and privacy — Observed/Gap**

- No runtime product-analytics exporter was identified in the reviewed source tree. The documentation build does enable Google Analytics, which is a separate documentation-site channel. ([documentation configuration](https://github.com/SWE-agent/SWE-agent/blob/3ea751c087f32b16e039a2233dd6eefecef325d5/mkdocs.yml#L22-L31))

**Rover lessons**

- Make execution and history policies declarative inputs that can be capability-checked before launch.
- Persist raw observations and derived summaries separately; a summary should never be the only record of a consequential action.
- Keep PR/publish/merge actions outside normal task completion and bind them to explicit external-write grants.
- Use reviewer agents for candidate ranking only. Rover should require a separate verifier and acceptance authority.

#### Aider

**Interfaces and agent loop — Observed**

- Aider is terminal-first, supports `--message`/`--message-file` scripting and an optional browser UI, and can watch files for AI comments. ([scripting](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/website/docs/scripting.md#L13-L40), [browser UI](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/website/docs/usage/browser.md#L34-L42), [watch files](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/website/docs/usage/watch.md#L35-L59))
- The coder sends repository context and receives a streaming model response, applies edits, auto-commits when configured, then optionally runs lint and tests. Lint/test failures can become reflected messages that start another bounded model exchange. ([message/edit/lint/test loop](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L1419-L1623), [bounded reflection loop](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L924-L944), [auto-commit](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L2375-L2409))
- Commands run through a subprocess with `shell=True` in the user's working directory. The default workflow is therefore host-authoritative, not sandboxed. ([subprocess execution](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/run_cmd.py#L44-L73))

**Context and repository mapping — Observed**

- Aider builds a token-limited repository map from files outside the current chat set. It uses tree-sitter tags, graph ranking/PageRank, caching, and refresh policies rather than sending the entire repository blindly. ([repo-map entry](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/repomap.py#L103-L145), [PageRank](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/repomap.py#L382-L529), [cache and refresh](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/repomap.py#L576-L618))
- `ChatSummary` recursively summarizes older messages up to a configured depth of three while keeping the current exchange visible. This is a simple, understandable context-retention model. ([history summarization](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/history.py#L7-L74))
- Architect/editor configurations can split planning from file edits, but the primary unit remains a user-driven coding session rather than a general multi-agent control plane. ([architect/editor coder](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/architect_coder.py#L18-L47))

**Git recovery and verification — Observed**

- Aider's value is strongest in its git-aware human workflow: changes can be inspected, automatically committed, undone, diffed, and selectively committed or reset. These are recovery controls, not independent acceptance. ([commit](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/commands.py#L337-L438), [reset and undo](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/commands.py#L439-L656), [diff](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/commands.py#L657-L720), [dirty-file commit](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L2175-L2238))
- Lint/test commands are fed back to the model as observations. Rover should preserve this feedback loop but separately record the command, working directory, candidate revision, exit status, and captured output.

**Telemetry and privacy — Observed**

- Analytics are disabled until opt-in. Aider initializes PostHog only after consent, attaches system metadata as super-properties, and enables exception autocapture. Model names are redacted in the reviewed event path. ([consent and initialization](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/analytics.py#L60-L108), [event properties and capture](https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/analytics.py#L220-L254))

**Rover lessons**

- Borrow repo-map ranking, visible git checkpoints, diff-first recovery, and lint/test reflection.
- Put repo-map content in provenance-aware context blocks so Rover can show why files or symbols were selected.
- Never label a commit, a successful lint command, or a model-authored test result as task acceptance.
- Keep host execution visibly distinct from container or VM execution.

#### OpenCode

**Interfaces and persistence — Observed**

- OpenCode offers a TUI, desktop client, web/IDE clients, a server, SDK, and ACP integration. Plugins can install custom tools, commands, agents, MCP servers, or event handlers. ([project interfaces](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/README.md#L15-L32), [plugin API](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/plugin/src/index.ts#L43-L71))
- The server defaults to loopback, but binding elsewhere does not automatically enable authentication: without `OPENCODE_SERVER_PASSWORD`, the CLI warns that the server is unsecured. Setting the password enables HTTP Basic authentication checked by the authorization middleware. ([network default](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/cli/network.ts#L6-L20), [unsecured warning](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/cli/cmd/serve.ts#L13-L20), [authorization](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/server/routes/instance/httpapi/middleware/authorization.ts#L40-L53))
- Session sharing uploads session, message, part, diff, and model data to `opncd.ai` or a configured enterprise URL and produces a share URL. Sharing is manual unless configured as `auto`, and can be disabled; `OPENCODE_DISABLE_SHARE` disables the transport. ([share data and destination](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/share/share-next.ts#L23-L67), [destination and auth](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/share/share-next.ts#L206-L232), [share policy](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/share/session.ts#L26-L45))

**Agent loop and context — Observed**

- The session loop loads prior messages, calls the model with tools, persists assistant/tool messages, streams output, and continues until completion, abort, or a configured maximum step count. Transient provider errors use bounded exponential backoff, and the loop compacts when the context window is exceeded. ([session loop](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/prompt.ts#L1081-L1203), [retry integration](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/processor.ts#L670-L695), [retry policy](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/retry.ts#L26-L41))
- Compaction summarizes older messages, records the replacement history, and retains a configured recent-token tail. An optional pruning pass clears old tool outputs while preserving tool-call/result structure. ([compaction flow](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/compaction.ts#L358-L483), [pruning](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/compaction.ts#L273-L315))
- Session revert/unrevert restores a recorded filesystem snapshot and removes or restores later messages, providing a concrete recovery path independent of model reasoning. ([revert and unrevert](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/session/revert.ts#L38-L113))

**Permissions and execution — Observed**

- OpenCode's security documentation explicitly says the agent is not sandboxed and that Docker or a VM should be used for containment. Permissions are described as a user-experience mechanism, not a security boundary. ([security model](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/SECURITY.md#L9-L32))
- Permission evaluation flattens ordered wildcard rules, selects the last matching rule, and defaults to `ask` when none matches. The inspected v2 rule shape separates action/resource patterns from allow/deny/ask effect. ([permission evaluation](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/core/src/permission.ts#L76-L85))
- The shell tool parses the command for safety analysis, checks directory access, asks the permission system, and then spawns a subprocess with shell execution. Permission approval is therefore separate from OS confinement. ([shell execution](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/tool/shell.ts#L91-L173), [subprocess launch](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/tool/shell.ts#L305-L326))

**Orchestration and extensibility — Observed**

- The `task` tool launches foreground or background specialized subagents, can resume a prior child by `task_id`, records parent/session/model/background metadata, and applies a nesting-depth limit. A child inherits the parent model unless its agent definition selects another model. ([task parameters](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/tool/task.ts#L24-L61), [depth, child session, and model](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/tool/task.ts#L92-L190))
- Subagents have separate sessions but inherit the parent's deny rules and external-directory rules. They are not equivalent to independent OS identities or sandboxes. ([subagent permissions](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/agent/subagent-permissions.ts#L1-L27))
- MCP, LSP, plugins, custom agents, commands, SDK/server APIs, and ACP create several extension surfaces. Rover should consume only a reviewed, versioned subset rather than treating plugin presence as a trusted capability. ([plugins](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/plugin/src/index.ts#L26-L71), [tool registration](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/tool/registry.ts#L15-L82))

**Telemetry and privacy — Observed/Gap**

- No first-party product analytics client was identified in the reviewed core source. OpenCode uses Effect's OpenTelemetry tracer in agent and LLM code, and its v2 design says observability is process-level and should use standard OpenTelemetry environment or declarative configuration rather than a product-specific analytics toggle. ([agent tracing](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/agent/agent.ts#L23-L30), [v2 observability decision](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/specs/v2/config.md#L371-L381))
- Session sharing is a separate remote-write path and should not be conflated with telemetry. It is manual by default, but configuration can enable automatic sharing; operators can instead set `share = "disabled"`. ([share policy](https://github.com/anomalyco/opencode/blob/1d6c3c0e29f6a6ceff5204fa40ea3fb54338f159/packages/opencode/src/share/session.ts#L26-L45))

**Rover lessons**

- Adopt last-match-wins permission rules only inside a versioned policy schema and always render the effective rule before execution.
- Never describe an `ask/allow/deny` permission layer as sandboxing.
- Preserve session snapshots and task-parent metadata, but bind recovery points to immutable candidate revisions and workspace state.
- Make sharing a separate, redacted, user-initiated external write with a destination preview and revocation/expiry policy.
- Treat plugins, MCP servers, LSP servers, and subagents as untrusted extensions until their requested capabilities are reviewed.

#### OpenAI Codex

**Interfaces and architecture — Observed/Official claim**

- The repository contains separate TypeScript and Python SDKs, an app-server binary, and the TUI/exec runtimes. The TypeScript SDK wraps the CLI and exchanges JSONL events; the Python SDK starts Codex threads and runs turns. ([TypeScript SDK](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/sdk/typescript/README.md#L1-L36), [Python SDK](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/sdk/python/README.md#L1-L29), [app-server binary](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/app-server/src/main.rs#L32-L124))
- The official app-server documentation describes a JSON-RPC service for rich coding-agent experiences, with stdio and WebSocket transports, lifecycle initialization, and thread/run/turn events. ([official app-server documentation, accessed 2026-09-24](https://developers.openai.com/codex/app-server))
- The official noninteractive documentation describes `codex exec` as JSONL output for CI and scripted use, with structured final messages and exit-code behavior. ([official noninteractive documentation, accessed 2026-09-24](https://developers.openai.com/codex/noninteractive))
- The official MCP documentation describes stdio and streamable HTTP transports and configuration-based server registration. ([official MCP documentation, accessed 2026-09-24](https://developers.openai.com/codex/mcp))

**Turn loop and context — Observed**

- `run_turn` loops over model responses, executes tool calls, handles local and remote turn contexts, and returns after the model completes the task or a terminal/interrupt condition is reached. ([turn loop](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/session/turn.rs#L151-L220))
- `run_turn` attempts to auto-compact when usage reaches the configured percentage of the context window. Manual/auto compaction routes to remote V2 when supported and otherwise uses the local compaction path; the local path preserves recent user context while building a summary-backed replacement history. ([auto-compaction](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/session/turn.rs#L1260-L1372), [compaction routing](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/tasks/compact.rs#L41-L68), [local replacement](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/compact.rs#L347-L401))
- The compaction code records a summary-backed replacement plus response ID, window number/IDs, model hash, and a reference-context baseline. This makes replacement lineage more explicit than ordinary message truncation. ([checkpoint metadata](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/compact.rs#L79-L90), [replacement lineage](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/compact.rs#L347-L401))

**Sandbox and approvals — Observed/Official claim**

- Codex separates approval policy from sandbox policy. Official documentation states that interactive defaults are workspace-write, network is disabled by default, and local OS sandboxing constrains filesystem and process access on supported platforms. ([official security/approval documentation, accessed 2026-09-24](https://developers.openai.com/codex/agent-approvals-security))
- Source protocol identifiers include `MacosSeatbelt`, `LinuxSeccomp`, `WindowsRestrictedToken`, and `WindowsMxc`; this demonstrates an OS-sandbox abstraction rather than a prompt-only restriction. ([sandbox protocol](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/exec-server-protocol/src/protocol.rs#L27-L86))
- Escalation requests are separate from initial command execution and can require user approval. The official docs warn that `dangerously-bypass-approvals-and-sandbox` removes a critical layer of protection. ([approval documentation](https://developers.openai.com/codex/agent-approvals-security), [sandbox policy types](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/protocol/src/protocol.rs#L248-L283))
- **Official claim, not independently tested:** exact policy enforcement varies by operating system, sandbox backend, repository trust, and configuration. Rover must not infer a universal guarantee from the existence of backend names.

**Multi-agent control — Observed**

- Multi-agent V2 exposes tools to spawn an agent, send it a message, submit a follow-up task, list live agents, wait for mailbox/status updates, and interrupt an agent. The spawn path reserves an execution slot under the configured per-session concurrency limit. ([V2 handlers](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/tools/handlers/multi_agents_v2.rs#L28-L42), [tool contracts](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/tools/handlers/multi_agents_spec.rs#L100-L363), [capacity reservation](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/agent/control/spawn.rs#L646-L670))
- Thread metadata records parent/child relationships, and `codex exec` can create a managed linked worktree when the worktree feature is enabled and the source is not explicitly untrusted. This is a useful reference for making delegation relationships inspectable. ([managed worktree creation](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/exec/src/lib.rs#L409-L475), [thread initialization metadata](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/analytics/src/events.rs#L239-L253))

**Telemetry, traces, and privacy — Observed/Official claim**

- Codex has two distinct telemetry paths: user-configured OpenTelemetry tracing/logging and first-party structured analytics. OTEL is documented as disabled unless configured, while first-party analytics have a surface-dependent default. ([official OTEL documentation, accessed 2026-09-24](https://developers.openai.com/codex/otel))
- Core configuration leaves `analytics_enabled` optional because the default depends on the client. App-server starts with analytics disabled, while TUI and `codex exec` pass a true default into their telemetry provider unless configuration overrides it. ([analytics config](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/core/src/config/mod.rs#L1124-L1137), [app-server default](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/app-server/src/main.rs#L116-L127), [TUI default](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/tui/src/startup_orchestration.rs#L591-L598), [exec default](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/exec/src/lib.rs#L173-L173))
- The first-party event schema includes thread/turn identifiers, model and runtime metadata, sandbox and approval settings, token/tool counts, file-change counts, command categories and exit codes, MCP tool names, and timing. It is structured operational telemetry, not a blanket “no content” guarantee. ([turn event schema](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/analytics/src/events.rs#L1069-L1138), [command event schema](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/analytics/src/events.rs#L794-L817), [MCP event schema](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/analytics/src/events.rs#L857-L875))
- Rollout trace bundles are a separate, explicitly local, opt-in debugging feature. The trace source describes append-only raw events and replayable reduced traces. ([trace format](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/rollout-trace/src/lib.rs#L1-L7), [trace enable variable](https://github.com/openai/codex/blob/7e5054d32f1dae4f30136f078116100ee9722e5c/codex-rs/rollout-trace/src/lib.rs#L59-L64))

**Rover lessons**

- Model sandbox policy, approval policy, and network policy as separate negotiated capabilities; do not collapse them into one “safe mode” flag.
- Use an app-server-like bidirectional protocol for UI/automation clients, but require a versioned handshake and explicit capability advertisement.
- Preserve append-only raw traces alongside compacted context, with content-addressed summary and checkpoint artifacts.
- Surface parent/child session lineage and worktree identity in orchestration views.
- Keep first-party analytics, operator-configured OTEL, and local diagnostic traces distinct; if an adapter adds session sharing, make it a fourth explicit policy surface.

#### Cross-project findings relevant to verification

- **Observed:** SWE-agent runs in an environment designed around SWE-bench, exposes trajectories, and can use tests plus a reviewer; Aider feeds lint/test output back to the model and uses git checkpoints; OpenHands records tool results and run events; OpenCode records commands, file changes, and session state; Codex records command/MCP/file-change events and supports review surfaces.
- **Rover interpretation:** these are different forms of feedback and provenance. None establishes that a named verifier independently evaluated the exact candidate revision against the declared acceptance policy.
- **Required Rover distinction:** `executor reported success`, `tool exited zero`, `tests observed passing`, `model reviewer selected candidate`, `artifact committed`, and `Rover acceptance authority accepted` must be separate evidence records.

### Inferences / Rover Implications

1. **Adopt a two-layer session model.** Preserve an append-only event ledger and derive a bounded model context from it. All five projects ultimately need this distinction, though they implement it as event logs, history processors, repo maps, snapshots, or compaction.
2. **Make execution profiles explicit.** “Local,” “container,” “remote,” and “OS sandbox” are materially different authority domains. A provider/model selection must not imply an execution security level.
3. **Treat approvals as protocol operations.** Codex's policy engine, OpenCode's permission matcher, and OpenHands' confirmation policy are adapter capabilities. Rover should own the user-facing grant, render its scope, and pass a time-bounded capability to the adapter.
4. **Orchestrate above the model loop.** Subagents are useful implementation details, but Rover's deterministic workflow graph, policy gates, and evidence ledger should remain outside any one agent's prompt or tool loop.
5. **Use plugins and MCP as untrusted extension surfaces.** Discover capabilities, review requested authority, pin versions/contracts, and record which extension participated in each consequential action.
6. **Keep privacy surfaces separate.** Local transcript, first-party analytics, optional telemetry export, diagnostic trace upload, and collaborative session sharing require independent settings and visible destinations.
7. **Make recovery candidate-specific.** Aider's git checkpoints, OpenCode's snapshots, and Codex's linked worktrees show the value of recovery, but Rover should bind each checkpoint to repository revision, workspace identity, policy version, and context lineage.

### Gaps

- This was static source/documentation review. No agent was installed, no provider was called, no benchmark was run, and no sandbox or telemetry claim was tested dynamically.
- Native sandbox behavior varies by OS and configuration; source identifiers and official claims do not prove identical enforcement everywhere.
- “No runtime analytics client identified” for SWE-agent and OpenCode is a bounded source-review result, not proof that no deployment, wrapper, documentation service, or optional component emits telemetry.
- Model quality, patch correctness, latency, and cost were not compared. Any ranking effect from those dimensions would require controlled, current, provider-pinned evaluations.
- Official Codex documentation is a moving target; the cited pages were accessed on 2026-09-24 and should be archived or rechecked when contracts are implemented.

## 3. What should Rover borrow, avoid, and verify next?

### Takeaway

Rover should not become another agent loop. Its differentiated value is to remain the terminal-first, agent-neutral authority and evidence layer around multiple agents. The strongest design combines Codex-style negotiated execution policy, OpenHands-style event/condenser lineage, OpenCode-style inspectable permissions and extension metadata, Aider-style git/repo recovery, and SWE-agent-style declarative reproducibility—while keeping acceptance, external writes, and human override outside the executor.

The most important design rule is: **a model or agent may propose a candidate, but it may not silently promote that candidate to verified, accepted, publishable, mergeable, or externally written state.** Rover's security model already separates executor, verifier, and acceptance authority; the surveyed projects reinforce why that separation is necessary rather than redundant ([Rover security model](../../docs/SECURITY_MODEL.md#L3-L13)).

### Cited Findings

Rover's current documented boundaries provide the local baseline:

- Rover's intended scope is a full-platform control plane, not only a single coding loop, and the project must remain terminal-first and agent-neutral. ([README](../../README.md#L5-L8))
- Current status distinguishes implemented controls and workflows from unvalidated or future claims. ([STATUS](../../STATUS.md#L7-L18))
- The security model keeps filesystem/process authority, network and credential use, verification, and acceptance as explicit policy surfaces. It requires conservative defaults and separates execution from external writes. ([security model](../../docs/SECURITY_MODEL.md#L3-L13))

The external evidence supports five concrete patterns:

1. **Bounded loops exist everywhere, but limits are not acceptance.** OpenHands, SWE-agent, OpenCode, and Codex all bound iterations/steps; Aider relies more on user/session flow and model response termination.
2. **Context replacement needs lineage.** OpenHands, OpenCode, and Codex persist events/messages and then summarize or prune them; Aider summarizes older chat; SWE-agent uses configurable history processors.
3. **Execution isolation must be concrete.** SWE-agent defaults to Docker, Codex exposes OS sandbox backends, while OpenHands local, Aider, and OpenCode can execute with host authority.
4. **Subagents need inspectable lineage and resource limits.** OpenHands, OpenCode, Codex, and Prime Agent expose parent/child/session relationships; concurrency and filesystem collision behavior remain separate concerns.
5. **Privacy is not one switch.** The reviewed projects combine consent-based analytics, pre-consent install events, optional telemetry, local traces, and remote sharing in different combinations.

### Inferences / Rover Implications

#### Patterns to borrow

**1. Capability-negotiated agent adapters**

Define an adapter handshake that reports, at minimum:

- supported model/provider transports;
- stream and cancellation behavior;
- filesystem/workspace modes;
- process execution and network modes;
- approval/confirmation hooks;
- context checkpoint/compaction support;
- subagent limits and parent/child correlation;
- MCP/plugin/tool discovery;
- telemetry and sharing surfaces;
- protocol version and compatibility range.

Do not infer support from README text, a successful model response, or a tool list. An adapter should be accepted only after conformance tests against the negotiated profile.

**2. Event ledger plus derived context**

Store immutable events for requests, model responses, tool calls, tool results, file changes, policy decisions, checkpoints, verification, acceptance, and external writes. Generate model context from that ledger through pluggable selectors/condensers. Every summary should record:

- source event range or digest;
- producer and policy version;
- creation time and token estimate;
- whether it is advisory or authoritative;
- supersession/replacement lineage.

This combines OpenHands' event model, Aider's summaries, OpenCode's snapshots, and Codex's compaction checkpoints without allowing a lossy summary to replace the record.

**3. Explicit execution profiles**

Use at least these distinct profiles:

- `local-readonly`;
- `local-workspace-write`;
- `container-workspace-write`;
- `remote-environment`;
- `os-sandboxed-workspace-write`.

Each run should display filesystem roots, process/network authority, credential availability, and external-write capability before execution. A permission matcher can mediate UX inside a profile, but it must not upgrade the profile's security level.

**4. Inspectable delegation**

Support optional subagents, but keep orchestration outside them. Persist parent run, child run, task, dependency, model/provider, workspace/worktree, resource budget, status, and result lineage. Default parallel work that writes to the same repository to separate worktrees or serialized execution. Shared writable state must be an explicit, visible fallback.

**5. Evidence-bound verification**

Bind every verification result to:

- declared acceptance policy/version;
- candidate artifact digest or immutable revision;
- workspace and environment identity;
- verifier implementation/version;
- command/test identity and relevant configuration;
- captured output and exit status;
- timestamp and actor;
- pass/fail/unknown result.

A model reviewer may rank candidates but cannot serve as independent verifier unless Rover explicitly classifies that role as advisory. A user acceptance action must remain separate from executor success.

**6. Recovery and human override**

Provide checkpoints, diffs, undo/revert, and candidate selection as first-class operations. A checkpoint should include repository revision, uncommitted workspace state, context lineage, policy snapshot, and external-write state. Human override should be able to stop a run, reject a candidate, revoke a grant, or select a prior checkpoint without mutating the evidence record.

**7. Extension review**

Treat MCP servers, plugins, custom agents, commands, hooks, and LSP integrations as external code. Require declared capabilities, a reviewed contract, version pinning, and per-run attribution. Plugin installation or remote plugin upload is an external write and should be separately authorized.

**8. Layered privacy controls**

Use separate settings for:

- local transcript retention;
- local diagnostic trace bundles;
- first-party operational analytics;
- user-configured OTLP export;
- collaborative session sharing;
- remote compaction/provider-side processing.

Show the effective destination and payload class. Default Rover to local-only evidence; make every remote export an explicit action.

#### Patterns to avoid

- **Do not copy permissive confirmation defaults.** OpenHands' `NeverConfirm` is convenient SDK behavior but unsafe as Rover's platform default.
- **Do not call prompt-level permissions a sandbox.** OpenCode explicitly documents this limitation; Rover's UI and schemas should use stronger language.
- **Do not treat a model-authored test or reviewer vote as independent acceptance.** Keep executor, verifier, reviewer, and acceptance authority distinct.
- **Do not make commit, PR, merge, publish, deployment, or credential upload ordinary completion actions.** These remain explicit external writes.
- **Do not collapse telemetry, trace upload, and session sharing.** They expose different data to different destinations and need separate consent and retention.
- **Do not expose unsupported capabilities in stubs.** Adapter discovery and UI affordances must reflect tested protocol behavior.
- **Do not discard raw context after compaction.** Lossy context is useful for the model but is not an audit ledger.
- **Do not infer project health from stars.** Record revision, release channel, maintenance notices, and evidence depth.

#### Rover roadmap implications

These are recommendations for traceability, not claims that the features are implemented:

| Candidate contract/test area | External evidence informing it | Minimum Rover regression scenario |
|---|---|---|
| Adapter capability handshake | Codex app-server; OpenCode SDK/ACP; OpenHands SDK | Unsupported sandbox, cancellation, checkpoint, and subagent claims are rejected before launch. |
| Execution profile enforcement | Codex OS sandbox; SWE-agent Docker; OpenHands local/remote | A local-workspace run cannot silently claim container/OS isolation; network and credential scopes are visible. |
| Context lineage | OpenHands events/condenser; Codex compaction; Aider summary; OpenCode snapshots | Resume from a compacted checkpoint reconstructs declared context lineage and does not mutate prior evidence. |
| Worktree-safe orchestration | OpenHands worktree/shared children; OpenCode task metadata; Codex linked worktrees | Concurrent writers receive separate workspaces or an explicit serialized/shared-state decision. |
| Evidence-bound verification | SWE-agent tests/reviewer; Aider lint/test reflection; Codex command events | A passing test recorded against revision A cannot accept candidate revision B. |
| External-write authority | SWE-agent PR flag; Rover security model | Merge/publish/deploy/credential actions fail without a scoped, unexpired grant and produce an attributable record. |
| Privacy defaults | OpenHands Canvas/SDK split; Aider opt-in; Codex analytics/OTel/trace split; OpenCode sharing | Fresh install sends no remote analytics; each remote surface has an independent visible setting and destination. |
| Session sharing | OpenCode share flow | Preview/redaction, explicit destination, expiry/revocation, and immutable sharing evidence are required. |
| Extension isolation | OpenCode plugins/MCP; Codex MCP; OpenHands extensibility | An unapproved extension cannot acquire filesystem, process, network, credential, or external-write authority. |

#### Suggested next research passes

1. Source-audit the B-level projects in descending relevance: Prime Agent, `mini-swe-agent`, Goose, Qwen Code, Cline, and Gemini CLI.
2. Build a common protocol matrix for cancellation, approvals, context checkpoint/resume, subagent correlation, sandbox identity, telemetry, and external writes.
3. Reproduce only narrow, controlled cases: parallel worktree conflict, compaction resume, candidate-to-verifier binding, and opt-in telemetry. Do not turn this into a model-quality benchmark.
4. Recheck repository identities and release feeds before publishing the shortlist because several projects are in rapid transition.
5. Archive or date-stamp official Codex documentation used for adapter requirements.

### Gaps and explicit non-claims

- No live-provider test, model benchmark, native sandbox test, traffic capture, or security certification was performed.
- No claim is made that OpenHands, OpenCode, Aider, SWE-agent, or Codex is "secure," "private," or "agent-neutral" in every deployment. Those properties depend on configuration, extensions, provider, environment, and operator choices.
- No current code claim is made about Rover. The recommendations above describe patterns to consider and test against Rover's existing contracts, not shipped functionality.
- The top-20 order is intentionally conservative and should be revisited after the B-level source audits, especially for Prime Agent, `mini-swe-agent`, Goose, and current editor-agent projects.
