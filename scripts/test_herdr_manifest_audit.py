"""Verify the byte-identical Herdr detection data reuse record."""

import hashlib
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
AUDIT = ROOT / "docs/design/UPSTREAM_SOURCE_AUDIT.md"
SOURCE_COMMIT = "c411883ec639c9893ed9c33021c890485e91727b"
MANIFEST_ROW = re.compile(
    r"^\| `([^`]+\.toml)` \| `([^`]+\.toml)` \| `([0-9a-f]{64})` \|$",
    re.MULTILINE,
)


class HerdrManifestAuditTests(unittest.TestCase):
    def test_all_bundled_manifest_files_match_the_documented_upstream_hashes(self):
        audit = AUDIT.read_text(encoding="utf-8")
        self.assertIn(SOURCE_COMMIT, audit)
        rows = MANIFEST_ROW.findall(audit)
        self.assertEqual(len(rows), 22)
        upstream_paths = set()
        rover_paths = set()
        for source_path, rover_path, expected in rows:
            self.assertTrue(source_path.startswith("src/detect/manifests/"))
            self.assertTrue(rover_path.startswith("crates/rover-agents/src/herdr_manifests/"))
            upstream_paths.add(source_path)
            rover_paths.add(rover_path)
            actual = hashlib.sha256((ROOT / rover_path).read_bytes()).hexdigest()
            self.assertEqual(actual, expected, rover_path)
        self.assertEqual(len(upstream_paths), 22)
        self.assertEqual(len(rover_paths), 22)

    def test_root_apache_license_copy_matches_the_recorded_digest(self):
        expected = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"
        license_path = ROOT / "licenses/upstream/herdr/LICENSE"
        self.assertEqual(hashlib.sha256(license_path.read_bytes()).hexdigest(), expected)


if __name__ == "__main__":
    unittest.main()
