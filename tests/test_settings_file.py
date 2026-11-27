"""The one settings lock helper every script uses (settings_file.py, audit
#786 #815 #824): the shell's own lock on the settings folder, a re-read
inside it, a write that keeps a dotfiles link a link and its permissions,
a change laid over one the shell made meanwhile; and the health record's
lock shared with health_event.py."""
import fcntl
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent.parent / "shell/assets/scripts"


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / file)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


SF = load("settings_file", "settings_file.py")


class SettingsFile(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name) / "tessera"
        self.dir.mkdir()
        self.path = self.dir / "config.json"

    def tearDown(self):
        self.tmp.cleanup()

    def test_edit_reads_again_inside_the_lock(self):
        self.path.write_text(json.dumps({"dock": {"iconSize": 48}}))
        self.assertTrue(SF.edit(lambda c: SF.put(c, "features.offer", "done"), path=str(self.path)))
        c = json.loads(self.path.read_text())
        self.assertEqual(c, {"dock": {"iconSize": 48}, "features": {"offer": "done"}})
        # (as the shell writes it: four spaces, a newline at the end)
        self.assertTrue(self.path.read_text().endswith("}\n"))
        self.assertIn('\n    "dock"', self.path.read_text())
        # nothing changed: nothing written
        before = self.path.stat().st_mtime_ns
        self.assertFalse(SF.edit(lambda c: SF.put(c, "features.offer", "done"), path=str(self.path)))
        self.assertEqual(self.path.stat().st_mtime_ns, before)

    def test_missing_and_broken_files(self):
        self.assertTrue(SF.edit(lambda c: SF.put(c, "a.b", 1), path=str(self.path)))
        self.assertEqual(json.loads(self.path.read_text()), {"welcome": {"done": False}, "a": {"b": 1}})
        self.path.write_text("{ not json")
        self.assertFalse(SF.edit(lambda c: SF.put(c, "a.b", 2), path=str(self.path)))
        self.assertEqual(self.path.read_text(), "{ not json")

    def test_waits_for_the_shells_lock(self):
        self.path.write_text("{}")
        fd = os.open(str(self.dir), os.O_RDONLY)
        fcntl.flock(fd, fcntl.LOCK_EX)
        took = []

        def later():
            time.sleep(0.3)
            # (the shell's write, made while it held the lock)
            self.path.write_text(json.dumps({"shell": True}))
            fcntl.flock(fd, fcntl.LOCK_UN)
            os.close(fd)
        t = threading.Thread(target=later)
        t.start()
        start = time.monotonic()
        SF.edit(lambda c: SF.put(c, "script", True), path=str(self.path))
        took.append(time.monotonic() - start)
        t.join()
        self.assertGreaterEqual(took[0], 0.25)
        # the shell's change is kept: the file was read again inside the lock
        self.assertEqual(json.loads(self.path.read_text()), {"shell": True, "script": True})

    def test_a_link_stays_a_link_with_its_mode(self):
        real = Path(self.tmp.name) / "dotfiles" / "config.json"
        real.parent.mkdir()
        real.write_text("{}")
        os.chmod(real, 0o600)
        self.path.symlink_to(real)
        SF.edit(lambda c: SF.put(c, "x", 1), path=str(self.path))
        self.assertTrue(self.path.is_symlink())
        self.assertEqual(json.loads(real.read_text()), {"x": 1})
        self.assertEqual(real.stat().st_mode & 0o777, 0o600)
        # (no temporary file left in either folder)
        self.assertEqual(sorted(os.listdir(real.parent)), ["config.json"])
        self.assertEqual(sorted(os.listdir(self.dir)), ["config.json"])

    def test_rebase_keeps_what_the_shell_changed_meanwhile(self):
        base = {"dock": {"iconSize": 48, "autohide": False}, "gone": 1, "same": "x"}
        planned = {"dock": {"iconSize": 64, "autohide": False}, "same": "x", "new": True}
        now = {"dock": {"iconSize": 48, "autohide": True}, "gone": 1, "same": "x", "shell": 2}
        self.assertEqual(SF.rebase(base, planned, now),
                         {"dock": {"iconSize": 64, "autohide": True}, "same": "x", "new": True, "shell": 2})
        self.path.write_text(json.dumps(now))
        SF.replace(base, planned, str(self.path))
        self.assertEqual(json.loads(self.path.read_text())["dock"], {"iconSize": 64, "autohide": True})
        # unchanged since: the planned file as it is
        self.path.write_text(json.dumps(base))
        SF.replace(base, planned, str(self.path))
        self.assertEqual(json.loads(self.path.read_text()), planned)

    def test_keep_newest(self):
        for i in range(5):
            p = self.dir / ("before-restore-%d.tar.gz" % i)
            p.write_text(str(i))
            os.utime(p, (1000 + i, 1000 + i))
        (self.dir / "other").write_text("")
        gone = SF.keep_newest(str(self.dir), "before-restore-", 3)
        self.assertEqual(len(gone), 2)
        self.assertEqual(sorted(os.listdir(self.dir)), ["before-restore-2.tar.gz", "before-restore-3.tar.gz",
                                                       "before-restore-4.tar.gz", "config.json", "other"]
                         if self.path.exists() else ["before-restore-2.tar.gz", "before-restore-3.tar.gz",
                                                     "before-restore-4.tar.gz", "other"])

    def test_command_line_set(self):
        env = dict(os.environ, XDG_CONFIG_HOME=self.tmp.name)
        subprocess.run([sys.executable, "-I", str(SCRIPTS / "settings_file.py"), "set", "appearance.look", "material",
                        "dock.iconSize", "56"], env=env, check=True)
        c = json.loads(self.path.read_text())
        self.assertEqual(c["appearance"]["look"], "material")
        self.assertEqual(c["dock"]["iconSize"], 56)


