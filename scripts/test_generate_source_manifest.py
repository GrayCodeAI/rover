"""Unit tests for the source-manifest staleness report."""

import json
import unittest

from generate_source_manifest import manifest_differences


def manifest(version, *files):
    rows = [{"path": path, "sha256": digest, "size": size} for path, digest, size in files]
    return (json.dumps({"version": version, "scope": "s", "files": rows}, indent=2) + "\n").encode()


class ManifestDifferenceTests(unittest.TestCase):
    def test_reports_changed_missing_and_untracked_paths(self):
        expected = manifest("0.0.1", ("a", "1" * 64, 1), ("b", "2" * 64, 2), ("new", "3" * 64, 3))
        actual = manifest("0.0.1", ("a", "1" * 64, 1), ("b", "9" * 64, 2), ("gone", "4" * 64, 4))
        self.assertEqual(
            manifest_differences(expected, actual),
            ["missing: new", "no longer tracked: gone", "changed: b"],
        )

    def test_reports_version_change(self):
        self.assertEqual(
            manifest_differences(manifest("0.0.2"), manifest("0.0.1")),
            ["version: '0.0.1' -> '0.0.2'"],
        )

    def test_caps_the_report(self):
        expected = manifest("0.0.1", *[(f"p{i:02}", "1" * 64, 1) for i in range(30)])
        lines = manifest_differences(expected, manifest("0.0.1"), limit=5)
        self.assertEqual(len(lines), 6)
        self.assertEqual(lines[-1], "... and 25 more")

    def test_invalid_committed_manifest(self):
        self.assertEqual(
            manifest_differences(manifest("0.0.1"), b"{not json"),
            ["SOURCE_MANIFEST.json is not a valid manifest"],
        )


if __name__ == "__main__":
    unittest.main()
