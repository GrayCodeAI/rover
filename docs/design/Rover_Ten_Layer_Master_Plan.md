# Rover — Ten-Layer Master Plan and Use Cases

**Revision:** design draft 1.0, prepared 15 September 2026.  
**Status:** proposed architecture; no claim that the features or package-manager commands already ship.  
**Research basis:** primary documentation and selected research sections listed in the source register. This is not an exhaustive repository audit, hands-on competitor benchmark, or security certification.

## Product and scope

Rover is a terminal-first, agent-agnostic platform where humans and coding agents plan, execute, coordinate, inspect, verify, review, and deliver software. It preserves Herdr-style persistent runtime, Luvus-style orchestration, and Orca-inspired review continuity while adding independent evidence and controlled learning.

The ten layers are logical responsibility boundaries, not ten microservices and not an execution sequence. Security and evidence apply throughout. Go, SQLite, and the proposed interfaces are design choices, not confirmed existing implementation facts.

Coverage means that each major responsibility has an owner, an integration contract, a delivery stage, and a testable acceptance condition. It does not mean every language, agent, platform, defect, edge case, or verification technique is already supported. Unsupported, unchecked, inconclusive, and out-of-scope remain explicit results.

## System topology

```text
Human / external coding-agent CLI
                 |
          L1 CLI / TUI / API
                 |
        Rover control application
  L5 orchestration | L6 context | L7 authority
      L8 check coordination | L9 evidence
          /                         \
 development worker           verification worker
 L2 agent adapter             L8 verifier adapter
 L3 session runtime           approved CheckSpec
 L4 mutable workspace         L4 fixed source snapshot
          |                         |
       candidate ---------------> observations
                                    |
                        L9 controller-owned evidence
                                    |
                   authorized review / integration / CI publisher
                                    |
                          confirmed outcome records
                                    |
                        L10 evaluated improvements
```

A worker process name is not a sandbox. The selected executor and credentials must enforce the documented boundary. The publisher is separately authorized in protected operation; local same-user mode cannot claim equivalent tamper resistance.

## The ten layers

### L1. Developer experience and distribution

**Purpose:** Turn a task into a usable human/agent workflow without forcing a new IDE.

Own the CLI, small keyboard-first TUI, command discovery, stable JSON responses, progress subscriptions, notifications, approval inbox, session attachment, and editor/browser handoff. Provide a plain-text/non-color mode and screen-reader-friendly alternatives. Make read operations available without changing the project.

A future installer must verify a pinned release against an approved trust identity, prefer user-local installation, and avoid automatic administrator elevation. `init` proposes configuration from existing repository tooling; it does not execute install scripts or overwrite AGENTS.md/CLAUDE.md. Instruction-file changes require a preview, approval, and undo record. The Rover binary can be lightweight while adapters still require their external tools.

Desktop, web, and mobile are later clients of the same domain API, not separate products. Package managers and operating systems become supported only after release tests; no installer command in this plan claims an existing public package.

**Contract:** Inputs: authenticated user/agent requests. Outputs: validated commands and presentations; never an independently computed acceptance decision.

**Delivery:** First release: CLI, task/session/evidence TUI, JSON, diagnostic and setup preview. Later: MCP interface and other clients.

**Acceptance:** A user can start a task, detach, reconnect, inspect failures, and approve the exact displayed candidate. JSON and TUI represent the same authoritative state.

### L2. Agent and tool interoperability

**Purpose:** Connect different producers without pretending their capabilities are identical.

Maintain three integration levels: code-only verification, terminal-backed execution, and structured integration. Start with one structured adapter, then add a second before freezing the interface. Claude Code and Codex are proposed first targets; a generic executable backend is deliberately less capable. Installed-agent presence is not evidence of tested compatibility.

Each adapter declares launch, attach, cancel, native resume, structured events, permission mediation, usage reporting, and supported platform/version combinations. Maintain contract tests for malformed/duplicated events, rate limits, permission requests, disconnects, and unknown versions. Keep subscription authentication, API-key authentication, and provider policy separate. Never route around account restrictions or rate limits.

Use native interfaces where useful; ACP requires capability negotiation and MCP exposes tools rather than mandatory enforcement [S04-S08]. External framework agents can later use the same task API. Extensions run out of process with a versioned schema, pinned artifact identity, explicit permissions, and installation approval. Do not silently install executable plugins on a model's request.

**Contract:** Inputs: versioned AgentRunSpec/ToolRequest. Outputs: normalized events and declared capabilities. Native details remain available for debugging.

**Delivery:** First release: one structured adapter and documented fallback. Next: second structured adapter plus conformance suite. Later: ACP/MCP and reviewed extensions.

**Acceptance:** An unsupported safety capability prevents admission to the mode requiring it. Switching agents preserves the task, but does not falsely promise portable conversations.

### L3. Persistent execution runtime

**Purpose:** Keep work observable and recoverable under precisely documented failure conditions.

Own sessions, PTYs or headless processes, process-tree cancellation, output backpressure, liveness reporting, native-session references, and reconciliation. Reuse a mature session backend before considering a terminal emulator. Define platform-specific behavior; a compiled Windows binary is not proof of terminal parity.

Distinguish UI detach, network disconnection, controller restart, supervisor restart, and host loss. Supported sessions may survive client detach while their supervisor remains alive. Conversation resume after a process dies is not continuation of arbitrary external side effects. Herdr documents these distinctions explicitly [S01].

Use task-wide budgets for attempts, elapsed work, subprocesses, logs, disk use, and reported/estimated spending. Pause admission, request cancellation, and show processes that remain alive; never imply cancellation reverses prior external operations. Apply bounded queues and log retention. Sanitize untrusted terminal/report output in noninteractive views. Reconcile uncertain outcomes before retries.

**Contract:** Inputs: authorized execution requests. Outputs: process/session observations, artifacts, and resource accounting. Agent completion does not mean task acceptance.

**Delivery:** First release: detach/reattach, cancellation, backpressure, restart reconciliation, explicit lost state. Later: remote session migration only where validated.

