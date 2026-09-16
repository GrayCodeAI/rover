# TypeScript / Node CLI client

`src/rover_client.ts` requires only Node 18+ built-ins. It invokes your explicitly
selected Rover binary with argv, never a shell. No npm credential or provider magic.

```ts
import { Rover } from "./src/rover_client.ts";
const client = new Rover("/absolute/path/to/rover", "/absolute/path/to/private-state");
const result = client.status();
console.log(result.exitCode, result.data);
```

Task submission exit zero means admitted, not accepted software. Verification
codes 1/2/3 remain ordinary structured `Result`s rather than being hidden by an
exception. A client timeout may leave an admitted detached task running. Reconcile
before retrying. External installations and credentials are never automatic.

Run tests: `npm test` (uses `node --test`).

Mirrors `sdk/python/rover_client.py` — same argv surface, bounds, and error semantics.
