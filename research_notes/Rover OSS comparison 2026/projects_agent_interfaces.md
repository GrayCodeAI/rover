# Rover OSS comparison: five agent interfaces, September 2026

## Scope, identity, and high-level comparison

### Takeaway

The five projects are not direct substitutes for Rover. Gemini CLI, Cline, Goose, Continue, and Roo Code are primarily **model-agent runtimes and user-facing clients** that own an inference/tool loop. Rover is a **terminal-first, agent-neutral supervisor and assurance substrate** that can sit beside an existing coding agent, run a generic command, verify the resulting candidate, retain evidence, and expose a bounded MCP surface. The most useful comparison is therefore not “which agent should replace Rover?” but “which interaction, session, sandbox, extension, and evaluation patterns strengthen Rover's control plane?”

As of **2026-09-24**, the activity split is decisive:

- **Gemini CLI** and **Cline** are highly active.
- **Block Goose** is highly active under its new canonical identity, `aaif-goose/goose`, and the Linux Foundation's Agentic AI Foundation.
- **Continue** reached a final 2.0 release, says its repository is read-only, and has joined Cursor.
- **Roo Code** shut down its products and archived its repository on 2026-05-15.
- All five repositories identify as **Apache-2.0**, although one Goose documentation index contains a stale MIT statement; the repository license and GitHub metadata agree on Apache-2.0.

### Method and Rover baseline

All web sources below were retrieved on **2026-09-24** unless a source's own publication or update date is stated. Star counts are point-in-time `stargazers_count` values from the GitHub REST API, not permanent totals. “Latest release” means the repository's `releases/latest` endpoint at retrieval time. No CI run, provider account, benchmark, or security certification was executed for this research; workflow and benchmark claims below mean that the primary repository documents or defines them, not that this research observed them pass.

Rover's comparison baseline comes from the local project:

- Rover describes itself as **terminal-first** and **agent-neutral**, with persistent sessions, parallel workflows, verification, review, and evidence ([Rover README, lines 5–8](../../README.md)).
- It supports generic headless/PTY profiles plus bounded Codex and Claude adapters, but explicitly does **not** claim full App Server/ACP support, live-provider certification, or portable native conversation migration ([Rover README, lines 60–75](../../README.md)).
- Rover's current MCP implementation is a deliberately limited subset, read-only by default, with explicit flags and grants for execution ([Rover README, lines 121–143](../../README.md)).
- Rover states that local execution is **not a sandbox** and that worktrees, scrubbed environments, and grants do not isolate same-user code ([Rover security model, lines 3–13](../../docs/SECURITY_MODEL.md)).
- Rover's implemented-versus-remaining matrix explicitly leaves full MCP transport coverage, ACP, live sandbox validation, cross-host fencing, external writes, and independent evaluation as future or partial work ([Rover status, lines 7–18](../../STATUS.md)).

This category distinction matters: an inference loop can generate a candidate, but it does not become an acceptance authority merely because it reports completion. Rover's central responsibility should remain independent observation, deterministic verification, explicit review, and retained evidence.

### Identity, license, stars, and activity snapshot

