"""The lock's on-screen keyboard (#763) is drawn on the lock's own surface:
no Hyprland rule lets any other layer be drawn over the session lock or take
its input (one would let any program name itself the keyboard and phish the
password). Static checks of the config and the markup; no display."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class LockKeyboard(unittest.TestCase):
    def test_no_layer_is_shown_over_the_lock(self):
        for lua in sorted((ROOT / "hypr").rglob("*.lua")):
            text = lua.read_text()
            self.assertNotRegex(text, r"above_?lock", f"{lua}: nothing may be drawn over the session lock")
            self.assertNotRegex(text, r"session_lock_xray", lua)

    def test_the_lock_draws_the_keys_itself(self):
        lock = (ROOT / "native/tessera-ui/surfaces/lock/ui/lock.slint").read_text()
        self.assertIn('import { KeyGrid, OskRow } from "@tessera/oskgrid.slint";', lock)
        self.assertRegex(lock, r"KeyGrid \{\s*rows: LockData\.osk-rows;")
        rs = (ROOT / "native/tessera-ui/surfaces/lock/src/lib.rs").read_text()
        # (its presses go into the field here, never to Panels' keyboard window)
        self.assertNotIn('"panels.toggle"', rs)
        self.assertIn("in_field(key", rs)

    def test_the_keyboard_window_and_the_lock_share_the_keys(self):
        osk = (ROOT / "native/tessera-ui/surfaces/osk/ui/osk.slint").read_text()
        self.assertIn('from "@tessera/oskgrid.slint"', osk)
        self.assertFalse(re.search(r"^component Cap\b", osk, re.M), "the caps live in core/ui/oskgrid.slint")


if __name__ == "__main__":
    unittest.main()
