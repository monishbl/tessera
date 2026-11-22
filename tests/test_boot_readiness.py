"""The boot check after every update, and what it undoes (tessera-root.py
boot_check, boot_check_and_repair), Software Update's warning
(sysupdate.py boot_check_notice) and Health's row (health-check.py
check_login). All in a folder standing in for /: nothing real is read or
changed, no PAM runs (unprivileged, the helper only reads PAM files), no
systemctl runs (a recorder stands in).
"""
import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "shell/assets/scripts"


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


R = load(SCRIPTS / "tessera-root.py", "tessera_root_boot")
assert not R.PAM_STAND_INS

SYSTEM_AUTH = ("auth       required                    pam_faillock.so      preauth\n"
               "auth       [success=1 default=bad]     pam_unix.so          try_first_pass nullok\n"
               "auth       [default=die]               pam_faillock.so      authfail\n"
               "auth       required                    pam_faillock.so      authsucc\n"
               "account    required                    pam_unix.so\nsession    required                    pam_unix.so\n")
GREETD_PAM = ("#%PAM-1.0\nauth       required     pam_securetty.so\nauth       include      system-local-login\n"
              "account    include      system-local-login\nsession    include      system-local-login\n")


class Recorder:
    def __init__(self, answers=None):
        self.calls = []
        self.answers = answers or {}

    def __call__(self, cmd, *a, **k):
        self.calls.append(list(cmd))
        code, out = self.answers.get(cmd[0], (0, ""))
        return subprocess.CompletedProcess(cmd, code, out, "")

    def ran(self, *words):
        return any(c[:len(words)] == list(words) for c in self.calls)


def which_with(*names):
    return lambda n: f"/usr/bin/{n}" if n in names else None


class Machine(unittest.TestCase):
    """A folder laid out as a computer that starts greetd with Tessera's
    login screen"""

    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.root)
        self.etc = self.p("/etc/pam.d")
        self.etc.mkdir(parents=True)
        for name, text in (("system-auth", SYSTEM_AUTH), ("system-login", "auth include system-auth\n"),
                           ("system-local-login", "auth include system-login\n"), ("greetd", GREETD_PAM),
                           ("sudo", "auth include system-auth\n")):
            (self.etc / name).write_text(text)
        units = self.p("/usr/lib/systemd/system")
        units.mkdir(parents=True)
        (units / "greetd.service").write_text("[Service]\nExecStart=greetd\n")
        self.p("/etc/systemd/system").mkdir(parents=True)
        os.symlink("/usr/lib/systemd/system/greetd.service", self.p("/etc/systemd/system/display-manager.service"))
        self.p("/etc/greetd").mkdir(parents=True)
        self.p("/etc/greetd/config.toml").write_bytes(R.GREETD_TOML)
        share = self.p(R.LOGIN_SHARE)
        (share / "shell/assets/scripts").mkdir(parents=True)
        for rel in ("greeter-session.sh", "greeter.lua", "shell/assets/scripts/greeter-start.sh"):
            (share / rel).write_text("#!/bin/sh\n")
            (share / rel).chmod(0o755)
        self.real_write = R.write_root

    def p(self, path):
        return Path(self.root, path.lstrip("/"))

    def check(self, **k):
        return R.boot_check(self.root, run=Recorder(), which=which_with("Hyprland", "agreety", "greetd"), **k)


