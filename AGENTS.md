# Instructions for coding agents contributing to Rover

Read README.md, STATUS.md and docs/SECURITY_MODEL.md first. Preserve terminal-first,
agent-neutral full-platform scope. Do not describe the design reference as shipped code.

Before changing code, identify the relevant test and domain owner. Add regression tests
for acceptance/security changes. Run make check, make race, and make demo in an appropriate
owned environment. Report exact commands and limitations. Do not invent passing CI runs,
live provider tests, code-coverage metrics, or security certification.

Do not weaken tests or required policy to make checks green. Keep schema changes explicit.
Never copy model assertions into evidence as observed facts. No automatic publication,
merge, release, deployment, credential upload, or changes to unrelated repositories.

Dependency additions, trust-mode changes, native adapters, and external writes require
reviewed contracts and appropriate tests. No stub may advertise unsupported capabilities.
