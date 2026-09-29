# Support and publication status

Rover is published at <https://github.com/GrayCodeAI/rover> as a pre-1.0 alpha.
The `v0.0.1` GitHub release is a source-only tag: it has no binary assets, and
there is no package-manager listing and no support SLA. Build from a clone as
described in the [README](README.md). `rover help` and [docs/CLI.md](docs/CLI.md)
describe the implemented command surface.

Report bugs and ask questions in
[GitHub issues](https://github.com/GrayCodeAI/rover/issues). Include platform,
toolchain, exact command, and sanitized evidence. For anything else, email
hello@graycodeai.com. Security problems go through [SECURITY.md](SECURITY.md),
never a public issue.

`go install github.com/GrayCodeAI/rover/cmd/rover@...` is not a documented or
tested install path. The build needs cgo and system SQLite, and `make build`
injects the commit into the version metadata.
