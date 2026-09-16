"""Small standard-library client for Rover's CLI (v0.x API).
No shell, automatic retries, provider credentials, or acceptance inference.
A CLI submission may continue after a client timeout; reconcile before retrying.
"""
from __future__ import annotations
from dataclasses import dataclass
import json
import os
from pathlib import Path
import subprocess
import tempfile
from typing import Any, Sequence

class RoverError(RuntimeError):
    """Protocol, invocation or bounded-output failure."""

@dataclass(frozen=True)
class Result:
    exit_code: int
    data: Any

    @property
    def decision(self) -> str | None:
        return self.data.get("decision") if isinstance(self.data, dict) else None

class Rover:
    def __init__(self, binary: str | os.PathLike[str], state: str | os.PathLike[str], *, timeout: float = 60, max_output: int = 16 << 20):
        self.binary = str(Path(binary).resolve())
        self.state = str(Path(state).resolve())
        if timeout <= 0 or max_output < 1024:
            raise ValueError("positive timeout and output bound required")
        self.timeout = timeout
        self.max_output = max_output

    def call(self, args: Sequence[str]) -> Result:
        if isinstance(args, str) or any(not isinstance(x, str) for x in args):
            raise TypeError("args must be an argv sequence, not a shell string")
        argv = [self.binary, "--state", self.state, *args, "--json"]
        # Avoid accumulating unbounded pipe output in memory. Rover's own output
        # limits remain relevant; operating-system disk quotas are still needed.
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            try:
                proc = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, timeout=self.timeout, check=False)
            except subprocess.TimeoutExpired as exc:
                raise RoverError("Client timed out. An admitted task may still be running; inspect state before retrying.") from exc
            stdout.seek(0); raw = stdout.read(self.max_output + 1)
            stderr.seek(0); err = stderr.read(65536).decode("utf-8", "replace")
            if len(raw) > self.max_output:
                raise RoverError("Rover output exceeded configured client limit")
            try:
                data = json.loads(raw)
            except (ValueError, UnicodeError) as exc:
                raise RoverError(f"Rover did not return JSON (exit {proc.returncode}): {err}") from exc
            return Result(proc.returncode, data)

    def inspect(self, repo: str, *, base: str = "HEAD", worktree: bool = False) -> Result:
        return self.call(["inspect", "--repo", repo, "--base", base] + (["--worktree"] if worktree else []))

    def verify(self, repo: str, *, base: str = "HEAD", allow_local: bool = False, worktree: bool = False) -> Result:
        return self.call(["verify", "--repo", repo, "--base", base] + (["--allow-local"] if allow_local else []) + (["--worktree"] if worktree else []))

    def run_task(self, task_file: str, *, allow_local: bool = False, key: str | None = None) -> Result:
        argv = ["task", "run", "--file", task_file]
        if allow_local: argv.append("--allow-local")
        if key is not None: argv += ["--key", key]
        return self.call(argv)

    def status(self, task_id: str | None = None) -> Result:
        return self.call(["status"] + (["--id", task_id] if task_id else []))

    def report(self, investigation_id: str) -> Result:
        return self.call(["report", "--id", investigation_id])
