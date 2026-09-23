# Execution and infrastructure projects compared with Rover

**Research date:** 2026-09-24
**Scope:** E2B (`e2b-dev/E2B`), Daytona (`daytonaio/daytona`), Dagger (`dagger/dagger`), Temporal (`temporalio/temporal`), and Cosign (`sigstore/cosign`). Primary repositories and official project/Sigstore documentation were used.
**Source-date convention:** All undated official pages were accessed 2026-09-24. Repository activity dates below are the dates GitHub displayed for the newest commit on `main` when accessed 2026-09-24. Star values are approximate GitHub HTML values, not stable facts. GitHub's REST API returned 403 during this research, so the repository pages and commit pages were used instead.
**Rover baseline:** local docs read 2026-09-24: [README](../../README.md), [STATUS](../../STATUS.md), and [security model](../../docs/SECURITY_MODEL.md).

**Evidence labels used below:**

- **Observed** — directly visible in the linked primary repository, commit page, or official documentation.
- **Official claim** — stated by the project; not independently reproduced here.
- **Rover interpretation** — an engineering inference from the cited facts, not a project assertion.
- **Gap** — not established by the reviewed primary sources.

## 1. What are these projects, how active are they, and how do execution, scheduling, APIs, and identity work?

### Takeaway

These are five different layers, not five substitutes. **E2B** is an agent-oriented compute/sandbox service built around per-sandbox Firecracker microVMs. **Daytona** has a useful control/compute-plane design, but its public repository explicitly says core development moved to a private codebase in June 2026, so current hosted documentation must not be presented as the code in the archived public repository. **Dagger** is a programmable, container-backed CI engine with typed APIs, caching, modules, and OpenTelemetry; it is not a durable business-workflow service. **Temporal** is a durable workflow state machine whose Workers remain operator-controlled execution environments; it does not sandbox them. **Cosign** is a signing, attestation, transparency, and verification tool, not an execution scheduler.

### Cited Findings

#### Identity, stars, activity, and license

