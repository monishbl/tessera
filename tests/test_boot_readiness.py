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


# The Passkeys lines an older Tessera wrote (removed Nov 2026; still on
# machines until the next setup takes them out, so they must stay harmless)
def legacy_passkey_lines():
    return [R.GATE_CONTROL + R.PK_GATE + "   " + R.PK_MARK,
            "-auth      sufficient   pam_u2f.so authfile=/etc/tessera/passkeys-u2f origin=pam://tessera appid=pam://tessera"
            " cue nouserok userverification=1   " + R.PK_MARK]


def with_legacy_passkey(text):
    lines = R.pam_without_passkey(text).splitlines(keepends=True)
    at = 0
    while at < len(lines) and (lines[at].strip() == "" or lines[at].lstrip().startswith("#")):
        at += 1
    return "".join(lines[:at] + [l + "\n" for l in legacy_passkey_lines()] + lines[at:])
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
        (self.etc / "greetd").write_text(R.pam_login_fingerprint(with_legacy_passkey(R.pam_with_keyring(GREETD_PAM))))
        (self.etc / "sudo").write_text(R.pam_with_fingerprint(with_legacy_passkey("auth include system-auth\n"), R.FP_LINES["sudo"]))
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


class LoginRescue(Machine):
    """tessera-login-rescue.service: one step further each time in a boot"""

    def rescue(self, run=None, which=None):
        said = []
        r = R.login_rescue(self.root, run or self.run, which or which_with("Hyprland", "start-hyprland", "agreety", "greetd"), said.append)
        return r, said

    def setUp(self):
        super().setUp()
        self.run = Recorder({"journalctl": (0, "greetd[1]: error: PAM 'greetd' authentication error\n")})
        # (the Tessera lines of the 22 Nov fault: the old gate, which only the
        # real PAM finds (as root); unprivileged, step 1's strip "anyway" does)
        self.old = R.pam_login_fingerprint(GREETD_PAM).replace(R.GATE_CONTROL, R.OLD_GATE_CONTROL)
        (self.etc / "greetd").write_text(self.old)

    def test_the_steps(self):
        r, said = self.rescue()
        self.assertEqual(r, "repaired")
        self.assertEqual((self.etc / "greetd").read_text(), GREETD_PAM)
        self.assertTrue(self.run.ran("systemctl", "reset-failed", "greetd.service"))
        self.assertTrue(self.run.ran("systemctl", "restart", "--no-block", "greetd.service"))
        note = R.read_note(self.root)
        self.assertEqual(Path(note["saved"]["greetd"]).read_text(), self.old)
        self.assertIn("PAM 'greetd' authentication error", " ".join(note["log"]))
        self.assertTrue(any("error" in s for s in said))
        # failed again: the text login
        r, _ = self.rescue()
        self.assertEqual(r, "text-login")
        self.assertTrue(self.p(R.TEXT_DROPIN).exists())
        # and again: a console login
        r, _ = self.rescue()
        self.assertEqual(r, "console")
        self.assertTrue(self.run.ran("systemctl", "start", "--no-block", "getty@tty1.service"))
        self.assertIn("a console login on tty1 (getty)", R.read_note(self.root)["did"])

    def test_without_agreety_straight_to_the_console(self):
        (self.etc / "greetd").write_text(GREETD_PAM)  # (nothing of Tessera's to take out)
        r, _ = self.rescue(which=which_with("Hyprland"))
        self.assertEqual(r, "console")
        self.assertFalse(self.p(R.TEXT_DROPIN).exists())

    def test_another_login_screen_took_over(self):
        os.unlink(self.p("/etc/systemd/system/display-manager.service"))
        r, _ = self.rescue()
        self.assertEqual(r, "elsewhere")
        self.assertFalse(self.run.ran("systemctl", "restart", "--no-block", "greetd.service"))

    def test_anything_unexpected_is_a_console(self):
        orig = R._login_rescue
        R._login_rescue = lambda *a: 1 / 0
        self.addCleanup(setattr, R, "_login_rescue", orig)
        r, said = self.rescue()
        self.assertEqual(r, "console")
        self.assertTrue(self.run.ran("systemctl", "start", "--no-block", "getty@tty1.service"))

    def test_from_a_console(self):
        said = []
        self.assertEqual(R.rescue([], self.root, self.run, which_with("Hyprland", "agreety", "greetd"), said.append), 0)
        self.assertEqual((self.etc / "greetd").read_text(), GREETD_PAM)
        self.assertTrue(self.p(R.TEXT_DROPIN).exists())
        self.assertTrue(self.run.ran("systemctl", "restart", "--no-block", "greetd.service"))
        out = []
        R.rescue(["status"], self.root, self.run, say=out.append)
        self.assertTrue(json.loads(out[0])["textGreeter"])
        with self.assertRaises(R.Refused):
            R.rescue(["now"], self.root, self.run, say=out.append)

    def test_the_units(self):
        unit = R.RESCUE_UNITS["/etc/systemd/system/tessera-login-rescue.service"]
        self.assertIn("ExecStart=/usr/lib/tessera/tessera-root login-rescue", unit)
        self.assertIn("getty@tty1.service", unit)
        self.assertNotIn("[Install]", unit)  # (only greetd failing starts it)
        dropin = R.RESCUE_UNITS["/etc/systemd/system/greetd.service.d/tessera-rescue.conf"]
        self.assertIn("OnFailure=tessera-login-rescue.service", dropin)
        if shutil.which("systemd-analyze"):
            d = tempfile.mkdtemp()
            self.addCleanup(shutil.rmtree, d)
            Path(d, "tessera-login-rescue.service").write_text(unit)
            p = subprocess.run(["systemd-analyze", "verify", str(Path(d, "tessera-login-rescue.service"))], capture_output=True, text=True)
            self.assertEqual(p.stderr.strip(), "")

    def test_uninstall_removes_them(self):
        text = (ROOT / "uninstall.sh").read_text()
        for path in list(R.RESCUE_UNITS) + [R.TEXT_DROPIN, R.TEXT_GREETER]:
            self.assertIn(path, text)