| Project | Verified identity and activity on 2026-09-24 | Approximate stars | License and activity judgment |
|---|---|---:|---|
| **Google Gemini CLI** | Canonical repository `google-gemini/gemini-cli`; description says an open-source agent bringing Gemini to the terminal. API reported `pushed_at` 2026-09-23; latest release `v0.60.0` published 2026-09-15. ([repository API](https://api.github.com/repos/google-gemini/gemini-cli), [latest release API](https://api.github.com/repos/google-gemini/gemini-cli/releases/latest)) | **107,138** (≈107.1k) ([repository API](https://api.github.com/repos/google-gemini/gemini-cli)) | **Apache-2.0; very active.** Caveat: the official docs banner says Gemini CLI was replaced by Antigravity CLI on 2026-06-18 for unpaid-tier and Google One users. This is a product-segment migration notice, not a repository shutdown. ([repository README](https://github.com/google-gemini/gemini-cli/blob/main/README.md), [official docs](https://geminicli.com/docs/)) |
| **Cline** | Canonical repository `cline/cline`; current README describes one harness across CLI, desktop, VS Code, JetBrains, and SDK. API reported `pushed_at` 2026-09-23; `releases/latest` returned `desktop-v0.0.34`, published 2026-09-22. ([repository API](https://api.github.com/repos/cline/cline), [latest release API](https://api.github.com/repos/cline/cline/releases/latest), [repository README](https://github.com/cline/cline/blob/main/README.md)) | **69,171** (≈69.2k) ([repository API](https://api.github.com/repos/cline/cline)) | **Apache-2.0; very active and expanding across clients.** |
| **Block Goose** | The user-supplied API URL for `block/goose` resolves to canonical repository `aaif-goose/goose`. README says Goose is now stewarded by the Agentic AI Foundation at the Linux Foundation. API reported `pushed_at` 2026-09-23; `v1.52.0` was published 2026-09-23. ([requested repository API](https://api.github.com/repos/block/goose), [canonical repository API](https://api.github.com/repos/aaif-goose/goose), [latest release API](https://api.github.com/repos/aaif-goose/goose/releases/latest), [governance](https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md)) | **54,594** (≈54.6k) ([canonical repository API](https://api.github.com/repos/aaif-goose/goose)) | **Apache-2.0; very active and foundation-governed.** The old `block/goose` path is an identity redirect, not the current canonical home. |
| **Continue** | Canonical repository `continuedev/continue`. Its final README says the repository is no longer actively maintained and is read-only; the official site says Continue was acquired by Cursor. API reported `pushed_at` 2026-09-22, but the final 2.0.0 release was published 2026-06-19, so a push timestamp must not be treated as proof of ongoing maintenance. ([repository API](https://api.github.com/repos/continuedev/continue), [latest release API](https://api.github.com/repos/continuedev/continue/releases/latest), [repository README](https://github.com/continuedev/continue/blob/main/README.md), [official site](https://continue.dev/)) | **36,005** (≈36.0k) ([repository API](https://api.github.com/repos/continuedev/continue)) | **Apache-2.0; final/read-only, not an active dependency target.** |
| **Roo Code** | Canonical repository `RooCodeInc/Roo-Code`. GitHub marks it archived; API reported last push on 2026-05-15. Latest release `v3.54.0` was published 2026-05-15, the same day the project announced shutdown. ([repository API](https://api.github.com/repos/RooCodeInc/Roo-Code), [latest release API](https://api.github.com/repos/RooCodeInc/Roo-Code/releases/latest), [repository README](https://github.com/RooCodeInc/Roo-Code/blob/main/README.md)) | **24,299** (≈24.3k) ([repository API](https://api.github.com/repos/RooCodeInc/Roo-Code)) | **Apache-2.0 historically; shut down and archived.** It is useful as a design reference, not a maintained integration target. |

### Interface, provider, and protocol comparison

| Project | Terminal vs desktop/browser model | Provider neutrality | MCP and adjacent tool protocols | Extensibility pattern |
|---|---|---|---|---|
| **Gemini CLI** | Explicitly terminal-first interactive REPL, with headless/streamed JSON modes, IDE companion integrations, and experimental browser-agent features. It is not a desktop or browser-hosted product. ([README](https://github.com/google-gemini/gemini-cli/blob/main/README.md), [official docs](https://geminicli.com/docs/)) | **Not provider-neutral.** Official authentication choices are Google account/Gemini Code Assist, Gemini Developer API, or Vertex AI; “model routing” is routing across Gemini-family models, not arbitrary agent/model vendors. ([authentication](https://geminicli.com/docs/get-started/authentication), [terms/privacy](https://geminicli.com/docs/resources/tos-privacy)) | MCP client with stdio, SSE, and Streamable HTTP, OAuth handling, tools, and resources. Docs also expose A2A remote-agent support and ACP mode for IDE integrations. ([MCP](https://geminicli.com/docs/tools/mcp-server/), [telemetry client surfaces](https://geminicli.com/docs/cli/telemetry)) | Rich first-party extension format bundles prompts, MCP servers, commands, themes, hooks, subagents, and skills; plus custom commands, agent skills, policies, worktrees, and remote agents. ([extensions](https://geminicli.com/docs/extensions/)) |
| **Cline** | One agent harness surfaced as terminal CLI/TUI, headless scripts, native macOS/Windows desktop, VS Code, JetBrains, SDK, Kanban/web surfaces, and ACP editor clients. Browser automation is a tool, not the primary application model. ([README](https://github.com/cline/cline/blob/main/README.md), [Desktop](https://docs.cline.bot/usage/cline-desktop), [CLI](https://docs.cline.bot/usage/cli-overview)) | **Strongly provider-neutral:** Anthropic, OpenAI, Google, OpenRouter, Bedrock, Azure/Vertex, local models, and OpenAI-compatible endpoints, alongside Cline-hosted access. ([provider docs](https://docs.cline.bot/provider-config/other-30-plus-providers), [README](https://github.com/cline/cline/blob/main/README.md)) | MCP client with stdio, Streamable HTTP, and legacy SSE; ACP server for Zed/JetBrains/Neovim/Emacs-style clients; SDK events and tools API. ([MCP](https://docs.cline.bot/mcp/mcp-overview), [ACP](https://docs.cline.bot/usage/acp), [SDK](https://docs.cline.bot/sdk/overview)) | SDK plugins can add tools and lifecycle hooks; rules, skills, workflows, MCP, subagents, teams, schedules, connectors, hub/remote backends, and a capability marketplace create a very broad extension surface. ([SDK plugins](https://docs.cline.bot/sdk/plugins), [ClineCore](https://docs.cline.bot/sdk/clinecore)) |
| **Goose** | Local-first general-purpose agent with a full terminal CLI, native desktop app for macOS/Linux/Windows, embeddable API/server, and remote ACP service. It is not primarily a browser-hosted agent. ([README](https://github.com/aaif-goose/goose/blob/main/README.md)) | **Strongly provider-neutral:** 15+ providers including Anthropic, OpenAI, Google, Ollama, OpenRouter, Azure, and Bedrock; ACP providers can use existing Claude/ChatGPT/Gemini subscriptions. ([README](https://github.com/aaif-goose/goose/blob/main/README.md), [ACP providers](https://goose-docs.ai/docs/guides/acp-providers)) | MCP is the central extension protocol. Goose also acts as an ACP server for clients and can consume ACP agents as providers, passing extensions through to those agents. ([extensions](https://goose-docs.ai/docs/getting-started/using-extensions), [ACP providers](https://goose-docs.ai/docs/guides/acp-providers)) | MCP extensions, recipes, built-in memory/chat-recall/todo tools, custom distributions, remote server, provider adapters, and open governance. ([custom distributions](https://github.com/aaif-goose/goose/blob/main/CUSTOM_DISTROS.md), [governance](https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md)) |
| **Continue** | Final product is a terminal CLI/TUI plus VS Code and JetBrains extensions; no maintained desktop/browser client was identified. The final README recommends the CLI over JetBrains. ([final README](https://github.com/continuedev/continue/blob/main/README.md), [CLI overview](https://docs.continue.dev/cli/overview)) | **Provider-neutral by design:** Anthropic, OpenAI, Gemini, Ollama, Bedrock, Azure, xAI, and other local/remote providers configured in YAML. The capability is real, but the project is no longer actively maintained. ([models](https://docs.continue.dev/customize/models), [final README](https://github.com/continuedev/continue/blob/main/README.md)) | MCP client supports stdio, SSE, and Streamable HTTP. MCP is available in agent mode. No maintained first-party ACP server/client was identified in the reviewed official material. ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)) | Shared `config.yaml`, models, context providers, rules, prompts, MCP blocks, agent files, and CLI flags. Extensibility is configuration-first rather than a broad plugin/runtime marketplace. ([CLI configuration](https://docs.continue.dev/cli/configuration), [config reference](https://docs.continue.dev/reference)) |
| **Roo Code** | Primarily a VS Code extension, not a desktop/browser product. It also executed commands in the VS Code terminal. A short-lived `roo` CLI existed by 3.48.0, including stdin mode and native Linux ARM64 artifacts, but the official final identity and shutdown notices center on the extension/cloud/router products. ([FAQ](https://docs.roocode.com/faq), [3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0), [sunset notice](https://docs.roocode.com/sunset)) | **Historically model-agnostic:** dozens of providers, BYOK, local models, and mode-specific/sticky model selection. Provider breadth mattered, but the implementation is now frozen. ([official docs](https://docs.roocode.com/), [privacy policy](https://github.com/RooCodeInc/Roo-Code/blob/main/PRIVACY.md)) | MCP client with stdio, Streamable HTTP, and SSE. The built-in Puppeteer browser tool was removed in 3.48.0 in favor of Playwright MCP or another MCP browser. ([MCP](https://docs.roocode.com/features/mcp/overview), [3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0)) | Custom modes with tool-group/file restrictions, custom commands, skills, provider profiles, orchestrator/subtasks, and a marketplace for modes and MCP servers. The marketplace and cloud are no longer active products. ([modes](https://docs.roocode.com/basic-usage/using-modes), [marketplace](https://docs.roocode.com/features/marketplace)) |

### Security disclosure and boundary-material comparison

Security-policy quality is not the same as runtime isolation. None of these policies should be read as proof that arbitrary local agent code is contained.

| Project | Primary security material | What the material actually establishes |
|---|---|---|
| **Gemini CLI** | Google vulnerability intake through `g.co/vulnz`; the repository SECURITY file promises a response within five working days. ([SECURITY.md](https://github.com/google-gemini/gemini-cli/blob/main/SECURITY.md)) | A disclosure channel, not a complete threat model. Runtime boundaries are documented separately through trusted folders, policy engine, approval modes, and sandbox backends. |
| **Cline** | Bugcrowd disclosure program; only the most recent minor release is actively patched, with older versions patched at discretion. ([SECURITY.md](https://github.com/cline/cline/blob/main/SECURITY.md)) | A disclosure and patch-support statement. It does not prove that the CLI, desktop, remote host, or auto-approval modes are sandboxed. |
| **Goose** | Detailed warning that Goose can run code and act on untrusted content; recommends a dedicated VM/container, human confirmation for significant actions, reviewed MCP extensions, smaller isolated tasks, and independent review of generated code/tests. ([SECURITY.md](https://github.com/aaif-goose/goose/blob/main/SECURITY.md)) | The clearest primary-source warning among the five about developer-agent risk. It explicitly acknowledges prompt injection and does not claim local approval alone is a sandbox. |
| **Continue** | Private reports by email to `security@continue.dev`. ([SECURITY.md](https://github.com/continuedev/continue/blob/main/SECURITY.md)) | A disclosure route only. The final repository is read-only, so future patch expectations are unclear. |
| **Roo Code** | Reports by `security@roocode.com`; the archived policy says only the latest minor is actively patched and stated acknowledgement/fix targets. ([SECURITY.md](https://github.com/RooCodeInc/Roo-Code/blob/main/SECURITY.md)) | Historical disclosure policy only; the product and repository are shut down, so this is not a current security-maintenance signal. |

### High-level inferences

1. **Rover should remain a supervisor, not become another broad model-agent client.** Cline and Goose demonstrate demand for multi-client surfaces, but copying their chat, marketplace, connector, and cloud scope would dilute Rover's verification/evidence mission.
2. **Provider neutrality is a strategic requirement, not a commodity feature.** Cline, Goose, and Continue demonstrate broad model configuration; Gemini CLI demonstrates the trade-off of a tightly integrated first-party path. Rover's generic command/PTY boundary is more aligned with its mission, but it needs stronger versioned adapter contracts.
3. **MCP is table stakes; protocol governance is not.** All five support MCP clients. Goose and Cline add useful agent-facing protocol structure through ACP. Rover should adopt open interfaces, but only after authority, cancellation, provenance, and approval semantics are explicit.
4. **Persistence is most useful when portable and inspectable.** Goose's SQLite plus JSON/Markdown export/import, ClineCore's file manifests/messages, and Gemini's project-scoped sessions offer concrete patterns. Imported transcripts must remain claims/context, not verification evidence.
5. **The strongest reusable evaluation patterns are Gemini's flakiness lifecycle and Goose's container benchmark harness.** Cline's eval README also candidly exposes a useful anti-pattern: an evaluation program can exist while key smoke/E2E gates are disabled.
6. **Telemetry defaults are widely inconsistent.** Gemini OpenTelemetry is off by default but can log prompts and identifiers when enabled; Cline and Roo describe opt-out telemetry; Goose describes opt-in anonymous metrics; Continue's final release removed anonymous telemetry. Rover's no-telemetry default is a differentiator worth preserving.

### Gaps and source conflicts

- The exact requested researcher path contained an extra path separator and did not exist. The installed file was found and read at the synced skill's actual path before substantive research.
- GitHub reports Continue's `pushed_at` as 2026-09-22, while its final README says the repository is read-only and no longer actively maintained. The most conservative interpretation is “final/read-only, with recent repository metadata activity,” not “actively developed.”
- Goose's `llms.txt` documentation index says “MIT licensed,” but the canonical repository README, LICENSE metadata, and GitHub API all identify Apache-2.0. This report follows the repository license. ([Goose documentation index](https://goose-docs.ai/llms.txt), [repository LICENSE](https://github.com/aaif-goose/goose/blob/main/LICENSE), [repository API](https://api.github.com/repos/aaif-goose/goose))
- Roo had a CLI release workflow and CLI release-note section, but the final official README and shutdown notice do not present it as a current supported client. This report does not infer present CLI support.
- No source-level packet capture, egress audit, binary inspection, hostile-code test, or live-provider run was performed. Privacy statements are documented posture, not independently verified network behavior.

## Project evidence profiles

### Google Gemini CLI (`google-gemini/gemini-cli`)

#### Takeaway

Gemini CLI is the best direct reference for Rover's **terminal ergonomics, project-scoped context, policy/sandbox layering, and behavioral-evaluation lifecycle**. It is the least aligned reference for **provider neutrality** because its official model/authentication surface remains Google-centric.

#### Cited findings: interface, context, and persistence

- The repository calls the product terminal-first and documents interactive REPL, headless text, structured JSON, and streamed JSON modes ([README](https://github.com/google-gemini/gemini-cli/blob/main/README.md)).
- Session history is automatically recorded with prompts, model responses, tool inputs/outputs, token usage, and available reasoning summaries. Sessions are project-scoped under `~/.gemini/tmp/<project_hash>/chats/`, can be resumed by latest session/index/UUID, searched, deleted, checkpointed, rewound, or exported/imported ([session management](https://geminicli.com/docs/cli/session-management), [manage sessions and history](https://geminicli.com/docs/cli/tutorials/session-management/)).
- Official release notes state that chat-history retention defaults to 30 days and that deletion removes associated plans, task trackers, tool outputs, and activity logs ([release notes](https://geminicli.com/docs/changelogs)).
- `GEMINI.md`, Auto Memory, Agent Skills, worktrees, checkpoints, and rewind provide several distinct context mechanisms rather than one opaque memory store ([official docs](https://geminicli.com/docs/), [release notes](https://geminicli.com/docs/changelogs)).
- Git worktrees are explicitly recommended for parallel sessions, which is directly relevant to Rover's worktree model ([session management](https://geminicli.com/docs/cli/session-management)).

#### Cited findings: authority and sandboxing

- Gemini CLI has multiple approval modes, including plan/read-only behavior and a YOLO mode that bypasses confirmation ([CLI cheatsheet](https://geminicli.com/docs/cli/cli-reference), [configuration](https://geminicli.com/docs/cli/configuration)).
- Trusted folders, a policy engine, path/workspace checks, and extension environment-change consent provide defense in depth ([trusted folders](https://geminicli.com/docs/cli/trusted-folders), [policy engine](https://geminicli.com/docs/reference/policy-engine), [v0.60.0 release](https://github.com/google-gemini/gemini-cli/releases/tag/v0.60.0)).
- The documented sandbox layer supports multiple isolation strategies, including OS/container mechanisms such as macOS Seatbelt, Linux bubblewrap/seccomp, Docker/Podman, LXD, and gVisor. Dynamic expansion can grant a specific command broader access only after approval ([sandboxing](https://geminicli.com/docs/cli/sandbox)).
- Direct user shell mode has the same authority as commands run directly in the terminal, so sandboxing must be understood as a mode/tool boundary rather than an unconditional property of the whole process in every path ([command reference](https://geminicli.com/docs/reference/commands)).

#### Cited findings: protocols and extensibility

- MCP client support covers stdio, SSE, and Streamable HTTP, with connection state, timeouts, resources, schema validation, OAuth discovery, and trust-dependent confirmation ([MCP servers](https://geminicli.com/docs/tools/mcp-server/)).
- Extensions can bundle prompts, MCP servers, custom commands, themes, hooks, subagents, and skills; the v0.60.0 release added consent for environment changes and provenance metadata for untrusted tool outputs ([extensions](https://geminicli.com/docs/extensions/), [v0.60.0 release](https://github.com/google-gemini/gemini-cli/releases/tag/v0.60.0)).
- The docs expose A2A remote agents and ACP mode as additional interoperability paths, but these remain separate from Rover's current bounded support ([official docs](https://geminicli.com/docs/)).

#### Cited findings: testing and evaluation

- The repository has distinct unit/integration, E2E, performance, memory, and behavioral-evaluation surfaces, plus active CI, E2E, smoke, and eval workflows ([repository tree](https://github.com/google-gemini/gemini-cli), [workflow catalog](https://api.github.com/repos/google-gemini/gemini-cli/actions/workflows)).
- Behavioral evals explicitly distinguish deterministic correctness tests from model-choice behavior. New evals start as `USUALLY_PASSES`, run nightly across supported models, and are promoted to `ALWAYS_PASSES` only after sustained 100% consistency; the README describes multi-run nightly scoring and historical-baseline checks ([behavioral eval README](https://github.com/google-gemini/gemini-cli/blob/main/evals/README.md)).
- The same README documents a probabilistic PR regression policy and dynamic comparison with `main`. This is useful engineering infrastructure, but it is not equivalent to a deterministic acceptance proof ([behavioral eval README](https://github.com/google-gemini/gemini-cli/blob/main/evals/README.md)).

#### Cited findings: privacy and telemetry

- Gemini CLI's built-in OpenTelemetry is disabled by default and can target local files or an external/GCP collector. When enabled, common attributes include session ID, installation ID, approval mode, and authenticated user email; `logPrompts` defaults to true, and detailed traces are off unless explicitly enabled ([OpenTelemetry](https://geminicli.com/docs/cli/telemetry)).
- Separately, Google service usage statistics are governed by the applicable Google privacy notices and an official opt-out is documented ([terms and privacy](https://geminicli.com/docs/resources/tos-privacy)).
- The terms page warns that using third-party software to access underlying Gemini services may violate service terms. Provider policy is therefore part of the product boundary even when the CLI source is open ([terms and privacy](https://geminicli.com/docs/resources/tos-privacy)).

#### Rover implications

**Borrow**

- Project-scoped session roots, explicit retention, delete-on-exit, and a visible session browser.
- A layered authority model: trust workspace → policy mode → tool confirmation → optional OS/container isolation.
- Explicit expansion approval for a sandboxed command that needs temporary broader access.
- Behavioral-eval lifecycle states for probabilistic behavior, with multi-trial history and a strict promotion gate.
- Extension manifests that declare environment/tool effects and require consent before activation.

**Avoid or constrain**

- Do not copy provider lock-in; Rover's adapters must remain provider-neutral.
- Do not make prompt-bearing telemetry a convenience default. Rover's local observability should default to redaction/off and require an explicit destination.
- Do not adopt probabilistic model-behavior gates as substitutes for deterministic required checks. Behavioral evals belong beside Rover's assurance layer, not inside it.
- Do not allow an agent to self-promote its own eval without human review; Gemini's automation is a useful workflow, but Rover's policy and acceptance authority must remain explicit.

**Defer**

- A first-party inference loop, Google-authenticated hosted service, browser agent, A2A federation, and broad extension gallery should remain outside the near-term core.
- Full ACP/A2A support can be reconsidered only with explicit session identity, cancellation, provenance, and approval semantics.

#### Gaps

- The reviewed public security file is disclosure-focused and does not provide a complete project threat model.
- The docs and release history are moving quickly; details such as worktree, ACP, A2A, and sandbox behavior should be pinned to a tested release/commit before Rover adopts a protocol contract.
- Public documentation does not establish how all telemetry fields are redacted across every exporter and failure path.

### Cline (`cline/cline`)

#### Takeaway

Cline is the richest example of a **shared multi-client agent harness** and the most feature-dense project in this set. Its session file model, provider abstraction, plugins, and layered eval design are useful; its default CLI auto-approval, broad always-on product surface, and currently incomplete eval gates are poor defaults for Rover.

#### Cited findings: interface, providers, and sessions

- The current README describes CLI, native desktop, VS Code, JetBrains, SDK, headless CI use, and a shared agent engine ([README](https://github.com/cline/cline/blob/main/README.md)).
- Cline Desktop supports parallel sessions, schedules, provider/model selection, capability marketplace, SSH execution, and imports of conversations from supported agents such as Claude Code and Codex ([Desktop](https://docs.cline.bot/usage/cline-desktop)).
- Provider neutrality is explicit: direct providers, gateways, cloud platforms, local models, and arbitrary OpenAI-compatible APIs are supported ([README](https://github.com/cline/cline/blob/main/README.md), [provider configuration](https://docs.cline.bot/provider-config/other-30-plus-providers)).
- ClineCore stores session manifests and message JSON as files and exposes start/send/list/get/read/abort/stop/delete operations. It supports local, hub, and remote backends ([ClineCore](https://docs.cline.bot/sdk/clinecore)).
- Checkpoints use a shadow Git repository and commit project state after tool use, including files not tracked by the main repository. Checkpoints persist across editor sessions and support restoring code independently from conversation ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).

#### Cited findings: approvals and sandbox boundaries

- The IDE experience is human-in-the-loop: edits and commands require approval unless auto-approve is enabled ([README](https://github.com/cline/cline/blob/main/README.md)).
- CLI documentation states that the global auto-approval default is **true**, while the same page exposes `CLINE_SANDBOX` and a sandbox data directory. The reviewed page does not establish that sandbox mode is a complete hostile-code boundary ([CLI reference](https://docs.cline.bot/cli/cli-reference)).
- Cline's command safety classification is not a fixed deterministic allowlist; the model marks commands as safe or approval-required. YOLO mode disables safety checks and auto-approves tools, browser actions, MCP calls, and mode transitions ([auto-approve and YOLO](https://docs.cline.bot/features/auto-approve)).
- SDK permission policies can enable, disable, or require approval per tool, but tools without explicit policies default to enabled and auto-approved in the documented Agent API. This is an important fail-open default for embedders to override ([permission handling](https://docs.cline.bot/sdk/guides/permission-handling)).

#### Cited findings: extensibility and protocols

- MCP client transports include stdio, Streamable HTTP, and legacy SSE; tools can have per-server `autoApprove` lists ([MCP overview](https://docs.cline.bot/mcp/mcp-overview)).
- Cline can operate as an ACP agent for external editors and exposes a TypeScript SDK with custom tools, lifecycle hooks, event streams, scheduling, multi-agent teams, and hub/remote execution ([ACP](https://docs.cline.bot/usage/acp), [SDK](https://docs.cline.bot/sdk/overview)).
- Desktop exposes a unified inventory of tools, plugins, skills, rules, MCP servers, and hooks, plus a marketplace ([Desktop](https://docs.cline.bot/usage/cline-desktop)).

#### Cited findings: testing and evaluation

- The evaluation README describes contract tests, real-model smoke scenarios, and E2E tasks from a 12-task `cline-bench` corpus using Docker/Daytona/Harbor. Metrics include pass@k, pass^k, and flakiness ([evaluation README](https://github.com/cline/cline/blob/main/evals/README.md)).
- The same README candidly says smoke CI is partially disabled while being repointed at the new SDK CLI, the PR gate is contract tests only, and nightly E2E is not yet implemented. This is direct evidence that an eval directory alone does not guarantee meaningful continuous evaluation ([evaluation README](https://github.com/cline/cline/blob/main/evals/README.md)).
- Repository workflows include SDK, VS Code, JetBrains, desktop, and extension E2E test jobs, but this research did not inspect or claim passing runs ([workflow catalog](https://api.github.com/repos/cline/cline/actions/workflows)).

#### Cited findings: privacy and telemetry

- Cline's telemetry documentation says usage data excludes code, file contents/paths, command arguments, conversation content, personal information, and credentials, and can be disabled in settings ([telemetry](https://docs.cline.bot/more-info/telemetry)).
- Enterprise monitoring can change the boundary: optional prompt storage can back up conversation history to S3 or Cloudflare R2, and OpenTelemetry can export detailed events to customer systems ([telemetry](https://docs.cline.bot/more-info/telemetry)).
- Provider selection still determines where prompts and relevant code are sent. “Code never leaves your machine” is only accurate relative to Cline's own servers, not relative to the chosen model or MCP service.

#### Rover implications

**Borrow**

- Separate the agent loop from durable harness concerns: stateless `Agent` plus a `ClineCore`-like session/harness layer.
- File-backed session manifests and messages with list/read/delete APIs; bind each to exact provider/model/tool-policy identity.
- Per-tool policy objects and explicit capability inventories.
- A capability manifest for plugins/MCP that lists tools, lifecycle hooks, environment access, and approval implications.
- Separate PR contract gates from slower real-model smoke/E2E jobs, while never labeling disabled eval jobs as coverage.

**Avoid**

- Do not default unattended CLI execution to auto-approved. Rover's local mode must remain explicit and operator-authorized.
- Do not let unlisted SDK tools fail open to auto-approval; default unknown tools to disabled or ask.
- Do not treat model-classified “safe commands” as a hard security boundary.
- Do not treat shadow-Git checkpoints as isolation or immutable evidence. Rover's exact snapshots and retained diffs should remain authoritative.
- Do not copy connectors, team marketplace, scheduling, and remote-control breadth before Rover's core authority and assurance contracts are stable.

**Defer**

- Native desktop, messaging connectors, remote SSH execution, hosted hub, cross-agent import, and a marketplace should follow—not precede—a stable Rover session/adapter schema.
- Full ACP support is attractive, but should be a bounded adapter with no implicit grant creation, approval, signing, merge, or deployment authority.

#### Gaps

- The reviewed CLI reference exposes a sandbox flag but not a complete isolation contract, escape analysis, or hostile-code validation.
- The evaluation README's stated current gaps mean benchmark infrastructure should not be summarized as continuously enforced coverage.
- The public security file does not document the trust model for remote hubs, imported sessions, connectors, or scheduled agents.

### Block Goose / AAIF Goose (`block/goose` → `aaif-goose/goose`)

#### Takeaway

Goose is the strongest mission-aligned comparison for Rover's **open standards, provider neutrality, local-first operation, explicit governance, and evidence-oriented benchmark harness**. Its session portability and permission vocabulary are especially reusable. Its LLM-assisted read/write classification and optional local execution should remain explicitly outside Rover's hard security boundary.

#### Cited findings: identity, interface, and providers

- The old `block/goose` repository path resolves to `aaif-goose/goose`; current README describes desktop, CLI, and API products and states that Goose is part of the Agentic AI Foundation at the Linux Foundation ([old path API](https://api.github.com/repos/block/goose), [canonical README](https://github.com/aaif-goose/goose/blob/main/README.md)).
- Governance says the project was founded by Block, is now stewarded by AAIF, is an LF Project, uses Apache-2.0 for code/specifications, and prioritizes open, flexible, choice-oriented development ([governance](https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md)).
- The README claims 15+ model providers and 70+ MCP extensions; these are project-reported counts and can change ([README](https://github.com/aaif-goose/goose/blob/main/README.md)).
- Goose supports direct LLM providers and ACP agent providers. ACP providers can reuse subscriptions, but the docs note that session fork/resume is not yet supported for ACP providers and provider session IDs differ from Goose IDs ([ACP providers](https://goose-docs.ai/docs/guides/acp-providers)).

#### Cited findings: sessions, memory, and extensibility

- Sessions are stored in a local SQLite database. Desktop and CLI share the same store and can resume, search, duplicate, fork, delete, import, and export sessions ([session management](https://goose-docs.ai/docs/guides/sessions/session-management)).
- Export preserves complete session data as JSON, with Markdown export available for human-readable archival. Import creates a new session ID rather than overwriting an existing record ([session management](https://goose-docs.ai/docs/guides/sessions/session-management)).
- Built-in extensions include memory, chat recall, todo, developer, extension manager, and computer control; recipes package prompts, extensions, parameters, subrecipes, schedules, and success checks ([documentation index](https://goose-docs.ai/), [recipes](https://goose-docs.ai/docs/guides/recipes)).
- MCP is the main extension boundary, and custom distributions can preconfigure providers, extensions, branding, and behavior ([extensions](https://goose-docs.ai/docs/getting-started/using-extensions), [custom distributions](https://github.com/aaif-goose/goose/blob/main/CUSTOM_DISTROS.md)).

#### Cited findings: approvals and sandbox boundaries

- Goose exposes Completely Autonomous, Manual Approval, and Smart Approval modes. Smart mode auto-approves lower-risk actions and asks for others ([permission modes](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)).
- Tool permissions can be Always Allow, Ask Before, or Never Allow, providing a useful three-level vocabulary ([tool permissions](https://goose-docs.ai/docs/guides/managing-tools/tool-permissions)).
- The documentation says read/write classification is a best-effort interpretation by the LLM provider. Therefore it is a UX/risk-reduction layer, not a deterministic authorization proof ([permission modes](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)).
- The security policy explicitly recommends a dedicated VM/container for untrusted use and says Goose can follow commands embedded in untrusted content. It also requires human confirmation for significant actions and review of generated code/tests ([SECURITY.md](https://github.com/aaif-goose/goose/blob/main/SECURITY.md)).

#### Cited findings: testing and evaluation

- Goose's Harbor tooling runs terminal-bench-style tasks in containers, uploads a selected Goose binary, records per-task trial JSON, and compares models, harnesses, extensions, turns, cost, timeout, and pass rates ([Harbor eval README](https://github.com/aaif-goose/goose/blob/main/evals/harbor/README.md)).
- The documented full dataset has 89 tasks. The README publishes repository-generated run summaries; those results were not independently reproduced here ([Harbor eval README](https://github.com/aaif-goose/goose/blob/main/evals/harbor/README.md)).
- Active workflow definitions include CI, smoke tests, live-provider tests, Terminal-Bench, and OpenSSF Scorecard supply-chain security. Presence of a workflow is not a claim that this research observed it pass ([workflow catalog](https://api.github.com/repos/aaif-goose/goose/actions/workflows)).

#### Cited findings: privacy and telemetry

- Goose asks for permission on first use to collect anonymous usage data. Documented fields include OS/version/architecture, Goose version/install method, provider/model, extension/tool-name counts, session metrics, and error types; conversations, code, tool arguments, detailed error messages, and personal data are excluded ([usage data](https://goose-docs.ai/docs/guides/usage-data)).
- The same page warns that prompts, conversations, and accessed information may be sent to the selected provider and are subject to that provider's retention policy ([usage data](https://goose-docs.ai/docs/guides/usage-data)).
- Logs are documented as local-only; this does not cover model-provider or extension egress ([logging](https://goose-docs.ai/docs/guides/logs)).

#### Rover implications

**Borrow**

- The three-level tool vocabulary: `never`, `ask`, `allow`, with unknown tools defaulting to `ask` or `never`.
- A shared local session database plus versioned JSON export/import, with imports always assigned new identities.
- Human-readable Markdown export alongside exact machine JSON, so context can be reviewed without confusing it for evidence.
- Containerized benchmark runs with pinned agent binary, task version, model, extension set, trial count, timeout, and per-task artifacts.
- AAIF-style governance and explicit succession planning as a model if Rover's governance matures beyond a single repository owner.
- A global extension allowlist and reviewed extension provenance as prerequisites for future marketplaces.

**Avoid**

- Do not let an LLM decide whether an action is “safe” and treat that output as authorization.
- Do not use autonomous mode as a hidden default for local or remote execution.
- Do not make a large enabled-tool surface the default; Goose's own guidance recommends fewer tools for performance and safety.
- Do not collapse benchmark score, model claim, and acceptance into one decision. A benchmark can guide development but cannot verify a particular candidate.

**Defer**

- Broad desktop computer control, general automation recipes, custom branded distributions, and a public extension registry should wait until Rover has reviewed extension contracts and isolated execution.
- ACP interoperability should be adopted selectively; provider-side session mismatch is a concrete warning that a shared UI does not imply shared state.

#### Gaps

- Public material does not prove a default OS-level sandbox; the security policy recommends isolation instead.
- The benchmark README's published results are project artifacts, not independent validation.
- The current docs index contains a stale MIT license statement, demonstrating a need for Rover to treat repository license files and legal metadata as canonical over generated documentation.

### Continue (`continuedev/continue`)

#### Takeaway

Continue offers a clean, configuration-first terminal/IDE pattern and a useful three-level permission model, but its final/read-only status makes it a reference implementation rather than a strategic dependency. Its final removal of anonymous telemetry is notable.

#### Cited findings: final identity and interface

- The final repository README says `continuedev/continue` is no longer actively maintained and read-only. It identifies 2.0.0 as the final VS Code, CLI, and JetBrains release and says that release removed anonymous telemetry ([final README](https://github.com/continuedev/continue/blob/main/README.md)).
- Continue's official site says the project was acquired by Cursor and that the codebase remains available as a foundation for others ([official site](https://continue.dev/)).
- The CLI uses the same underlying agent as the IDE extensions, supports TUI and headless modes, and persists the last-used configuration ([CLI overview](https://docs.continue.dev/cli/overview), [configuration](https://docs.continue.dev/cli/configuration)).
- Provider neutrality is broad: official docs list Anthropic, OpenAI, Gemini, Ollama, Amazon Bedrock, Azure, xAI, and others, with local/offline configurations ([models](https://docs.continue.dev/customize/models)).

#### Cited findings: MCP, context, and sessions

- MCP client support includes stdio, SSE, and Streamable HTTP, with local and remote servers and secret interpolation ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)).
- The TUI supports `@` context references, compaction, resume, fork, rename, background jobs, model/config switching, and MCP management. `--resume` restores the full conversation history ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- Configuration is deliberately shared across clients through `config.yaml`, local `.continue` rules, model/context-provider blocks, and repeatable CLI flags ([configuration](https://docs.continue.dev/cli/configuration), [rules](https://docs.continue.dev/customize/rules)).
- The reviewed TUI documentation does not specify the on-disk session format, retention policy, cross-client import/export contract, or cryptographic provenance. Those remain gaps rather than inferred features.

#### Cited findings: approvals and sandboxing

- Read-only tools default to allow; edit/write and Bash default to ask; exclude removes a tool from model visibility. Persistent user decisions are stored in `~/.continue/permissions.yaml` ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- `--readonly` excludes write tools, while `--auto` allows everything. In headless mode, tools with `ask` permission are excluded, reducing unattended surprise ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- The reviewed official material did not identify a first-party OS/container sandbox. Tool permission is therefore a policy/UX boundary, not evidence of process isolation.

#### Cited findings: testing and evaluation

- The repository retains many historical test, E2E, CLI, and release workflow definitions ([workflow catalog](https://api.github.com/repos/continuedev/continue/actions/workflows)).
- The final repository's `eval/` directory contains only `.gitignore` at the reviewed head; no maintained task corpus or eval methodology was present there ([`eval/` tree](https://github.com/continuedev/continue/tree/main/eval)).
- Because the project is final/read-only, this is a useful caution: broad historical CI configuration should not be represented as a current evaluation program.

#### Cited findings: privacy and telemetry

- The final repository README explicitly says 2.0.0 removed anonymous telemetry ([final README](https://github.com/continuedev/continue/blob/main/README.md)).
- Older or partially stale docs still describe Continue accounts and login flows, so users should pin versions and verify behavior rather than assume every current documentation page describes the final binary consistently ([CLI quickstart](https://docs.continue.dev/cli/quickstart), [final README](https://github.com/continuedev/continue/blob/main/README.md)).
- Model providers still receive prompts and relevant repository context according to their own policies; removing Continue telemetry does not make local inference private by itself.

#### Rover implications

**Borrow**

- The simple `allow` / `ask` / `exclude` permission vocabulary and pattern-matched tool policies.
- Fail-safe headless behavior: if a tool requires interactive approval and no interactive approver exists, remove it from the model's tool set rather than hanging or auto-approving.
- One configuration model shared by terminal and IDE clients, with explicit per-run override precedence.
- `/compact`, `/resume`, and `/fork` as explicit user-visible context lifecycle operations.

**Avoid**

- Do not base current security or maintenance assumptions on a read-only repository's historical workflows.
- Do not mix current account-centric documentation with final local-build claims without version pinning.
- Do not treat permission rules as sandboxing; Rover should still require an actual isolation backend for untrusted code.

**Defer**

- Continue does not provide a reason for Rover to add another hosted account/gateway layer. Local provider configuration and Rover's existing agent adapters are better aligned.
- Maintenance of a fork should be considered only if a specific missing capability cannot be implemented behind Rover's own stable contracts.

#### Gaps

- No current official maintenance roadmap, patch policy, or post-acquisition development commitment was found in the reviewed primary material.
- Session persistence exists, but storage/retention/portability details were not specified on the reviewed TUI page.
- No formal maintained agent-evaluation corpus was found in the final `eval/` directory.

### Roo Code (`RooCodeInc/Roo-Code`)

#### Takeaway

Roo Code provides useful historical patterns for **mode-scoped tool permissions, model stickiness, task histories, and approval UX**, but its shutdown makes several patterns cautionary. In particular, default CLI auto-approval and cloud-centric expansion are incompatible with Rover's fail-closed assurance posture.

#### Cited findings: identity, activity, and interface

- The repository is archived, the official README says the extension was shut down on 2026-05-15, and the final GitHub release is `v3.54.0` from that date ([repository API](https://api.github.com/repos/RooCodeInc/Roo-Code), [README](https://github.com/RooCodeInc/Roo-Code/blob/main/README.md), [release API](https://api.github.com/repos/RooCodeInc/Roo-Code/releases/latest)).
- The official sunset notice says the extension, cloud, and router would shut down, the extension repository would be archived, and users were directed to ZooCode or Cline ([sunset notice](https://docs.roocode.com/sunset)).
- Roo was primarily a VS Code extension with file, terminal, web, and MCP access. It was not a browser-hosted core product ([FAQ](https://docs.roocode.com/faq)).
- A CLI existed by version 3.48.0: release notes document stdin mode, native Linux ARM64 artifacts, and auto-approval by default. The final supported-product status of that CLI is not clear in the shutdown documentation ([3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0)).

#### Cited findings: providers, modes, and context

- Roo was explicitly model-agnostic and supported numerous providers, BYOK, local models, and mode-specific “sticky” model selection ([official docs](https://docs.roocode.com/), [using modes](https://docs.roocode.com/basic-usage/using-modes)).
- Built-in and custom modes combined role instructions with tool groups and file restrictions. Architect mode was planning-oriented, Ask was read-only, and custom modes could constrain tools/files ([using modes](https://docs.roocode.com/basic-usage/using-modes), [custom modes](https://docs.roocode.com/features/custom-modes)).
- The History view supported nested subtask trees, and the former cloud offered task sync and sharing until shutdown ([3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0), [cloud overview](https://docs.roocode.com/roo-code-cloud/overview)).
- The 3.48.0 release fixed history/condensation-resume bugs, illustrating the difficulty of preserving context and state across nested tasks ([3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0)).

#### Cited findings: approvals, tools, and extensibility

- Roo asked for permission by default, with granular auto-approval for reads, writes, commands, browser actions, MCP, and mode transitions ([official docs](https://docs.roocode.com/), [auto-approving actions](https://docs.roocode.com/features/auto-approving-actions)).
- Workspace-boundary protection and protected-file rules reduce accidental writes, but no first-party hostile-code sandbox was identified in the reviewed security material ([auto-approving actions](https://docs.roocode.com/features/auto-approving-actions)).
- MCP support covered stdio, Streamable HTTP, and SSE with per-tool always-allow/disabled controls ([using MCP](https://docs.roocode.com/features/mcp/using-mcp-in-roo)).
- The built-in Puppeteer browser tool was removed in 3.48.0; users were directed to Playwright MCP or another MCP browser, reducing native browser surface but increasing dependence on extension supply chain ([3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0)).
- Extensibility included custom modes, commands, skills, provider profiles, subtasks/orchestration, and a marketplace ([official docs](https://docs.roocode.com/), [marketplace](https://docs.roocode.com/features/marketplace)).

#### Cited findings: testing and evaluation

- The archived repository's workflow catalog includes code QA, CodeQL, CI, mocked E2E, and a named `Evals` workflow ([workflow catalog](https://api.github.com/repos/RooCodeInc/Roo-Code/actions/workflows)).
- The workflow catalog alone does not expose enough current detail to characterize Roo's eval methodology or whether an eval gate was meaningful at shutdown. It should not be reported as an active or independently validated benchmark program.
- The final repository's active-product status makes future regression evidence impossible through normal upstream maintenance.

#### Cited findings: privacy and telemetry

- The repository privacy policy, last updated 2025-09-11, says PostHog telemetry is enabled by default and can be disabled. It includes VS Code machine ID, feature usage patterns, and exception reports, but not code or AI prompts ([PRIVACY.md](https://github.com/RooCodeInc/Roo-Code/blob/main/PRIVACY.md)).
- Relevant code and prompts are sent to the chosen model provider and subject to that provider's policies; API keys remain local except when sent to the selected provider ([PRIVACY.md](https://github.com/RooCodeInc/Roo-Code/blob/main/PRIVACY.md)).
- Cloud, router, and related hosted processing shut down with the product. Historical privacy statements should not be interpreted as a current service posture.

#### Rover implications

**Borrow**

- Mode definitions that bind role, tool groups, path restrictions, and model preference into one reviewable contract.
- Human-readable nested task/subtask histories, provided Rover binds each item to immutable task, attempt, snapshot, and evidence identities.
- Dual permission gates for risky MCP operations: global enable plus per-tool allow.
- Removing a broad built-in capability when its supply-chain or maintenance burden exceeds its value; browser automation can remain an explicitly reviewed external tool.

**Avoid**

- Do not default a noninteractive CLI to auto-approval.
- Do not use persona/mode instructions as the primary security boundary. Mode-based tool allowlists help, but the executor must enforce them independently of the model.
- Do not make cloud synchronization/sharing a prerequisite for useful local history.
- Do not depend on a shut-down repository, hosted router, or soon-to-retire extension ecosystem as a core dependency.

**Defer**

- Orchestrator personas, community mode marketplaces, cloud task sync, and a built-in browser controller are not aligned with Rover's current core.
- A community fork may be studied for behavior, but Rover should implement against standards and its own contracts rather than inherit an unmaintained product lineage.

#### Gaps

- The official material does not provide a complete local storage, export, retention, or deletion contract for the historical Roo CLI.
- The archived `Evals` workflow's actual methodology could not be established from the workflow listing alone.
- The privacy policy predates shutdown and cannot establish what happens to telemetry endpoints or retained cloud data now.

## Cross-project synthesis and recommendations

### Takeaway

Rover should **borrow the control-plane primitives**—project-scoped durable sessions, explicit tool-policy schemas, real sandbox adapters, portable context formats, capability manifests, and disciplined behavioral evaluation—while **avoiding product sprawl, fail-open approval defaults, model-generated authority, and telemetry that can silently carry prompts**. Desktop/browser clients, marketplaces, cloud collaboration, messaging connectors, and cross-agent session federation should be deliberately deferred until Rover's local authority and evidence model is stronger than any single-project comparison.

### What Rover should borrow

#### 1. A two-layer agent architecture: stateless loop plus durable harness

Cline's separation of a stateless `Agent` from `ClineCore` is a strong pattern ([ClineCore](https://docs.cline.bot/sdk/clinecore)). Rover can apply the same separation conceptually:

- A provider/agent adapter owns one run and emits bounded events.
- A durable controller owns task identity, session history, approvals, snapshots, retries, artifacts, and verification.
- The controller must not infer authority from provider text or model output.

This aligns with Rover's existing native profiles and transcript fixture tests while avoiding coupling persistence to one vendor protocol.

#### 2. Explicit, fail-closed tool policy schemas

Use Goose's `allow` / `ask` / `never` vocabulary and Continue's headless behavior:

- Unknown tools default to `ask` or `never`, never `allow`.
- Approval-required tools are removed from the model's tool set in noninteractive mode unless a preauthorized grant applies.
- Policy is attached to exact tool name, input matcher, workspace, audience, expiry, and revocation identity.
- Model-provided “safe command” labels remain advisory metadata and never override deterministic policy.

Sources: [Goose tool permissions](https://goose-docs.ai/docs/guides/managing-tools/tool-permissions), [Continue tool permissions](https://docs.continue.dev/cli/tool-permissions), [Cline permission handling](https://docs.cline.bot/sdk/guides/permission-handling).

#### 3. Portable session context with quarantined imports

Goose's JSON/Markdown export/import and Cline Desktop's cross-agent import are useful interaction patterns ([Goose sessions](https://goose-docs.ai/docs/guides/sessions/session-management), [Cline Desktop](https://docs.cline.bot/usage/cline-desktop)). Rover should adopt a narrower version:

- Export exact conversation/context as versioned JSON.
- Export a human-readable Markdown rendering separately.
- Import creates a new session and marks provenance as imported.
- Imported transcripts, memories, and tool outputs remain untrusted assertions.
- Imported content cannot satisfy verification, become evidence, create grants, or authorize writes without the normal review path.

#### 4. A real isolation contract, not “approval equals safety”

Gemini CLI demonstrates multiple sandbox backends and approved temporary expansion ([sandboxing](https://geminicli.com/docs/cli/sandbox)). Cline exposes a sandbox flag but does not document an equivalent complete contract in the reviewed CLI page ([CLI reference](https://docs.cline.bot/cli/cli-reference)). Rover should:

- Define a backend-neutral sandbox interface with explicit filesystem, network, process, credential, and device capabilities.
- Record the actual runtime/image digest and effective mounts in evidence.
- Refuse silently falling back to host execution.
- Distinguish “approval required” from “technically isolated.”
- Require a live functional test before advertising a backend.

This is consistent with Rover's current explicit refusal of a Docker fallback and its stated lack of hostile-code certification ([Rover security model](../../docs/SECURITY_MODEL.md)).

#### 5. Behavioral evaluation as a separate, probabilistic layer

Gemini's eval lifecycle is the best documented example:

- New behavior starts in a usually-passing/flaky class.
- Nightly multi-trial measurements track stability.
- Promotion requires sustained perfect performance.
- Historical baselines distinguish pre-existing failures from branch regressions.

Source: [Gemini behavioral eval README](https://github.com/google-gemini/gemini-cli/blob/main/evals/README.md).

For Rover, add two constraints:

- Behavioral evals never replace deterministic checks, source-state checks, counterfactuals, or required-result integrity.
- Promotion to a blocking eval requires human-reviewed policy; an agent may propose promotion but must not self-approve it.

#### 6. Containerized benchmark provenance

Goose's Harbor harness is a strong reference for comparing agent harnesses under pinned conditions ([Harbor eval README](https://github.com/aaif-goose/goose/blob/main/evals/harbor/README.md)). Rover benchmark records should include:

- Exact Rover commit/build digest.
- Exact external agent binary/version and adapter contract version.
- Task corpus version and task hash.
- Model/provider identifier without credentials.
- Tool/extension allowlist.
- Container image digest and isolation flags.
- Trial count, timeout, turn/output limits, and per-task artifacts.
- Base/candidate labels and independent result classification.
- Separate public development and held-out evaluation datasets.

#### 7. Extension capability manifests and consent

Gemini's v0.60.0 environment-change consent and Cline's installed-capability inventory are directly relevant ([Gemini v0.60.0](https://github.com/google-gemini/gemini-cli/releases/tag/v0.60.0), [Cline Desktop](https://docs.cline.bot/usage/cline-desktop)). A future Rover extension/plugin contract should declare:

- Tool names and schemas.
- Filesystem roots and write behavior.
- Network destinations and credential classes.
- Environment-variable reads/writes.
- Lifecycle hooks.
- Subprocess/container requirements.
- Session/context data accessed.
- Approval requirements and cancellation behavior.
- Extension identity/version/content digest.

Installation should preview these effects, and activation should be explicit. No marketplace should precede this contract.

#### 8. Privacy-preserving local observability

Goose's opt-in anonymous metrics and Gemini's local OpenTelemetry option provide complementary patterns ([Goose usage data](https://goose-docs.ai/docs/guides/usage-data), [Gemini OpenTelemetry](https://geminicli.com/docs/cli/telemetry)). Rover should consider an opt-in local event stream with:

- No prompt, command argument, file content, path, secret, or model-output body by default.
- Redaction and size bounds enforced before write.
- Explicit destination selection; no automatic upload.
- Clear separation between local debugging telemetry, Google/provider service statistics, and product analytics.
- A hard default-off posture consistent with Rover's current no-telemetry claim.

### What Rover should avoid

#### 1. Auto-approval defaults

Cline's CLI documentation says auto-approval defaults true, and Roo's 3.48 CLI release made auto-approval the default ([Cline CLI](https://docs.cline.bot/cli/cli-reference), [Roo 3.48.0](https://docs.roocode.com/update-notes/v3.48.0)). Rover should do the opposite: unattended execution requires an explicit operator action and a constrained authority grant.

#### 2. Model-generated safety classification as authorization

Cline and Goose both describe model/provider-assisted classification of command or tool risk ([Cline auto-approve](https://docs.cline.bot/features/auto-approve), [Goose permissions](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)). That is useful UX but must remain advisory. Rover's executor should independently evaluate path, command, tool, grant, and environment constraints.

#### 3. Broad client/connector sprawl before core assurance

Cline now spans IDE, desktop, CLI, SDK, messaging connectors, schedules, teams, remote hubs, and marketplaces ([README](https://github.com/cline/cline/blob/main/README.md)). Rover should not copy this feature count. Each new remote or messaging surface multiplies authentication, replay, approval, session, and external-write risks.

#### 4. Cloud state as a prerequisite

Roo Cloud and Cline remote/hub features demonstrate useful remote coordination but also create additional data and trust boundaries. Rover's current remote MCP/CLI control should remain narrowly scoped and local-first until cross-host fencing, tenant identity, and recovery are validated.

#### 5. Shadow checkpoints as security or evidence

Cline's shadow Git checkpoints are excellent rollback UX but can be expensive and are not immutable acceptance evidence ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)). Rover should retain exact captured inputs, diffs, structured results, hashes, and review events as the authoritative record.

#### 6. Self-modifying or self-certifying workflows

Rover should borrow installable extensions, skills, hooks, and policy proposals, but not allow agents to silently modify their own authority policy, required checks, test policy, or acceptance criteria. Gemini's automated eval maintenance and Cline/Gemini extension systems are reasons to add review gates, not to delegate them fully.

#### 7. Prompt-bearing telemetry conveniences

Gemini OpenTelemetry can include prompts, tool arguments, API text, user email, and identifiers when enabled ([OpenTelemetry](https://geminicli.com/docs/cli/telemetry)). That may be appropriate for a user-controlled debugging exporter, but it is a poor default for an assurance tool. Rover should default to metadata-only, local-only, redacted events and require an explicit privacy review for richer traces.

#### 8. Depending on ended or frozen projects as strategic infrastructure

Roo is shut down; Continue is final/read-only. Both remain valuable source references, but Rover should not add build-time or runtime dependencies on them without a fork/governance plan, license review, security maintenance owner, and pinned compatibility tests.

### What Rover should deliberately defer

1. **Native desktop, browser, and mobile clients.** Rover's full-platform mission permits later clients, but terminal-first should remain the design center. A future thin client should consume a stable, minimal protocol rather than duplicate the controller.
2. **Marketplace and community extension economy.** Defer until Rover has signed/verified extension identity, capability manifests, review workflows, revocation, and sandbox admission.
3. **Messaging connectors and autonomous schedules.** Defer until remote grants, replay protection, cancellation, quotas, and external-write reconciliation are stronger.
4. **Cross-agent conversation import as a headline feature.** A constrained import can be useful, but universal conversation migration should wait for a common provenance-aware exchange format.
5. **Full ACP/A2A federation.** Worth a compatibility spike after Rover's own task/session/evidence schema is explicit. ACP/A2A interoperability should not weaken cancellation, approval, or provenance semantics.
6. **Autonomous model routing and self-optimization.** Rover's current `learn` capability deliberately limits local, explicit, non-agent-removing behavior ([Rover README, lines 159–163](../../README.md)). Do not replace this with opaque routing or self-modification.
7. **First-party browser/computer control.** Prefer explicit, reviewed external tools or sandboxed extensions. Do not give the controller ambient desktop authority.
8. **Hosted multi-tenant execution and remote worker fleets.** These remain explicitly outside Rover's current validated scope ([Rover status, lines 11–18](../../STATUS.md)).

### Recommended sequence for Rover

#### P0 — strengthen the existing terminal-first control plane

1. **Versioned session manifest and portable export**
   - Add explicit session/adapter/model/tool-policy identities.
   - Add JSON export and human-readable Markdown rendering.
   - Keep imports quarantined as untrusted context.

2. **Fail-closed tool-policy schema**
   - Encode `allow`, `ask`, `never`, and `disabled` distinctly.
   - Add pattern-bound policies and unknown-tool defaults.
   - Make headless execution remove interactive-only tools unless explicitly granted.

3. **Behavioral-eval registry**
   - Separate deterministic checks from model-behavior evals.
   - Track nightly trials, flakiness, model identity, and historical baseline.
   - Require human approval before promotion to blocking status.

4. **Sandbox backend contract and live tests**
   - Record image/runtime digest, mounts, capabilities, and expansion approvals.
   - Add a second backend only after the first is functionally tested in an owned environment.
   - Never claim isolation from admission tests alone.

#### P1 — add standards without surrendering authority

1. **Extension/plugin capability manifest**
2. **ACP compatibility spike for external agent adapters**
3. **Opt-in local, metadata-only observability**
4. **Session quarantine/import for a limited set of known formats**

#### P2 — only after the above are stable

1. Thin desktop or browser control clients
2. Remote worker/control topology
3. Marketplace
4. Messaging connectors and schedules
5. Cross-agent session federation and richer workflow automation

### Source register

All URLs were retrieved on **2026-09-24**. The following are the primary anchors used for the comparison.

#### Gemini CLI

- Repository identity/stars/license/activity: [GitHub API](https://api.github.com/repos/google-gemini/gemini-cli)
- Latest release: [GitHub release API](https://api.github.com/repos/google-gemini/gemini-cli/releases/latest)
- Product/interface/providers/headless modes: [repository README](https://github.com/google-gemini/gemini-cli/blob/main/README.md)
- Official documentation index: [geminicli.com/docs](https://geminicli.com/docs/)
- Authentication/provider scope: [authentication](https://geminicli.com/docs/get-started/authentication)
- Session storage/retention/resume: [session management](https://geminicli.com/docs/cli/session-management)
- Sandbox boundaries: [sandboxing](https://geminicli.com/docs/cli/sandbox)
- MCP transports/auth/resources: [MCP servers](https://geminicli.com/docs/tools/mcp-server/)
- Extensions: [extensions](https://geminicli.com/docs/extensions/)
- Behavioral evals: [evals README](https://github.com/google-gemini/gemini-cli/blob/main/evals/README.md)
- Telemetry fields/defaults: [OpenTelemetry](https://geminicli.com/docs/cli/telemetry)
- Terms/privacy/opt-out: [terms and privacy](https://geminicli.com/docs/resources/tos-privacy)
- Security reporting: [SECURITY.md](https://github.com/google-gemini/gemini-cli/blob/main/SECURITY.md)

#### Cline

- Repository identity/stars/license/activity: [GitHub API](https://api.github.com/repos/cline/cline)
- Latest release endpoint: [GitHub release API](https://api.github.com/repos/cline/cline/releases/latest)
- Product/interface/providers/extensibility: [repository README](https://github.com/cline/cline/blob/main/README.md)
- CLI approval/sandbox flags: [CLI reference](https://docs.cline.bot/cli/cli-reference)
- Desktop sessions/import/schedules/SSH: [Cline Desktop](https://docs.cline.bot/usage/cline-desktop)
- Durable harness/session files: [ClineCore](https://docs.cline.bot/sdk/clinecore)
- MCP transports: [MCP overview](https://docs.cline.bot/mcp/mcp-overview)
- Auto-approve/YOLO boundary: [auto-approve](https://docs.cline.bot/features/auto-approve)
- Telemetry/privacy/enterprise exports: [telemetry](https://docs.cline.bot/more-info/telemetry)
- Evaluation methodology/current gaps: [evals README](https://github.com/cline/cline/blob/main/evals/README.md)
- Security reporting: [SECURITY.md](https://github.com/cline/cline/blob/main/SECURITY.md)

#### Goose

- Original identity redirect: [`block/goose` API](https://api.github.com/repos/block/goose)
- Canonical identity/stars/license/activity: [`aaif-goose/goose` API](https://api.github.com/repos/aaif-goose/goose)
- Latest release: [GitHub release API](https://api.github.com/repos/aaif-goose/goose/releases/latest)
- Product/providers/extensions: [repository README](https://github.com/aaif-goose/goose/blob/main/README.md)
- AAIF governance: [GOVERNANCE.md](https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md)
- Sessions/import/export/fork: [session management](https://goose-docs.ai/docs/guides/sessions/session-management)
- Permission modes: [permission modes](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)
- Per-tool permissions: [tool permissions](https://goose-docs.ai/docs/guides/managing-tools/tool-permissions)
- ACP providers/session limits: [ACP providers](https://goose-docs.ai/docs/guides/acp-providers)
- Telemetry/provider privacy: [usage data](https://goose-docs.ai/docs/guides/usage-data)
- Terminal benchmark harness: [Harbor eval README](https://github.com/aaif-goose/goose/blob/main/evals/harbor/README.md)
- Developer-agent threat warning: [SECURITY.md](https://github.com/aaif-goose/goose/blob/main/SECURITY.md)

#### Continue

- Repository identity/stars/license/activity: [GitHub API](https://api.github.com/repos/continuedev/continue)
- Final release: [GitHub release API](https://api.github.com/repos/continuedev/continue/releases/latest)
- Final/read-only status and telemetry removal: [repository README](https://github.com/continuedev/continue/blob/main/README.md)
- Acquisition notice: [official site](https://continue.dev/)
- CLI/TUI/session behavior: [TUI mode](https://docs.continue.dev/cli/tui-mode)
- Provider scope: [models](https://docs.continue.dev/customize/models)
- Permission policy: [tool permissions](https://docs.continue.dev/cli/tool-permissions)
- MCP transports: [MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)
- Final eval directory: [GitHub tree](https://github.com/continuedev/continue/tree/main/eval)
- Security reporting: [SECURITY.md](https://github.com/continuedev/continue/blob/main/SECURITY.md)

#### Roo Code

- Archived identity/stars/license/activity: [GitHub API](https://api.github.com/repos/RooCodeInc/Roo-Code)
- Final release: [GitHub release API](https://api.github.com/repos/RooCodeInc/Roo-Code/releases/latest)
- Shutdown notice in repository: [repository README](https://github.com/RooCodeInc/Roo-Code/blob/main/README.md)
- Product sunset details: [official sunset notice](https://docs.roocode.com/sunset)
- Historical CLI/default auto-approval/browser removal: [3.48.0 release notes](https://docs.roocode.com/update-notes/v3.48.0)
- Mode/tool boundaries: [using modes](https://docs.roocode.com/basic-usage/using-modes)
- Approval controls: [auto-approving actions](https://docs.roocode.com/features/auto-approving-actions)
- MCP transports: [MCP overview](https://docs.roocode.com/features/mcp/overview)
- Telemetry/provider privacy: [PRIVACY.md](https://github.com/RooCodeInc/Roo-Code/blob/main/PRIVACY.md)
- Historical workflow inventory: [GitHub workflow API](https://api.github.com/repos/RooCodeInc/Roo-Code/actions/workflows)
- Security reporting: [SECURITY.md](https://github.com/RooCodeInc/Roo-Code/blob/main/SECURITY.md)

### Final gaps and limitations

- Approximate stars will drift after the stated retrieval date.
- Product documentation can lag source code, especially during active 2026 development. Release-pinned source review is required before implementation decisions.
- “Supports sandbox” does not mean “securely contains hostile code.” Gemini's documented multi-backend approach is the strongest implementation reference, but no reviewed source constituted a security certification.
- The evaluation sections describe repository-defined tests and benchmarks. No live model, hosted CI, provider billing, benchmark rerun, or security test was performed for this report.
- Privacy claims were taken from official documentation and policy files; they were not verified through packet capture, source-level egress tracing, deletion tests, or provider audits.
- The comparison intentionally avoids endorsing a model, provider, hosted control plane, or product lifecycle. Rover's differentiator should remain local observability, explicit authority, independent verification, review, and evidence.
