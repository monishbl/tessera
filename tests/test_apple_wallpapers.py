"""Apple's wallpapers taken back out of an older install
(shell/assets/scripts/apple-wallpapers.py): only the exact files the old
installer fetched go, never a picture of yours, and settings that pointed at
them go back to Tessera's built-in set."""
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "shell/assets/scripts/apple-wallpapers.py"


def load():
    spec = importlib.util.spec_from_file_location("apple_wallpapers", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class AppleWallpapers(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.home = Path(self.tmp.name)
        self.env = dict(os.environ)
        os.environ["HOME"] = str(self.home)
        os.environ["XDG_CONFIG_HOME"] = str(self.home / ".config")
        self.mod = load()
        self.folder = self.home / "Pictures/Wallpapers/macOS"
        (self.folder / "Tahoe Beach").mkdir(parents=True)
        # (stand-ins for the downloads: their sizes and hashes are the list's)
        self.mod.KNOWN = []
        for rel in ("Tahoe Light.jpg", "Tahoe Dark.jpg", "Tahoe Beach/1 Dawn.jpg", "Tahoe Beach/2 Day.jpg"):
            data = ("download " + rel).encode()
            (self.folder / rel).write_bytes(data)
            self.mod.KNOWN.append((rel, len(data), hashlib.sha256(data).hexdigest()))
        self.cfg = self.home / ".config/tessera/config.json"
        self.cfg.parent.mkdir(parents=True)

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self.env)
        self.tmp.cleanup()

    def settings(self, w):
        self.cfg.write_text(json.dumps({"appearance": {"look": "glass", "accent": "café"}, "wallpaper": w}, indent=4))

    def run_it(self, *args):
        self.assertEqual(self.mod.main(list(args)), 0)
        return json.loads(self.cfg.read_text()) if self.cfg.exists() else None

    def test_the_downloads_go_and_settings_return_to_the_built_in_set(self):
        light, dark = str(self.folder / "Tahoe Light.jpg"), str(self.folder / "Tahoe Dark.jpg")
        self.settings({"path": "", "dynamic": "appearance", "lightPath": light, "darkPath": dark,
                       "dynamicFolder": str(self.folder / "Tahoe Beach"), "directory": ""})
        c = self.run_it()
        self.assertFalse(self.folder.exists(), "both folders were left empty: gone")
        w = c["wallpaper"]
        self.assertEqual((w["lightPath"], w["darkPath"], w["dynamicFolder"]), ("", "", ""))
        self.assertEqual(w["dynamic"], "appearance")  # (the built-in day and dusk)
        self.assertEqual(c["appearance"]["accent"], "café")
        # nothing left to do: a no-op, settings unchanged
        before = self.cfg.read_text()
        self.run_it()
        self.assertEqual(self.cfg.read_text(), before)

    def test_your_own_pictures_stay(self):
        mine = self.folder / "Sonoma Light.jpg"  # (a name it knows, but not its file)
        self.mod.KNOWN.append(("Sonoma Light.jpg", 5, "0" * 64))
        mine.write_bytes(b"mine!")
        changed = self.folder / "Tahoe Dark.jpg"
        changed.write_bytes(b"edited by you")  # (same name, other bytes)
        other = self.folder / "Tahoe Beach/5 Late.jpg"
        other.write_bytes(b"added by you")
        linked = self.folder / "Tahoe Light.jpg"
        target = self.home / "real.jpg"
        target.write_bytes(linked.read_bytes())
        linked.unlink()
        linked.symlink_to(target)  # (a link isn't the download)
        self.settings({"path": str(mine), "dynamic": "time", "dynamicFolder": str(self.folder / "Tahoe Beach")})
        c = self.run_it()
        self.assertTrue(mine.exists() and changed.exists() and other.exists() and linked.is_symlink())
        self.assertFalse((self.folder / "Tahoe Beach/1 Dawn.jpg").exists())
        self.assertTrue((self.folder / "Tahoe Beach").is_dir(), "not empty: kept")
        w = c["wallpaper"]
        self.assertEqual(w["path"], str(mine))
        self.assertEqual(w["dynamicFolder"], str(self.folder / "Tahoe Beach"), "the folder is still there")

    def test_a_dry_run_changes_nothing(self):
        self.settings({"path": str(self.folder / "Tahoe Light.jpg")})
        before = self.cfg.read_text()
        self.run_it("--dry-run")
        self.assertEqual(self.cfg.read_text(), before)
        self.assertTrue((self.folder / "Tahoe Light.jpg").exists())

    def test_settings_elsewhere_are_left_alone(self):
        self.settings({"path": "/somewhere/Tahoe Light.jpg", "lightPath": "/gone/x.jpg"})
        c = self.run_it()
        self.assertEqual(c["wallpaper"]["path"], "/somewhere/Tahoe Light.jpg")
        self.assertEqual(c["wallpaper"]["lightPath"], "/gone/x.jpg")

    def test_no_folder_no_settings_is_fine(self):
        for p in sorted(self.folder.rglob("*"), reverse=True):
            p.unlink() if p.is_file() else p.rmdir()
        self.folder.rmdir()
        self.assertIsNone(self.run_it())


class TheList(unittest.TestCase):
    def test_every_download_is_listed_exactly(self):
        mod = load()
        names = [k[0] for k in mod.KNOWN]
        self.assertEqual(len(names), len(set(names)))
        for rel, size, digest in mod.KNOWN:
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
            self.assertGreater(size, 0)
            self.assertTrue(re.fullmatch(r"(Tahoe Beach/\d [A-Z][a-z]+|[A-Z][a-z]+( Sur)? (Light|Dark))\.jpg", rel), rel)

    def test_it_runs_on_its_own_and_never_fails(self):
        with tempfile.TemporaryDirectory() as h:
            env = dict(os.environ, HOME=h, XDG_CONFIG_HOME=os.path.join(h, ".config"))
            p = subprocess.run([sys.executable, "-I", str(SCRIPT)], env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual((p.returncode, p.stdout, p.stderr), (0, "", ""))

    def test_every_caller_runs_it(self):
        self.assertIn("apple-wallpapers.py", (ROOT / "install.sh").read_text())
        self.assertIn("apple-wallpapers.py", (ROOT / "tools/tessera-update.py").read_text())
        self.assertIn("apple-wallpapers.py", (ROOT / "native/tessera-ui/host/shell/src/bridges.rs").read_text())


if __name__ == "__main__":
    unittest.main()