**Acceptance:** Killing the TUI does not kill a supported session. Killing the supervisor or host does not leave a false RUNNING state or trigger a blind duplicate execution.

### L4. Workspaces, snapshots, and environments

**Purpose:** Separate mutable development from the exact software being verified.

Own worktrees/branches, source materialization, immutable candidate identities, environment manifests, dependency setup, service instances, port reservations, database namespaces, quotas, and cleanup. Git worktrees are separate working trees, not an operating-system sandbox [S09]. Setup hooks and dependency installation are code execution and need the same authorization as other commands.

Define Workspace (mutable editing), CandidateSnapshot (fixed source identity), and Environment (tools/dependencies/services/capabilities). Pause/cooperate with writers or validate a consistent snapshot rather than blindly copying a moving tree. Specify staged, unstaged, included untracked files, symlinks, submodules, LFS objects, generated sources, and unsupported cases. Verification uses a frozen input materialized in disposable check workspaces; check-generated outputs are separate.

For multi-repository work, capture a repository-to-revision vector plus compatible build inputs. Do not claim atomic commits/merges across providers. Candidate edits, changed lockfiles, changed tool images, and changed policy may invalidate earlier evidence. Cache reuse requires declared equivalence conditions, not merely matching source paths.

**Contract:** Inputs: repository references and approved EnvironmentSpec. Outputs: workspace leases, exact snapshots, environment identities, and explicit completeness limitations.

**Delivery:** First release: single-repository worktrees/snapshots and disposable restricted check workspaces. Later: multi-repo manifests, remote services, VM/cloud backends.

**Acceptance:** An agent editing during verification cannot change the checked candidate. Two tasks do not accidentally share ports or write to the same test database.

### L5. Task orchestration and automation

**Purpose:** Coordinate useful work while keeping completion, acceptance, and delivery separate.

Own approved TaskContracts, plan revisions, dependency DAGs, scheduling, fair admission, resource reservations, bounded repair loops, structured handoffs, and integration sequencing. Plans may be manually authored or model-proposed; admission validates permissions, references, budgets, dependency cycles, and acceptance requirements. A model cannot rewrite its own task contract without approval.

Support single-agent work first, then multiple workers and optional reviewers. Independent tasks can run concurrently; a dependent task names the exact predecessor artifact/integrated revision it needs. Reservations coordinate intended work; they do not enforce arbitrary filesystem access. Luvus documents this distinction [S02].

Handoffs reference a candidate, requirement IDs, findings, open questions, and relevant context. Bound autonomous retries, detect repeated nonprogress, and escalate. Racing agents is optional and budgeted; measure its benefit against a single-agent baseline. Scheduled/webhook work has trigger identities, timezone/missed-run rules, deduplication, and cancellation. Multi-agent research motivates explicit failure/termination contracts, not an assumption that more agents improve quality [S20].

**Contract:** Inputs: approved contracts, resources, events. Outputs: authorized run/check requests and delivery requests. Policy authority stays in L7 and interpretation in L8/L9.

**Delivery:** First release: one coherent task loop. Next: parallel DAG, reservations, structured handoffs, integration order. Later: recurring triggers, races, routing experiments.

**Acceptance:** Dependents cannot consume an unspecified or stale predecessor result. Repeated failure stops at the approved budget and cannot reset by launching a new session.

### L6. Context, knowledge, and memory

**Purpose:** Supply relevant information without leaking access or promoting stale memory to truth.

Own permission-aware repository search, requirements, selected files, issue/document references, architecture/service maps, skill references, and context bundles. Start with explicit search and versioned references; add semantic retrieval only after evaluating it. Symbol/call/service graphs should distinguish observed edges, declared edges, and inferred edges.

Each ContextBundle records its sources, versions, repository scope, authorized recipients, relevant task/candidate, retention, and retrieval method. Human requirements, agent assertions, model summaries, and tool output have distinct provenance. Protect task instructions from untrusted documents, issue comments, browser content, and tool output; permission enforcement must not rely on a model following delimiters.

Memory can be episodic (past work), procedural (approved recipes), or architectural (versioned facts). It is not fresh evidence that a new candidate is correct. Add stale/conflicting-memory indicators, deletion/export, and explicit consent for remote models or cross-project learning. Connectors must not enlarge the requesting agent's access. Do not duplicate OAuth/token/retry infrastructure unnecessarily.

**Contract:** Inputs: task scope and authenticated source grants. Outputs: bounded, attributable context and retrieval results, never authoritative policy updates.

**Delivery:** First release: requirements, repository search, selected docs, prior investigation references. Later: service maps, external connectors, approved skills, optional semantic retrieval.

**Acceptance:** An agent permitted on project A cannot retrieve project B through search, summaries, traces, or shared memory; stale knowledge is visibly versioned.

### L7. Identity, permissions, and security

**Purpose:** Determine and enforce who may act on which resources under which trust assumptions.

Own human/service/agent identities, resource ownership, grants, approval rules, credential brokerage, network/data-egress controls, trusted policy revisions, extension authorization, and revocation. Separate local advisory, controlled execution, and protected acceptance modes. Local same-user execution cannot provide tamper resistance against that user or a fully compromised host.

In protected mode, approved checks/policy and verifier publication authority remain outside candidate control. Candidate workers receive no merge credentials or evidence-signing keys. Restricted workers use allowlisted mounts, scoped credentials, controlled egress, explicit resource limits, and no host container socket or unrestricted controller API. Container configuration matters [S10-S11]. Command allowlists alone cannot constrain general-purpose interpreters, package scripts, or compilers.

Support RBAC/attribute-based policy as needed through one decision interface; start with explicit rules, not a mandatory policy-language platform. Approval binds to action parameters, candidate, contract/policy revision, and expiry. An approved exception remains an exception. Extension installation and permission to access a project are separate decisions. Automatic redaction is best-effort, not a boundary [S08,S18].

**Contract:** Inputs: principal, action, resource, context, approved policy. Outputs: permit/deny/approval-required with enforceable grants and audit references.

**Delivery:** First release: explicit trust modes, scoped local API, restricted verifier execution, protected policy/publisher. Later: organization administration, federation, fine-grained delegation.

