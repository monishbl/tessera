"""hypr/tessera/look.lua: Claude Desktop's sign-in goes to the keyring when
the login unlocks it (gnome-keyring in a login's PAM file), else to its own
obscured file ("basic"); a choice in the environment wins (#658). Run under
plain Lua with a stand-in `hl` and a stand-in / (TESSERA_SYSTEM_ROOT)."""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOOK = ROOT / "hypr/tessera/look.lua"

HARNESS = r'''
local got = {}
hl = setmetatable({ env = function(k, v) got[k] = v end }, { __index = function() return function() end end })
dofile(arg[1])
print(got.CLAUDE_PASSWORD_STORE or "")
'''


@unittest.skipUnless(shutil.which("lua"), "requires Lua")
class ClaudePasswordStore(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name, "root")
        (self.root / "etc/pam.d").mkdir(parents=True)
        (self.root / "usr/lib/security").mkdir(parents=True)
        self.harness = Path(self.dir.name, "harness.lua")
        self.harness.write_text(HARNESS)

    def tearDown(self):
        self.dir.cleanup()

    def store(self, **env):
        e = {k: v for k, v in os.environ.items() if k != "CLAUDE_PASSWORD_STORE"}
        e.update(TESSERA_SYSTEM_ROOT=str(self.root), XDG_CONFIG_HOME=str(Path(self.dir.name, "cfg")), **env)
        out = subprocess.run(["lua", str(self.harness), str(LOOK)], env=e, capture_output=True, text=True, check=True)
        return out.stdout.strip()

    def test_no_keyring_at_login_keeps_the_file(self):
        (self.root / "etc/pam.d/greetd").write_text("auth include system-local-login\n")
        self.assertEqual(self.store(), "basic")

    def test_keyring_unlocked_at_login(self):
        (self.root / "usr/lib/security/pam_gnome_keyring.so").write_text("")
        (self.root / "etc/pam.d/greetd").write_text(
            "auth include system-local-login\n# tessera: unlock the keyring with the login password\n"
            "-auth      optional     pam_gnome_keyring.so\n-session   optional     pam_gnome_keyring.so auto_start\n")
        self.assertEqual(self.store(), "gnome-libsecret")

    def test_a_line_without_the_module_or_commented_out_doesnt_count(self):
        (self.root / "etc/pam.d/login").write_text("-auth optional pam_gnome_keyring.so\n")
        self.assertEqual(self.store(), "basic")  # (pam_gnome_keyring.so isn't installed)
        (self.root / "usr/lib/security/pam_gnome_keyring.so").write_text("")
        (self.root / "etc/pam.d/login").write_text("#-auth optional pam_gnome_keyring.so\nsession optional pam_gnome_keyring.so auto_start\n")
        self.assertEqual(self.store(), "basic")

    def test_your_choice_wins(self):
        self.assertEqual(self.store(CLAUDE_PASSWORD_STORE="kwallet6"), "")  # (not set over it)


if __name__ == "__main__":
    unittest.main()
