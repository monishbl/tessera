"""tools/gen-keymap.py writes the shipped keymap.json and gestures.json: running
it again must not change them (a hand edit to the JSON would be lost)."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class GenKeymap(unittest.TestCase):
    def test_generator_matches_shipped_files(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "tools").mkdir()
            (Path(d) / "hypr" / "tessera").mkdir(parents=True)
            shutil.copy(ROOT / "tools" / "gen-keymap.py", Path(d) / "tools")
            subprocess.run([sys.executable, "-I", str(Path(d) / "tools" / "gen-keymap.py")],
                           check=True, capture_output=True)
            for name in ("keymap.json", "gestures.json"):
                self.assertEqual((Path(d) / "hypr" / "tessera" / name).read_text(),
                                 (ROOT / "hypr" / "tessera" / name).read_text(), name)

    def test_capture_keys_do_not_run_on_the_lock_screen(self):
        binds = {b["id"]: b for b in json.loads((ROOT / "hypr" / "tessera" / "keymap.json").read_text())}
        for id in ("record", "record-alt", "screenshot-print", "screenshot-ctrl-print", "screen-tools"):
            self.assertFalse(binds[id].get("opts", {}).get("locked"), id)


if __name__ == "__main__":
    unittest.main()