**Acceptance:** A candidate cannot weaken mandatory checks, access publishing credentials, or turn unapproved provider access into an authorized operation.

### L8. Verification and interrogation

**Purpose:** Test relevant properties and challenge claims without overstating the result.

Own check selection from approved policy, CheckSpec/CheckRun execution contracts, structured result interpretation, claim-to-evidence applicability, test integrity, targeted counterexamples, and evidence-gap questions. Integrate native tests/linters/builds, SARIF analyzers, coverage, dependency/secret checks, contracts, browser/accessibility tests, property tests, fuzzing, mutation, differential/counterfactual checks, benchmarks, failures, and applicable formal methods. Do not run every family on every change.

Every check declares the property, relevant source, environment, expected outputs/test identities, parser version, scope, exclusions, timeout, and allowed cache behavior. Candidate-authored tests and protected acceptance tests have different trust provenance. A generic exit-zero command only establishes successful process exit; it does not automatically establish correct tests, sufficient coverage, or security.

Regression counterfactuals must reproduce the intended failure on base and pass on candidate; import/setup failure is inconclusive. Mutation survivors need triage, and base behavior is not an oracle when the requirement intentionally changes it. Lean proofs retain statement/axiom/implementation assumptions; Kani checks retain harness/feature/bound/resource limitations [S14-S17]. Model suggestions can add questions, not fabricate evidence or acceptance.

**Contract:** Inputs: candidate, approved plan, requirements/claims. Outputs: scoped check results, artifacts, findings, gaps, and replayable failure cases.

**Delivery:** First release: command and structured test results, required-check completeness, policy-change detection. Next: counterfactual and selected analyzer packs. Later: domain-specific deep checks.

**Acceptance:** No collected required tests, malformed reports, wrong snapshots, disabled checks, or missing tools cannot become a green required check.

### L9. Evidence, review, and delivery

**Purpose:** Preserve what happened and make acceptance and external delivery accountable.

Own the evidence ledger, artifact references, claim/evidence links, reports, diff review, approvals, operation journal, CI publication, merge-candidate checks, and confirmed outcome links. Maintain separate execution status, check outcome, claim assessment, acceptance decision, and external delivery outcome. No universal correctness score.

Each evidence item records candidate/base/contract/plan/policy identities, tool/environment identities, producer trust class, all relevant attempts, output digests, and interpretation limits. Hashes identify content; signatures identify attestations under a trust policy, not absolute correctness. Candidate-supplied logs cannot impersonate controller-origin evidence. Bind comments and approvals to snapshots.

Use an authorized publisher outside candidate execution. Reverify the actual integration candidate and respect the provider's required checks and expected publisher. GitHub can accept skipped/neutral checks under some configurations, so a missing Rover investigation must not be published as a successful neutral shortcut [S12]. Preserve ambiguous external outcomes and reconcile before retrying. Release/deployment are explicit actions; destructive data migrations may require forward recovery, not automatic rollback. Report evidence expiry honestly.

**Contract:** Inputs: observations, artifacts, policy decisions, authorized review/delivery requests. Outputs: scoped decisions, review records, protected checks, and operation/outcome history.

**Delivery:** First release: local evidence and review plus a protected CI result path. Later: broader providers, browser trace review, attestations, release/deployment adapters.

**Acceptance:** An approval cannot transfer silently to changed code; a timeout after successful PR creation does not generate a duplicate PR.

### L10. Evaluation, learning, and controlled improvement

**Purpose:** Use observed outcomes to improve strategies without allowing self-certification.

Own privacy-controlled experience datasets, benchmark versions, baseline experiments, confirmed failure cases, statistics, optional learned scheduling, and improvement proposals. Separate product-outcome labels from weak signals: a merge is not correctness, no reported incident is not proof of no defect, and a nearby incident is not confirmed causal attribution.

Begin with retrieval and statistics. Evaluate contextual bandits or reinforcement learning only for an explicit objective such as optional-check ordering or bounded resource allocation. Mandatory checks, policies, interpretation rules, publishing keys, holdouts, and promotion rules remain independently controlled. Retecs and Darwin Godel Machine are relevant research references, not guarantees of improvement in Rover [S21-S22].

Adversarial mutation/self-play runs only in owned isolated fixtures. Keep real defects and synthetic examples separately labeled. Split data by repository and time to reduce leakage, account for nondeterministic outcomes and costs, and compare to simple fixed schedules. Promotion requires separate evaluation, security review where applicable, shadow use, explicit approval, and rollback. Repeatedly tuning against the same holdout must be tracked to avoid quietly turning it into training data.

**Contract:** Inputs: consented observations and confirmed labels. Outputs: versioned recommendations/proposals, never direct acceptance or unapproved production mutations.

**Delivery:** First release: outcome schema and a baseline evaluation harness. Later: empirical recommendations, then optional learning experiments and gated self-improvement.

**Acceptance:** A learned proposal cannot drop mandatory checks or alter evaluation criteria. No improvement is promoted solely because its author or own judge says it is better.

## Shared contracts and authoritative identities

Do not build one engine per noun. Use versioned typed records and a small number of domain services.

| Record | Responsibility |
|---|---|
| Project | Repository/resource namespace and access scope |
| TaskContract | Approved objective, requirements, scope, capabilities, budgets, checks and approval rules |
| Task | Requested work; owns contract revisions and attempts |
| Run | One admitted attempt under a specific contract revision |
| AgentSession | Provider/runtime conversation reference and declared capabilities |
| Workspace | Mutable development location and resource reservations |
| CandidateSnapshot | Exact source identity, including multi-repo revision vectors when supported |
| ContextBundle | Versioned, permission-filtered inputs supplied to a worker |
| CheckSpec | Approved property/check definition, executable inputs, parser and scope |
| CheckRun | A concrete execution attempt against a fixed candidate/environment |
| Evidence | Provenance-bearing observation/artifact with trust classification and limits |
| Claim | An assertion or requirement-derived hypothesis, with an explicit origin |
| Finding | Interpretation grounded in identified evidence; heuristic findings labeled |
| Decision | Policy evaluation bound to candidate/contract/check-plan/policy |
| Approval | An authorized decision for identified action parameters, scope and expiry |
| Operation | External side-effect intent, dispatch, confirmation and reconciliation state |
| Outcome | Confirmed or explicitly provisional post-delivery observation |
| FailureCase | Requirement, candidate, inputs, environment, failure signature, artifacts and replay conditions |
| ImprovementProposal | A candidate strategy change plus separately controlled evaluation and approval |

