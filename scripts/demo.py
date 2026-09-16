#!/usr/bin/env python3
"""Exercise the compiled CLI, real Git worktrees, detached supervisors and JUnit.
No model accounts or remote services are used. Only an intentionally defective,
owned fixture is executed. Requires Python 3, Git and a compiled Rover binary.
"""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from datetime import datetime, timezone

TERMINAL = {"CANDIDATE_READY", "REVIEW_READY", "CHECKS_BLOCKED", "FAILED", "ERROR", "CANCELLED", "TIMED_OUT", "LOST"}

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="bin/rover")
    parser.add_argument("--report", help="Write the observed smoke-test results here")
    parser.add_argument("--keep", action="store_true", help="Retain the temporary demo workspace")
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    if not binary.is_file():
        raise SystemExit("Build Rover first: make build")
    project = Path(__file__).resolve().parents[1]
    root = Path(os.path.realpath(tempfile.mkdtemp(prefix="rover-demo-")))
    repo, state = root / "repo", root / "state"
    shutil.copytree(project / "examples" / "fixture", repo)
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_AUTHOR_NAME="Rover Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
               GIT_COMMITTER_NAME="Rover Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid")
    results: list[dict] = []
    active_tasks: list[str] = []
    def git(*argv: str) -> None:
        subprocess.run(["git", "-c", "core.hooksPath=" + os.devnull, "-c", "commit.gpgsign=false", "-C", str(repo), *argv], env=env, check=True, capture_output=True, text=True)
    def rover(*argv: str, codes=(0,)) -> dict | list:
        run = subprocess.run([str(binary), "--state", str(state), *argv, "--json"], env=env, capture_output=True, text=True, timeout=30)
        if run.returncode not in codes:
            raise AssertionError(f"{argv}: exit={run.returncode}\n{run.stdout}\n{run.stderr}")
        try:
            return json.loads(run.stdout)
        except json.JSONDecodeError as exc:
            raise AssertionError(f"Invalid JSON: {run.stdout!r}") from exc
    def passed(name: str, detail: str) -> None:
        results.append({"scenario": name, "status": "passed", "detail": detail})
        print("PASS", name)
    def wait(task_id: str, timeout=20) -> dict:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            task = rover("status", "--id", task_id)
            if task["status"] in TERMINAL:
                return task
            time.sleep(0.05)
        raise AssertionError(f"Task {task_id} did not reach a terminal state")
    try:
        git("init", "-b", "main"); git("add", "-A"); git("commit", "-m", "intentional fixture defect")
        doctor = rover("doctor")
        assert doctor["tools"]["git"]["available"]
        passed("doctor", "Tool discovery executes no repository code")
        before = (repo / "AGENTS.md").read_bytes()
        preview = rover("init", "--repo", str(repo))
        assert not preview["written"] and (repo / "AGENTS.md").read_bytes() == before
        passed("setup_preview", "Existing instructions and config preserved")
        denied = rover("verify", "--repo", str(repo), codes=(2,))
        assert "allow-local" in denied["error"]
        passed("explicit_execution_grant", "Verification refused without --allow-local")
        broken = rover("verify", "--repo", str(repo), "--allow-local", codes=(1,))
        assert broken["decision"] == "BLOCKED" and broken["checks"][0]["tests"] == 3
        passed("broken_baseline", "Real JUnit test failures block the candidate")
        spec = {"schema":"rover/v1alpha1", "objective":"Repair the owned fixture without weakening tests", "repository":str(repo), "base":"HEAD", "argv":["python3", "tools/fixture_agent.py"], "timeout":"30s", "auto_verify":True}
        task_path = root / "task.json"; task_path.write_text(json.dumps(spec))
        launched = rover("task", "run", "--file", str(task_path), "--allow-local", "--key", "demo-fix")
        task_id = launched["id"]; active_tasks.append(task_id)
        # The launching CLI process has exited; the supervisor keeps working.
        task = wait(task_id)
        assert task["status"] == "REVIEW_READY", task
        passed("detached_execution", "Task completed after the launching CLI exited")
        assert "return True" in (repo / "app.py").read_text()
        passed("main_checkout_preserved", "Agent edits exist only in its detached Git worktree")
        report = rover("report", "--id", task["investigation_id"])
        assert report["candidate"] == task["candidate"] and report["checks"][0]["outcome"] == "PASS"
        assert report["decision"] == "REVIEW_REQUIRED"
        passed("candidate_bound_evidence", "Three passing tests tied to the exact resulting snapshot")
        approval = rover("review", "--id", report["id"], "--note", "Fixture reviewed locally")
        assert approval["candidate"] == report["candidate"]
        assert rover("report", "--id", report["id"])["decision"] == "REVIEW_REQUIRED"
        passed("review_is_not_fabricated_pass", "Local review is recorded without rewriting verification")
        again = rover("task", "run", "--file", str(task_path), "--allow-local", "--key", "demo-fix")
        assert again["id"] == task_id
        passed("idempotent_dispatch", "Same key and contract return the existing run")
        logs = rover("logs", "--id", task_id)
        assert "claims" in logs["text"]
        passed("log_reattachment", "A new CLI reads the completed supervisor-owned output")
        # Try weakening candidate configuration. Default verification must retain
        # the approved baseline configuration rather than use these new checks.
        malicious = {"schema":"rover/v1alpha1", "checks":[{"id":"fake", "argv":["true"], "timeout":"1s", "required":True, "parser":"exit-code"}], "policy":{"require_review":False}}
        (repo / ".rover/config.json").write_text(json.dumps(malicious))
        protected = rover("verify", "--repo", str(repo), "--worktree", "--allow-local", codes=(1,))
        assert protected["decision"] == "BLOCKED" and protected["checks"][0]["id"] == "authorization-tests"
        passed("baseline_config_preserved", "Candidate cannot silently replace the chosen baseline check plan")
        cancel_spec = dict(spec, objective="Cancellation fixture", argv=["python3", "-c", "import time; print('waiting',flush=True); time.sleep(20)"], auto_verify=False)
        task_path.write_text(json.dumps(cancel_spec))
        launched = rover("task", "run", "--file", str(task_path), "--allow-local", "--key", "demo-cancel")
        cid = launched["id"]; active_tasks.append(cid)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if rover("status", "--id", cid)["status"] == "RUNNING":
                break
            time.sleep(0.05)
        rover("cancel", "--id", cid)
        cancelled = wait(cid)
        assert cancelled["status"] == "CANCELLED", cancelled
        passed("detached_cancellation", "Cancellation request terminates the owned command group")
        summary = {"schema":"rover.smoke/v1alpha1", "observed_at":datetime.now(timezone.utc).isoformat(), "status":"passed", "scenario_count":len(results), "scenarios":results, "agent":"deterministic fixture, not a live coding-model provider", "docker_live_tested":False, "demo_directory":str(root) if args.keep else "temporary fixture removed", "fixed_candidate_report":report}
        if args.report:
            dest = Path(args.report); dest.parent.mkdir(parents=True, exist_ok=True); dest.write_text(json.dumps(summary, indent=2) + "\n")
        print(f"\n{len(results)} smoke scenarios passed. No model or network services were used.")
        if args.keep:
            print(f"Retained fixture: {root}")
    finally:
        for tid in active_tasks:
            try:
                task = rover("status", "--id", tid)
                if task["status"] not in TERMINAL:
                    rover("cancel", "--id", tid)
                    wait(tid, 5)
            except Exception:
                pass  # Preserve the original test exception.
        if not args.keep:
            shutil.rmtree(root, ignore_errors=True)

if __name__ == "__main__":
    main()
