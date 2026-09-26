# ADR-0008: canonical repository identity and project scope

- Status: accepted for P-025 implementation
- Date: 2026-09-25
- Owners: Rover maintainers; Go behavior owner is `internal/access`, Rust owner is `rover-core`

## Context

Go derives `ProjectID` from SHA-256 of a canonical repository path and derives
the control audience from the canonical state-root path, a NUL separator, and
that repository path. Its path rule is exact: empty input remains empty;
otherwise `filepath.Abs` is followed by `filepath.EvalSymlinks` when that fully
resolves, and a cleaned absolute path is used if resolution fails. Grants must
not authenticate under a different project or state root. See
`internal/access/grants.go` and `internal/access/grants_test.go`.

## Decision

- Add a Rust `RepositoryIdentity` value in `rover-core` that stores the
  canonical path and its `Sha256Digest` project ID. No new dependency is
  needed.
- Preserve the Go resolution fallback exactly, including empty input and
  unresolved paths beneath symlinked ancestors. Do not replace it with a
  stricter “must exist” or “must canonicalize” rule; callers may identify a
  repository before it exists.
- Add control-audience derivation using the canonical state-root bytes,
  `NUL`, and canonical repository-path bytes, with the existing
  `rover-control/v1:` prefix. This keeps grants bound to both project and
  state store.
- Keep identity separate from authorization. A digest identifies a project;
  it does not grant access. Grant issuance, expiry, tool allowlists, revocation,
  and authentication remain owned by the access layer and must check both the
  audience and project digest.
- Cross-project tests must prove that path spelling variants for one existing
  repository coalesce, a symlink alias coalesces only when full resolution
  succeeds, unresolved paths follow the Go fallback, and distinct projects or
  state roots never share an audience.

## Alternatives considered

- Hash raw user input: rejected because trailing separators, dot segments, and
  existing symlink aliases would split one filesystem project into several
  identities.
- Require every repository path to exist: rejected because current Go accepts
  unresolved paths and callers construct configs for future paths.
- Use project ID alone as audience: rejected because it would allow a bearer
  token to cross state roots that happen to contain the same repository path.

## Acceptance evidence

Port the Go path rule and digest framing into Rust unit tests. Compare stable
SHA-256 results with the Go implementation's `model.Digest` behavior. Add grant
contract tests before marking P-025 complete; this ADR alone is not evidence of
cross-project isolation.