A baseline policy revision must come from an approved authority/reference, not automatically from the current candidate. Candidate changes to check scripts, rules, CI, evaluation data, or acceptance criteria are changes requiring their own review.

### Result semantics

Keep these fields separate rather than inventing one ambiguous DONE/PASS value.

| Dimension | Proposed values / meaning |
|---|---|
| Execution | QUEUED, RUNNING, WAITING, COMPLETED, CANCELLED, LOST |
| Check outcome | PASS, FAIL, ERROR, INCONCLUSIVE, NOT_APPLICABLE |
| Claim assessment | SUPPORTED_WITHIN_SCOPE, PARTIAL, CONTRADICTED, UNKNOWN, OUT_OF_SCOPE |
| Acceptance | PENDING, ACCEPTED, BLOCKED, REVIEW_REQUIRED, INCONCLUSIVE |
| External action | PLANNED, DISPATCHED, CONFIRMED, OUTCOME_UNKNOWN, RECONCILING |

Record exceptions separately with approver/reason/expiry. An exception can permit an action under explicit policy, but must not rewrite the underlying check outcome. NOT_APPLICABLE requires a documented applicability decision. All required obligations must be accounted for before acceptance. The display should say “accepted under policy P for candidate S,” not “software correct.”

### TaskContract example — illustrative schema, not an implemented configuration

```yaml
schema_version: rover.task/v1alpha1
id: refresh-token-fix
objective: Prevent a refresh token from being successfully reused.
requirements:
  - id: AUTH-1
    text: Reusing an already consumed refresh token must be rejected.
    source: approved-user-requirement
scope:
  project: example-api
  intended_paths: [src/auth/**, tests/auth/**]
  protected_inputs: [.github/**, .rove/**]
policy_ref: approved-policy/auth-change@3
execution:
  mode: controlled
  agent: codex
  network_profile: approved-build-and-provider-access
limits:
  max_attempts: 3
  max_parallel_agents: 2
acceptance:
  required_checks: [unit-auth, contract-auth, regression-auth]
  human_review: required
delivery:
  create_pull_request: requires_approval
  merge: requires_approval
  deploy: denied
```

The numerical limits are example design inputs, not measured optimal defaults. Intended paths are coordination/diff constraints unless the executor actually enforces them. Policy_ref is meaningful only after authenticated resolution to an approved revision. Unknown fields and unresolved references should be rejected, not ignored.

## Command and integration contract

All commands here are proposed interfaces, not claims that a public Rover package exists.

```text
rover doctor --json
rover init --plan
rover init --apply
rover agent list --json
rover agent capabilities <adapter> --json
rover task run <contract-file>
rover status --json
rover attach <session-id>
rover cancel <run-id>
rover inspect --base <approved-ref>
rover verify --candidate <candidate-id> --json
rover findings <investigation-id> --json
rover review <candidate-id>
rover report <investigation-id> --format json
rover export <task-id>
```

Commands require authenticated/authorized context. `init --apply` is explicit authorization for the displayed setup changes, not arbitrary execution of repository hooks. Sensitive actions require a separate approved action. A missing --base may use an explicit configured default; never assume every repository uses a branch named main.

MCP tools later call the same application use cases. Hooks/instruction files are workflow assistance, not proof that an agent cannot bypass verification. Structured failures should carry schema version, stable code, context identifiers, retryability and next action. Do not put secrets in error details.

## Deployment and storage design

Use one repository and shared domain libraries. Initial roles are CLI/TUI client, controller, development worker, check worker, and an authorized CI/result publisher. Several roles may share a binary with subcommands, but privilege separation comes from the runtime/OS/CI, not the command name.

Suggested source layout:

```text
cmd/rover/
internal/domain/       # identities, records, invariants
internal/app/          # application workflows / command handling
internal/interfaces/   # CLI, TUI, JSON, optional MCP
internal/control/      # tasks, policy, scheduler, context
internal/execution/    # sessions, workspace, executors
internal/assurance/    # plans, results, evidence, review/delivery
internal/adapters/     # agents, Git, tools, connectors, publishers
internal/storage/      # SQLite, artifacts, migrations, outbox
schemas/
docs/adr/
examples/
testdata/adversarial/
```

Do not create placeholder implementations for every future integration. Depend on interfaces only at real substitution or trust boundaries.

Proposed implementation choices: Go core, a small Bubble Tea-style TUI, system Git, a mature terminal/session backend, controller-owned SQLite, filesystem artifact objects, strict YAML/JSON schemas, and optional OpenTelemetry export. Select actual library versions after a dependency/platform/license spike. Do not claim that choosing Go makes all target binaries equally supported or every build completely self-contained.

SQLite WAL has one writer and same-host constraints [S13]. Keep remote workers on an authenticated API; do not share the database over NFS. Use transactions for state transitions plus an outbox for externally dispatched work. A standalone CLI invocation either uses the running controller or acquires exclusive local state ownership through the same application service; it is not a second independent decision engine.

Store blobs through bounded temporary writes, digest validation and atomic publication. Apply per-task quotas and garbage collection that respects active references and retention. Backups must include consistent metadata and required artifacts. Evidence deletion/expiry removes reproducibility capability; reports must say so. Never copy active WAL database files naively and assume a valid backup.

## Recovery, distributed behavior, and delivery

Persist operation intent before dispatch. Include stable operation IDs and provider idempotency keys where supported. Lost responses go to reconciliation; no universal exactly-once promise for arbitrary APIs. Workers receive scoped leases and ownership generations; controller-mediated writes and result publication reject stale generations. Revocation must also constrain credentials, not only an in-memory scheduler record.