class FallbackKeepsUnknownKeys(unittest.TestCase):
    """(audit #667) the QML fallback writes only what it declares: what it
    left out goes back in after its write"""

    def test_keep_then_fill(self):
        with tempfile.TemporaryDirectory() as d:
            cfg, copy = os.path.join(d, "config.json"), os.path.join(d, "state", ".config-kept.json")
            rust = {"dock": {"iconSize": 48, "rustOnly": {"per-app": {"kitty": 1}}}, "displays": {"DP-1": {"scale": 2}},
                    "performance": {"gpu": "low"}}
            with open(cfg, "w") as f:
                json.dump(rust, f)
            self.assertTrue(SF.keep(cfg, copy))
            self.assertEqual(os.stat(copy).st_mode & 0o777, 0o600)
            # the adapter's write: its own keys only, one value changed
            with open(cfg, "w") as f:
                json.dump({"dock": {"iconSize": 64, "autohide": False}, "performance": {}}, f)
            self.assertTrue(SF.fill(cfg, copy))
            c = json.load(open(cfg))
            self.assertEqual(c["dock"], {"iconSize": 64, "autohide": False, "rustOnly": {"per-app": {"kitty": 1}}})
            self.assertEqual(c["displays"], {"DP-1": {"scale": 2}})
            self.assertEqual(c["performance"], {"gpu": "low"})
            self.assertFalse(os.path.exists(copy))
            # nothing kept (a first start): fill changes nothing
            self.assertFalse(SF.fill(cfg, copy))
            self.assertFalse(SF.keep(os.path.join(d, "none.json"), copy))

    def test_config_qml_saves_through_keep_and_fill(self):
        qml = (SCRIPTS.parent.parent / "common/Config.qml").read_text()
        self.assertIn('onTriggered: root.save()', qml)
        self.assertIn('"keep", file.path', qml)
        self.assertIn('"fill", file.path', qml)
        self.assertIn("blockWrites: true", qml)


class EveryScriptUsesIt(unittest.TestCase):
    def test_no_script_writes_the_settings_on_its_own(self):
        # (each of these rewrites config.json: through the helper only)
        for name in ("features.py", "apple-wallpapers.py", "backup.py", "settings-backup.py", "migrate-names.py",
                     "desktop-look.py"):
            text = (SCRIPTS / name).read_text()
            self.assertIn("settings_file", text, name)
        install = (SCRIPTS.parent.parent.parent / "install.sh").read_text()
        self.assertIn("settings_file.py", install)
        self.assertNotIn('os.replace(path + ".tmp", path)', install)


class HealthRecordLock(unittest.TestCase):
    def test_event_waits_for_the_shells_trim(self):
        HE = load("health_event", "health_event.py")
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "health.jsonl")
            fd = os.open(os.path.join(d, ".health.jsonl.lock"), os.O_WRONLY | os.O_CREAT, 0o600)
            fcntl.flock(fd, fcntl.LOCK_EX)

            def trim():
                # (the shell cutting the file: read, a new file renamed over it)
                time.sleep(0.3)
                with open(p + ".tmp", "w") as f:
                    f.write('{"t":1,"ev":"start"}\n')
                os.replace(p + ".tmp", p)
                fcntl.flock(fd, fcntl.LOCK_UN)
                os.close(fd)
            t = threading.Thread(target=trim)
            t.start()
            self.assertTrue(HE.record("fallback", where=p, why="test"))
            t.join()
            lines = [json.loads(x) for x in open(p).read().splitlines()]
            self.assertEqual([x["ev"] for x in lines], ["start", "fallback"])

    def test_event_command(self):
        with tempfile.TemporaryDirectory() as d:
            env = dict(os.environ, XDG_STATE_HOME=d)
            subprocess.run([sys.executable, "-I", str(SCRIPTS / "health_event.py"), "event", "fallback", "why", "it  crashed"],
                           env=env, check=True)
            line = json.loads(open(os.path.join(d, "tessera", "health.jsonl")).read())
            self.assertEqual((line["ev"], line["why"]), ("fallback", "it crashed"))


if __name__ == "__main__":
    unittest.main()