class GreeterSession(unittest.TestCase):
    """hypr/greeter-session.sh: the login screen failing twice in a row
    gives greetd's text login in the same session"""

    def setUp(self):
        self.d = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.d)
        self.bin = self.d / "bin"
        self.bin.mkdir()
        for tool in ("cat", "mktemp", "rm", "cut"):
            os.symlink(shutil.which(tool), self.bin / tool)
        # (the journal's stand-in: runs what it's given, writes nowhere)
        self.stub("systemd-cat", 'while [ $# -gt 0 ]; do case "$1" in -t|-p) shift 2;; *) break;; esac; done\n'
                                 '[ $# -gt 0 ] && exec "$@"\ncat >/dev/null\n')
        # start-hyprland: run N says FAKE_RESULTS' Nth word ("none": nothing)
        self.stub("start-hyprland", 'n=$(cat "$FAKE_COUNT" 2>/dev/null || echo 0); n=$((n + 1)); echo $n >"$FAKE_COUNT"\n'
                                    'w=$(echo "$FAKE_RESULTS" | cut -d" " -f$n)\n'
                                    '[ "$w" = none ] || printf "%s\\n" "$w" >"$TESSERA_GREETER_RESULT"\nexit "${FAKE_CODE:-0}"\n')
        self.stub("agreety", 'echo "agreety $*" >"$FAKE_AGREETY"\n')

    def stub(self, name, body):
        (self.bin / name).write_text("#!/bin/sh\n" + body)
        (self.bin / name).chmod(0o755)

    def session(self, results, code=0, agreety=True):
        if not agreety:
            (self.bin / "agreety").unlink()
        env = {"PATH": str(self.bin), "XDG_RUNTIME_DIR": str(self.d), "FAKE_RESULTS": results, "FAKE_CODE": str(code),
               "FAKE_COUNT": str(self.d / "count"), "FAKE_AGREETY": str(self.d / "agreety.txt"),
               "TESSERA_GREETER_LUA": str(self.d / "greeter.lua")}
        p = subprocess.run(["/bin/sh", str(ROOT / "hypr/greeter-session.sh")], env=env, capture_output=True, text=True, timeout=30)
        runs = int((self.d / "count").read_text()) if (self.d / "count").exists() else 0
        said = (self.d / "agreety.txt").read_text().strip() if (self.d / "agreety.txt").exists() else ""
        return p.returncode, runs, said

    def test_a_login_ends_it(self):
        self.assertEqual(self.session("0"), (0, 1, ""))

    def test_one_failure_then_a_login(self):
        self.assertEqual(self.session("1 0"), (0, 2, ""))

    def test_two_failures_give_the_text_login(self):
        code, runs, said = self.session("1 none", code=1)
        self.assertEqual(runs, 2)
        self.assertEqual(said, "agreety --cmd start-hyprland")

    def test_a_login_screen_from_before_that_says_nothing(self):
        self.assertEqual(self.session("none"), (0, 1, ""))

    def test_no_agreety(self):
        self.assertEqual(self.session("1 1", agreety=False), (1, 2, ""))

    def test_greeter_start_says_how_it_ended(self):
        self.assertIn('"$1" >"$TESSERA_GREETER_RESULT"', (SCRIPTS / "greeter-start.sh").read_text())