class BootCheck(Machine):
    def test_a_good_machine(self):
        c = self.check()
        self.assertTrue(c["ok"], c)
        self.assertEqual(c["dm"], "greetd")

    def test_a_missing_login_screen(self):
        os.unlink(self.p(R.LOGIN_SHARE + "/greeter-session.sh"))
        ids = {p["id"] for p in self.check()["problems"]}
        self.assertIn("greeter-command", ids)
        self.assertIn("tessera-greeter", ids)

    def test_a_config_that_doesnt_parse(self):
        self.p("/etc/greetd/config.toml").write_text("[default_session\ncommand = \n")
        self.assertIn("greetd-config", {p["id"] for p in self.check()["problems"]})

    def test_hyprland_that_doesnt_run(self):
        run = Recorder({"/usr/bin/Hyprland": (1, "")})
        c = R.boot_check(self.root, run=run, which=which_with("Hyprland"))
        self.assertIn("hyprland", {p["id"] for p in c["problems"]})
        self.assertTrue(run.ran("/usr/bin/Hyprland", "--version"))

    def test_tessera_lines_that_skip_the_password(self):
        bad = R.pam_with_fingerprint(GREETD_PAM, R.FP_LINES["login"].replace("sufficient", "[success=done default=2]"))
        (self.etc / "greetd").write_text(bad)
        c = self.check()
        self.assertEqual([p["id"] for p in c["problems"]], ["pam:greetd"])

    def test_the_helpers_own_lines_are_fine(self):
        (self.etc / "greetd").write_text(R.pam_login_fingerprint(R.pam_with_passkey(R.pam_with_keyring(GREETD_PAM))))
        (self.etc / "sudo").write_text(R.pam_with_fingerprint(R.pam_with_passkey("auth include system-auth\n"), R.FP_LINES["sudo"]))
        self.assertTrue(self.check()["ok"])

    def test_a_system_stack_that_never_gets_to_the_password_is_said_not_repaired(self):
        (self.etc / "system-auth").write_text("auth [default=1] pam_permit.so\nauth required pam_unix.so\nauth required pam_permit.so\n")
        c = self.check()
        self.assertEqual([p["id"] for p in c["problems"]], ["password:greetd"])
        rec = R.boot_check_and_repair(self.root, run=Recorder(), which=which_with("Hyprland"))
        self.assertEqual(rec["did"], [])
        self.assertEqual((self.etc / "system-auth").read_text()[:6], "auth [")

    def test_another_login_screen(self):
        os.unlink(self.p("/etc/systemd/system/display-manager.service"))
        (self.p("/usr/lib/systemd/system") / "sddm.service").write_text("")
        os.symlink("/usr/lib/systemd/system/sddm.service", self.p("/etc/systemd/system/display-manager.service"))
        (self.etc / "sddm").write_text(GREETD_PAM)
        self.assertTrue(self.check()["ok"])
        (self.etc / "sddm").write_text("auth required pam_missing.so   " + R.FP_MARK + "\n" + GREETD_PAM)
        self.assertEqual([p["id"] for p in self.check()["problems"]], ["pam:sddm"])


