"""Hyprland 0.57 crashes (CEventLoopManager::scheduleRecalc) when a timer is
made while it first reads its config: at login, and on every
`Hyprland --verify-config` the installer runs (22 Nov). Every hl.timer in the
shipped config is made by a start() that runs once Hyprland is running.

SOURCE-TEXT checks (audit #676): These assert on the text of the files,
not on what they do. A backstop against a rule quietly coming undone; a
green run here is not behaviour coverage.
(tests/test_findpointer_timers.py runs one file under a stub `hl`.)"""
import re
import unittest
from pathlib import Path

HYPR = Path(__file__).resolve().parent.parent / "hypr"


class TimersWaitForHyprland(unittest.TestCase):
    def test_every_timer_is_made_after_the_start(self):
        files = sorted(HYPR.glob("*.lua")) + sorted(HYPR.glob("tessera/*.lua"))
        self.assertTrue(files)
        for f in files:
            text = f.read_text()
            if "hl.timer(" not in text:
                continue
            with self.subTest(f.name):
                self.assertIn('hl.on("hyprland.start", start)', text)
                self.assertRegex(text, r"pcall\(hl\.get_active_monitor\)")
                # every timer line sits inside start()
                body = re.search(r"local function start\(\)\n(.*?)\n\s*end\n", text, re.S)
                self.assertIsNotNone(body)
                self.assertEqual(text.count("hl.timer("), body.group(1).count("hl.timer("))


if __name__ == "__main__":
    unittest.main()
