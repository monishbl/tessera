"""Every PAM line Tessera's root helper can write leaves the login screen
able to start and the password able to log you in (22 Nov: the
fingerprint gate's line made pam_setcred come back PAM_IGNORE when greetd
started its greeter, so the login screen never came up).

Where the real Linux-PAM runs here (pam_start_confdir, on a folder of the
test's own), it runs only on stand-ins: pam_permit for a finger or key that
answers, pam_deny for one that doesn't (and for the password, which here is
always wrong), pam_exec /bin/true or /bin/false for a gate, and module
names that aren't installed for one removed. The helper itself refuses any
other module while unprivileged (stand_ins_only). Nothing here copies
/etc/pam.d, wakes a reader or checks anyone's password.
"""
import ctypes
import itertools
import os
import shutil
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "shell/assets/scripts/tessera-root.py"


def load():
    import importlib.util
    spec = importlib.util.spec_from_file_location("tessera_root_pam", SOURCE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


R = load()
MISSING = "pam_tessera_test_not_installed.so"
PAM_AUTH_ERR, PAM_SUCCESS = 7, 0


def have_pam():
    try:
        ctypes.CDLL("libpam.so.0").pam_start_confdir
        return True
    except (OSError, AttributeError):
        return False


def stand_in(text, finger=MISSING, key=MISSING, keyring=MISSING, gate="/bin/false"):
    """The helper's lines with stand-ins for the real modules and gates"""
    for real, fake in (("pam_fprintd.so", finger), ("pam_u2f.so", key), ("pam_gnome_keyring.so", keyring),
                       (R.FP_GATE + " check", gate), (R.FP_GATE + " mark", gate), (R.PK_GATE, gate)):
        text = text.replace(real, fake)
    return text


# The password, as a stand-in: always the wrong one (pam_deny)
BASE = ("#%PAM-1.0\n\nauth       required     pam_deny.so\naccount    required     pam_permit.so\n"
        "session    required     pam_permit.so\n")
# (for a session started without a password, the password's stand-in is
# pam_permit: pam_unix's setcred succeeds, pam_deny's doesn't)
CRED = BASE.replace("pam_deny.so", "pam_permit.so", 1)


def authenticate(d, name):
    """pam_authenticate on the test's own folder of stand-ins (checked)"""
    assert R.stand_ins_only(d), "stand-ins only"
    pam = ctypes.CDLL("libpam.so.0")

    class Conv(ctypes.Structure):
        _fields_ = [("conv", ctypes.c_void_p), ("appdata", ctypes.c_void_p)]
    cb_t = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)
    cb = cb_t(lambda n, m, r, a: 19)
    conv = Conv(ctypes.cast(cb, ctypes.c_void_p), None)
    h = ctypes.c_void_p()
    assert pam.pam_start_confdir(name.encode(), b"root", ctypes.byref(conv), d.encode(), ctypes.byref(h)) == 0
    r = pam.pam_authenticate(h, 0)
    pam.pam_end(h, r)
    return r


def helper_text(base, fingerprint, passkey, keyring, name="greetd"):
    """What the helper writes into NAME with these on"""
    text = base
    if keyring:
        text = R.pam_with_keyring(text) or text
    if passkey:
        text = R.pam_with_passkey(text)
    if fingerprint:
        text = R.pam_login_fingerprint(text) if name in R.FP_FILES["login"] else R.pam_with_fingerprint(text, R.FP_LINES["sudo"])
    return text


class EveryLineIsDashed(unittest.TestCase):
    """A module removed later is skipped quietly; its control already makes
    the miss harmless (sufficient, optional, a gate's skip)"""

    def test_every_added_line_starts_with_a_dash(self):
        lines = list(R.FP_LINES.values()) + R.FP_LOGIN_HEAD + R.FP_LOGIN_TAIL + R.PK_LINES + [R.polkit_face_line()]
        text = R.pam_with_keyring(BASE)
        lines += [l for l in text.splitlines() if "pam_gnome_keyring" in l]
        for line in lines:
            self.assertTrue(line.startswith("-auth") or line.startswith("-session"), line)
            self.assertNotRegex(line, r"\s(required|requisite)\s", line)

    def test_every_gate_uses_the_safe_control(self):
        for line in SOURCE.read_text().splitlines():
            if "pam_exec.so" in line and "default=1]" in line and "GATE_CONTROL" not in line:
                self.fail("a gate line not built from GATE_CONTROL: " + line.strip())

    def test_no_pam_call_but_setcred_in_the_helper(self):
        src = SOURCE.read_text()
        for call in ("pam_authenticate(", "pam_acct_mgmt(", "pam_open_session(", "pam_chauthtok("):
            self.assertNotIn("pam." + call, src)


