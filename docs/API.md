# Model API Reference

Rover's data model is defined in `internal/model/model.go`. Records are versioned
JSON objects identified by `schema` strings and persisted via `store.Store` keyed
by `(kind, id)`. All identifiers conform to `ValidID`:
`^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$`.

## Constants

| Constant   | Value         | Description                                      |
|------------|---------------|--------------------------------------------------|
| `Version`  | `"0.0.1"`     | Binary version label                             |
| `Schema`   | `"rover/v1alpha1"` | Base schema namespace for all records        |

## Identity & digests

### `ValidID(s string) bool`
Returns true if `s` matches `^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$`.

### `ID(prefix string) string`
Generates a random ID with the given prefix (e.g. `"task_"`). Panics only on
catastrophic syscall failure.

### `TryID(prefix string) (string, error)`
Fallible form of `ID`.

### `Digest(b []byte) string`
SHA-256 hex digest of `b`. Never fails.

### `Hash(v any) string`
JSON-marshals `v` and returns its `Digest`. Panics on marshal failure.

### `TryHash(v any) (string, error)`
Fallible form of `Hash`.

### `Now() string`
Current UTC time in RFC3339Nano format.

## Configuration

### `Config`
```json
{"schema":"rover/v1alpha1","checks":[...],"policy":{...}}
```

### `Policy`
```json
{"require_review":true,"review_paths":["..."]}
```

### `CheckSpec`
A check specification with the following fields:

| Field        | Type     | JSON key        | Required | Description                              |
|--------------|----------|-----------------|----------|------------------------------------------|
| `ID`         | string   | `id`            | yes      | Non-empty, ValidID                       |
| `Argv`       | []string | `argv`          | yes      | Command and arguments                    |
| `Timeout`    | string   | `timeout`       | yes      | Duration string, e.g. `"10s"`            |
| `Required`   | bool     | `required`      | yes      | Must pass for acceptance                 |
| `Parser`     | string   | `parser`        | yes      | One of: `exit-code`, `go-test-json`, `sarif`, `junit` |
| `MinTests`   | int      | `min_tests`     | no       | Minimum tests for parser to be conclusive|
| `ReportPath` | string   | `report_path`   | no       | Path for evidence report                 |
| `PassEnv`    | []string | `pass_env`      | no       | Environment variables to inherit         |
| `FailLevel`  | string   | `fail_level`    | no       | Minimum severity for failure             |
| `Property`   | string   | `property`      | no       | Property to evaluate                     |
| `Scope`      | string   | `scope`         | no       | Scope descriptor                         |
| `Assumptions`| []string | `assumptions`   | no       | Documented assumptions                   |

## Source records

### `File`
```json
{"path":"src/main.go","mode":420,"sha256":"...","size":1024}
```

### `Snapshot`
```json
{"schema":"rover/v1alpha1","id":"...","repository":"...","source_ref":"...","files":[...]}
```

### `Change`
```json
{"path":"src/main.go","status":"modified","category":"edit"}
```

### `Inspection`
```json
{"schema":"rover/v1alpha1","base":{...},"candidate":{...},"changes":[...],"findings":[...]}
```

### `Finding`
```json
{"rule":"R-001","message":"...","severity":"high","path":"file.go","line":42,"evidence":"..."}
```

## Process & check results

### `ProcessResult`
```json
{"started_at":"2024-01-01T00:00:00Z","finished_at":"...","exit_code":0,"timed_out":false,
 "cancelled":false,"truncated":false,"stdout_sha256":"...","stderr_sha256":"..."}
```

### `CheckResult`
```json
{"parser":"exit-code","id":"lint","required":true,"outcome":"pass","meaning":"...",
 "tests":10,"skipped":0,"process":{...},"spec_digest":"..."}
```

### `Investigation`
The top-level record for a verification run:

| Field             | Type           | JSON key          | Description                        |
|-------------------|----------------|-------------------|------------------------------------|
| `Schema`          | string         | `schema`          | `"rover/v1alpha1"`                 |
| `ID`              | string         | `id`              | Unique investigation ID            |
| `RoverVersion`    | string         | `rover_version`   | `model.Version`                    |
| `StartedAt`       | string         | `started_at`      | ISO 8601 start time                |
| `FinishedAt`      | string         | `finished_at`     | ISO 8601 finish time               |
| `Repository`      | string         | `repository`      | Repository identifier              |
| `Base`            | string         | `base`            | Base snapshot ID                   |
| `Candidate`       | string         | `candidate`       | Candidate snapshot ID              |
| `ConfigDigest`    | string         | `config_digest`   | SHA-256 of frozen config           |
| `PolicySource`    | string         | `policy_source`   | Human-readable policy origin       |
| `Executor`        | string         | `executor`        | Execution mode                     |
| `Trust`           | string         | `trust`           | Trust model description            |
| `Environment`     | map[string]string | `environment`   | Build/runtime environment          |
| `Checks`          | []CheckResult  | `checks`          | Check results                      |
| `Findings`        | []Finding      | `findings`        | Discovered findings                |
| `Unknowns`        | []string       | `unknowns`        | Unresolved areas                   |
| `Decision`        | string         | `decision`        | Final decision                     |
| `DecisionReason`  | string         | `decision_reason` | Human-readable reason              |

**Decision values:** `ACCEPT`, `REJECT`, `REVIEW` (with review required),
`PENDING` (initial).

## Tasks

### `TaskSpec`
```json
{"schema":"rover/v1alpha1","objective":"Fix bug","repository":"...","base":"...",
 "argv":["make","test"],"timeout":"300s","auto_verify":true}
```

Additional fields: `agent`, `agent_options`, `interactive`, `max_attempts`,
`repair_argv`, `reservations`, `initial_snapshot`, `config_path`.

### `AgentOptions`
```json
{"executable":"python3","model":"gpt-4","write":true,
 "allowed_tools":["read","write"],"max_turns":10}
```

### `TaskRun`
```json
{"schema":"rover/v1alpha1","id":"...","contract":{...},"status":"CANDIDATE_READY",
 "workspace":"...","base_snapshot":"...","candidate":"...","attempts":[...]}
```

**Status values:** `CANDIDATE_READY`, `REVIEW_READY`, `CHECKS_BLOCKED`,
`FAILED`, `ERROR`, `CANCELLED`, `TIMED_OUT`, `LOST`, `PENDING`.

### `Attempt`
```json
{"number":1,"process":{...},"candidate":"...","agent":{...}}
```

### `AgentResult`
```json
{"adapter":"generic-pty","completed":true,"usage":{"tokens":1234},
 "estimated_cost_usd":0.01,"claims":["..."],"provenance":"..."}
```

## Agent capabilities

### `Capabilities`
```json
{"name":"generic-pty","launch":true,"cancel":true,"log_follow":true,
 "interactive_pty":true,"native_resume":true,"permission_mediation":true,
 "usage_reporting":true,"status":"native"}
```

### `Approval`
```json
{"schema":"rover/v1alpha1","id":"...","investigation_id":"...",
 "candidate":"...","config_digest":"...","authority":"same-user local review"}
```

## Terminal states

### `Terminal(s string) bool`
Returns true for: `CANDIDATE_READY`, `REVIEW_READY`, `CHECKS_BLOCKED`,
`FAILED`, `ERROR`, `CANCELLED`, `TIMED_OUT`, `LOST`.