Use sequence numbers and cursor-based event replay with deduplication. Keep audit-relevant events durable; high-volume terminal streaming can be bounded/lossy with explicit truncation. Controller time is not a universal ordering oracle across machines. Schedules retain an explicit timezone and missed-run policy.

No task-to-task communication may expand a worker's original permissions. Multi-repository releases use an explicit revision vector, rollout order and compensation/recovery plan. A deploy, migration, or public message is a separate authorized side effect; cancelling the agent does not undo it. Production chaos tests are opt-in, scoped, approved operations with abort conditions, never a default deep-verification mode.

## Verification capability catalog

These are adapter families and candidates to evaluate, not audited/shipped support claims for every named tool. Validate versions, platform support, licenses, output formats and security requirements before marking an adapter supported.

| Risk family | Technique / candidate integrations | Important limit |
|---|---|---|
| Compile/type/static correctness | Native compilers, type checkers, linters, reviewdog/SARIF ingestion | Warnings and exit codes need tool-specific interpretation |
| Functional behavior | Unit, integration, user-journey and contract tests | Tests can be incomplete or candidate-controlled |
| Regression adequacy | Intended failure on base; pass on candidate; changed-check review | Setup errors do not reproduce the intended defect |
| Edge inputs/state sequences | Hypothesis or other property-based tools; native/engine fuzzing | Only defined strategies, harnesses and campaign scope are checked |
| Test sensitivity | Mutation tools such as Stryker, PIT, mutmut or cargo-mutants | Surviving equivalent mutants and timeouts need classification |
| Compatibility | API/schema/ABI/event-contract comparison; consumer tests | Source/schema compatibility is not all runtime behavior |
| Differential/metamorphic behavior | Approved baselines and relations; generated corpora | Existing behavior or a generated relation can itself be wrong |
| Security and privacy | Scoped SAST/DAST, secret/dependency scanning, authorization tests, privacy checks | No findings is not a proof of security; active testing needs authorization |
| Supply chain | Lockfile/registry/version validation, SBOM, license rules, attestations | Missing network/private registry access is inconclusive, not proof of hallucination |
| Concurrency and memory | Race detectors, sanitizers, scheduling/stress tools, applicable model checking | Supported languages/models and observed executions limit the claim |
| Distributed reliability | Fault injection, deterministic simulation where available, TLA+/TLC models | Model-to-implementation correspondence is separate |
| Data and migrations | Upgrade/compatibility/integrity checks, recovery rehearsals, representative synthetic fixtures | Not every migration is reversibly rollable back |
| Performance/resources | Controlled baseline/candidate benchmarks, load, tail latency and memory tests | Noise, different machines, and small samples can make results inconclusive |
| Frontend/product access | Browser E2E, screenshots, visual diff, accessibility, localization, browser/device checks | Screenshots and automated accessibility checks are partial evidence |
| Infrastructure/release | IaC/manifests/config checks, build provenance, canary/SLO checks | No automatic production changes or destructive tests without authorization |
| Formal assurance | Lean, Kani, Verus, Dafny, CBMC, Frama-C, Rocq, Alloy, TLA+ as applicable | Preserve theorem/model, assumptions, bounds, exclusions and checker identity |
| ML/agent product behavior | Dataset validation, evals, prompt-injection tests, bias/safety/quality checks where relevant | Version prompts/models/data; stochastic evaluation is not deterministic proof |
| Maintainability/documentation | Structural change reports, dead-code/complexity tools, docs/examples validation | Style or complexity metrics do not establish a design is good |

Policy selects required obligations; a resource-aware scheduler orders them. Optional deeper checks can be proposed. A learning policy may not drop mandatory checks to improve latency. Four requested effort profiles (fast/standard/deep/critical) are budget/configuration profiles, not universal assurance rankings. A docs-only profile may be accepted without runtime tests only when explicit policy supplies its own checks and criteria.

## Use cases

| ID | Use case | Workflow | Layers | Expected outcome / boundary |
|---|---|---|---|---|

| U01 | Bring your own coding-agent CLI | User stays in a shell-capable agent, authorizes Rover installation/setup, edits normally, then invokes inspect/verify/report. | L1,L2,L7,L8,L9 | Scoped results for the exact captured candidate; no direct integration required for code-only verification. |

| U02 | Persistent personal development | Start a task, detach, reconnect later, read the attention queue, and continue or review the work. | L1,L3,L4,L5 | Session continues only under the documented supervisor/host conditions; lost work is not falsely shown as running. |

| U03 | Parallel feature implementation | Backend and frontend agents work in distinct workspaces; contract artifacts bind their handoff; integration is rechecked. | L2,L4,L5,L6,L8,L9 | Integrated feature candidate, with separate attempts and no implied atomic multi-repository merge. |

| U04 | Repair a failing regression | Agent creates a fix; Rover runs approved checks and attempts intended-failure-on-base/candidate-pass reproduction. | L4,L5,L8,L9 | Reproducible failure case and bounded repair loop; setup failure is inconclusive. |

| U05 | Frontend user journey | Agent edits UI; isolated browser tests exercise keyboard paths, assertions, screenshots, accessibility, and network failure handling. | L4,L6,L8,L9 | Reviewable behavior evidence; a screenshot or successful browser action alone is not acceptance. |

| U06 | Sensitive authentication change | Task targets tenant isolation; scoped credentials and protected tests validate configured authorization invariants. | L5,L7,L8,L9 | Security findings, required human decision, and explicit unchecked scenarios rather than a blanket secure label. |

| U07 | Dependency or supply-chain update | Agent proposes dependency changes; approved adapters validate resolution, lockfiles, registries, known issues, and license policy. | L2,L4,L7,L8,L9 | Version-specific evidence; an unavailable/private package is unresolved, not automatically a malicious hallucination. |

| U08 | Database migration | Use disposable databases and test upgrade compatibility, integrity, mixed-version operation, and the declared recovery plan. | L4,L5,L7,L8,L9 | Explicit destructive-change approval; no promise that every migration has a lossless rollback. |

