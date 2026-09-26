#!/usr/bin/env python3
"""Check the locked Rust dependency licenses/notices and emit CycloneDX SBOM."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib
from collections import defaultdict
from urllib.parse import quote

ROOT = pathlib.Path(__file__).resolve().parents[1]
NOTICE = ROOT / "licenses" / "THIRD_PARTY_NOTICES.md"
LOCKFILE = ROOT / "Cargo.lock"
LICENSE_FILENAMES = (
    "LICENSE",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "LICENSE-UNICODE",
    "LICENSE-CC0",
    "LICENSE-BOOST",
    "LICENSE-Apache-2.0_WITH_LLVM-exception",
    "LICENSE.txt",
    "LICENSE.md",
    "license-apache-2.0",
    "license-mit",
    "COPYING",
    "COPYING-MIT",
    "COPYING-APACHE",
    "COPYRIGHT",
    "AUTHORS",
    "UNLICENSE",
)
LICENSE_FILENAMES_CASEFOLD = {filename.casefold() for filename in LICENSE_FILENAMES}
ALLOWED_LICENSE_EXPRESSIONS = {
    "MIT",
    "Apache-2.0",
    "MIT OR Apache-2.0",
    "Apache-2.0 OR MIT",
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
    "(MIT OR Apache-2.0) AND Unicode-3.0",
    "MIT/Apache-2.0",
    "Apache-2.0/MIT",
    "Unlicense OR MIT",
    "Apache-2.0 WITH LLVM-exception",
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
    "BSD-2-Clause OR Apache-2.0",
    "MIT OR Apache-2.0 OR Zlib",
    "MIT OR Apache-2.0 OR CC0-1.0",
    "Zlib",
    "Apache-2.0 OR BSL-1.0",
}
LICENSE_EXPRESSION_NORMALIZATION = {
    "MIT/Apache-2.0": "MIT OR Apache-2.0",
    "Apache-2.0/MIT": "Apache-2.0 OR MIT",
}
SUPPORTED_TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
)
LOCK_REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
NOTICE_ROW = re.compile(r"^\| `([^`]+)` \| ([^|]+) \| ([^|]+) \| (.+) \|$", re.M)
NOTICE_FILE = re.compile(r"\[`([^`]+)`\]\(([^)]+)\) `([0-9a-f]{64})`")
BUNDLED_SQLITE_ROW = re.compile(
    r"^\| `SQLite ([^`]+)` \| `libsqlite3-sys ([^`]+)` \| public-domain blessing \| "
    r"\[`SQLITE_BLESSING\.txt`\]\(([^)]+)\) `([0-9a-f]{64})` \|$",
    re.M,
)
SQLITE_SOURCE_PATH = pathlib.Path("sqlite3") / "sqlite3.c"
TREE_LINE = re.compile(r"^(\d+)([A-Za-z0-9_.+-]+) v([^\s]+)(?:\s+.*)?$")


class AuditError(Exception):
    """A lockfile, license, or notice integrity failure."""


def recognized_license_files(source_dir: pathlib.Path) -> dict[str, str]:
    """Map stable bundled names to actual source names without case aliases."""
    return {
        bundle_license_filename(path.name): path.name
        for path in source_dir.iterdir()
        if path.is_file() and path.name.casefold() in LICENSE_FILENAMES_CASEFOLD
    }


def license_source(package: dict, packages: dict) -> tuple[pathlib.Path, dict[str, str]]:
    """Find exact license files, including omitted files shared by one upstream repository."""
    source_dir = pathlib.Path(package["manifest_path"]).parent
    files = recognized_license_files(source_dir)
    if files:
        return source_dir, files

    repository = package.get("repository")
    expression = canonical_license_expression(package.get("license"))
    candidates = sorted(
        (
            candidate
            for candidate in packages.values()
            if candidate is not package
            and repository
            and candidate.get("repository") == repository
            and canonical_license_expression(candidate.get("license")) == expression
        ),
        key=lambda candidate: (candidate["name"], candidate["version"]),
    )
    for candidate in candidates:
        candidate_dir = pathlib.Path(candidate["manifest_path"]).parent
        files = recognized_license_files(candidate_dir)
        if files:
            return candidate_dir, files
    return source_dir, {}


def bundle_license_filename(source_filename: str) -> str:
    """Use stable bundle casing while retaining source spelling for byte reads."""
    return {
        "license-mit": "LICENSE-MIT",
        "license-apache": "LICENSE-APACHE",
        "license-apache-2.0": "LICENSE-APACHE-2.0",
        "license-apache-2.0_with_llvm-exception": "LICENSE-APACHE-2.0_WITH_LLVM-exception",
        "license-unicode": "LICENSE-UNICODE",
        "license-cc0": "LICENSE-CC0",
        "license-boost": "LICENSE-BOOST",
        "license.txt": "LICENSE.txt",
        "license.md": "LICENSE.md",
        "unlicense": "UNLICENSE",
    }.get(source_filename.casefold(), source_filename)


def package_key(package: dict) -> tuple[str, str]:
    return package["name"], package["version"]


def package_purl(name: str, version: str) -> str:
    return f"pkg:cargo/{quote(name, safe='-._~')}@{quote(version, safe='-._~')}"


def canonical_license_expression(expression: str) -> str:
    if expression not in ALLOWED_LICENSE_EXPRESSIONS:
        raise AuditError(f"unreviewed SPDX license expression: {expression}")
    return LICENSE_EXPRESSION_NORMALIZATION.get(expression, expression)


def sqlite_bundled_notice(package: dict) -> tuple[str, bytes, bytes]:
    source_dir = pathlib.Path(package["manifest_path"]).parent
    sqlite_source = source_dir / SQLITE_SOURCE_PATH
    source_bytes = sqlite_source.read_bytes()
    version_match = re.search(rb'^#define SQLITE_VERSION\s+"([^"\r\n]+)"', source_bytes, re.M)
    if not version_match:
        raise AuditError(f"SQLite version not found in bundled source: {sqlite_source}")
    lines = source_bytes.splitlines(keepends=True)
    start = next(
        (index for index, line in enumerate(lines) if b"The author disclaims copyright to this source code." in line),
        None,
    )
    if start is None:
        raise AuditError(f"SQLite public-domain blessing not found in {sqlite_source}")
    end = next(
        (index for index in range(start + 1, len(lines)) if lines[index].startswith(b"********")),
        None,
    )
    if end is None:
        raise AuditError(f"SQLite public-domain blessing terminator not found in {sqlite_source}")
    notice = b"".join(lines[start:end])
    return version_match.group(1).decode("ascii"), source_bytes, notice


def registry_packages(metadata: dict) -> dict[tuple[str, str], dict]:
    packages = {}
    for package in metadata["packages"]:
        source = package.get("source")
        if source is None:
            continue
        if source != LOCK_REGISTRY:
            raise AuditError(f"unreviewed Rust package source: {package['name']} {source}")
        key = package_key(package)
        if key in packages:
            raise AuditError(f"duplicate registry package in metadata: {key}")
        packages[key] = package
    return packages


def read_notice_rows(text: str) -> dict[tuple[str, str], tuple[str, list[tuple[str, str, str]]]]:
    rows = {}
    package_table = text.partition("## Bundled third-party source notice")[0]
    for match in NOTICE_ROW.finditer(package_table):
        name, version, license_expression, file_column = match.groups()
        key = (name, version.strip())
        if key in rows:
            raise AuditError(f"duplicate third-party notice row: {key}")
        files = NOTICE_FILE.findall(file_column)
        rows[key] = (license_expression.strip(), files)
    return rows


def load_cargo_data(root: pathlib.Path = ROOT) -> tuple[dict, dict]:
    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
    ]
    try:
        result = subprocess.run(command, cwd=root, text=True, capture_output=True, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", "") or str(error)
        raise AuditError(f"Cargo metadata failed: {detail.strip()}") from error
    return json.loads(result.stdout), tomllib.loads((root / "Cargo.lock").read_text())


def check_notices(metadata: dict, lock: dict, root: pathlib.Path = ROOT) -> dict:
    packages = registry_packages(metadata)
    lock_registry = {
        package_key(package): package
        for package in lock["package"]
        if package.get("source") == LOCK_REGISTRY
    }
    if set(packages) != set(lock_registry):
        missing = sorted(set(lock_registry) - set(packages))
        extra = sorted(set(packages) - set(lock_registry))
        raise AuditError(f"Cargo.lock/metadata package mismatch; missing={missing}, extra={extra}")

    rows = read_notice_rows((root / "licenses" / "THIRD_PARTY_NOTICES.md").read_text())
    if set(rows) != set(packages):
        missing = sorted(set(packages) - set(rows))
        extra = sorted(set(rows) - set(packages))
        raise AuditError(f"notice package mismatch; missing={missing}, extra={extra}")

    expected_bundle_files = set()
    for key, package in packages.items():
        name, version = key
        license_expression = package.get("license")
        if not license_expression:
            raise AuditError(f"missing SPDX license expression: {name} {version}")
        try:
            canonical_expression = canonical_license_expression(license_expression)
        except AuditError as error:
            raise AuditError(
                f"unreviewed SPDX license expression for {name} {version}: {license_expression}"
            ) from error
        notice_expression, files = rows[key]
        if notice_expression != canonical_expression:
            raise AuditError(f"notice license mismatch for {name} {version}")

        source_dir, source_files = license_source(package, packages)
        noticed_files = {filename for filename, _, _ in files}
        if not source_files or noticed_files != set(source_files):
            raise AuditError(
                f"license/notice file mismatch for {name} {version}: "
                f"source={sorted(source_files)}, bundle={sorted(noticed_files)}"
            )

        for filename, relative_path, recorded_hash in files:
            source_filename = source_files[filename]
            expected_relative = pathlib.Path("third-party") / f"{name}-{version}" / filename
            if pathlib.Path(relative_path) != expected_relative:
                raise AuditError(f"unexpected notice path for {name} {version}: {relative_path}")
            source_bytes = (source_dir / source_filename).read_bytes()
            bundled_path = root / "licenses" / expected_relative
            if not bundled_path.is_file():
                raise AuditError(f"missing bundled license/notice: {bundled_path}")
            bundled_bytes = bundled_path.read_bytes()
            actual_hash = hashlib.sha256(source_bytes).hexdigest()
            if actual_hash != recorded_hash or bundled_bytes != source_bytes:
                raise AuditError(f"license/notice bytes or SHA-256 differ: {name} {version}/{filename}")
            expected_bundle_files.add(bundled_path.resolve())

    sqlite_packages = [
        package
        for key, package in packages.items()
        if key[0] == "libsqlite3-sys" and key[1] == "0.38.2"
    ]
    if sqlite_packages:
        if len(sqlite_packages) != 1:
            raise AuditError("unexpected duplicate bundled SQLite package")
        sqlite_version, _, sqlite_notice = sqlite_bundled_notice(sqlite_packages[0])
        bundled_path = pathlib.Path("third-party") / "libsqlite3-sys-0.38.2" / "SQLITE_BLESSING.txt"
        bundled_match = BUNDLED_SQLITE_ROW.search((root / "licenses" / "THIRD_PARTY_NOTICES.md").read_text())
        if not bundled_match:
            raise AuditError("missing bundled SQLite public-domain notice row")
        row_version, row_crate_version, relative_path, recorded_hash = bundled_match.groups()
        expected_hash = hashlib.sha256(sqlite_notice).hexdigest()
        output_path = root / "licenses" / bundled_path
        if (
            row_version != sqlite_version
            or row_crate_version != "0.38.2"
            or pathlib.Path(relative_path) != bundled_path
            or recorded_hash != expected_hash
            or not output_path.is_file()
            or output_path.read_bytes() != sqlite_notice
        ):
            raise AuditError("bundled SQLite public-domain notice or source version differs")
        expected_bundle_files.add(output_path.resolve())
    elif BUNDLED_SQLITE_ROW.search((root / "licenses" / "THIRD_PARTY_NOTICES.md").read_text()):
        raise AuditError("stale bundled SQLite notice row without locked libsqlite3-sys")

    actual_bundle_files = {
        path.resolve()
        for path in (root / "licenses" / "third-party").rglob("*")
        if path.is_file()
    }
    if actual_bundle_files != expected_bundle_files:
        stale = sorted(str(path.relative_to(root)) for path in actual_bundle_files - expected_bundle_files)
        missing = sorted(str(path.relative_to(root)) for path in expected_bundle_files - actual_bundle_files)
        raise AuditError(f"third-party notice file set differs; stale={stale}, missing={missing}")
    return packages


def refresh_notice_bundle(metadata: dict, root: pathlib.Path = ROOT) -> None:
    """Copy the exact locked license files and render their inventory table."""
    packages = registry_packages(metadata)
    third_party_root = root / "licenses" / "third-party"
    if third_party_root.exists():
        for path in sorted(third_party_root.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
            elif path.is_dir():
                try:
                    path.rmdir()
                except OSError:
                    pass
    expected_files = set()
    rows = [
        "# Third party notices",
        "",
        "This inventory covers all registry packages in `Cargo.lock`, including optional and target-specific packages.",
        "Legacy slash-separated license alternatives are normalized to SPDX `OR` expressions.",
        "License and author files are copied from the resolved crate sources and checked byte-for-byte; when a target-specific package omits them, an identical SPDX declaration from the same upstream repository supplies the files.",
        "",
        "| Package | Version | SPDX license expression | Included license/author files (SHA-256) |",
        "|---|---:|---|---|",
    ]

    for key, package in sorted(packages.items()):
        name, version = key
        expression = canonical_license_expression(package.get("license"))
        source_dir, source_files = license_source(package, packages)
        if not source_files:
            raise AuditError(f"no recognized license/notice files for {name} {version}")
        file_links = []
        for filename, source_filename in sorted(source_files.items()):
            relative = pathlib.Path("third-party") / f"{name}-{version}" / filename
            output = root / "licenses" / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            data = (source_dir / source_filename).read_bytes()
            output.write_bytes(data)
            expected_files.add(output.resolve())
            file_links.append(
                f"[`{filename}`]({relative.as_posix()}) `{hashlib.sha256(data).hexdigest()}`"
            )
        rows.append(
            f"| `{name}` | {version} | {expression} | "
            + "<br>".join(file_links)
            + " |"
        )

    sqlite_package = packages.get(("libsqlite3-sys", "0.38.2"))
    if sqlite_package:
        sqlite_version, _, sqlite_notice = sqlite_bundled_notice(sqlite_package)
        relative = pathlib.Path("third-party/libsqlite3-sys-0.38.2/SQLITE_BLESSING.txt")
        output = root / "licenses" / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(sqlite_notice)
        expected_files.add(output.resolve())
        rows.extend(
            [
                "",
                "## Bundled third-party source notice",
                "",
                "The `bundled` rusqlite feature compiles this SQLite amalgamation into Rover. The exact public-domain blessing block is retained separately from the libsqlite3-sys MIT license.",
                "",
                "| Source component | Bundling crate | License notice | Included notice (SHA-256) |",
                "|---|---|---|---|",
                f"| `SQLite {sqlite_version}` | `libsqlite3-sys 0.38.2` | public-domain blessing | [`SQLITE_BLESSING.txt`]({relative.as_posix()}) `{hashlib.sha256(sqlite_notice).hexdigest()}` |",
            ]
        )

    if third_party_root.exists():
        for path in sorted(third_party_root.rglob("*"), reverse=True):
            if path.is_file() and path.resolve() not in expected_files:
                path.unlink()
            elif path.is_dir():
                try:
                    path.rmdir()
                except OSError:
                    pass
    (root / "licenses" / "THIRD_PARTY_NOTICES.md").write_text("\n".join(rows) + "\n")


def cargo_tree_targets(
    root: pathlib.Path = ROOT,
) -> tuple[dict[str, set[tuple[str, str]]], dict[str, set[tuple[tuple[str, str], tuple[str, str]]]]]:
    active_by_target = {}
    edges_by_target = {}
    for target in SUPPORTED_TARGETS:
        command = [
            "cargo",
            "tree",
            "--locked",
            "--offline",
            "--target",
            target,
            "--prefix",
            "depth",
            "-e",
            "normal,build",
        ]
        try:
            result = subprocess.run(command, cwd=root, text=True, capture_output=True, check=True)
        except (OSError, subprocess.CalledProcessError) as error:
            detail = getattr(error, "stderr", "") or str(error)
            raise AuditError(f"Cargo tree failed for {target}: {detail.strip()}") from error
        found = set()
        edges = set()
        stack = []
        for line in result.stdout.splitlines():
            match = TREE_LINE.match(line)
            if match:
                depth = int(match.group(1))
                package = (match.group(2), match.group(3))
                found.add(package)
                if depth and len(stack) >= depth:
                    edges.add((stack[depth - 1], package))
                stack = stack[:depth]
                stack.append(package)
        if not found:
            raise AuditError(f"Cargo tree returned no packages for target {target}")
        active_by_target[target] = found
        edges_by_target[target] = edges
    return active_by_target, edges_by_target


def generate_sbom(
    metadata: dict,
    lock: dict,
    packages: dict,
    active_by_target: dict[str, set[tuple[str, str]]],
    edges_by_target: dict[str, set[tuple[tuple[str, str], tuple[str, str]]]],
    root: pathlib.Path = ROOT,
) -> dict:
    lock_packages = {package_key(package): package for package in lock["package"]}
    components = []
    package_refs = {}

    workspace_packages = [
        package
        for package in metadata["packages"]
        if package["id"] in metadata["workspace_members"]
    ]
    for package in workspace_packages:
        key = package_key(package)
        license_expression = package.get("license")
        try:
            license_expression = canonical_license_expression(license_expression)
        except AuditError as error:
            raise AuditError(
                f"unreviewed SPDX license expression for workspace package {key}: {license_expression}"
            ) from error
        package_refs[key] = package_purl(*key)
        components.append(
            {
                "type": "library",
                "bom-ref": package_purl(*key),
                "name": key[0],
                "version": key[1],
                "purl": package_purl(*key),
                "scope": "required",
                "licenses": [{"license": {"id": license_expression, "acknowledgement": "declared"}}],
                "externalReferences": [
                    {"type": "vcs", "url": "https://github.com/GrayCodeAI/rover"}
                ],
            }
        )

    for key, package in sorted(packages.items()):
        locked = lock_packages[key]
        if not locked.get("checksum"):
            raise AuditError(f"registry crate lacks Cargo.lock checksum: {key}")
        active_targets = sorted(target for target, active in active_by_target.items() if key in active)
        package_refs[key] = package_purl(*key)
        component = {
            "type": "library",
            "bom-ref": package_purl(*key),
            "name": key[0],
            "version": key[1],
            "purl": package_purl(*key),
            "scope": "required" if active_targets else "optional",
            "licenses": [
                {
                    "expression": canonical_license_expression(package["license"]),
                    "acknowledgement": "declared",
                }
            ],
            "hashes": [{"alg": "SHA-256", "content": locked["checksum"]}],
            "externalReferences": [
                {"type": "distribution", "url": f"https://crates.io/crates/{key[0]}/{key[1]}"}
            ],
        }
        if active_targets:
            component["properties"] = [
                {"name": "rover:rust:targets", "value": ",".join(active_targets)}
            ]
        else:
            component["properties"] = [
                {"name": "rover:rust:locked-but-not-active", "value": "true"}
            ]
        components.append(component)

    sqlite_package = packages.get(("libsqlite3-sys", "0.38.2"))
    sqlite_identity = None
    if sqlite_package:
        sqlite_version, sqlite_source, _ = sqlite_bundled_notice(sqlite_package)
        sqlite_identity = ("sqlite-amalgamation", sqlite_version)
        sqlite_ref = f"pkg:generic/sqlite@{quote(sqlite_version, safe='-._~')}"
        package_refs[sqlite_identity] = sqlite_ref
        components.append(
            {
                "type": "library",
                "bom-ref": sqlite_ref,
                "name": "SQLite amalgamation",
                "version": sqlite_version,
                "purl": sqlite_ref,
                "scope": "required",
                "licenses": [
                    {"license": {"name": "SQLite public-domain blessing"}}
                ],
                "hashes": [
                    {
                        "alg": "SHA-256",
                        "content": hashlib.sha256(sqlite_source).hexdigest(),
                    }
                ],
                "externalReferences": [
                    {"type": "website", "url": "https://www.sqlite.org/"}
                ],
                "properties": [
                    {
                        "name": "rover:bundled-by",
                        "value": "libsqlite3-sys 0.38.2 bundled feature",
                    }
                ],
            }
        )

    all_edges = set().union(*edges_by_target.values())
    dependencies_by_package: dict[tuple[str, str], set[tuple[str, str]]] = defaultdict(set)
    for parent, child in all_edges:
        dependencies_by_package[parent].add(child)
    if sqlite_identity:
        dependencies_by_package[("libsqlite3-sys", "0.38.2")].add(sqlite_identity)
    dependency_items = []
    for package_key_value, ref in sorted(package_refs.items()):
        dependencies = sorted(
            package_refs[child]
            for child in dependencies_by_package.get(package_key_value, set())
            if child in package_refs
        )
        dependency_items.append({"ref": ref, "dependsOn": dependencies})

    # Cargo metadata describes all lockfile-resolved edges, including optional
    # and target-specific dependencies. The scoped component inventory is exact;
    # the graph records Cargo's resolved lockfile relationships.
    version = (root / "VERSION").read_text().strip()
    root_ref = f"pkg:generic/rover@{quote(version, safe='-._~')}"
    root_component = {
        "type": "application",
        "bom-ref": root_ref,
        "name": "rover",
        "version": version,
        "purl": root_ref,
        "licenses": [{"license": {"id": "MIT", "acknowledgement": "declared"}}],
    }
    dependency_items.append(
        {
            "ref": root_ref,
            "dependsOn": sorted(
                package_purl(package["name"], package["version"])
                for package in workspace_packages
            ),
        }
    )

    bom = {
        "$schema": "https://cyclonedx.org/schema/bom-1.7.schema.json",
        "bomFormat": "CycloneDX",
        "specVersion": "1.7",
        "version": 1,
        "metadata": {"component": root_component},
        "components": components,
        "dependencies": dependency_items,
    }
    return bom


def run_check(root: pathlib.Path = ROOT) -> tuple[dict, dict, dict]:
    metadata, lock = load_cargo_data(root)
    packages = check_notices(metadata, lock, root)
    active, edges = cargo_tree_targets(root)
    return metadata, lock, generate_sbom(metadata, lock, packages, active, edges, root)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify lockfile license metadata and bundled notices")
    parser.add_argument("--refresh-notices", action="store_true", help="copy exact locked license notices into licenses/third-party")
    parser.add_argument("--sbom", type=pathlib.Path, help="write a CycloneDX 1.7 JSON SBOM at this path")
    arguments = parser.parse_args(argv)
    try:
        if arguments.refresh_notices:
            metadata, _ = load_cargo_data()
            refresh_notice_bundle(metadata)
        _, _, bom = run_check()
        if arguments.sbom:
            arguments.sbom.parent.mkdir(parents=True, exist_ok=True)
            arguments.sbom.write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n")
            print(f"Wrote {len(bom['components'])} Rust SBOM components to {arguments.sbom}")
        else:
            print("Rust dependency licenses and bundled notices match Cargo.lock.")
        return 0
    except (AuditError, OSError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"Rust dependency audit failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