class RescueNote(unittest.TestCase):
    """rescue-note.py: the note once at the next login, with Restore"""

    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.d)
        os.environ["TESSERA_SYSTEM_ROOT"] = self.d
        os.environ["XDG_STATE_HOME"] = os.path.join(self.d, "state")
        self.addCleanup(os.environ.pop, "TESSERA_SYSTEM_ROOT", None)
        self.n = load(SCRIPTS / "rescue-note.py", "rescue_note")
        os.makedirs(os.path.dirname(self.n.NOTE))
        self.calls = []

    def run_(self, cmd, **k):
        self.calls.append(cmd)
        out = "restore" if cmd[0] == "notify-send" and "-w" in cmd else ""
        return subprocess.CompletedProcess(cmd, 0, out, "")

    def test_once_with_restore(self):
        Path(self.n.NOTE).write_text(json.dumps({"time": 100, "why": ["greetd kept failing"],
                                                 "did": ["took Tessera's lines out of /etc/pam.d/greetd"]}))
        self.n.HELPER = shutil.which("true")
        self.n.shutil = type("S", (), {"which": staticmethod(lambda n: "/usr/bin/" + n)})
        self.n.main([], run=self.run_)
        self.assertTrue(any(c[0] == "notify-send" and "restore=Restore" in c for c in self.calls))
        self.assertTrue(any(c[:3] == ["pkexec", self.n.HELPER, "rescue"] for c in self.calls))
        self.calls.clear()
        self.n.main([], run=self.run_)
        self.assertEqual(self.calls, [])  # (shown once)

    def test_nothing_without_a_note(self):
        self.n.main([], run=self.run_)
        self.assertEqual(self.calls, [])


class HardwareGates(unittest.TestCase):
    """Features that need hardware never touch the login without it: read
    from a stand-in /sys, nothing asked of a device"""

    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.root)
        self.r = load(SCRIPTS / "tessera-root.py", "tessera_root_hw")

    def usb(self, vendor, product):
        d = Path(self.root, "sys/bus/usb/devices", f"1-{vendor}")
        d.mkdir(parents=True)
        (d / "idVendor").write_text(vendor + "\n")
        (d / "idProduct").write_text(product + "\n")

    def test_no_reader(self):
        self.usb("046d", "c52b")  # (a mouse's receiver)
        self.assertFalse(self.r.fingerprint_reader(self.root))

    def test_a_reader_by_its_maker_or_the_hwdb_or_enrolled_fingers(self):
        self.usb("27c6", "609c")
        self.assertTrue(self.r.fingerprint_reader(self.root))
        other = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, other)
        hwdb = Path(other, self.r.FP_HWDB.lstrip("/"))
        hwdb.parent.mkdir(parents=True)
        hwdb.write_text("usb:v1234p5678*\n ID_AUTOSUSPEND=1\n")
        d = Path(other, "sys/bus/usb/devices/1-1")
        d.mkdir(parents=True)
        (d / "idVendor").write_text("1234\n")
        (d / "idProduct").write_text("5678\n")
        self.assertTrue(self.r.fingerprint_reader(other))
        third = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, third)
        self.assertFalse(self.r.fingerprint_reader(third))
        Path(third, "var/lib/fprint/you/goodix").mkdir(parents=True)
        Path(third, "var/lib/fprint/you/goodix/7").write_text("")
        self.assertTrue(self.r.fingerprint_reader(third))

    def test_fingerprint_on_without_a_reader_changes_nothing(self):
        etc = Path(self.root, "etc")
        etc.mkdir()
        (etc / "greetd").write_text(GREETD_PAM)
        module = Path(self.root, "pam_fprintd.so")
        module.write_text("")
        with self.assertRaises(self.r.Refused):
            self.r.fingerprint(True, "login", etc=str(etc), vendor=str(Path(self.root, "vendor")), module=(str(module),),
                               gate=str(Path(self.root, "gate")), reader=False)
        self.assertEqual((etc / "greetd").read_text(), GREETD_PAM)
        # (off still works without one)
        self.r.fingerprint(False, "login", etc=str(etc), vendor=str(Path(self.root, "vendor")), module=(), reader=False)

    def test_face_never_touches_a_login(self):
        src = (SCRIPTS / "tessera-root.py").read_text()
        # (Howdy's line only in polkit's copy, shown to polkit's helper by its drop-in)
        self.assertEqual(src.count("polkit_with_face("), 4)
        self.assertNotIn("FACE_MARK", "".join(l for l in src.splitlines() if "FP_FILES[\"login\"]" in l))


if __name__ == "__main__":
    unittest.main()
