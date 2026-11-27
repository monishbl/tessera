"""speedtest.sh: Ookla's licence and privacy notice are answered for you only
once you accepted them in Settings (the "accepted" argument, #338). Run with
a stand-in `speedtest` that records its arguments."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "shell/assets/scripts/speedtest.sh"

FAKE = r'''#!/bin/sh
[ "$1" = "--version" ] && { echo "Speedtest by Ookla 1.2.0"; exit 0; }
echo "$@" > "$(dirname "$0")/args"
echo '{"type":"result"}'
'''


class OoklaTerms(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        d = Path(self.dir.name)
        for name in ("speedtest", "speedtest-cli"):
            (d / name).write_text(FAKE)
            (d / name).chmod(0o755)
        self.env = dict(os.environ, PATH=f"{d}:/usr/bin:/bin")

    def tearDown(self):
        self.dir.cleanup()

    def run_it(self, *args):
        return subprocess.run(["bash", str(SCRIPT), *args], env=self.env, capture_output=True, text=True, timeout=20)

    def test_not_accepted_runs_nothing(self):
        for backend in ("ookla", "ookla-py"):
            out = self.run_it(backend)
            self.assertNotEqual(out.returncode, 0)
            self.assertTrue(out.stdout.startswith("error terms "), out.stdout)
            self.assertFalse(Path(self.dir.name, "args").exists())

    def test_accepted_answers_the_terms(self):
        out = self.run_it("ookla", "accepted")
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)
        self.assertIn("ookla {\"type\":\"result\"}", out.stdout)
        args = Path(self.dir.name, "args").read_text()
        self.assertIn("--accept-license", args)
        self.assertIn("--accept-gdpr", args)


if __name__ == "__main__":
    unittest.main()
