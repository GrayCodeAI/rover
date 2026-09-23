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
            if not target.is_file() or target.read_bytes() != expected:
                print("SOURCE_MANIFEST.json is stale; run make manifest", file=sys.stderr)
                return 1
            return 0
        write_manifest(root, expected)
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"source manifest: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