| U09 | Remote heavy verification | Keep the UI local while a permitted remote worker runs broad tests/fuzzing under budgets. | L3,L4,L5,L7,L9 | Authenticated, snapshot-bound results; disconnects and stale worker generations handled explicitly. |

| U10 | Multi-repository service change | Track source revision vectors, API/event contracts, schema compatibility, order of rollout, and compensation steps. | L4,L5,L6,L8,L9 | A coordinated release manifest, not an unsupported distributed atomic-merge guarantee. |

| U11 | Untrusted external contribution | Read a PR without executing hooks; protected CI materializes its candidate in a restricted worker and uses approved checks. | L1,L4,L7,L8,L9 | Contributor cannot replace mandatory checks or impersonate the trusted publisher. |

| U12 | Formal or distributed invariant | Use an existing model/harness for Lean, Kani, TLA+ or another applicable tool and connect it to the requirement and implementation. | L6,L8,L9 | Scope, assumptions, bounds, excluded code and requirement/model correspondence remain visible. |

| U13 | Recurring maintenance | An approved schedule creates deduplicated tasks for selected dependency/review work, with missed-run and timezone behavior. | L3,L5,L7,L9 | Bounded unattended runs; no catch-up storm or automatic publication beyond approved policy. |

| U14 | Incident-to-regression learning | A confirmed incident is linked to a candidate and a replayable regression case; evaluate a changed verification strategy. | L6,L8,L9,L10 | Versioned failure corpus and independently evaluated proposal, not automatic causal blame or self-promotion. |

| U15 | Organization or private/offline operation | Use project-scoped identities and context; disable cloud destinations; execute only tools/models available in the approved environment. | L1,L2,L6,L7,L9 | No hidden code upload, permission leakage, or claim that an unavailable cloud-only agent works offline. |

| U16 | Human-only maintenance | A human writes a patch and uses Rover for workspaces, checks, evidence, and review without a coding-agent session. | L1,L4,L7,L8,L9 | The same acceptance semantics as agent-written code; AI is not required for core verification. |


## Delivery plan: complete workflows, not ten half-built services

Calendar estimates require capacity and integration spikes. Use these exit gates rather than fixed promises.

| Release gate | Deliverables | Exit condition |
|---|---|---|
| R0 — Design and feasibility | Threat model, TaskContract/snapshot/result schemas, fake agent/worker fixtures, platform/session/SQLite spikes, chosen dependency licenses | One normal run and one adversarial run can be represented end-to-end; key boundary tests are written |
| R1 — Secure vertical slice | CLI + small TUI, one real adapter, persistent session, workspace, exact candidate, approved checks, restricted check worker, evidence/review, protected CI path | U01/U02/U04/U11 pass within declared support; no silent policy weakening or fabricated green results |
| R2 — Parallel development | Second real adapter, compatibility tests, task DAG, resource reservations, bounded repairs, artifact handoffs, integration queue | U03 and concurrency/recovery tests pass; agent variety does not change acceptance semantics |
| R3 — Context and deep review | Permission-aware connectors, search/context bundles, browser artifacts, selected security/contract/counterfactual/property integrations | Relevant U05-U08 workflows add measurable value over the same native tools without Rover |
| R4 — Remote/team/automation | Worker authentication and grants, remote recovery, scheduling/webhooks, quotas, multi-repo manifests, organization policy | U09/U10/U13/U15 pass, including stale workers, missed triggers, permissions, and partial external success |
| R5 — Domain assurance packs | Formal/model/simulation/performance/ML adapters selected by real repository needs | Each pack has fixtures, declared scope, compatibility/support documentation and usable evidence |
| R6 — Evaluated learning | Consented outcomes, failure corpus, baseline experiments, shadow recommendations; optional bandit/RL experiments | Holdout and live shadow results justify the change without weakening mandatory checks or privacy |
| R7 — Stable OSS contract | Public schema/API commitments, migration/update/restore/export tests, security process and maintenance policy | Declared support matrix and release gates pass; unresolved limitations are public |

Multiple tracks can proceed only after shared schemas and trust boundaries exist. A browser/formal tool pack does not require waiting for RL; a secure publisher and check execution do not wait until the remote/team release. Desktop/mobile are additional clients justified by usage, not prerequisites for the terminal-first product.

## Definition of done and release tests

Every capability needs an owner, versioned contract, successful-path tests, failure-path tests, security/authorization tests where applicable, support documentation, and an evidence-backed demonstration. A placeholder, mock-only adapter, or unreachable UI is not a completed feature.

The following are proposed test specifications. They have not been executed against Rover code. The companion JSON file is an acceptance-test inventory, not an executable test suite.

| ID | Layers | Scenario | Required result |
|---|---|---|---|

| A01 | L1, L2 | Setup preview | Existing agent/configuration files are preserved until explicitly approved; no repository code executes during discovery. |

| A02 | L1, L7 | Installer trust | Wrong signature/identity, expired trusted metadata when required, or artifact digest mismatch fails installation without privilege escalation. |

| A03 | L2, L7 | Capability mismatch | An adapter missing required permission or cancellation semantics cannot silently enter a mode promising those capabilities. |

| A04 | L2 | Protocol robustness | Malformed, duplicated, and reordered events produce bounded errors or deduplicated updates, not duplicated runs or fabricated state. |

| A05 | L3 | Detach | Closing the supported client leaves supervisor-owned sessions running and attachable. |

| A06 | L3, L5 | Supervisor/host loss | Reconciliation marks lost execution accurately; no unsupported promise of process restoration. |

| A07 | L3, L7 | Cancellation | Cancellation reports pending/failed termination accurately and distinguishes it from external-action reversal. |

| A08 | L3, L5 | Task-wide budgets | Retries, new sessions, and provider swaps do not reset the approved task budget; usage remains observed/estimated/unknown. |

| A09 | L3, L9 | Output/disk pressure | Bounded buffers and quotas prevent controller memory exhaustion; required lost/truncated evidence prevents unsupported acceptance. |

| A10 | L4, L9 | Moving workspace | Concurrent edits cannot contaminate a fixed verification snapshot or receive its acceptance automatically. |

