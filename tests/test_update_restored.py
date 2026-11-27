"""The update screen's "Nothing was lost." only when the updater says it
put everything back (audit #697): tools/tessera-update.py's answer carries
"restored", update-run.sh passes it on in the status line, and the screen
(Rust and its Slint) says it for yes only; otherwise where the backup is.
"""
import importlib.machinery
import importlib.util
import io
import json
import os
import re
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent


def load():
    name = "tu_restored_" + os.urandom(3).hex()
    loader = importlib.machinery.SourceFileLoader(name, str(ROOT / "tools/tessera-update.py"))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


class TheUpdaterSaysWhetherItWasRestored(unittest.TestCase):
    def answer(self, mod, raised):
        out = io.StringIO()
        with mock.patch.object(mod, "apply", side_effect=raised), mock.patch.object(mod, "health_record"), \
                mock.patch.object(sys, "argv", ["tessera-update.py", "apply", "--config", "/nonexistent"]), redirect_stdout(out):
            code = mod.main()
        return code, json.loads(out.getvalue())

    def test_put_back_says_restored(self):
        mod = load()
        code, out = self.answer(mod, mod.UpdateFailed("The update didn't finish: x. Nothing was changed.", True))
        self.assertEqual(code, 1)
        self.assertIs(out["restored"], True)

    def test_not_all_put_back_says_so(self):
        mod = load()
        _, out = self.answer(mod, mod.UpdateFailed("The update didn't finish: x. It couldn't all be put back (y).", False))
        self.assertIs(out["restored"], False)

    def test_an_error_that_doesnt_know_says_nothing(self):
        mod = load()
        _, out = self.answer(mod, SystemExit("Couldn't download the update"))
        self.assertNotIn("restored", out)
        _, out = self.answer(mod, RuntimeError("broken"))
        self.assertNotIn("restored", out)

    def test_packages_missing_changed_nothing(self):
        mod = load()
        _, out = self.answer(mod, mod.DepsUnmet("Not updated: x needs y. Nothing was changed."))
        self.assertIs(out["restored"], True)

    def test_both_rollbacks_say_which(self):
        src = (ROOT / "tools/tessera-update.py").read_text()
        self.assertNotIn('raise SystemExit("The update didn\'t finish: "', src)
        self.assertEqual(src.count('raise UpdateFailed("The update didn\'t finish: " + why + "." + restored, back)'), 2)


class TheStatusLineCarriesIt(unittest.TestCase):
    def test_update_run_passes_restored_on(self):
        src = (ROOT / "shell/assets/scripts/update-run.sh").read_text()
        self.assertIn('.get("restored")', src)
        errors = [l for l in src.splitlines() if re.match(r'\s*status error 100 "(The update didn\'t finish|Tessera didn\'t start again)"', l)]
        self.assertEqual(len(errors), 2)
        for line in errors:
            # detail \x1f backup \x1f restored
            self.assertEqual(line.count("$'\\x1f'"), 2, line)
        # (the update went in but the shell didn't come back: never "Nothing was lost")
        self.assertTrue(errors[1].rstrip().endswith('"no"'), errors[1])

    def test_the_screen_says_it_only_when_restored(self):
        ui = (ROOT / "native/tessera-ui/surfaces/updatescreen/ui/main.slint").read_text()
        for m in re.finditer(r'"Nothing was lost\."', ui):
            line = ui[ui.rfind("\n", 0, m.start()) + 1:ui.find("\n", m.end())]
            self.assertIn("Up.restored ?", line)
        lib = (ROOT / "native/tessera-ui/surfaces/updatescreen/src/lib.rs").read_text()
        self.assertIn("set_restored(self.status.restored)", lib)


if __name__ == "__main__":
    unittest.main()
