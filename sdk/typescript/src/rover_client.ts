/**
 * Small Node client for Rover's CLI (v0.x API).
 * No shell, automatic retries, provider credentials, or acceptance inference.
 * A CLI submission may continue after a client timeout; reconcile before retrying.
 */
import { spawnSync } from "node:child_process";
import * as path from "node:path";

export class RoverError extends Error {
  override name = "RoverError";
}

export interface Result {
  exitCode: number;
  data: unknown;
  /** convenience when data is {decision: string} */
  get decision(): string | null;
}

class ResultImpl implements Result {
  exitCode: number;
  data: unknown;
  constructor(exitCode: number, data: unknown) {
    this.exitCode = exitCode;
    this.data = data;
  }
  get decision(): string | null {
    if (dataIsRecord(this.data) && typeof this.data["decision"] === "string") return this.data["decision"] as string;
    return null;
  }
}

function dataIsRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

export class Rover {
  readonly binary: string;
  readonly state: string;
  readonly timeoutMs: number;
  readonly maxOutput: number;

  constructor(binary: string, state: string, opts: { timeoutMs?: number; maxOutput?: number } = {}) {
    this.binary = path.resolve(binary);
    this.state = path.resolve(state);
    this.timeoutMs = opts.timeoutMs ?? 60_000;
    this.maxOutput = opts.maxOutput ?? 16 * 1024 * 1024;
    if (this.timeoutMs <= 0 || this.maxOutput < 1024) throw new RangeError("positive timeout and output bound required");
  }

  call(args: string[]): Result {
    if (typeof args === "string" || !Array.isArray(args) || args.some((x) => typeof x !== "string")) {
      throw new TypeError("args must be an argv string[] , not a shell string");
    }
    const argv = [this.binary, "--state", this.state, ...args, "--json"];
    // spawnSync avoids shell; timeout is OS-level. Rover's own output limits remain relevant.
    const r = spawnSync(argv[0], argv.slice(1), {
      input: undefined,
      timeout: this.timeoutMs,
      maxBuffer: this.maxOutput + 1024,
      encoding: "buffer",
    } as const);

    if (r.error) {
      const e = r.error as NodeJS.ErrnoException;
      if (e.code === "ETIMEDOUT" || r.signal === "SIGTERM") {
        throw new RoverError("Client timed out. An admitted task may still be running; inspect state before retrying.");
      }
      throw new RoverError(String(e.message));
    }

    const stdout = r.stdout as Buffer | null;
    const stderr = r.stderr as Buffer | null;
    const raw = stdout ?? Buffer.alloc(0);
    const errText = (stderr ?? Buffer.alloc(0)).toString("utf8", 0, 65536).replace(/\0/g, "");

    if (raw.length > this.maxOutput) throw new RoverError("Rover output exceeded configured client limit");

    let data: unknown;
    try {
      data = JSON.parse(raw.toString("utf8"));
    } catch (e) {
      throw new RoverError(`Rover did not return JSON (exit ${r.status ?? -1}): ${errText.slice(0, 400)}`);
    }
    return new ResultImpl(r.status ?? 0, data);
  }

  inspect(repo: string, opts: { base?: string; worktree?: boolean } = {}): Result {
    const a = ["inspect", "--repo", repo, "--base", opts.base ?? "HEAD"];
    if (opts.worktree) a.push("--worktree");
    return this.call(a);
  }

  verify(repo: string, opts: { base?: string; allowLocal?: boolean; worktree?: boolean } = {}): Result {
    const a = ["verify", "--repo", repo, "--base", opts.base ?? "HEAD"];
    if (opts.allowLocal) a.push("--allow-local");
    if (opts.worktree) a.push("--worktree");
    return this.call(a);
  }

  runTask(taskFile: string, opts: { allowLocal?: boolean; key?: string } = {}): Result {
    const a = ["task", "run", "--file", taskFile];
    if (opts.allowLocal) a.push("--allow-local");
    if (opts.key) a.push("--key", opts.key);
    return this.call(a);
  }

  status(taskId?: string): Result {
    return this.call(taskId ? ["status", "--id", taskId] : ["status"]);
  }

  report(investigationId: string): Result {
    return this.call(["report", "--id", investigationId]);
  }
}