@unittest.skipUnless(have_pam(), "no Linux-PAM 1.4+ here")
class RealPamOnStandIns(unittest.TestCase):
    """The real Linux-PAM, on every combination the helper can write"""

    def setUp(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root)
        # (a root of its own: write_pam's backups land under it)
        self.etc, self.vendor = os.path.join(root, "etc/pam.d"), os.path.join(root, "usr/lib/pam.d")
        os.makedirs(self.etc)
        os.makedirs(self.vendor)
        R.PAM_STAND_INS = True
        self.addCleanup(setattr, R, "PAM_STAND_INS", False)

    def starts(self, text, name="greetd"):
        return R.login_screen_starts(self.etc, name, text, vendor=self.vendor)

    def auth(self, text, name="greetd"):
        Path(self.etc, name).write_text(text)
        return authenticate(self.etc, name)

    def test_the_old_gate_is_caught(self):
        old = stand_in(R.pam_login_fingerprint(CRED).replace(R.GATE_CONTROL, R.OLD_GATE_CONTROL))
        self.assertFalse(self.starts(old))
        self.assertIn("PAM_IGNORE", R.pam_problem(self.etc, "greetd", old, vendor=self.vendor))
        # (and the undashed form of 22 Nov's fix still works: the file fixed by hand)
        fixed = stand_in(R.pam_login_fingerprint(CRED).replace(R.GATE_CONTROL, R.GATE_CONTROL[1:].replace("auth      ", "auth       ", 1)))
        self.assertTrue(self.starts(fixed))

    def test_every_combination_starts_and_reaches_the_password(self):
        for name in ("greetd", "sudo"):
            for fp, pk, kr in itertools.product((False, True), repeat=3):
                text = helper_text(BASE, fp, pk, kr, name)
                for finger, key, gate in itertools.product(("pam_deny.so", MISSING, "pam_permit.so"), ("pam_deny.so", MISSING),
                                                           ("/bin/true", "/bin/false")):
                    t = stand_in(text, finger=finger, key=key, keyring=MISSING, gate=gate)
                    with self.subTest(name=name, fingerprint=fp, passkeys=pk, keyring=kr, finger=finger, key=key, gate=gate):
                        if name == "greetd":
                            self.assertTrue(self.starts(t.replace("required     pam_deny.so", "required     pam_permit.so")))
                        self.assertEqual(R.password_path_problem(self.etc, name, t, vendor=self.vendor), "")
                        # a finger let in only when it answered and (at the
                        # login screen) the gate said yes; else the password
                        let_in = fp and finger == "pam_permit.so" and (name != "greetd" or gate == "/bin/true")
                        self.assertEqual(self.auth(t, name), PAM_SUCCESS if let_in else PAM_AUTH_ERR)

    def test_a_line_that_would_lock_you_out_is_caught_both_ways(self):
        bad = BASE.replace("auth       required     pam_deny.so",
                           "auth       required     " + MISSING + "   " + R.FP_MARK + "\nauth       required     pam_deny.so")
        self.assertNotEqual(self.auth(bad, "sudo"), PAM_AUTH_ERR)  # (PAM_MODULE_UNKNOWN: the password never decides)
        self.assertIn("failed password", R.password_path_problem(self.etc, "sudo", bad, vendor=self.vendor))
        skip = BASE.replace("auth       required     pam_deny.so",
                            "-auth      [success=ignore ignore=ignore default=2] pam_exec.so quiet type=auth /bin/false   "
                            + R.FP_MARK + "\n-auth      sufficient   pam_permit.so   " + R.FP_MARK
                            + "\nauth       required     pam_deny.so\nauth       required     pam_deny.so")
        self.assertIn("skip", R.password_path_problem(self.etc, "sudo", skip, vendor=self.vendor))

    def test_write_pam_refuses_and_keeps_a_backup(self):
        Path(self.etc, "greetd").write_text(CRED)
        old = stand_in(R.pam_login_fingerprint(CRED).replace(R.GATE_CONTROL, R.OLD_GATE_CONTROL))
        R_write = R.write_root
        R.write_root = lambda dest, data, mode: Path(dest).write_bytes(data)
        self.addCleanup(setattr, R, "write_root", R_write)
        with self.assertRaises(R.Refused):
            R.write_pam(self.etc, "greetd", old, self.vendor)
        self.assertEqual(Path(self.etc, "greetd").read_text(), CRED)
        good = stand_in(R.pam_login_fingerprint(CRED))
        R.write_pam(self.etc, "greetd", good, self.vendor)
        backup = Path(R.pam_backup_dir(self.etc))
        self.assertEqual((backup / "greetd").read_text(), CRED)
        self.assertEqual((backup / "greetd.first").read_text(), CRED)
        R.write_pam(self.etc, "greetd", CRED, self.vendor)
        self.assertEqual((backup / "greetd").read_text(), good)
        self.assertEqual((backup / "greetd.first").read_text(), CRED)  # (the first kept for good)

    def test_an_update_rewrites_old_lines(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root)
        pam = Path(root, "etc/pam.d")
        pam.mkdir(parents=True)
        real = R.pam_setcred_code
        # (the helper's own lines, run as stand-ins)
        empty = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, empty)
        R.pam_setcred_code = lambda etc, name, text, *a, **k: real(empty, name, stand_in(text), *a, **k)
        self.addCleanup(setattr, R, "pam_setcred_code", real)
        old = R.pam_login_fingerprint(CRED).replace(R.GATE_CONTROL, R.OLD_GATE_CONTROL)
        undashed = R.pam_with_fingerprint(CRED, R.FP_LINES["sudo"][1:].replace("auth      ", "auth       ", 1))
        (pam / "greetd").write_text(old)
        (pam / "sudo").write_text(undashed)
        gate = Path(root, R.FP_GATE.lstrip("/"))
        gate.parent.mkdir(parents=True)
        gate.write_bytes(R.FP_GATE_TEXT)
        done = R.migrate_old_names(root, run=lambda *a, **k: None)
        self.assertEqual((pam / "greetd").read_text(), R.pam_login_fingerprint(CRED))
        self.assertEqual((pam / "sudo").read_text(), R.pam_with_fingerprint(CRED, R.FP_LINES["sudo"]))
        self.assertIn("/etc/pam.d/sudo", done)
        self.assertEqual(R.migrate_old_names(root, run=lambda *a, **k: None), [])

    def test_the_helper_refuses_real_modules_in_tests(self):
        with self.assertRaises(AssertionError):
            R.pam_setcred_code(self.etc, "greetd", "auth required pam_unix.so\n", vendor=self.vendor)
        with self.assertRaises(AssertionError):
            R.pam_setcred_code(self.etc, "greetd", "auth required pam_exec.so /usr/bin/id\n", vendor=self.vendor)

    def test_off_unprivileged_nothing_runs(self):
        R.PAM_STAND_INS = False
        self.assertIsNone(R.pam_setcred_code(self.etc, "greetd", BASE, vendor=self.vendor))


