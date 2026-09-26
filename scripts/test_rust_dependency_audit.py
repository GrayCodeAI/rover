import hashlib
import pathlib
import tempfile
import unittest

from rust_dependency_audit import (
    AuditError,
    canonical_license_expression,
    check_notices,
    generate_sbom,
    license_source,
    package_purl,
    recognized_license_files,
    sqlite_bundled_notice,
)


class DependencyAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        (self.root / "VERSION").write_text("0.1.0\n")
        self.licenses = self.root / "licenses"
        self.licenses.mkdir()
        self.source = self.root / "registry" / "sample-1.2.3"
        self.source.mkdir(parents=True)
        self.license_text = b"Sample license notice\n"
        (self.source / "LICENSE-MIT").write_bytes(self.license_text)
        self.bundle_file = self.licenses / "third-party" / "sample-1.2.3" / "LICENSE-MIT"
        self.bundle_file.parent.mkdir(parents=True)
        self.bundle_file.write_bytes(self.license_text)
        self.license_hash = hashlib.sha256(self.license_text).hexdigest()
        (self.licenses / "third-party").mkdir(exist_ok=True)
        (self.licenses / "THIRD_PARTY_NOTICES.md").write_text(
            "| Package | Version | SPDX license expression | Included license/author files (SHA-256) |\n"
            "|---|---:|---|---|\n"
            "| `sample` | 1.2.3 | MIT OR Apache-2.0 | "
            f"[`LICENSE-MIT`](third-party/sample-1.2.3/LICENSE-MIT) `{self.license_hash}` |\n"
        )
        self.registry_package = {
            "name": "sample",
            "version": "1.2.3",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "license": "MIT OR Apache-2.0",
            "manifest_path": str(self.source / "Cargo.toml"),
        }
        (self.source / "Cargo.toml").write_text('[package]\nname="sample"\n')
        self.metadata = {"packages": [self.registry_package]}
        self.lock = {
            "package": [
                {
                    "name": "sample",
                    "version": "1.2.3",
                    "source": self.registry_package["source"],
                    "checksum": "a" * 64,
                }
            ]
        }

    def tearDown(self):
        self.temp.cleanup()

    def test_purl_encodes_package_identity(self):
        self.assertEqual(package_purl("sample+crate", "1.2.3"), "pkg:cargo/sample%2Bcrate@1.2.3")

    def test_license_file_inventory_preserves_actual_case_on_case_insensitive_filesystems(self):
        lowercase_source = self.root / "lowercase-license"
        lowercase_source.mkdir()
        (lowercase_source / "license-mit").write_text("MIT license\n")
        self.assertEqual(recognized_license_files(lowercase_source), {"LICENSE-MIT": "license-mit"})

    def test_markdown_license_files_are_included_in_the_notice_inventory(self):
        markdown_source = self.root / "markdown-license"
        markdown_source.mkdir()
        (markdown_source / "LICENSE.md").write_text("MIT license\n")
        self.assertEqual(
            recognized_license_files(markdown_source), {"LICENSE.md": "LICENSE.md"}
        )

    def test_cc0_and_boost_license_files_are_retained(self):
        source = self.root / "alternative-licenses"
        source.mkdir()
        (source / "license-cc0").write_text("CC0 license\n")
        (source / "LICENSE-BOOST").write_text("Boost license\n")
        self.assertEqual(
            recognized_license_files(source),
            {"LICENSE-CC0": "license-cc0", "LICENSE-BOOST": "LICENSE-BOOST"},
        )

    def test_omitted_target_license_uses_matching_files_from_the_same_repository(self):
        self.registry_package["repository"] = "https://example.test/upstream"
        target_source = self.root / "target-import"
        target_source.mkdir()
        target = {
            "name": "sample-target-import",
            "version": "0.1.0",
            "license": "MIT OR Apache-2.0",
            "repository": "https://example.test/upstream",
            "manifest_path": str(target_source / "Cargo.toml"),
        }
        source_dir, files = license_source(
            target, {("sample", "1.2.3"): self.registry_package, ("sample-target-import", "0.1.0"): target}
        )
        self.assertEqual(source_dir, self.source)
        self.assertEqual(files, {"LICENSE-MIT": "LICENSE-MIT"})

    def test_legacy_manifest_license_label_normalizes_to_spdx(self):
        self.assertEqual(canonical_license_expression("Apache-2.0"), "Apache-2.0")
        self.assertEqual(canonical_license_expression("MIT/Apache-2.0"), "MIT OR Apache-2.0")
        self.assertEqual(canonical_license_expression("Apache-2.0/MIT"), "Apache-2.0 OR MIT")
        self.assertEqual(
            canonical_license_expression("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
            "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
        )
        self.assertEqual(
            canonical_license_expression("BSD-2-Clause OR Apache-2.0"),
            "BSD-2-Clause OR Apache-2.0",
        )
        for expression in (
            "MIT OR Apache-2.0 OR CC0-1.0",
            "Zlib",
            "Apache-2.0 OR BSL-1.0",
        ):
            with self.subTest(expression=expression):
                self.assertEqual(canonical_license_expression(expression), expression)
        with self.assertRaises(AuditError):
            canonical_license_expression("GPL-3.0-only")

    def test_bundled_sqlite_notice_is_extracted_from_pinned_source(self):
        source = self.root / "sqlite-source"
        sqlite_dir = source / "sqlite3"
        sqlite_dir.mkdir(parents=True)
        source_text = (
            b'#define SQLITE_VERSION "3.53.2"\n'
            b'** The author disclaims copyright to this source code.  In place of\n'
            b'** a legal notice, here is a blessing:\n'
            b'**\n'
            b'**    May you do good and not evil.\n'
            b'*************************************************************************\n'
        )
        (sqlite_dir / "sqlite3.c").write_bytes(source_text)
        package = {"manifest_path": str(source / "Cargo.toml")}
        version, exact_source, notice = sqlite_bundled_notice(package)
        self.assertEqual(version, "3.53.2")
        self.assertEqual(exact_source, source_text)
        self.assertTrue(notice.startswith(b"** The author disclaims copyright"))
        self.assertIn(b"May you do good and not evil.", notice)

    def test_notice_check_rejects_byte_or_hash_drift(self):
        packages = check_notices(self.metadata, self.lock, self.root)
        self.assertEqual(set(packages), {("sample", "1.2.3")})
        self.bundle_file.write_bytes(b"changed notice")
        with self.assertRaises(AuditError):
            check_notices(self.metadata, self.lock, self.root)

    def test_notice_check_rejects_unreviewed_license_expression(self):
        self.registry_package["license"] = "GPL-3.0-only"
        with self.assertRaises(AuditError):
            check_notices(self.metadata, self.lock, self.root)

    def test_cyclonedx_inventory_contains_pinned_component_and_graph(self):
        workspace = {
            "id": "path+file:///rover#rover-core@0.1.0",
            "name": "rover-core",
            "version": "0.1.0",
            "license": "MIT",
        }
        metadata = {
            "packages": [workspace],
            "workspace_members": [workspace["id"]],
        }
        lock = {
            "package": [
                {"name": "rover-core", "version": "0.1.0"},
                self.lock["package"][0],
            ]
        }
        active = {"test-target": {("rover-core", "0.1.0"), ("sample", "1.2.3")}}
        edges = {"test-target": {(('rover-core', '0.1.0'), ('sample', '1.2.3'))}}
        bom = generate_sbom(metadata, lock, {("sample", "1.2.3"): self.registry_package}, active, edges, self.root)
        self.assertEqual(bom["bomFormat"], "CycloneDX")
        self.assertEqual(bom["specVersion"], "1.7")
        component_refs = {component["bom-ref"] for component in bom["components"]}
        self.assertIn("pkg:cargo/sample@1.2.3", component_refs)
        self.assertIn("pkg:cargo/rover-core@0.1.0", component_refs)
        refs = {row["ref"] for row in bom["dependencies"]}
        self.assertEqual(refs, component_refs | {bom["metadata"]["component"]["bom-ref"]})
        self.assertTrue(all(set(row["dependsOn"]) <= refs for row in bom["dependencies"]))


if __name__ == "__main__":
    unittest.main()
