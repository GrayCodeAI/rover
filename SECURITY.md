# Security

Rover is a pre-1.0 alpha, not hardened multi-tenant or production assurance
software. Read [the complete threat model](docs/SECURITY_MODEL.md) before executing
code. Only owned/trusted local inputs were used in validation. Do not put hostile
repositories or sensitive credentials on an unrestricted alpha worker.

## Reporting a vulnerability

Do not open a public GitHub issue for a vulnerability, and do not publish secrets
or working exploit details anywhere public.

- Email **security@graycodeai.com** with the affected version or commit, platform,
  reproduction steps, and impact.
- GitHub private vulnerability reporting is **not yet enabled** on
  `GrayCodeAI/rover`. Once the maintainers enable it, the repository's
  [Security tab](https://github.com/GrayCodeAI/rover/security) will offer
  "Report a vulnerability" as a second private channel.

No response-time SLA is offered.

## Supported versions and releases

There is no supported production release line yet. Fixes land on `main` and in
the next `0.0.x` release. `v0.0.1` is a source-only tag with no binary assets.
Binaries built by the release workflow carry GitHub build-provenance
attestations; verify a download with
`gh attestation verify <file> --repo GrayCodeAI/rover` (see
[docs/CI.md](docs/CI.md)). Maintainers must review security updates,
third-party/toolchain versions, permissions, and release provenance before
distributing binaries.