class PasswordPathRead(unittest.TestCase):
    """The password's path, read from the text: Arch's own stacks (as text,
    never run) with Tessera's lines"""

    SYSTEM_AUTH = """#%PAM-1.0
auth       required                    pam_faillock.so      preauth
-auth      [success=2 default=ignore]  pam_systemd_home.so
auth       [success=1 default=bad]     pam_unix.so          try_first_pass nullok
auth       [default=die]               pam_faillock.so      authfail
auth       optional                    pam_permit.so
auth       required                    pam_env.so
auth       required                    pam_faillock.so      authsucc
"""
    SUDO = "#%PAM-1.0\nauth\t\tinclude\t\tsystem-auth\naccount\t\tinclude\t\tsystem-auth\nsession\t\tinclude\t\tsystem-auth\n"
    GREETD = ("#%PAM-1.0\nauth       required     pam_securetty.so\nauth       requisite    pam_nologin.so\n"
              "auth       include      system-local-login\naccount    include      system-local-login\n"
              "session    include      system-local-login\n")

    def setUp(self):
        self.etc = tempfile.mkdtemp()
        self.vendor = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.etc)
        self.addCleanup(shutil.rmtree, self.vendor)
        for name, text in (("system-auth", self.SYSTEM_AUTH), ("system-login", "auth include system-auth\n"),
                           ("system-local-login", "auth include system-login\n"), ("sudo", self.SUDO), ("greetd", self.GREETD)):
            Path(self.etc, name).write_text(text)

    def test_every_combination(self):
        for name, base in (("sudo", self.SUDO), ("greetd", self.GREETD), ("polkit-1", self.SUDO)):
            for fp, pk, kr in itertools.product((False, True), repeat=3):
                text = helper_text(base, fp, pk, kr, name)
                with self.subTest(name=name, fingerprint=fp, passkeys=pk, keyring=kr):
                    self.assertEqual(R.password_path_problem(self.etc, name, text, vendor=self.vendor), "")
                    self.assertTrue(R.password_reached(self.etc, name, text, vendor=self.vendor))

    def test_face_on_polkit(self):
        text = R.polkit_with_face(R.pam_with_fingerprint(self.SUDO, R.FP_LINES["polkit"]))
        self.assertEqual(R.password_path_problem(self.etc, "polkit-1", text, vendor=self.vendor), "")

    def test_a_stack_without_a_password(self):
        # (no module it knows: not for it to judge)
        self.assertIsNone(R.password_reached(self.etc, "x", "auth required pam_deny.so\n", vendor=self.vendor))
        # (pam_unix there, but jumped over)
        self.assertFalse(R.password_reached(self.etc, "x", "auth [default=1] pam_permit.so\nauth required pam_unix.so\n"
                                            "auth required pam_permit.so\n", vendor=self.vendor))
        self.assertIsNone(R.password_reached(self.etc, "nothing-here", vendor=self.vendor))

    def test_a_jump_over_the_password_is_caught(self):
        jump = R.FP_LINES["sudo"].replace("sufficient", "[success=done default=3]")
        text = R.pam_with_fingerprint(self.GREETD, jump)
        self.assertIn("skip", R.password_path_problem(self.etc, "greetd", text, vendor=self.vendor))


if __name__ == "__main__":
    unittest.main()
