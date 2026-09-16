# ADR 0002 — Observations and acceptance remain separate

Status: implemented for local advisory scope.

Checks read a frozen approved configuration and an exact content snapshot. Each gets a
fresh work directory. Parsing, required-check completeness, input-integrity checks and
policy are separate from the agent process. A review record does not rewrite a verdict.

SQLite/artifact permissions and hashes provide local integrity hygiene, not independence
from the same OS user. The selected base, explicit config, and local reviewer remain
user-controlled. No protected acceptance or authenticated external publisher is claimed.
Formal proof, model-generated test quality, and arbitrary requirement truth are not
inferred from an exit code or a parsed JUnit/Go event stream.