class Repair(Machine):
    def test_tessera_lines_out_then_back_only_when_fixed(self):
        bad = R.pam_with_fingerprint(R.pam_with_keyring(GREETD_PAM), R.FP_LINES["login"].replace("sufficient", "[success=done default=2]"))
        (self.etc / "greetd").write_text(bad)
        rec = R.boot_check_and_repair(self.root, run=Recorder(), which=which_with("Hyprland"))
        self.assertTrue(rec["ok"], rec)
        self.assertEqual(len(rec["did"]), 1)
        self.assertIn("took Tessera's lines out of /etc/pam.d/greetd", rec["did"][0])
        self.assertEqual((self.etc / "greetd").read_text(), GREETD_PAM)  # (the keyring's lines too)
        stored = json.loads(self.p(R.BOOT_CHECK).read_text())
        self.assertTrue(stored["ok"])
        self.assertEqual(stored["found"][0]["id"], "pam:greetd")
        note = R.read_note(self.root)
        saved = note["saved"]["greetd"]
        self.assertEqual(Path(saved).read_text(), bad)
        # Restore: refused while it would still skip the password
        said = []
        self.assertEqual(R.rescue_undo(self.root, Recorder(), said.append), 1)
        self.assertEqual((self.etc / "greetd").read_text(), GREETD_PAM)
        self.assertIn("Not put back", said[0])
        # once the copy is one that works (the line fixed), it goes back
        good = R.pam_login_fingerprint(GREETD_PAM)
        Path(saved).write_text(good)
        self.assertEqual(R.rescue_undo(self.root, Recorder(), said.append), 0)
        self.assertEqual((self.etc / "greetd").read_text(), good)
        self.assertFalse(self.p(R.RESCUE_NOTE).exists())

    def test_a_backup_from_before_tessera_when_stripping_isnt_enough(self):
        broken = "auth required pam_missing_thing.so\n" + R.pam_login_fingerprint(GREETD_PAM)
        (self.etc / "greetd").write_text(broken)
        backups = Path(R.pam_backup_dir(str(self.etc)))
        backups.mkdir(parents=True)
        (backups / "greetd.first").write_text(GREETD_PAM)
        how, copy = R.repair_pam(self.root, "greetd", "t")
        # (stripping leaves the system's own missing module: not a problem PAM can
        # see unprivileged, so stripping is taken; the static check passes it)
        self.assertTrue(how)
        self.assertEqual(Path(copy).read_text(), broken)

    def test_a_missing_login_screen_gets_the_text_login(self):
        os.unlink(self.p(R.LOGIN_SHARE + "/greeter.lua"))
        run = Recorder()
        rec = R.boot_check_and_repair(self.root, run=run, which=which_with("Hyprland", "start-hyprland", "agreety", "greetd"))
        self.assertTrue(any("agreety" in d for d in rec["did"]))
        toml = self.p(R.TEXT_GREETER).read_text()
        self.assertIn("command = \"/usr/bin/agreety --cmd 'start-hyprland'\"", toml)
        self.assertIn("vt = 1", toml)
        import tomllib
        self.assertEqual(tomllib.loads(toml)["default_session"]["user"], "greeter")
        dropin = self.p(R.TEXT_DROPIN).read_text()
        self.assertIn("ExecStart=\nExecStart=/usr/bin/greetd --config " + R.TEXT_GREETER, dropin)
        self.assertTrue(R.read_note(self.root)["textGreeter"])
        # Restore keeps the text login while the login screen's files are missing
        self.assertEqual(R.rescue_undo(self.root, Recorder(), lambda s: None), 1)
        self.assertTrue(self.p(R.TEXT_DROPIN).exists())
        self.p(R.LOGIN_SHARE + "/greeter.lua").write_text("")
        orig = R.boot_check
        R.boot_check = lambda prefix, run, hyprland=True: orig(prefix, run, which_with("Hyprland"), False)
        self.addCleanup(setattr, R, "boot_check", orig)
        self.assertEqual(R.rescue_undo(self.root, Recorder(), lambda s: None), 0)
        self.assertFalse(self.p(R.TEXT_DROPIN).exists())

    def test_no_hyprland_gives_a_shell(self):
        R.text_greeter_on(self.root, Recorder(), which_with("agreety", "greetd"), hyprland_runs=False)
        self.assertIn("--cmd '/bin/bash -l'", self.p(R.TEXT_GREETER).read_text())

    def test_said_lines(self):
        lines = []
        R.boot_check_said({"ok": True, "did": [], "problems": []}, lines.append)
        self.assertEqual(lines, ["@@boot-check ok"])
        lines = []
        R.boot_check_said({"ok": False, "did": ["x"], "problems": [{"what": "y"}]}, lines.append)
        self.assertEqual(lines, ["@@boot-check undid x", "@@boot-check problem y"])


class AfterEveryUpdate(unittest.TestCase):
    def test_the_check_runs_whatever_the_update_did(self):
        r = load(SCRIPTS / "tessera-root.py", "tessera_root_boot_update")
        seen = []
        r.boot_check_and_repair = lambda prefix: seen.append(prefix) or {"ok": True, "did": [], "problems": []}
        import io
        for outcome in (21, RuntimeError("pacman")):
            out = io.StringIO()

            def fake(*a, outcome=outcome, **k):
                if isinstance(outcome, Exception):
                    raise outcome
                return outcome
            r._update = fake
            if isinstance(outcome, Exception):
                with self.assertRaises(RuntimeError):
                    r.update([], 0, out)
            else:
                self.assertEqual(r.update([], 0, out), 21)
            self.assertIn("@@boot-check ok", out.getvalue())
        self.assertEqual(len(seen), 2)

    def test_the_check_failing_never_fails_the_update(self):
        r = load(SCRIPTS / "tessera-root.py", "tessera_root_boot_update2")
        r._update = lambda *a, **k: 0

        def boom(prefix):
            raise ValueError("x")
        r.boot_check_and_repair = boom
        import io
        out = io.StringIO()
        self.assertEqual(r.update([], 0, out), 0)
        self.assertIn("@@boot-check problem", out.getvalue())

    def test_setup_installs_the_rescue_where_greetd_is(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root)
        self.assertEqual(R.rescue_units(root, Recorder()), [])
        Path(root, "usr/lib/systemd/system").mkdir(parents=True)
        Path(root, "usr/lib/systemd/system/greetd.service").write_text("")
        self.assertEqual(sorted(R.rescue_units(root, Recorder())), sorted(R.RESCUE_UNITS))
        self.assertEqual(R.rescue_units(root, Recorder()), [])
        os.unlink(Path(root, "usr/lib/systemd/system/greetd.service"))
        self.assertEqual(sorted(R.rescue_units(root, Recorder())), sorted(R.RESCUE_UNITS))
        self.assertFalse(any(Path(root, p.lstrip("/")).exists() for p in R.RESCUE_UNITS))