| A11 | L4 | Snapshot completeness | Untracked files, symlinks, submodules, missing LFS content, and unsupported source states are explicitly handled or rejected. |

| A12 | L4, L5 | Resource isolation | Parallel tasks do not accidentally share allocated ports, writable database namespaces, or browser profiles. |

| A13 | L5 | Dependency integrity | Cycles are rejected; dependent work cannot consume an unspecified or unapproved predecessor artifact. |

| A14 | L5, L7 | Contract changes | Agent edits to requirements, permissions, or required checks cannot silently alter the approved TaskContract. |

| A15 | L5, L9 | Duplicate trigger | A duplicated webhook or schedule occurrence does not create duplicate admitted work for the same trigger identity. |

| A16 | L6, L7 | Cross-project context | Unauthorized project content cannot leak through search, summaries, context caches, memory, or evidence views. |

| A17 | L6, L7 | Prompt injection | Untrusted issue/tool/browser content cannot authorize a tool, credential, policy change, or external transfer. |

| A18 | L7 | Worker credentials | A restricted candidate cannot read controller keys, publish authoritative evidence, or use merge/deploy credentials. |

| A19 | L7 | Sandbox fallback | If the required isolation backend is unavailable, execution is refused rather than downgraded to unrestricted local commands. |

| A20 | L7, L9 | Approval binding | Changing candidate, action parameters, contract, or relevant policy invalidates the corresponding approval as specified. |

| A21 | L8 | Missing tests | Zero expected tests, missing required tools, malformed reports, and required checks not run never count as successful verification. |

| A22 | L8 | False command success | A generic zero exit code is not labeled as tests/security/compatibility passed without the applicable structured interpretation. |

| A23 | L8, L9 | Wrong-candidate report | A cached or imported report for another snapshot/environment is not accepted for the current investigation. |

| A24 | L8 | Counterfactual validity | Import/build/setup failure on base does not count as reproducing the target defect. |

| A25 | L8 | Flaky retries | Failed attempts remain visible; acceptance follows an explicit approved retry/flakiness rule rather than cherry-picking a pass. |

| A26 | L8, L9 | Formal scope | A passed formal tool cannot label excluded code, unsupported concurrency, unchecked axioms, or unbound requirements as proved. |

| A27 | L8, L9 | No-op verification | An accidentally empty plan is inconclusive; explicit no-runtime-check profiles require their own approved acceptance conditions. |

| A28 | L9 | External uncertainty | A lost response after PR creation/push triggers reconciliation, not an automatic duplicate side effect. |

| A29 | L3, L7, L9 | Stale remote worker | An obsolete ownership generation cannot publish authoritative completion or perform controller-mediated writes. |

| A30 | L9 | Integration drift | Passing individual branches does not waive verification of the actual integration/merge candidate. |

| A31 | L7, L9 | CI publisher | An untrusted actor or skipped/neutral path cannot masquerade as the required successful Rover acceptance result. |

| A32 | L9 | Artifact retention | Expired/deleted evidence remains identified as unavailable; the UI does not claim reproducibility from absent inputs. |

| A33 | L1, L9 | Upgrade recovery | Interrupted migration/update has a tested recovery path; restoring an old executable alone is not assumed sufficient. |

| A34 | L10 | Learning authority | A proposed strategy cannot remove mandatory checks, alter truth semantics, access evaluator secrets, or promote itself. |

| A35 | L10 | Outcome validity | Merge/no-report/temporal-proximity signals are not silently converted into correctness or confirmed causal defect labels. |

| A36 | L7, L10 | Evaluation leakage | Training, tuning, and holdout access are tracked; private repository data does not enter shared learning without authorization. |

| A37 | L7, L9 | Untrusted artifacts | Path traversal, symlink escape, oversized reports, and malicious terminal escape sequences cannot compromise extraction/rendering. |

| A38 | L4, L9 | Cache trust | Candidate-controlled cache outputs cannot certify another build; reuse is denied when relevant mutable inputs are unaccounted for. |

| A39 | L7, L9 | Destructive delivery | Database, infrastructure, and deployment operations require scoped approval and explicit recovery/compensation behavior. |

| A40 | L1, L9 | Export and uninstall | User-owned code and Git history remain usable; export marks unsupported session portability and evidence omissions. |


## Rover's own engineering quality

Use unit tests for domain/policy/parsers; golden tests for stable reports; temporary Git repositories for integration; fake adapters/providers for faults; real compatibility fixtures for supported tools; property tests for scheduling/state invariants; protocol fuzzing and malicious artifact tests for input boundaries; and end-to-end restricted-worker tests. Security-critical changes require independent review. Run broad suites on suitable CI/lab resources rather than requiring every edit to saturate the developer laptop.

Test reliability separately from product value. Reliability gates include no duplicate confirmed external writes in the defined failure tests, no stale evidence acceptance, no candidate authority escalation in the tested threat model, and recoverable interrupted updates. These are targeted guarantees, not claims that every security flaw is eliminated.

For product evaluation compare Rover with the same agents, repositories, tools and task set without Rover. Record active human supervision, accepted useful findings, false positives, confirmed misses, repair attempts, cost estimates/observations, verification latency, resource use and lost/duplicated work. Use chronological and repository-separated evaluation where appropriate, preserve uncertainty, and do not optimize only for number of agents, generated lines or initial check-pass rate.

## Open-source and release operations

Select and publish an actual license and contribution policy before accepting outside code. A permissive license is a possible product choice, not a conclusion that all integrated code may be copied. Audit dependency and adapter licensing, distinguish process/service integration from code redistribution, and preserve required notices. Do not assume a directory's “open source” label establishes reuse rights.

Publish README, architecture/threat model, support matrix, CONTRIBUTING, SECURITY, GOVERNANCE, CODE_OF_CONDUCT, ROADMAP, CHANGELOG, privacy/retention behavior and versioned schemas. Provide vulnerability reporting, maintainer responsibilities, reviewed releases, checksums/signatures, SBOM/provenance where implemented, incident handling and a supported-version policy. Dependency installs and updates must be controlled operations.

