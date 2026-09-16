# ADR 0001 — Small offline-capable Go foundation

Status: implemented bootstrap decision; revisit before stable release.

Use Go standard-library CLI/JSON/process packages and a small system-SQLite cgo wrapper.
The build container had no external module cache and dependency/toolchain downloads failed.
This avoids pretending untestable third-party integrations are finished, but it requires
CGO, headers and a C toolchain. It does not satisfy the eventual static-binary distribution
vision. Strict JSON is the alpha configuration contract; YAML is not silently accepted.

The text dashboard is intentionally not advertised as Bubble Tea or a full terminal.
Keep the ten-layer architecture in ownership/docs rather than generating empty services.
A native runtime/adapter and live restricted executor are next functional milestones.