class SoftwareUpdateSaysIt(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir)
        os.environ["XDG_RUNTIME_DIR"] = self.dir
        self.s = load(SCRIPTS / "sysupdate.py", "sysupdate_boot")
        os.makedirs(self.s.DIR, exist_ok=True)
        self.s.log = open(os.path.join(self.dir, "log"), "w")
        self.addCleanup(self.s.log.close)
        self.rec = os.path.join(self.dir, "boot-check.json")
        self.sent = []

    def notice(self, rec):
        Path(self.rec).write_text(json.dumps(rec))
        return self.s.boot_check_notice(self.rec, notify=lambda cmd, **k: self.sent.append(cmd))

    def test_all_good_is_quiet(self):
        self.s.state["text"] = "Updated"
        self.notice({"ok": True, "problems": [], "did": [], "time": self.s.state["started"] + 1})
        self.assertEqual(self.sent, [])
        self.assertEqual(self.s.state["text"], "Updated")
        self.assertTrue(self.s.state["bootCheck"]["ok"])

    def test_a_problem_is_loud(self):
        self.s.state["text"] = "Updated"
        self.notice({"ok": False, "problems": [{"id": "pam:greetd", "what": "greetd: x"}], "did": [],
                     "time": self.s.state["started"] + 1})
        self.assertEqual(len(self.sent), 1)
        self.assertIn("critical", self.sent[0])
        self.assertIn("Don't log out", self.sent[0][-2])
        self.assertIn("needs attention", self.s.state["text"])

    def test_a_repair_is_said(self):
        self.notice({"ok": True, "problems": [], "did": ["took Tessera's lines out of /etc/pam.d/greetd"],
                     "time": self.s.state["started"] + 1})
        self.assertIn("repaired", self.sent[0][-2])

    def test_an_old_record_is_ignored(self):
        self.assertIsNone(self.notice({"ok": False, "problems": [], "did": [], "time": self.s.state["started"] - 3600}))
        self.assertEqual(self.sent, [])


class HealthRow(Machine):
    def health(self):
        os.environ["TESSERA_SYSTEM_ROOT"] = self.root
        self.addCleanup(os.environ.pop, "TESSERA_SYSTEM_ROOT", None)
        h = load(SCRIPTS / "health-check.py", "health_boot")
        return h.check_login(None)

    def test_ok(self):
        r = self.health()
        self.assertEqual(r["status"], "ok", r)

    def test_a_problem_has_a_fix(self):
        os.unlink(self.p(R.LOGIN_SHARE + "/greeter-session.sh"))
        r = self.health()
        self.assertEqual(r["status"], "fail")
        self.assertTrue(r["canFix"])
        self.assertIn("boot-check --repair", r["fixCommand"])
        self.assertTrue(r["steps"])

    def test_the_rescue_note(self):
        R.write_root = lambda dest, data, mode: (Path(dest).parent.mkdir(parents=True, exist_ok=True), Path(dest).write_bytes(data))
        self.addCleanup(setattr, R, "write_root", self.real_write)
        R.add_to_note(self.root, ["greetd failed"], ["took Tessera's lines out of /etc/pam.d/greetd"])
        r = self.health()
        self.assertEqual(r["status"], "warn")
        self.assertIn("rescue undo", " ".join(r["steps"]))


if __name__ == "__main__":
    unittest.main()
