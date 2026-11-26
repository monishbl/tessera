"""The root helper's `bootmenu` verb and its `pam-keyring` entry (install.sh,
run as you), with fakes: nothing runs as root, nothing in /etc is read."""
import contextlib
import importlib.util
import io
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "shell/assets/scripts/tessera-root.py"
GREETD_PAM = "#%PAM-1.0\n\nauth       include      system-local-login\naccount    include      system-local-login\n" \
             "session    include      system-local-login\n"


def load():
    spec = importlib.util.spec_from_file_location("tessera_root_bootmenu", SOURCE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class Bootmenu(unittest.TestCase):
    def setUp(self):
        self.r = load()
        self.calls = []

    def run_fake(self, fail=()):
        def run(cmd, **kw):
            self.calls.append(cmd)
            return subprocess.CompletedProcess(cmd, 1 if cmd[1:2] and cmd[1] in fail else 0, "", "")
        return run

    def test_refused_without_grub_btrfs(self):
        with self.assertRaises(self.r.Refused):
            self.r.bootmenu(True, run=self.run_fake(), which=lambda n: None, exists=lambda p: True)
        with self.assertRaises(self.r.Refused):
            self.r.bootmenu(True, run=self.run_fake(), which=lambda n: "/usr/bin/" + n, exists=lambda p: False)
        self.assertEqual(self.calls, [])

    def test_on_enables_and_writes_the_submenu(self):
        did = self.r.bootmenu(True, run=self.run_fake(), which=lambda n: "/usr/bin/" + n, exists=lambda p: True)
        self.assertEqual(did, ["enabled grub-btrfsd"])
        self.assertEqual(self.calls, [["systemctl", "enable", "--now", "grub-btrfsd.service"], [self.r.GRUB_BTRFS_SCRIPT]])

    def test_on_fails_when_the_unit_does_not_start(self):
        with self.assertRaises(self.r.Refused):
            self.r.bootmenu(True, run=self.run_fake(fail={"enable"}), which=lambda n: "/usr/bin/" + n, exists=lambda p: True)
        self.assertNotIn([self.r.GRUB_BTRFS_SCRIPT], self.calls)

    def test_off_disables(self):
        did = self.r.bootmenu(False, run=self.run_fake(), which=lambda n: "/usr/bin/" + n, exists=lambda p: True)
        self.assertEqual(did, ["disabled grub-btrfsd"])
        self.assertEqual(self.calls, [["systemctl", "disable", "--now", "grub-btrfsd.service"]])


class PamKeyringEntry(unittest.TestCase):
    def setUp(self):
        self.r = load()
        self.d = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(self.d))
        (self.d / "system-local-login").write_text("auth required pam_unix.so\naccount required pam_unix.so\nsession required pam_unix.so\n")
        self.r.PAM_VENDOR = str(self.d / "vendor")

    def main(self, args, euid=1000):
        self.r.os.geteuid = lambda: euid
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = self.r.main(args)
        return code, out.getvalue(), err.getvalue()

    def test_prints_the_file_with_the_lines(self):
        f = self.d / "greetd"
        f.write_text(GREETD_PAM)
        code, out, _ = self.main(["pam-keyring", str(f)])
        self.assertEqual(code, 0)
        self.assertEqual(out, self.r.pam_with_keyring(GREETD_PAM))
        self.assertIn("pam_gnome_keyring.so", out)
        self.assertEqual(f.read_text(), GREETD_PAM)  # (printed, never written)

    def test_nothing_to_add_or_no_file(self):
        f = self.d / "greetd"
        f.write_text(self.r.pam_with_keyring(GREETD_PAM))
        self.assertEqual(self.main(["pam-keyring", str(f)])[:2], (1, ""))
        self.assertEqual(self.main(["pam-keyring", str(self.d / "missing")])[:2], (1, ""))

    def test_refused_when_the_password_path_would_break(self):
        f = self.d / "greetd"
        f.write_text(GREETD_PAM)
        self.r.password_path_problem = lambda *a, **k: "greetd: one of Tessera's lines would count as a failed password"
        code, out, err = self.main(["pam-keyring", str(f)])
        self.assertEqual((code, out), (1, ""))
        self.assertIn("failed password", err)

    def test_never_as_root(self):
        f = self.d / "greetd"
        f.write_text(GREETD_PAM)
        code, out, _ = self.main(["pam-keyring", str(f)], euid=0)
        self.assertNotEqual(code, 0)
        self.assertEqual(out, "")


if __name__ == "__main__":
    unittest.main()
