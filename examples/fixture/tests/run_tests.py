"""Dependency-free JUnit fixture runner used to exercise Rover's real adapter."""
from pathlib import Path
import sys
import unittest
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app import is_allowed

class AuthorizationTests(unittest.TestCase):
    def test_admin_is_allowed(self):
        self.assertTrue(is_allowed("admin"))
    def test_guest_is_rejected(self):
        self.assertFalse(is_allowed("guest"))
    def test_empty_role_is_rejected(self):
        self.assertFalse(is_allowed(""))

class Results(unittest.TestResult):
    def __init__(self):
        super().__init__()
        self.root = ET.Element("testsuite", name="fixture")
        self.active = None
    def startTest(self, test):
        super().startTest(test)
        self.active = ET.SubElement(self.root, "testcase", name=test.id())
    def addFailure(self, test, error):
        super().addFailure(test, error)
        ET.SubElement(self.active, "failure").text = self._exc_info_to_string(error, test)
    def addError(self, test, error):
        super().addError(test, error)
        ET.SubElement(self.active, "error").text = self._exc_info_to_string(error, test)

results = Results()
unittest.defaultTestLoader.loadTestsFromTestCase(AuthorizationTests).run(results)
results.root.set("tests", str(results.testsRun))
results.root.set("failures", str(len(results.failures)))
results.root.set("errors", str(len(results.errors)))
output = Path(".rover-results/tests.xml")
output.parent.mkdir(parents=True, exist_ok=True)
ET.ElementTree(results.root).write(output, encoding="utf-8", xml_declaration=True)
print(f"Executed {results.testsRun} tests; {len(results.failures)} failures; {len(results.errors)} errors")
sys.exit(0 if results.wasSuccessful() else 1)
