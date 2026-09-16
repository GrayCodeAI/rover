# Implementation references and provenance

This upgrade builds on the delivered Rover 0.1.0-alpha.1 repository. The ten-layer
master plan remains a design target, not a claim of implemented/validated coverage.

Primary interface references consulted during implementation:

- Go release policy: https://go.dev/doc/devel/release
- Go setup action: https://github.com/actions/setup-go
- SQLite C API / online backup: https://www.sqlite.org/backup.html
- SQLite WAL: https://sqlite.org/wal.html
- Git worktrees: https://git-scm.com/docs/git-worktree
- Codex noninteractive CLI: https://developers.openai.com/codex/noninteractive
- Claude Code programmatic/headless use: https://code.claude.com/docs/en/headless
- MCP pinned 2025-11-25 lifecycle, tools and transports:
  https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- Docker security: https://docs.docker.com/engine/security/

These links identify interface references; they are not evidence of live provider
compatibility or proof that all current versions are supported. Native model profiles
were tested with deterministic transcripts and subprocess fixtures only.

The tested compiler available here was Go 1.23.2 (Linux amd64); it is not a production
recommendation. The CI workflow asks setup-go for a supported stable toolchain, but
hosted CI has NOT run. The pinned checkout/setup action commits are retained from the
original alpha rather than represented as the newest actions.

No third-party competitor implementation was copied. No model credentials, production
resources, public repository writes, or provider sessions were used. Tests and demo
reports are scoped to owned fixtures and local networking. See validation/v0.2.0/REPORT.md.
