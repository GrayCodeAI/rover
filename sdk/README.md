# Rover SDKs — monorepo

All three clients are thin argv wrappers around your explicit `bin/rover --state <private-dir> --json`. No shell, no auto-retry, no provider credentials. `task run` exit 0 = admitted, not accepted.

| SDK | Path | Install | Test |
|-----|------|---------|------|
| Python | `sdk/python/` | `pip install rover-client` (after publish) or copy `rover_client.py` | `python3 -m unittest discover -s sdk/python -p 'test_*.py'` |
| TypeScript | `sdk/typescript/` | `npm install @graycodeai/rover` (after publish) | `node --test sdk/typescript/test/*.test.ts` |
| Go | `sdk/go/` | `go get github.com/GrayCodeAI/rover/sdk/go` | `go test ./sdk/go -count=1` |

Or run all: `make sdk-test`.

Mirrors: same `Rover` surface — `inspect` / `verify` / `runTask` / `status` / `report` + bounded JSON + explicit timeout. No telemetry or auto-publish.
