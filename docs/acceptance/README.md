# Original acceptance plan: implementation mapping

All 40 original scenarios remain specifications. **No claim is made that all 40 pass.**
The following matrix states the implemented subset or explicit deferral.

| ID | Scenario | Alpha scope | Limitation |
|---|---|---|---|
| A01 | Setup preview | implemented_local_subset | Setup previews preserve files; signed installer remains absent. |
| A02 | Installer trust | deferred | Not implemented or not validated in this source alpha. |
| A03 | Capability mismatch | partial | Generic manifest and unsupported modes tested; native adapter negotiation absent. |
| A04 | Protocol robustness | deferred | Not implemented or not validated in this source alpha. |
| A05 | Detach | headless_subset | Headless supervisor survives launcher exit; no interactive PTY. |
| A06 | Supervisor/host loss | partial | Linux definitely-gone process reconciliation; no actual host failover. |
| A07 | Cancellation | original_process_group_subset | No guarantee for deliberately escaped process groups or external effects. |
| A08 | Task-wide budgets | partial | Task timeout exists; no global cost/concurrency/storage accounting. |
| A09 | Output/disk pressure | partial | Per-stream capture limits; no global disk quotas/retention. |
| A10 | Moving workspace | partial | Snapshot bytes isolated, double read not filesystem-atomic. |
| A11 | Snapshot completeness | reject_unsupported_subset | Unsupported modes and limits rejected rather than implemented. |
| A12 | Resource isolation | deferred | Not implemented or not validated in this source alpha. |
| A13 | Dependency integrity | deferred | Not implemented or not validated in this source alpha. |
| A14 | Contract changes | partial | Freezes chosen base config; same-user can choose another base/override. |
| A15 | Duplicate trigger | dispatch_subset | Local task keys only; no webhooks/scheduled/external writes. |
| A16 | Cross-project context | deferred | Not implemented or not validated in this source alpha. |
| A17 | Prompt injection | deferred | Not implemented or not validated in this source alpha. |
| A18 | Worker credentials | deferred | Not implemented or not validated in this source alpha. |
| A19 | Sandbox fallback | admission_only | Docker absence/pin failures refuse fallback; no live Docker boundary test. |
| A20 | Approval binding | local_subset | Snapshot/config binding is a local assertion, not authenticated team approval. |
| A21 | Missing tests | parser_subset | Test parser completeness; no proof of requirement coverage. |
| A22 | False command success | scoped_interpretation | Exit-code only claims exit success; dishonest scripts still outside trusted-local assumptions. |
| A23 | Wrong-candidate report | policy_subset | Candidate/spec digest binding checked; no independent provenance authority. |
| A24 | Counterfactual validity | deferred | Not implemented or not validated in this source alpha. |
| A25 | Flaky retries | deferred | Not implemented or not validated in this source alpha. |
| A26 | Formal scope | deferred | Not implemented or not validated in this source alpha. |
| A27 | No-op verification | policy_subset | Zero required checks => inconclusive. |
| A28 | External uncertainty | deferred | Not implemented or not validated in this source alpha. |
| A29 | Stale remote worker | deferred | Not implemented or not validated in this source alpha. |
| A30 | Integration drift | deferred | Not implemented or not validated in this source alpha. |
| A31 | CI publisher | deferred | Not implemented or not validated in this source alpha. |
| A32 | Artifact retention | deferred | Not implemented or not validated in this source alpha. |
| A33 | Upgrade recovery | partial | Basic schema compatibility and reopen only; backup/upgrade rollback deferred. |
| A34 | Learning authority | deferred | Not implemented or not validated in this source alpha. |
| A35 | Outcome validity | recording_only | Explicit user outcome label and note; no automated causal attribution. |
| A36 | Evaluation leakage | deferred | Not implemented or not validated in this source alpha. |
| A37 | Untrusted artifacts | partial | Local artifact/digest validation, no malicious same-user isolation. |
| A38 | Cache trust | no_reuse_implemented | No result cache exists; every verification reruns checks. |
| A39 | Destructive delivery | no_delivery_implemented | No merge/deploy/destructive delivery operations implemented. |
| A40 | Export and uninstall | partial | Bounded metadata export only; no full backup/uninstall automation. |