Test signed update identity/freshness and recovery rather than relying on a checksum fetched beside the artifact [S19]. Updates need compatibility negotiation, session handling and a migration plan; old executable plus new database is not an assumed valid rollback. Export tasks/evidence with schema versions and leave user-owned Git work intact on uninstall.

Keep telemetry/content upload/learning export disabled until explicitly enabled. Artifact retention is configurable; logs can contain source, prompts and secrets [S01,S18]. Secret redaction is best effort. Publish a naming/domain/package availability review before public release; the user's selected name Rover is not evidence of exclusive brand availability.

## Proposed ADR set

ADR-001: Ten logical responsibility layers; modular control application rather than ten services.  
ADR-002: Terminal-first interface; external coding-agent use and Rover-managed use share the core.  
ADR-003: Explicit TaskContract revisions and protected acceptance obligations.  
ADR-004: Agent capabilities are negotiated/tested, never inferred from a product name.  
ADR-005: Session persistence, conversation resume and side-effect recovery are distinct.  
ADR-006: Workspace, exact snapshot and execution environment are distinct.  
ADR-007: Development/check execution and authoritative publication have separate authority.  
ADR-008: Execution, check, claim, acceptance and delivery states are distinct.  
ADR-009: Controller-owned metadata, transactional state/outbox and content-addressed artifacts.  
ADR-010: Same core locally and in CI; different environments/trust modes may yield different results.  
ADR-011: No silent unrestricted fallback or missing-check success.  
ADR-012: External verifier federation; scope/assumptions stay attached to results.  
ADR-013: Memory/context are permission-aware suggestions, not current proof.  
ADR-014: Privacy, retention, export, updates and recovery are first-class contracts.  
ADR-015: Learning proposes changes; separate evaluation controls promotion.  
ADR-016: Support is version/platform/capability-specific; no universal parity or capacity claims.

## Unresolved decisions to settle through implementation spikes

The exact session/PTY backend and initial supported OS matrix; SQLite driver/build portability; first structured adapter versions; container/backend threat model; package distribution names; API transport and authentication; initial verifier parsers; configuration precedence; retention defaults; dependency licenses; and measurable performance targets. These are deliberate decision gates, not hidden assumptions or promised capabilities.

## Source register

Sources were consulted on 15 September 2026. They support the observations stated in the notes below; the complete Rover architecture remains a design proposal. Pinned protocol/specification versions are references, not assertions that they are the newest version. No competitor performance or support-count claims are inferred from marketing.

| ID | Source | Design relevance |
|---|---|---|

| S01 | [Herdr: session state and restore](https://herdr.dev/docs/session-state/) | Persistence, server restart, and native conversation restoration are different guarantees. |

| S02 | [Luvus: orchestration](https://luvus.dev/docs/guides/orchestration/) | Dependencies, worktrees, path reservations, quality gates, integration, and the limits of leases. |

| S03 | [Orca: product documentation](https://www.onorca.dev/) | Worktree-scoped development, browser functionality, and diff feedback. |

| S04 | [Codex: App Server](https://developers.openai.com/codex/app-server/) | Structured agent interaction and approval requests; verify the installed protocol version. |

| S05 | [Claude Code: hooks](https://code.claude.com/docs/en/hooks) | Agent-specific lifecycle integrations, not universal control of arbitrary agent behavior. |

| S06 | [ACP: initialization](https://agentclientprotocol.com/protocol/v1/initialization) | Version/capability negotiation; omitted capabilities are unsupported. |

| S07 | [MCP: tools, 2025-11-25 specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) | Exposing a tool does not require the model to invoke it. |

| S08 | [MCP: security best practices](https://modelcontextprotocol.io/docs/2025-11-25/tutorials/security/security_best_practices) | Scoped authorization and risks of executing local tool servers. |

| S09 | [Git: worktree](https://git-scm.com/docs/git-worktree) | Separate working trees attached to a repository; security isolation needs other controls. |

| S10 | [SLSA 1.2: build requirements](https://slsa.dev/spec/v1.2/build-requirements) | Separate provenance/signing authority from user-controlled build steps. |

| S11 | [Docker: engine security](https://docs.docker.com/engine/security/) | Container boundaries depend on configuration, mounts, privileges, and daemon access. |

| S12 | [GitHub: protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) | Expected check publishers, up-to-date/integration checks, stale approvals, and bypass controls. |

| S13 | [SQLite: WAL](https://sqlite.org/wal.html) | Single-writer behavior and same-host constraints inform controller-owned storage. |

| S14 | [Lean: validating proofs](https://lean-lang.org/doc/reference/latest/ValidatingProofs/) | Proof validity, statement meaning, axioms, malicious build code, and independent checking. |

| S15 | [Kani: getting started](https://model-checking.github.io/kani/) | Rust model checking; supported features and resource limits must be explicit. |

| S16 | [Hypothesis documentation](https://hypothesis.readthedocs.io/en/latest/) | Property-based testing over defined strategies, not exhaustive coverage of arbitrary behavior. |

| S17 | [reviewdog repository](https://github.com/reviewdog/reviewdog) | Federate existing analyzers and normalized diagnostics instead of rebuilding them. |

| S18 | [OpenTelemetry: handling sensitive data](https://opentelemetry.io/docs/security/handling-sensitive-data/) | Minimize sensitive collection; do not treat redaction as complete protection. |

| S19 | [The Update Framework: security](https://theupdateframework.io/docs/security/) | Update trust, signed metadata, freshness, and compromise recovery. |

| S20 | [Why Do Multi-Agent LLM Systems Fail?](https://arxiv.org/html/2503.13657v1) | Selected research on specification, coordination, verification, and termination failures. |

| S21 | [Retecs: RL for test prioritization](https://arxiv.org/html/1811.04122) | A bounded application of learning to test selection/prioritization, not evidence of universal benefit. |

| S22 | [Darwin Godel Machine, v3](https://arxiv.org/html/2505.22954v3) | Empirically evaluated self-modification; not justification for unreviewed self-certification. |
