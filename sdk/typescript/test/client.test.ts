import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { Rover, RoverError } from "../src/rover_client.ts";

describe("Rover client", () => {
  it("argv and failed decision are not hidden", () => {
    const d = fs.mkdtempSync(path.join(os.tmpdir(), "rover-ts-"));
    const fixture = path.join(d, "fixture");
    fs.writeFileSync(
      fixture,
      '#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps({"decision":"BLOCKED","argv":sys.argv[1:]}))\n',
      { mode: 0o700 },
    );
    const r = new Rover(fixture, path.join(d, "state")).call(["inspect", "--repo", "space ; $(not-a-shell)"]);
    assert.equal(r.exitCode, 0);
    assert.equal(r.decision, "BLOCKED");
    assert.ok(String(JSON.stringify((r.data as any).argv)).includes("space ; $(not-a-shell)"));
  });

  it("non-JSON is rejected", () => {
    const d = fs.mkdtempSync(path.join(os.tmpdir(), "rover-ts-"));
    const p = path.join(d, "fixture");
    fs.writeFileSync(p, "#!/bin/sh\nprintf not-json\n", { mode: 0o700 });
    assert.throws(() => new Rover(p, path.join(d, "state")).call(["status"]), RoverError);
  });

  it("no shell strings", () => {
    assert.throws(() => new Rover("/missing", "/tmp/missing-state").call("status; echo bad" as unknown as string[]), TypeError);
  });
});
