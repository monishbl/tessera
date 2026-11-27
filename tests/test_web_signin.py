"""web-signin.py: Microsoft 365's sign-in in tessera-web for GNOME Online
Accounts. goa-signin's web links reach it alone (its own desktop name, the
handler first on its data folders), the environment is put back before
anything opens, tessera-web's answer goes to GOA's handler, and without
tessera-web the default browser opens the page as before. Nothing is run
for real: a stand-in records the commands."""
import importlib.util
import os
import stat
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

SCRIPT = Path(__file__).resolve().parent.parent / "shell/assets/scripts/web-signin.py"
spec = importlib.util.spec_from_file_location("web_signin", SCRIPT)
ws = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ws)

AUTH = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id=x&redirect_uri=goa-oauth2%3A%2F%2Flocalhost%2Fx"
DONE = "goa-oauth2://localhost/x?code=abc&state=s"


class Runs:
    """subprocess.run's stand-in: what was run, and tessera-web's answer"""

    def __init__(self, stdout=b"", code=0, fails=False):
        self.calls, self.stdout, self.code, self.fails = [], stdout, code, fails

    def __call__(self, argv, env=None, **kw):
        self.calls.append((argv, env))
        if argv[0] != "xdg-open" and self.fails:
            raise OSError("not there")
        return SimpleNamespace(returncode=self.code if argv[0] != "xdg-open" else 0, stdout=self.stdout if argv[0] != "xdg-open" else None)


class Prepare(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        t = self.tmp.name
        self.env = {"HOME": t, "XDG_CONFIG_HOME": os.path.join(t, "cfg"), "XDG_RUNTIME_DIR": os.path.join(t, "run"),
                    "XDG_CURRENT_DESKTOP": "Hyprland", "XDG_DATA_DIRS": "/usr/share"}
        os.makedirs(self.env["XDG_RUNTIME_DIR"])

    def tearDown(self):
        self.tmp.cleanup()

    def test_only_goa_signin_sends_web_links_here(self):
        env = ws.prepare(self.env, "/opt/tessera/web-signin.py", "/bin/tessera-web")
        self.assertEqual(env["XDG_CURRENT_DESKTOP"], "tessera-signin")
        rt = os.path.join(self.env["XDG_RUNTIME_DIR"], "tessera-signin")
        self.assertEqual(env["XDG_DATA_DIRS"], rt + ":/usr/share")
        self.assertEqual(env["TESSERA_WEB"], "/bin/tessera-web")
        entry = Path(rt, "applications", ws.HANDLER).read_text()
        self.assertIn('Exec="python3" -I "/opt/tessera/web-signin.py" open %u', entry)
        self.assertIn("x-scheme-handler/https;", entry)
        # (the list read only under that desktop name: nobody else's links change)
        lst = Path(self.env["XDG_CONFIG_HOME"], "tessera-signin-mimeapps.list").read_text()
        self.assertIn("x-scheme-handler/https=" + ws.HANDLER, lst)
        self.assertFalse(Path(self.env["XDG_CONFIG_HOME"], "mimeapps.list").exists())
        self.assertEqual(stat.S_IMODE(os.stat(rt).st_mode), 0o700)
        # (the caller's own environment untouched)
        self.assertEqual(self.env["XDG_CURRENT_DESKTOP"], "Hyprland")

    def test_the_environment_is_put_back_before_anything_opens(self):
        env = ws.restored(ws.prepare(self.env, "/s.py", "/bin/tessera-web"))
        self.assertEqual(env["XDG_CURRENT_DESKTOP"], "Hyprland")
        self.assertEqual(env["XDG_DATA_DIRS"], "/usr/share")
        self.assertNotIn("TESSERA_SIGNIN_DESKTOP", env)
        # (no desktop name before: none after)
        del self.env["XDG_CURRENT_DESKTOP"]
        self.assertNotIn("XDG_CURRENT_DESKTOP", ws.restored(ws.prepare(self.env, "/s.py", "/w")))

    def test_exec_arguments_are_quoted(self):
        self.assertEqual(ws.exec_quote('/a b/$x"y'), '"/a b/\\$x\\"y"')


class Answer(unittest.TestCase):
    def test_the_final_address_is_gos_redirect_only(self):
        self.assertEqual(ws.done_url('{"event":"progress"}\n{"event":"done","url":"%s"}\n' % DONE), DONE)
        self.assertIsNone(ws.done_url('{"event":"done","url":"https://evil.example/?code=1"}'))
        self.assertIsNone(ws.done_url('{"event":"closed"}'))
        self.assertIsNone(ws.done_url("not json"))
        self.assertTrue(ws.closed('{"event":"closed"}\n'))
        self.assertFalse(ws.closed(""))


class Open(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.NamedTemporaryFile(delete=False)
        self.tmp.close()
        self.env = {"TESSERA_WEB": self.tmp.name, "TESSERA_SIGNIN_DESKTOP": "Hyprland", "XDG_CURRENT_DESKTOP": "tessera-signin",
                    "TESSERA_SIGNIN_DATA": "/run/x/tessera-signin", "XDG_DATA_DIRS": "/run/x/tessera-signin:/usr/share"}

    def tearDown(self):
        os.unlink(self.tmp.name)

    def test_the_page_opens_in_tessera_web_and_its_answer_goes_to_goa(self):
        r = Runs(stdout=('{"event":"done","url":"%s"}\n' % DONE).encode())
        ws.open_url(AUTH, self.env, run=r)
        self.assertEqual(r.calls[0][0], [self.tmp.name, "--signin", AUTH, "--done", "goa-oauth2://", "--title", "Microsoft 365"])
        self.assertEqual(r.calls[1][0], ["xdg-open", DONE])
        # (both with the desktop's own environment: GOA's handler, not this again)
        self.assertEqual(r.calls[1][1]["XDG_CURRENT_DESKTOP"], "Hyprland")
        self.assertEqual(r.calls[1][1]["XDG_DATA_DIRS"], "/usr/share")

    def test_without_tessera_web_the_browser_opens_it(self):
        r = Runs(fails=True)
        ws.open_url(AUTH, self.env, run=r)
        self.assertEqual(r.calls[-1][0], ["xdg-open", AUTH])
        self.assertEqual(r.calls[-1][1]["XDG_CURRENT_DESKTOP"], "Hyprland")
        env = dict(self.env, TESSERA_WEB="")
        r = Runs()
        ws.open_url(AUTH, env, run=r)
        self.assertEqual(r.calls, [(["xdg-open", AUTH], r.calls[0][1])])

    def test_failing_to_start_falls_back_and_closing_cancels(self):
        r = Runs(stdout=b"", code=1)
        ws.open_url(AUTH, self.env, run=r)
        self.assertEqual(r.calls[-1][0], ["xdg-open", AUTH])
        r = Runs(stdout=b'{"event":"closed"}\n', code=0)
        ws.open_url(AUTH, self.env, run=r)
        self.assertEqual(len(r.calls), 1)

    def test_only_https_goes_to_tessera_web(self):
        r = Runs()
        ws.open_url("http://example.com/", self.env, run=r)
        self.assertEqual([c[0] for c in r.calls], [["xdg-open", "http://example.com/"]])


if __name__ == "__main__":
    unittest.main()
