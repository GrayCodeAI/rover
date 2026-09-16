# Go CLI client

`rover_client.go` requires only Go's standard library. It invokes your explicitly
selected Rover binary with argv, never a shell. No external dependencies.

```go
import rover "github.com/GrayCodeAI/rover/sdk/go"

client, _ := rover.New("/absolute/path/to/rover", "/absolute/path/to/private-state")
result, _ := client.Status()
fmt.Println(result.ExitCode, result.Data)
```

Task submission exit zero means admitted, not accepted software. Verification
codes 1/2/3 remain ordinary structured `Result`s rather than being hidden by an
error. A client timeout may leave an admitted detached task running. Reconcile
before retrying. External installations and credentials are never automatic.

Run tests: `go test ./sdk/go -count=1`

Mirrors `sdk/python/rover_client.py` and `sdk/typescript/src/rover_client.ts` — same argv surface, bounds, and error semantics.
