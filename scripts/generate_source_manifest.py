import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

VERSION_PATTERN = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")


def read_version(root):
    raw = (root / "VERSION").read_text(encoding="utf-8").strip()
    if not VERSION_PATTERN.fullmatch(raw):
        raise ValueError("VERSION is not a valid release version")
    return raw


def tracked_files(root):
    raw = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
    paths = [
        p
        for p in raw.decode("utf-8").split("\0")
        if p
        and p != "SOURCE_MANIFEST.json"
        and not p.startswith("reports/")
        and not p.startswith("research_notes/")
    ]
    generator = "scripts/generate_source_manifest.py"
    if generator not in paths and (root / generator).is_file():
        paths.append(generator)
    return sorted(paths)


def file_entry(root, relative):
    path = root / relative
    info = os.lstat(path)
    if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
        raise ValueError(f"tracked path is not a regular file: {relative}")
    data = path.read_bytes()
    return {
        "path": relative,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size": len(data),
    }


def manifest_bytes(root):
    manifest = {
        "version": read_version(root),
        "scope": "Source package inventory; inventory itself excluded",
        "files": [file_entry(root, relative) for relative in tracked_files(root)],
    }
    return (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def manifest_differences(expected, actual, limit=20):
    """Describe how the committed manifest bytes differ from the regenerated ones."""
    try:
        want = json.loads(expected)
        have = json.loads(actual)
        want_files = {row["path"]: row for row in want["files"]}
        have_files = {row["path"]: row for row in have["files"]}
    except (ValueError, KeyError, TypeError):
        return ["SOURCE_MANIFEST.json is not a valid manifest"]
    lines = []
    if want.get("version") != have.get("version"):
        lines.append(f"version: {have.get('version')!r} -> {want.get('version')!r}")
    for path in sorted(want_files.keys() - have_files.keys()):
        lines.append(f"missing: {path}")
    for path in sorted(have_files.keys() - want_files.keys()):
        lines.append(f"no longer tracked: {path}")
    for path in sorted(want_files.keys() & have_files.keys()):
        if want_files[path] != have_files[path]:
            lines.append(f"changed: {path}")
    if not lines and expected != actual:
        lines.append("formatting differs")
    if len(lines) > limit:
        lines = lines[:limit] + [f"... and {len(lines) - limit} more"]
    return lines


def write_manifest(root, data):
    fd, temporary = tempfile.mkstemp(prefix=".SOURCE_MANIFEST.", dir=root)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, root / "SOURCE_MANIFEST.json")
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        expected = manifest_bytes(root)
        target = root / "SOURCE_MANIFEST.json"
        if args.check:
            if not target.is_file():
                print("SOURCE_MANIFEST.json is missing; run make manifest", file=sys.stderr)
                return 1
            actual = target.read_bytes()
            if actual != expected:
                print("SOURCE_MANIFEST.json is stale; run make manifest", file=sys.stderr)
                for line in manifest_differences(expected, actual):
                    print(f"  {line}", file=sys.stderr)
                return 1
            return 0
        write_manifest(root, expected)
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"source manifest: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