| Project | Verified identity and approximate stars | Activity observed by 2026-09-24 | License |
|---|---|---|---|
| **E2B** | [`e2b-dev/E2B`](https://github.com/e2b-dev/E2B), about **13.9k stars** on 2026-09-24. Its README identifies it as open-source infrastructure for running AI-generated code in cloud sandboxes, with JavaScript/TypeScript and Python SDKs. ([repository, accessed 2026-09-24](https://github.com/e2b-dev/E2B)) | The newest displayed `main` commit was 2026-09-18 and included SDK/API release work; recent commits also exercised auto-resume and added sandbox fork support. This is an active public client/SDK surface, not proof that every infrastructure component remains in this repository. ([commit history, accessed 2026-09-24](https://github.com/e2b-dev/E2B/commits/main/)) | [Apache-2.0](https://github.com/e2b-dev/E2B/blob/main/LICENSE) |
| **Daytona** | [`daytonaio/daytona`](https://github.com/daytonaio/daytona), about **71.7k stars** on 2026-09-24. The repository says its core development moved to a private codebase in June 2026; the public code will receive no further fixes or releases and is provided without support or warranty. ([repository notice, accessed 2026-09-24](https://github.com/daytonaio/daytona)) | The newest displayed `main` commit was 2026-06-25, a README license-link fix. The prior maintenance-notice commit was 2026-06-23; the activity signal is therefore **unmaintained public code**, despite a very large star count. ([commit history, accessed 2026-09-24](https://github.com/daytonaio/daytona/commits/main/)) | [GNU AGPL-3.0 at the final linked public tag `v0.190.0`](https://github.com/daytonaio/daytona/blob/v0.190.0/LICENSE) |
| **Dagger** | [`dagger/dagger`](https://github.com/dagger/dagger), about **16.3k stars** on 2026-09-24. This is the Dagger software-delivery automation engine, not Google's Java dependency-injection project. The repository describes build/test/ship automation locally, in CI, or in the cloud. ([repository, accessed 2026-09-24](https://github.com/dagger/dagger)) | The newest displayed `main` commit was 2026-09-23, with same-day work across modules, agent sessions, nesting, and UI/runtime behavior. The official docs are on a `1.0-beta` line, so interface volatility matters. ([commit history, accessed 2026-09-24](https://github.com/dagger/dagger/commits/main/), [official docs, accessed 2026-09-24](https://docs.dagger.io/)) | [Apache-2.0](https://github.com/dagger/dagger/blob/main/LICENSE) |
| **Temporal** | [`temporalio/temporal`](https://github.com/temporalio/temporal), about **23.3k stars** on 2026-09-24. The repository is the Temporal Server; Workflow, Activity, and Worker implementations live in SDKs. ([repository, accessed 2026-09-24](https://github.com/temporalio/temporal)) | The newest displayed `main` commit was 2026-09-21, including API, OpenTelemetry, replication, visibility, retry, and scheduling work. ([commit history, accessed 2026-09-24](https://github.com/temporalio/temporal/commits/main/)) | [MIT](https://github.com/temporalio/temporal/blob/main/LICENSE) |
| **Cosign** | [`sigstore/cosign`](https://github.com/sigstore/cosign), about **6.3k stars** on 2026-09-24. It is Sigstore's code-signing and transparency tool for OCI artifacts. ([repository, accessed 2026-09-24](https://github.com/sigstore/cosign)) | The newest displayed `main` commit was 2026-09-18, including OCI referrer, verification-key, bundle, privacy, and verification fixes. ([commit history, accessed 2026-09-24](https://github.com/sigstore/cosign/commits/main/)) | [Apache-2.0](https://github.com/sigstore/cosign/blob/main/LICENSE) |

#### E2B

**Execution and isolation — Official claim / Observed**

- E2B describes a Sandbox as a full Linux machine with its own kernel. Every managed sandbox is a separate Firecracker microVM; sandboxes do not share a kernel, filesystem, or memory. This is hypervisor-level isolation rather than a container or process boundary. ([security page, accessed 2026-09-24](https://e2b.dev/security))
- Managed E2B sandboxes run on Google Cloud, use Google Cloud encryption at rest by default, and encrypt traffic with TLS. E2B also offers Enterprise BYOC in a customer's AWS or Google Cloud account; BYOC keeps sandbox traffic, build sources, snapshots, and logs in the customer VPC but sends anonymized cluster CPU/memory metrics to E2B. ([security page, accessed 2026-09-24](https://e2b.dev/security))
- Sandboxes are destroyed on timeout or shutdown. E2B documents pause/resume of both filesystem and memory state, so lifecycle state can outlive an active VM; this is state preservation, not proof of arbitrary workflow replay. ([security page, accessed 2026-09-24](https://e2b.dev/security), [documentation index, accessed 2026-09-24](https://docs.e2b.dev/.md))

**Scheduling and recovery semantics — Observed / Gap**

- The public client supports sandbox lifecycle, template builds, shell/filesystem/process operations, desktop control, and pause/resume behavior. A build may run in the background and be polled by status/log offset. ([template build documentation, accessed 2026-09-24](https://e2b.dev/docs/template/build), [repository, accessed 2026-09-24](https://github.com/e2b-dev/E2B))
- The reviewed E2B sources did **not** establish a Temporal-style durable event log, workflow replay, exactly-once Activity contract, or Rover-style DAG supervisor. E2B's pause/resume and retries should therefore be treated as sandbox lifecycle semantics, not general durable orchestration. ([documentation index, accessed 2026-09-24](https://docs.e2b.dev/.md), [commit history, accessed 2026-09-24](https://github.com/e2b-dev/E2B/commits/main/))

**API and identity/authorization — Observed / Gap**

- The primary API is SDK-oriented: Python and JavaScript/TypeScript clients create/control sandboxes, and the documented setup uses an `E2B_API_KEY`. The repository also exposes SDK, CLI, and related packages; deployment documentation points to a separate infrastructure surface. ([repository, accessed 2026-09-24](https://github.com/e2b-dev/E2B), [documentation index, accessed 2026-09-24](https://docs.e2b.dev/.md))
- The reviewed public material establishes API-key use but did not establish Rover-style per-project, per-audience, per-tool grants, an independent approval identity, or offline token verification. Treat cloud account/API-key authority as a deployment trust boundary, not a fine-grained Rover grant model. ([repository, accessed 2026-09-24](https://github.com/e2b-dev/E2B))

**Extensibility and observability — Official claim / Gap**

- Templates can derive from standard images or arbitrary OCI images, add environment variables/files, and customize start behavior; Python and JavaScript SDKs expose a common sandbox model. ([template quickstart, accessed 2026-09-24](https://e2b.dev/docs/template/quickstart), [base-image documentation, accessed 2026-09-24](https://e2b.dev/docs/template/base-image))
- E2B's official product page describes lifecycle, networking, storage, and observability APIs, while its security page documents BYOC metrics. The reviewed pages did not specify a signed run record, content-addressed provenance, independent verifier identity, or retention/erasure contract suitable for Rover evidence. ([official product page, accessed 2026-09-24](https://e2b.dev/index.md), [security page, accessed 2026-09-24](https://e2b.dev/security))

#### Daytona

**Public-code versus current-product boundary — Observed**

- The public repository's maintenance notice is decisive: as of June 2026, core development moved to a private codebase, and the public repository will not receive updates, fixes, or releases. The final linked public release is `v0.190.0`; current official documentation identifies a later `v0.216` product. Current product capabilities must not be attributed to the archived OSS tag without source-level comparison. ([repository notice, accessed 2026-09-24](https://github.com/daytonaio/daytona), [current architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- The archived repository README still describes Daytona as a secure/elastic runtime with sandboxes, snapshots, REST/SDK/CLI access, and interface/control/compute planes. Those statements are useful as the final public project's stated design, but the maintenance notice controls current adoption conclusions. ([archived repository README, accessed 2026-09-24](https://github.com/daytonaio/daytona))

**Current isolation and execution model — Official claim, not verified against the private implementation**

- Current docs distinguish three isolation boundaries: runtime, network, and organization. Sandbox classes may be namespace/resource-isolated containers, full Linux/Windows VMs with their own kernels, or GPU-allocated containers; only the VM class supplies the hardware-virtualization boundary described in the docs. ([isolation documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/isolation))
- Current docs say runners poll for jobs and create/start/stop/destroy/resize/backup sandboxes. Each runner allocates dedicated vCPU, RAM, and disk; the sandbox daemon exposes filesystem, Git, process/code, log, terminal, and computer-use operations. ([architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- Network controls are per sandbox: block-all, CIDR/domain allowlists, outbound proxy, authenticated preview/SSH access, and explicit linking for child sandboxes. Organization boundaries are enforced through organizations, scoped API keys, secrets, and volume subpaths. ([isolation documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/isolation))

**Scheduling and recovery — Official claim**

- The current control plane schedules sandboxes onto runners and continuously reconciles state. PostgreSQL stores metadata/configuration, Redis supplies cache/session/distributed locks, snapshots are stored through S3-compatible object storage and an internal OCI registry, and volumes persist independently of sandbox lifecycle. ([architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- Current VM classes support pause/resume, fork, and hot snapshots; sandbox state is not equivalent to deterministic Workflow replay. No public source in scope establishes how a failed runner preserves an in-flight process, terminal state, or exactly-once side effect. ([isolation documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/isolation))

**API and identity/authorization — Official claim**

- The current interface plane offers Python, TypeScript, Ruby, Go, and Java SDKs, CLI, dashboard, MCP, and SSH. The control-plane API is a REST service; sandbox operations are also exposed through a Toolbox API. ([architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- Current docs describe Auth0/OIDC authentication, optional organization SSO, organization ownership of resources, and scoped API keys whose child permissions must be a subset of a manager key. Secrets can remain outside the sandbox as opaque placeholders while an outbound proxy injects a value only for allowed destinations. ([architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture), [isolation documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/isolation))
- An official 2026-04-09 advisory disclosed that API credentials had previously been passed into default sandboxes with passwordless sudo, allowing shell code to read them; Daytona reported stripping the authorization header at the proxy before it entered sandbox memory. This is direct evidence that broad sandbox root and control-plane credential placement are a serious boundary, not a convenience detail. ([official security advisory, dated 2026-04-09](https://www.daytona.io/dotfiles/updates/security-advisory-api-credential-exposure-in-sandboxes))

**Extensibility, observability, evidence, and operations — Official claim / Gap**

- Current docs expose snapshots, declarative builders, volumes, webhooks, MCP, agent tools, audit logs, and OpenTelemetry collection. These are broad integration and operations surfaces, but the reviewed material did not establish signed execution provenance or an independent acceptance authority. ([repository feature map, accessed 2026-09-24](https://github.com/daytonaio/daytona), [architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- The current product requires several stateful dependencies—API services, PostgreSQL, Redis, object storage, OCI snapshot storage, runners, identity, and analytics—and its PostHog integration is explicitly part of the API plane. This is materially different from embedding the final AGPL public tag as a simple local component. ([architecture documentation, accessed 2026-09-24](https://www.daytona.io/docs/en/architecture))
- **Rover interpretation:** Daytona is a design reference for reconciliation, resource classes, scoped keys, snapshots, and control/compute separation. It is not a current OSS dependency candidate because the requested repository no longer receives fixes or releases.

#### Dagger

**Execution model and API — Observed / Official claim**

- Dagger requires a Linux container runtime such as Docker. Its repository says tools run in containers, host dependencies are explicit/typed, and operations are incremental and content-addressed. This is reproducible container execution, not a VM-level hostile-code boundary. ([repository, accessed 2026-09-24](https://github.com/dagger/dagger), [installation documentation, accessed 2026-09-24](https://docs.dagger.io/getting-started/installation))
- The CLI, modules, checks, generators, services, and clients use one GraphQL API served by the Engine; generated SDKs expose typed, composable operations. The current docs list Dang plus Go, TypeScript, Python, Java, PHP, and Elixir module SDKs, while the repository advertises a broader generated-client set. ([API documentation, accessed 2026-09-24](https://docs.dagger.io/reference/api), [SDK/module documentation, accessed 2026-09-24](https://docs.dagger.io/reference/sdks), [repository, accessed 2026-09-24](https://github.com/dagger/dagger))
- Modules are reusable typed APIs for functions, objects, checks, services, and generators. Current module guidance explicitly says external effects belong in clearly named publish/export functions and file edits should return reviewable changesets rather than silently applying them. ([SDK/module documentation, accessed 2026-09-24](https://docs.dagger.io/reference/sdks))

**Scheduling and recovery semantics — Official claim / Rover interpretation**

- Dagger describes its execution model as a portable DAG and caches operations by content-addressed inputs. The Engine resolves the requested graph and reuses cached effects. ([official introduction, accessed 2026-09-24](https://docs.dagger.io/getting-started/introduction), [repository, accessed 2026-09-24](https://github.com/dagger/dagger))
- The reviewed official material did not establish a durable Event History, workflow replay, durable timer/message state, or exactly-once Activity semantics. Content-addressed caching reduces repeated work but is not proof that an interrupted run resumes at the same logical point. This distinction is important because Dagger's module documentation also uses the word “durable” for a named/versioned interface, not Temporal-style durable execution. ([SDK/module documentation, accessed 2026-09-24](https://docs.dagger.io/reference/sdks), [repository, accessed 2026-09-24](https://github.com/dagger/dagger))

**Identity, authorization, and secrets — Official claim / Gap**

- Dagger has a first-class `Secret` type. Current `1.0-beta` docs say values are redacted from logs, are not written into container layers, and are excluded from cache operations; providers include host environment variables/files/commands, Vault, 1Password, AWS Secrets Manager/Parameter Store, Google Secret Manager, and libsecret. ([secrets documentation, accessed 2026-09-24](https://docs.dagger.io/adopting/secrets))
- Secret provenance and redaction are not a user authorization model. The reviewed core docs establish Dagger Cloud login and secret/module scoping, but did not establish a Rover-equivalent per-project, per-audience, per-tool grant or an approval identity for destructive API operations. Treat remote Engine access and secret-provider credentials as high-authority deployment inputs. ([CLI documentation, accessed 2026-09-24](https://docs.dagger.io/reference/cli), [secrets documentation, accessed 2026-09-24](https://docs.dagger.io/adopting/secrets))

**Observability, evidence, and extensibility — Observed / Gap**

- The repository states that every operation emits an OpenTelemetry trace with granular logs and metrics; traces can be viewed in the TUI/web UI or exported to an OTel backend. ([repository, accessed 2026-09-24](https://github.com/dagger/dagger))
- Typed objects, content-addressed values, modules, generated SDKs, services, checks, generators, and changesets form a strong extension model. OTel spans and cache IDs are valuable execution evidence, but the reviewed sources did not establish signed provenance, independent verifier identity, or a policy that prevents a module from publishing externally. ([repository, accessed 2026-09-24](https://github.com/dagger/dagger), [SDK/module documentation, accessed 2026-09-24](https://docs.dagger.io/reference/sdks))

**Operational tradeoffs — Rover interpretation**

- Dagger replaces shell/YAML orchestration with a powerful engine, but adds an Engine/container runtime, cache storage, module dependencies, generated clients, and a large execution API. Host credential files, cloud credential chains, network access, cache correctness, and module side effects remain deployment concerns. Local reproducibility does not make an arbitrary module trusted. ([installation documentation, accessed 2026-09-24](https://docs.dagger.io/getting-started/installation), [secrets documentation, accessed 2026-09-24](https://docs.dagger.io/adopting/secrets), [repository, accessed 2026-09-24](https://github.com/dagger/dagger))

#### Temporal

**Execution and isolation model — Official claim**

- Temporal separates Client, Server, and Worker. The Client uses gRPC control calls; the Server persists state and schedules Tasks; Workers poll Task Queues and run Workflow and Activity code. The Server does not execute application code, and Worker deployment remains under the user's control. ([architecture walkthrough, accessed 2026-09-24](https://docs.temporal.io/encyclopedia/architecture/how-temporal-works.md))
- A Workflow replays Event History and issues Commands; an Activity performs external I/O and reports success/failure. Temporal's durability is a state-machine/replay protocol, not a container, VM, credential scrub, or tenant sandbox. ([architecture walkthrough, accessed 2026-09-24](https://docs.temporal.io/encyclopedia/architecture/how-temporal-works.md), [activity documentation, accessed 2026-09-24](https://docs.temporal.io/activity-definition.md))

**Scheduling and recovery semantics — Official claim**

- Event History is an append-only, durably persisted log used to recover after crashes. Workflow code is replayed to reconstruct logical state; recorded commands/events prevent completed Activities from being redone during ordinary replay. ([event-history documentation, accessed 2026-09-24](https://docs.temporal.io/workflow-execution/event.md))
- Retryable Activity failures and start-to-close timeouts schedule retries. Temporal guarantees that a retryable Activity is **observed as completed exactly once**, but explicitly warns that Activity code may execute multiple times, including partially completing more than once. Activities that write should be idempotent, and external services should enforce idempotency keys. ([activity documentation, accessed 2026-09-24](https://docs.temporal.io/activity-definition.md))
- Workflows have no fixed time limit, but Event History is bounded: the current docs warn after 10,240 Events and terminate above 51,200 Events, 2,000 Updates, or 10,000 Signals; Continue-As-New closes one run and begins another. Reset copies history only to a valid reset point. ([event-history documentation, accessed 2026-09-24](https://docs.temporal.io/workflow-execution/event.md))

**API and protocol design — Official claim**

- The public protocol is gRPC/Protobuf, with SDKs providing start, cancel, signal, query, result, Workflow, Activity, Worker, and Task Queue abstractions. The Server API defines event enums, and the architecture walkthrough shows the Frontend → History/Matching → Worker command/event flow. ([architecture walkthrough, accessed 2026-09-24](https://docs.temporal.io/encyclopedia/architecture/how-temporal-works.md), [event-history documentation, accessed 2026-09-24](https://docs.temporal.io/workflow-execution/event.md))
- The protocol is language-neutral and durable-state-centric, but Workflow code must remain deterministic. Side effects and I/O belong in Activities or recorded side-effect mechanisms; this is a substantial programming-model constraint compared with Rover's persisted supervisor commands. ([activity documentation, accessed 2026-09-24](https://docs.temporal.io/activity-definition.md), [event-history documentation, accessed 2026-09-24](https://docs.temporal.io/workflow-execution/event.md))

**Identity and authorization — Official claim / critical default**

- Temporal security is opt-in. It supports mTLS for internode/frontend traffic, JWT claim mapping, and pluggable `ClaimMapper`/`Authorizer` authorization. The current security page warns that the default `noopAuthorizer` allows every API request and is not safe when reachable by untrusted clients. ([security documentation, accessed 2026-09-24](https://docs.temporal.io/self-hosted-guide/security.md))
- The default JWT model maps namespace permissions into read/write/worker/admin roles. UI SSO and cross-cluster token providers are separate configurations; a secure deployment must pair TLS, claims, authorization, and fail-closed behavior explicitly. ([security documentation, accessed 2026-09-24](https://docs.temporal.io/self-hosted-guide/security.md))
- Temporal identity controls access to the workflow service; it does not automatically constrain what an Activity does inside a Worker or what external system credentials that Worker can reach. ([architecture walkthrough, accessed 2026-09-24](https://docs.temporal.io/encyclopedia/architecture/how-temporal-works.md), [activity documentation, accessed 2026-09-24](https://docs.temporal.io/activity-definition.md))

**Evidence, observability, extensibility, and operations — Official claim / Gap**

- Event History is also a useful debugging/audit log. Optional Principal Attribution can stamp an authenticated caller's identity into history when a custom Authorizer supplies it; the current docs label parts of this capability pre-release. This is stronger caller attribution than plain logs, but it is not a cryptographic signature over the candidate or proof that an external effect was correct. ([event-history documentation, accessed 2026-09-24](https://docs.temporal.io/workflow-execution/event.md))
- Temporal provides asynchronous Visibility/Search Attributes, a Web UI/CLI, separate SDK and Service Prometheus-compatible metrics, and OpenTelemetry context propagation. Visibility can be stale and is not the authoritative state of one Workflow. ([observability documentation, accessed 2026-09-24](https://docs.temporal.io/evaluate/features/observability.md))
- Self-hosting requires persistence and usually a separately operated Visibility store; closed histories remain queryable only for the Namespace retention period unless Archival is configured. Activity arguments and return values are recorded in history, so durability and observability also create sensitive-data retention obligations. ([observability documentation, accessed 2026-09-24](https://docs.temporal.io/evaluate/features/observability.md), [activity documentation, accessed 2026-09-24](https://docs.temporal.io/activity-definition.md))
- Extension points include Workflow/Activity code, Signals/Updates/Queries, SDKs, custom data converters/payload codecs, OTel interceptors, ClaimMapper, Authorizer, and TokenProvider plugins. These are substantial contracts with versioning, storage, security, and operability costs. ([security documentation, accessed 2026-09-24](https://docs.temporal.io/self-hosted-guide/security.md), [observability documentation, accessed 2026-09-24](https://docs.temporal.io/evaluate/features/observability.md))

#### Cosign

**Execution model and scheduling — Observed / Gap**

- Cosign is a CLI/Go client that signs and verifies OCI artifacts, attaches attestations, manages Sigstore bundles/trusted roots, and interacts with registries. It has no Worker scheduler, sandbox lifecycle, retry ledger, or durable Workflow runtime. ([repository CLI, accessed 2026-09-24](https://github.com/sigstore/cosign/blob/main/doc/cosign.md), [repository, accessed 2026-09-24](https://github.com/sigstore/cosign))
- Signatures/attestations and transparency entries are stored outside Cosign in OCI registries and Sigstore services. Offline bundle verification is possible when the artifact and required trust material are already local; ordinary online verification may need registry, TUF, Fulcio/Rekor, or timestamp services. ([repository, accessed 2026-09-24](https://github.com/sigstore/cosign), [keyless-signing documentation, accessed 2026-09-24](https://docs.sigstore.dev/cosign/signing/overview/))

**API and protocol design — Observed / Official claim**

- Cosign supports OCI image/blob signing, keyless signing, managed keys, KMS/hardware keys, and bring-your-own PKI. Its CLI is supplemented by Sigstore language clients. ([repository, accessed 2026-09-24](https://github.com/sigstore/cosign), [Sigstore documentation, accessed 2026-09-24](https://docs.sigstore.dev/about/overview/))
- Cosign's attestation format is in-toto serialized as a DSSE envelope and represented in an OCI Image Manifest. Multiple attestations may refer to an image; verification must check the subject relationship, while support for narrower subject relationships is optional. ([primary attestation specification, accessed 2026-09-24](https://github.com/sigstore/cosign/blob/main/specs/ATTESTATION_SPEC.md))
- Current Sigstore bundle formats and OCI 1.1 referrers are used to make metadata more interoperable across language clients, and Cosign exposes bundle create/inspect/upgrade operations. ([repository CLI, accessed 2026-09-24](https://github.com/sigstore/cosign/blob/main/doc/cosign.md), [policy-controller documentation, accessed 2026-09-24](https://docs.sigstore.dev/policy-controller/overview/))

**Identity and authorization — Official claim**

- Keyless signing binds an ephemeral in-memory key to a short-lived Fulcio certificate derived from an OIDC identity; the signing event is recorded in Rekor. Verification checks the expected certificate identity and OIDC issuer. Self-managed keys, KMS, hardware tokens, and custom PKI are alternatives. ([keyless-signing documentation, accessed 2026-09-24](https://docs.sigstore.dev/cosign/signing/overview/), [repository, accessed 2026-09-24](https://github.com/sigstore/cosign))
- Sigstore distributes its Fulcio/Rekor trust root through TUF. Rekor is append-only and supplies a timestamped existence witness; Fulcio publishes issued certificates to certificate transparency. These controls improve discoverability and tamper evidence but depend on trust-root distribution, OIDC correctness, CA/log behavior, and monitoring. ([Sigstore security model, accessed 2026-09-24](https://docs.sigstore.dev/about/security/))
- The identity assertion says which OIDC identity signed an artifact under an expected issuer. It is not a grant to push, merge, deploy, run arbitrary commands, or access Rover's local state. ([keyless-signing documentation, accessed 2026-09-24](https://docs.sigstore.dev/cosign/signing/overview/), [Rover security model](../../docs/SECURITY_MODEL.md#L15-L30))

**Evidence and provenance — Official claim / explicit limit**

- Cosign can create and verify in-toto attestations; Sigstore's policy controller can require signature/attestation authorities, predicate types, and CUE or Rego policies, including SLSA provenance predicate types. ([repository, accessed 2026-09-24](https://github.com/sigstore/cosign), [policy-controller documentation, accessed 2026-09-24](https://docs.sigstore.dev/policy-controller/overview/))
- A valid signature/attestation proves that a trusted signing identity vouched for bytes and/or a signed statement under the selected policy. It does not by itself prove the build was secure, the predicate is true, the tests passed, or the candidate is correct. Sigstore itself notes that OIDC, Fulcio, or Rekor compromise/misbehavior can require third-party log monitoring for detection. ([Sigstore security model, accessed 2026-09-24](https://docs.sigstore.dev/about/security/))

**Observability, extensibility, and operations — Official claim / Observed**

- Rekor's signed log/tree heads, certificate transparency, artifact-owner monitoring, `cosign tree`, and verification output are supply-chain observability—not process telemetry. Verification can use public or custom trust roots and keyless or key authorities. ([Sigstore security model, accessed 2026-09-24](https://docs.sigstore.dev/about/security/), [policy-controller documentation, accessed 2026-09-24](https://docs.sigstore.dev/policy-controller/overview/))
- CUE/Rego policies, custom Sigstore instances/trust roots, KMS providers, hardware keys, OCI registries, and language clients make the model extensible. The policy controller is explicitly described as actively under development, so it should not be treated as a stable universal admission boundary. ([policy-controller documentation, accessed 2026-09-24](https://docs.sigstore.dev/policy-controller/overview/))
- Keyless signing exposes identity information in public, durable logs and performs external Fulcio/Rekor/registry operations. Offline verification reduces runtime network dependence but still requires deliberately managed trust material. The repository states that v2 is the stable line while future feature work is focused on the next major release and `sigstore-go`; Rover should pin and test a reviewed version rather than accept an arbitrary `cosign` binary as a verifier. ([repository, accessed 2026-09-24](https://github.com/sigstore/cosign), [keyless-signing documentation, accessed 2026-09-24](https://docs.sigstore.dev/cosign/signing/overview/))

### Inferences

1. **E2B and Daytona solve hostile-compute placement; Rover currently solves repository/process control on an operator-owned host.** They could eventually complement Rover as execution backends, but neither supplies Rover's review/evidence/grant contract.
2. **Dagger and Temporal solve different orchestration problems.** Dagger is a programmable content-addressed effect graph; Temporal is a durable logical state machine. Rover's current persisted local DAG supervisor is neither, so replacing one with the other would change programming and trust semantics substantially.
3. **Temporal's Worker boundary is the key integration hazard.** Temporal can recover Workflow state when a Worker dies, but if the replacement Worker has the same host credentials, network reach, and writable paths, recovery can repeat an unsafe external effect. Durable orchestration does not provide isolation or idempotency for application code.
4. **Cosign's identity is evidence about an artifact, not authorization to control Rover.** A verified signature may enrich an investigation, but it must not create a grant, approve a review, sign a Rover result, or authorize an external write.
5. **Stars are a poor activity proxy.** Daytona's 71.7k stars coexist with an explicit end of public maintenance, while Temporal's server design remains active despite requiring substantial operations.

### Gaps

- No live E2B, Daytona, Dagger, Temporal, Fulcio/Rekor, registry, or Kubernetes policy-controller service was exercised; all product behavior and performance claims remain official claims.
- E2B's current public repository does not by itself establish the complete separate infrastructure/self-host control-plane implementation, data-retention behavior, or total cost.
- Daytona's current `v0.216` docs and private implementation cannot be source-compared to the final public `v0.190.0` tag. No current OSS Daytona dependency recommendation can be made from this repository.
- Dagger's current docs are on a beta line. The reviewed docs did not establish a stable crash-resume contract independent of Engine/cache behavior.
- Temporal's deployment sizing, database/visibility availability, Worker fleet behavior, and upgrade/versioning procedures were not reproduced.
- Cosign's current major-version transition and policy-controller maturity require a separate version-specific security review before adoption.

## 2. How do evidence, observability, extensibility, and operations compare?

### Takeaway

The projects form complementary layers: E2B/Daytona for isolated compute, Dagger for programmable container effects, Temporal for durable logical execution, and Cosign for artifact identity and transparency. Their strongest evidence is different. E2B/Daytona provide execution placement claims; Dagger provides traces and content-addressed effects; Temporal provides a durable event ledger and caller attribution; Cosign provides cryptographic subject/signer evidence. **None of those facts alone proves candidate correctness, independent acceptance, or safe authorization of an external write.**

### Cited Findings

#### Comparative matrix

| Dimension | E2B | Daytona | Dagger | Temporal | Cosign |
|---|---|---|---|---|---|
| **Primary job** | Per-agent microVM sandboxes | Stateful sandbox control/compute platform | Programmable container CI/effect engine | Durable Workflow/Activity state machine | Artifact signing, attestation, transparency, verification |
| **Isolation authority** | Firecracker microVM and own kernel (official claim) | Current classes include container namespaces and full VMs; current implementation is private | Container runtime plus Engine; not a hostile-code VM boundary | None supplied by Server; Worker deployment is user-controlled | Not an execution runtime |
| **Durable/recovery unit** | Sandbox lifecycle and pause/resume | Reconciled sandbox/snapshot/volume state | Content-addressed cache/effect graph; no Temporal-style replay established | Append-only Event History, replay, Task Queues, retries, timers/messages | External registry/log/bundle state; no scheduler |
| **Primary API** | Python/JS SDK, API key, CLI | REST/Toolbox APIs, five SDKs, CLI, MCP, SSH | One GraphQL Engine API, generated SDKs/modules, CLI | gRPC/protobuf service and language SDKs | CLI/Go plus OCI, DSSE/in-toto, Sigstore bundles, language clients |
| **Identity/authorization** | API-key authority; fine-grained grant model not established | Current product: OIDC/SSO, organizations, scoped keys, secret proxy | Secret types/providers; no Rover-equivalent grant established in reviewed core docs | Opt-in mTLS/JWT/ClaimMapper/Authorizer; unsafe allow-all default without authorizer | OIDC/Fulcio identity or key/KMS identity; artifact authorization only |
| **Evidence strength** | Execution/lifecycle records, not signed provenance | Audit/OTel described, not signed provenance | OTel traces/logs/metrics and content IDs, not signed provenance | Durable event audit plus optional authenticated principal; not signed candidate proof | Strongest cryptographic artifact/signature/attestation/transparency evidence |
| **Extensibility** | SDKs, templates, desktop, code interpreter | SDKs, snapshots, builder, webhooks, MCP | Typed modules, 8 SDK claims, checks/generators/services/changesets | Workflows, Activities, signals/updates, codecs, auth plugins, OTel | Key providers, CUE/Rego, custom roots/instances, policy controller |
| **Main operational burden** | MicroVM fleet, cloud/account dependency, network/data policy | Stateful control plane, DB/Redis/object storage/runners/identity | Engine/runtime/cache/module supply chain and credential providers | Service/DB/visibility/archival, determinism/versioning, idempotent Activities | Trust roots, OIDC/CA/log behavior, registry metadata, policy/version pinning |

Sources for the matrix are the project-specific official pages cited in Section 1: [E2B repository/security](https://github.com/e2b-dev/E2B), [Daytona architecture/isolation](https://www.daytona.io/docs/en/architecture), [Dagger repository/API](https://github.com/dagger/dagger), [Temporal architecture/security/history](https://docs.temporal.io/encyclopedia/architecture/how-temporal-works.md), and [Cosign repository/security model](https://docs.sigstore.dev/about/security/) (all accessed 2026-09-24).

#### Evidence hierarchy and limits

- **Execution placement is not candidate evidence.** E2B's dedicated-kernel microVM and Daytona's VM/container class describe where code runs; neither source says tests passed, a patch was reviewed, or a candidate is safe. ([E2B security](https://e2b.dev/security), [Daytona isolation](https://www.daytona.io/docs/en/isolation), accessed 2026-09-24)
- **Observability is not proof.** Dagger OTel traces and Temporal Event History are valuable for reconstructing execution, but the producer/operator still controls the logs and code. Rover must bind observations to the exact candidate, inputs, command, exit status, and trust domain. ([Dagger repository](https://github.com/dagger/dagger), [Temporal event history](https://docs.temporal.io/workflow-execution/event.md), accessed 2026-09-24)
- **Retry/durability is not exactly-once side effect.** Temporal explicitly requires Activity idempotency because code can execute multiple times. Rover's repair loops and external adapters need the same discipline, regardless of whether a local supervisor or Temporal schedules them. ([Temporal Activity documentation](https://docs.temporal.io/activity-definition.md), accessed 2026-09-24)
- **Artifact provenance is not acceptance.** Cosign can bind a signature/attestation to bytes and an OIDC identity. It cannot establish that an in-toto predicate is factually true, that the builder was trustworthy, that Rover's required checks ran, or that a human approved deployment. ([Cosign attestation specification](https://github.com/sigstore/cosign/blob/main/specs/ATTESTATION_SPEC.md), [Sigstore security model](https://docs.sigstore.dev/about/security/), accessed 2026-09-24)
- **Transparency is not continuous monitoring by default.** Sigstore states that users/artifact owners are responsible for monitoring logs for unauthorized use; unmonitored Fulcio/Rekor misbehavior may go undetected. ([Sigstore security model](https://docs.sigstore.dev/about/security/), accessed 2026-09-24)
- **Rover's existing signature limit remains correct.** Rover's Ed25519 attestation proves a signature against an explicitly trusted key; it does not prove correctness, independent administration, SLSA level, or in-toto compliance. ([Rover README](../../README.md#L155-L163), [Rover security model](../../docs/SECURITY_MODEL.md#L32-L41))

#### Extensibility comparison

- **E2B** is easiest to extend at the environment boundary: templates, arbitrary OCI base images, SDKs, desktop, and code-interpreter packages. This is runtime extensibility, not policy extensibility. ([template documentation](https://e2b.dev/docs/template/quickstart), accessed 2026-09-24)
- **Daytona's** strongest current extension surfaces are SDKs, declarative snapshots/builders, webhooks, MCP, SSH/terminal tools, and platform hooks. Because the core is private, these interfaces cannot be assumed stable or fully OSS. ([repository feature map](https://github.com/daytonaio/daytona), [current architecture](https://www.daytona.io/docs/en/architecture), accessed 2026-09-24)
- **Dagger** makes execution itself programmable through typed modules and generated clients. The current module contract asks authors to expose small typed APIs, make side effects explicit, and return changesets for file edits. That aligns well with Rover's reviewed-contract philosophy, but module installation and execution still need a Rover-side trust review. ([SDK/module documentation](https://docs.dagger.io/reference/sdks), accessed 2026-09-24)
- **Temporal** is extensible through application-level Workflow/Activity code and infrastructure plugins. Its deterministic replay and data-conversion contracts make extensions more foundational than a simple tool plugin. ([security documentation](https://docs.temporal.io/self-hosted-guide/security.md), [observability documentation](https://docs.temporal.io/evaluate/features/observability.md), accessed 2026-09-24)
- **Cosign** is extensible through signing identities, KMS/hardware providers, trust roots, custom Sigstore deployments, OCI metadata, and CUE/Rego policies. These extensions decide whose statements count and under which root; trust configuration is part of the security policy. ([policy-controller documentation](https://docs.sigstore.dev/policy-controller/overview/), [Sigstore security model](https://docs.sigstore.dev/about/security/), accessed 2026-09-24)

#### Operational tradeoffs

- **E2B:** stronger compute isolation than Rover local mode, but cloud placement, API-key custody, egress, snapshot/log retention, regional availability, and per-sandbox compute cost become external dependencies. BYOC reduces data egress but moves substantial infrastructure responsibility to the customer. ([security page](https://e2b.dev/security), accessed 2026-09-24)
- **Daytona:** the current control/compute design supports reconciliation and elastic runners, but it is operationally broad and no longer represented by an actively maintained public core. High stars do not offset the maintenance discontinuity. ([architecture](https://www.daytona.io/docs/en/architecture), [repository notice](https://github.com/daytonaio/daytona), accessed 2026-09-24)
- **Dagger:** reproducible container effects and incremental cache reduce repeated work, but add Engine/runtime/cache operations and a broad programmable trust surface. Secret providers and publish/export modules can cross Rover's present no-upload/no-auto-write boundary. ([repository](https://github.com/dagger/dagger), [secrets](https://docs.dagger.io/adopting/secrets), [modules](https://docs.dagger.io/reference/sdks), accessed 2026-09-24)
- **Temporal:** durability moves state and scheduling into a database-backed service and imposes deterministic Workflow code, versioning, history growth, visibility storage/archival, and Activity idempotency. It does not reduce Worker isolation requirements. ([event history](https://docs.temporal.io/workflow-execution/event.md), [observability](https://docs.temporal.io/evaluate/features/observability.md), [Activity](https://docs.temporal.io/activity-definition.md), accessed 2026-09-24)
- **Cosign:** verification can be local and deterministic given explicit trust material, while keyless signing and normal online verification introduce external trust and availability dependencies. Public keyless identity records, registry mutations, trust-root rotation, and policy maintenance are operational responsibilities. ([repository](https://github.com/sigstore/cosign), [keyless documentation](https://docs.sigstore.dev/cosign/signing/overview/), accessed 2026-09-24)

### Inferences

1. **Use projects as layers, not as a replacement chain.** Rover can remain the repository/task/evidence authority while adopting concepts from compute isolation, programmable verification, durable scheduling, and artifact provenance independently.
2. **The best near-term evidence upgrade is verification, not signing.** Importing and locally verifying an existing signed blob/attestation can add signer/subject evidence without publishing Rover state. Automatic signing would add a new authority and external-write path.
3. **The best near-term orchestration lesson is explicit effect typing.** Dagger's separation of checks, changesets, services, and publish/export functions maps well to Rover's distinction among local inspection, reviewed edits, execution, and prohibited external effects.
4. **The best recovery lesson is event lineage.** Temporal's append-only history, command/event distinction, Continue-As-New, and idempotency warning are useful design inputs for Rover's persisted supervisors even if Temporal is not adopted.
5. **Observability exports need the same authority boundary as execution.** OTel, Rekor, PostHog, hosted visibility, and remote logs can disclose source paths, prompts, commands, identities, or secrets. They should be explicit destinations with redaction, retention, and opt-in policy.

### Gaps

- No source in this review establishes an end-to-end chain from isolated execution through independent verification, signed provenance, human approval, and protected publication. Rover would still need to define and test that chain itself.
- Comparative latency, throughput, cache efficiency, recovery time, and cost were not benchmarked. Marketing values such as “90ms” or “sub-200ms” were not treated as comparable measurements.
- No review of vendor SLAs, pricing, regional failover, disaster recovery, retention schedules, or contractual data processing terms was completed beyond the cited official pages.
- The evidence models were compared conceptually, not through a shared adversarial test corpus or cross-project implementation.

## 3. What is applicable to Rover now, and what would violate Rover's constraints?

### Takeaway

Rover should now borrow **design patterns and explicit local verification hooks**, not silently replace its trust model with a cloud or workflow platform. Worktrees are candidate-change isolation, not hostile-code isolation; supervisors and DAGs are local orchestration, not a distributed fleet; grants authorize Rover API tools, not arbitrary worker code; Ed25519 signatures bind a completed investigation to a key, not to universal correctness. Any future E2B/Daytona/Dagger/Temporal/Cosign integration needs a reviewed contract, explicit data-flow policy, failure/recovery tests, and a disabled-by-default external-write surface.

### Cited Findings

#### Rover's actual boundary

- Rover uses separate worktrees, reservations, exact dependency snapshots, and conflict-stopping integration for parallel workflows. ([Rover README](../../README.md#L95-L101))
- Rover has detached local supervisors, terminal/PTY attach, bounded output, repair attempts, and resource admission, but cross-host worker recovery/fencing and host-loss continuation are not implemented. ([Rover status](../../STATUS.md#L7-L18))
- Rover's remote interface is a JSON-only stateless Streamable HTTP subset pinned to MCP 2025-11-25, with read-only default, explicit execution enablement, TLS requirements, and project/audience/tool-scoped bearer grants. It is remote control, not a validated multi-host worker fleet. ([Rover README](../../README.md#L121-L143))
- Local execution and local administration share the OS user's authority. Worktrees, scrubbed environments, and API grants do not sandbox arbitrary executable code. ([Rover security model](../../docs/SECURITY_MODEL.md#L3-L13))
- Rover has no API tool that can create grants, approve reviews, sign results, merge, or deploy. ([Rover security model](../../docs/SECURITY_MODEL.md#L15-L30))
- Rover's evidence limit is explicit: Ed25519 proves origin relative to a supplied public key, not correctness, independent administration, SLSA, or in-toto compliance. ([Rover security model](../../docs/SECURITY_MODEL.md#L32-L41))
- Rover does not automatically upload repository data, fetch provider credentials, publish, push, merge, or deploy. ([Rover README](../../README.md#L56-L58), [Rover README](../../README.md#L194-L195))

#### Applicable now as design or local-user-controlled behavior

| Rover area | Applicable now | Why it remains inside Rover's boundary |
|---|---|---|
| **Worktrees and candidate review** | Preserve worktree isolation as change collision control, while labeling it **not a sandbox**. Borrow E2B/Daytona's explicit runtime-class/resource/network distinctions for future documentation, but do not call a worktree hostile-code isolation. | Rover already owns local operator authority and must state that boundary. ([Rover security model](../../docs/SECURITY_MODEL.md#L3-L13)) |
| **Supervisors** | Add Temporal-inspired append-only event vocabulary, command/result lineage, explicit retry/idempotency contracts, and bounded Continue-As-New-style run rollover in the design. | This is local evidence/state design; it does not require sending work to Temporal Cloud or another service. ([Temporal event history](https://docs.temporal.io/workflow-execution/event.md), accessed 2026-09-24) |
| **DAGs and verification** | Allow an operator to invoke an already reviewed Dagger check locally when the module, container runtime, inputs, network policy, expected structured output, and side-effect contract are explicit. Prefer check-only modules; reject unknown publish/deploy functions. | A user-authorized local command is not automatic publication. Rover must still parse the actual output and cannot infer correctness from exit zero. ([Dagger module guidance](https://docs.dagger.io/reference/sdks), [Rover README](../../README.md#L103-L119), accessed 2026-09-24) |
| **Remote control** | Keep Rover's scoped grants and no-grant-mint API. A future remote compute adapter must request only a capability needed for that task and must not forward the Rover grant token to the worker. | Rover grants constrain Rover API operations, not arbitrary code after execution is authorized. ([Rover security model](../../docs/SECURITY_MODEL.md#L15-L30)) |
| **Attestations and evidence** | Permit an operator to run an explicitly reviewed Cosign verifier as an approved local command for an existing signature/attestation bundle, with a pinned Cosign version, trusted root, expected issuer/identity or public key, exact subject digest, predicate type, policy, and captured verification output. Preserve the result as an imported assertion, not as Rover acceptance. | Verification can be performed from local material without uploading repository/evidence or publishing signatures. ([Cosign repository](https://github.com/sigstore/cosign), [attestation specification](https://github.com/sigstore/cosign/blob/main/specs/ATTESTATION_SPEC.md), [Rover evidence limits](../../docs/SECURITY_MODEL.md#L32-L41), accessed 2026-09-24) |
| **Observability** | Permit explicit OTel export to an operator-selected local collector or approved endpoint, with a manifest of what fields leave the host and a fail-closed/no-export default. | Optional export is compatible only when destination, scope, redaction, and retention are reviewed; automatic upload is not. ([Dagger repository](https://github.com/dagger/dagger), [Temporal observability](https://docs.temporal.io/evaluate/features/observability.md), accessed 2026-09-24) |

#### Future integration ideas that require a new reviewed contract

1. **E2B execution adapter:** potentially useful for hostile-candidate execution, but only after Rover defines explicit repository transfer, API-key custody, network policy, snapshot/log retention, cancellation, cleanup, billing, data residency, and provider outage behavior. The default Rover local path must remain unchanged.
2. **Temporal-backed long-running orchestration:** potentially useful for cross-host durable tasks, but it does not replace Worker isolation, grants, evidence, or external-write gates. Rover would need deterministic command contracts, idempotent Activities, namespace authorization, history retention/redaction, Worker fencing, and a migration/recovery plan.
3. **Dagger verification module contract:** potentially useful for portable containerized checks, but modules must be pinned/reviewed, secrets disabled by default, network disabled unless approved, outputs parsed into Rover's evidence schema, and external publish/deploy APIs denied.
4. **Cosign evidence importer/verifier:** potentially useful for existing signed build artifacts, SBOMs, and attestations, but trust-root selection and predicate interpretation are policy decisions. Rover must keep its Ed25519 investigation signature and any Cosign result as separate evidence types.
5. **Daytona architecture study only:** because the requested public repository is unmaintained and current core is private, no current dependency, SDK compatibility promise, or security claim should be based on the public brand alone.

#### Would violate current Rover constraints if enabled by default

- **Automatic E2B or Daytona sandbox creation** would upload selected repository/context data and require provider credentials, network access, billing, and a new external processing agreement. Rover currently does not fetch provider credentials or upload repository data. ([Rover README](../../README.md#L56-L58), [E2B security](https://e2b.dev/security), [Daytona architecture](https://www.daytona.io/docs/en/architecture))
- **Passing host/provider/API credentials into an agent sandbox** conflicts with Rover's rule that arbitrary worker code shares host authority unless truly isolated and that private signing keys must not be exposed to agents. Daytona's 2026 advisory is a concrete warning about this pattern. ([Rover security model](../../docs/SECURITY_MODEL.md#L10-L13), [Daytona advisory](https://www.daytona.io/dotfiles/updates/security-advisory-api-credential-exposure-in-sandboxes))
- **Automatic Dagger Cloud Checks or GitHub integration** would connect repositories, fetch repository source, and initiate remote jobs after pushes. That violates the present absence of automatic upload and external write/publish behavior. ([Dagger Cloud Checks documentation](https://docs.dagger.io/cloud-checks), accessed 2026-09-24; [Rover README](../../README.md#L194-L195))
- **Automatic Dagger secret-provider access** from host files, environment, command output, cloud credential chains, Vault, 1Password, or KMS would cross Rover's explicit credential-broker and no-secret-upload boundary unless separately reviewed and user-authorized. ([Dagger secrets](https://docs.dagger.io/adopting/secrets), [Rover status](../../STATUS.md#L15-L18), accessed 2026-09-24)
- **Running Temporal Activities with unrestricted host/network authority** would make retries and Worker failover capable of repeating external effects. Temporal's own docs require idempotency; Rover additionally requires worktree/resource/grant boundaries and explicit external-write review. ([Temporal Activity documentation](https://docs.temporal.io/activity-definition.md), [Rover security model](../../docs/SECURITY_MODEL.md#L15-L30), accessed 2026-09-24)
- **Deploying a Temporal Server with the default `noopAuthorizer`** would expose every reachable API, including administrative operations. This is explicitly warned against by Temporal and is incompatible with Rover's scoped authority model. ([Temporal security documentation](https://docs.temporal.io/self-hosted-guide/security.md), accessed 2026-09-24)
- **Automatic `cosign sign` or `cosign attest`** can perform keyless identity exchange, upload entries to transparency services, and mutate OCI registries. It would introduce public durable identity data and an external write; Rover currently performs neither by default. ([Cosign repository](https://github.com/sigstore/cosign), [keyless documentation](https://docs.sigstore.dev/cosign/signing/overview/), [Rover README](../../README.md#L194-L195), accessed 2026-09-24)
- **Giving Cosign, Fulcio, Rekor, a registry credential, or private signing/KMS authority to an agent** would conflate evidence tooling with execution authority and could permit publication under an identity the user did not intend to use. ([Sigstore security model](https://docs.sigstore.dev/about/security/), [Rover security model](../../docs/SECURITY_MODEL.md#L15-L41), accessed 2026-09-24)
- **Describing any of these signals as proof that Rover accepted a candidate** would violate Rover's explicit evidence limits. A microVM, green trace, durable replay, registry signature, SBOM, provenance predicate, or model assertion still requires Rover's exact-candidate verification and human/policy acceptance path. ([Rover security model](../../docs/SECURITY_MODEL.md#L32-L41))
- **Replacing worktrees with ordinary containers as a claim of hostile-code isolation** would be incorrect. Rover's restricted Docker adapter has explicit limits and no claim of hostile-code security certification; E2B microVMs and Daytona VM classes are different isolation products. ([Rover README](../../README.md#L165-L172), [E2B security](https://e2b.dev/security), [Daytona isolation](https://www.daytona.io/docs/en/isolation), accessed 2026-09-24)

#### Recommended decision posture

| Project | Decision posture for Rover now |
|---|---|
| **E2B** | **Design reference; optional future adapter only.** Do not make it the default worker or enable data transfer without a separate contract and explicit user action. |
| **Daytona** | **Design reference only for the requested OSS repository.** Do not adopt the public repo as a maintained dependency; evaluate the current private product separately if it is ever in scope. |
| **Dagger** | **Applicable as an explicitly reviewed local verification command/module pattern.** No automatic Cloud Checks, secret providers, publish functions, or host-authoritative module execution. |
| **Temporal** | **Architecture reference for durable state, replay, task queues, and idempotency.** Not a substitute for Worker isolation, Rover grants, or evidence; any backend adoption is a major reviewed contract. |
| **Cosign** | **Applicable first as local verification/import of existing signatures and attestations.** Automatic keyless signing, Rekor/registry upload, private-key/KMS delegation, or identity publication is outside current defaults. |

### Inferences

1. **Rover's strongest near-term composite is local and layered:** worktree + explicit approved verifier (optionally Dagger) + exact evidence capture + optional local Cosign verification + Ed25519 binding to the retained investigation. Each layer has a narrow claim.
2. **Remote compute should be an adapter capability, not a trust shortcut.** A future E2B/Daytona/Temporal Worker may isolate or recover execution, but Rover must still mediate grants, inputs, network, outputs, evidence, and external writes.
3. **Durable state and hostile isolation should be designed independently.** Temporal-style recovery and microVM/container placement solve different problems; combining them does not remove the need for idempotency, fencing, cleanup, and secret minimization.
4. **Verification is the lowest-risk Cosign adoption path.** It can consume pre-existing evidence without making Rover a signer or publisher, provided the exact trust root and subject are visible and recorded.
5. **Any adapter that can reach a registry, Git host, cloud API, secret manager, or deployment endpoint needs a distinct external-effect contract.** A generic “sandbox,” “agent,” “module,” “Activity,” or “publish” capability must never imply that permission.

### Gaps

- No implementation plan or schema was produced; this file is a research comparison, not approval to add a dependency or external adapter.
- No candidate verification plan was defined for Cosign, Dagger, Temporal, E2B, or Daytona. Each would need domain-owner tests, adversarial fixtures, pinned versions, redaction cases, failure injection, and explicit acceptance criteria.
- No hosted account, provider API key, registry, GitHub organization, Kubernetes cluster, Temporal namespace, or transparency service was accessed.
- No claim was made about vendor certifications, contractual compliance, independent penetration results, production reliability, or full-platform support beyond the exact official statements cited.
- Current 2026 interfaces may change after the retrieval date; Daytona is especially sensitive to this because public and current product code no longer coincide.
