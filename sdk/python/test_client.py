import json
from pathlib import Path
import tempfile
import unittest
from rover_client import Rover, RoverError

class ClientTests(unittest.TestCase):
    def test_argv_and_failed_decision_are_not_hidden(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'fixture'
            p.write_text('#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps({"decision":"BLOCKED","argv":sys.argv[1:]}))\nsys.exit(1)\n')
            p.chmod(0o700)
            r = Rover(p, Path(d)/'state').call(['inspect', '--repo', 'space ; $(not-a-shell)'])
            self.assertEqual(r.exit_code, 1)
            self.assertEqual(r.decision, 'BLOCKED')
            self.assertIn('space ; $(not-a-shell)', r.data['argv'])
    def test_non_json_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'fixture';p.write_text('#!/bin/sh\nprintf not-json\n');p.chmod(0o700)
            with self.assertRaises(RoverError): Rover(p,Path(d)/'state').call(['status'])
    def test_no_shell_strings(self):
        with self.assertRaises(TypeError): Rover('/missing','/tmp/missing-state').call('status; echo bad')
if __name__=='__main__':unittest.main()
