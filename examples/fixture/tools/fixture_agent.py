"""Deterministic fake agent for testing lifecycle, NOT an LLM integration."""
from pathlib import Path
import time
print("Fixture agent started; preparing a patch.", flush=True)
time.sleep(0.7)
p = Path("app.py")
before = p.read_text()
if "return True" not in before:
    raise SystemExit("Fixture input does not match the expected broken baseline")
p.write_text(before.replace("return True", 'return role == "admin"'))
print("Fixture agent claims the issue is fixed. Rover must verify independently.", flush=True)
