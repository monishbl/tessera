"""The theme and icon scripts' behaviour (audit #961), each with a throwaway
home: desktop-theme.py's Qt half (apply_kde: kdeglobals, dolphinrc, qt6ct,
and a scheme of yours left alone), icon-index.py (the chain, the best file,
an icon only in your own hicolor), app-energy.py (/proc read, grouped, the
hour's deltas and the top apps) and conflicts.py (another notifier, bar,
duplicate recorder or session unit).
"""
import configparser
import importlib.machinery
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "shell/assets/scripts"


def load(name, file):
    loader = importlib.machinery.SourceFileLoader(name + "_" + os.urandom(3).hex(), str(SCRIPTS / file))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def ini(path):
    p = configparser.ConfigParser(interpolation=None, strict=False)
    p.optionxform = str
    p.read(path)
    return p


BREEZE_DARK = """[General]
Name=Breeze Dark

[Colors:View]
BackgroundNormal=20,22,24
BackgroundAlternate=29,31,34
ForegroundNormal=252,252,252
ForegroundLink=29,153,243

[Colors:Window]
BackgroundNormal=32,35,38
ForegroundNormal=252,252,252
ForegroundInactive=161,169,177

[Colors:Button]
BackgroundNormal=41,44,48
ForegroundNormal=252,252,252

[Colors:Selection]
BackgroundNormal=61,174,233
ForegroundNormal=252,252,252

[ColorEffects:Disabled]
Color=56,56,56
"""


class Kde(unittest.TestCase):
    """desktop-theme.py apply_kde: the Qt and KDE palette"""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.cfg, self.data, self.sys = d / "home/.config", d / "home/.local/share", d / "usr/share"
        for p in (self.cfg, self.data, self.sys / "color-schemes"):
            p.mkdir(parents=True)
        (self.sys / "color-schemes/BreezeDark.colors").write_text(BREEZE_DARK)
        self.env = mock.patch.dict(os.environ, {"HOME": str(d / "home"), "XDG_CONFIG_HOME": str(self.cfg),
                                                "XDG_DATA_HOME": str(self.data), "XDG_DATA_DIRS": str(self.sys),
                                                "PATH": str(d / "nobin")})
        self.env.start()
        self.m = load("desktop_theme", "desktop-theme.py")

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def test_breeze_is_written_everywhere_qt_reads_it(self):
        (self.cfg / "kdeglobals").write_text("[General]\nColorScheme=Breeze\nfont=Inter,10\n\n[KDE]\nwidgetStyle=Breeze\n")
        self.m.apply_kde("dark")
        k = ini(self.cfg / "kdeglobals")
        self.assertEqual(k["General"]["ColorScheme"], "BreezeDark")
        self.assertEqual(k["Colors:View"]["BackgroundNormal"], "20,22,24")
        self.assertEqual(k["ColorEffects:Disabled"]["Color"], "56,56,56")
        # (only palette keys: the font and the style stay)
        self.assertEqual(k["General"]["font"], "Inter,10")
        self.assertEqual(k["KDE"]["widgetStyle"], "Breeze")
        self.assertEqual(ini(self.cfg / "dolphinrc")["UiSettings"]["ColorScheme"], "BreezeDark")
        qt = ini(self.cfg / "qt6ct/qt6ct.conf")["Appearance"]
        self.assertEqual(qt["custom_palette"], "true")
        self.assertEqual(qt["color_scheme_path"], str(self.cfg / "qt6ct/colors/TesseraDesktop.conf"))
        colors = ini(self.cfg / "qt6ct/colors/TesseraDesktop.conf")["ColorScheme"]["active_colors"].split(", ")
        # window text, then the window's own colour, as qt6ct orders them
        self.assertEqual((colors[0], colors[2], colors[9], colors[12]), ("#fcfcfc", "#202326", "#141618", "#3daee9"))
        # (Tooltip isn't in this scheme: the window's colours)
        self.assertEqual(colors[18], "#202326")
        self.assertEqual(len(colors), 22)

    def test_a_scheme_of_yours_stays(self):
        mine = "[General]\nColorScheme=Nordic\n\n[Colors:View]\nBackgroundNormal=1,2,3\n"
        (self.cfg / "kdeglobals").write_text(mine)
        (self.cfg / "dolphinrc").write_text("[UiSettings]\nColorScheme=Nordic\n")
        (self.cfg / "qt6ct").mkdir()
        (self.cfg / "qt6ct/qt6ct.conf").write_text("[Appearance]\ncolor_scheme_path=/usr/share/qt6ct/colors/darker.conf\ncustom_palette=true\n")
        self.m.apply_kde("dark")
        self.assertEqual((self.cfg / "kdeglobals").read_text(), mine)
        self.assertEqual(ini(self.cfg / "dolphinrc")["UiSettings"]["ColorScheme"], "Nordic")
        self.assertEqual(ini(self.cfg / "qt6ct/qt6ct.conf")["Appearance"]["color_scheme_path"], "/usr/share/qt6ct/colors/darker.conf")
        self.assertFalse((self.cfg / "qt6ct/colors/TesseraDesktop.conf").exists())

    def test_ours_switch_and_nothing_changed_writes_nothing(self):
        self.m.apply_kde("dark")
        before = (self.cfg / "kdeglobals").stat().st_mtime_ns
        self.assertFalse(self.m.update_ini(self.cfg / "kdeglobals", {"General": {"ColorScheme": "BreezeDark"}}))
        self.m.apply_kde("dark")
        self.assertEqual((self.cfg / "kdeglobals").stat().st_mtime_ns, before)
        # a Match-apps palette (Tessera.conf) is ours to replace
        self.assertTrue(self.m.OURS.match("TesseraMatugen"))
        self.assertFalse(self.m.OURS.match("Nordic"))

    def test_no_breeze_a_tessera_scheme_of_its_own(self):
        (self.sys / "color-schemes/BreezeDark.colors").unlink()
        self.m.apply_kde("light")
        made = self.data / "color-schemes/TesseraDesktopLight.colors"
        self.assertTrue(made.is_file())
        self.assertEqual(ini(self.cfg / "kdeglobals")["General"]["ColorScheme"], "TesseraDesktopLight")
        self.assertEqual(ini(made)["Colors:View"]["BackgroundNormal"], "255,255,255")

    def test_an_incomplete_scheme_is_refused(self):
        bad = Path(self.tmp.name) / "bad.colors"
        bad.write_text("[Colors:Window]\nBackgroundNormal=1,2,3\n")
        with self.assertRaises(ValueError):
            self.m.apply_kde("dark", bad)
        self.assertFalse((self.cfg / "kdeglobals").exists())


class IconIndex(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = self.d = Path(self.tmp.name)
        self.user = d / "home/.local/share/icons"
        self.sys = d / "usr/share/icons"
        self.env = dict(os.environ, HOME=str(d / "home"), XDG_DATA_HOME=str(d / "home/.local/share"), XDG_DATA_DIRS=str(d / "usr/share"))

    def tearDown(self):
        self.tmp.cleanup()

    def icon(self, root, rel):
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text("<svg/>" if rel.endswith(".svg") else "png")
        return p

    def theme(self, root, name, inherits=""):
        (root / name).mkdir(parents=True, exist_ok=True)
        (root / name / "index.theme").write_text("[Icon Theme]\nName=" + name + ("\nInherits=" + inherits if inherits else "") + "\n")

    def index(self, theme, cache=None):
        args = [sys.executable, "-I", str(SCRIPTS / "icon-index.py"), theme] + ([str(cache)] if cache else [])
        r = subprocess.run(args, env=self.env, capture_output=True, text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        text = Path(cache).read_text() if cache else r.stdout
        return dict(l.split("\t", 1) for l in text.splitlines() if "\t" in l)

    def test_the_chain_the_best_file_and_your_own_hicolor(self):
        self.theme(self.sys, "hicolor")
        self.theme(self.sys, "Base")
        self.theme(self.sys, "Mine", "Base")
        small = self.icon(self.sys, "Mine/16x16/apps/firefox.png")
        big = self.icon(self.sys, "Mine/scalable/apps/firefox.svg")
        self.icon(self.sys, "Mine/scalable/apps/firefox-symbolic.svg")
        sym = self.icon(self.sys, "Mine/symbolic/apps/term.svg")
        full = self.icon(self.sys, "Mine/48x48/apps/term.png")
        inherited = self.icon(self.sys, "Base/48x48/apps/files.png")
        self.icon(self.sys, "hicolor/48x48/apps/files.png")
        # (Steam's, web apps' and AppImages' icons: your hicolor has no index.theme; audit #124)
        steam = self.icon(self.user, "hicolor/256x256/apps/steam_icon_570.png")
        got = self.index("Mine")
        self.assertEqual(got["firefox"], str(big))
        self.assertNotEqual(got["firefox"], str(small))
        self.assertEqual(got["term"], str(full), "full colour over symbolic")
        self.assertNotEqual(got["term"], str(sym))
        self.assertEqual(got["files"], str(inherited), "the theme it inherits comes before hicolor")
        self.assertEqual(got["steam_icon_570"], str(steam))

    def test_the_cache_is_kept_until_an_icon_comes(self):
        self.theme(self.sys, "hicolor")
        self.icon(self.sys, "hicolor/48x48/apps/a.png")
        cache = self.d / "cache/icons.tsv"
        self.assertIn("a", self.index("hicolor", cache))
        # up to date: not rewritten
        old = time.time() - 60
        os.utime(cache, (old, old))
        for dirpath, _dirs, _files in os.walk(self.sys):
            os.utime(dirpath, (old - 60, old - 60))
        self.index("hicolor", cache)
        self.assertAlmostEqual(cache.stat().st_mtime, old, delta=1)
        # a new icon: its folder is newer, the cache is made again
        self.icon(self.sys, "hicolor/48x48/apps/b.png")
        self.assertIn("b", self.index("hicolor", cache))

    def test_score(self):
        m = load("icon_index", "icon-index.py")
        self.assertGreater(m.score("/i/scalable/apps/x.svg"), m.score("/i/256x256/apps/x.png"))
        self.assertGreater(m.score("/i/64x64/apps/x.png"), m.score("/i/16x16/apps/x.png"))
        self.assertLess(m.score("/i/scalable/apps/x-symbolic.svg"), m.score("/i/16x16/apps/x.png"))


def stat_line(pid, comm, ppid, utime, stime, start):
    # pid (comm) state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime stime cutime cstime prio nice threads itreal starttime …
    f = ["S", ppid, 0, 0, 0, 0, 0, 0, 0, 0, 0, utime, stime, 0, 0, 20, 0, 1, 0, start, 0, 0]
    return f"{pid} ({comm}) " + " ".join(str(x) for x in f) + "\n"


class AppEnergy(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.proc = Path(self.tmp.name) / "proc"
        self.m = load("app_energy", "app-energy.py")

    def tearDown(self):
        self.tmp.cleanup()

    def fake(self, pid, comm, ppid, ticks, start, cgroup):
        d = self.proc / str(pid)
        d.mkdir(parents=True)
        (d / "stat").write_text(stat_line(pid, comm, ppid, ticks, 0, start))
        (d / "cgroup").write_text("0::/user.slice/user-1000.slice/user@1000.service/app.slice/" + cgroup + "\n")

    def test_read_and_grouped_by_app(self):
        self.fake(100, "Hyprland", 1, 50, 10, "session.scope")
        self.fake(200, "firefox", 100, 900, 20, "app-firefox-1234.scope")
        self.fake(201, "Isolated Web Co", 200, 400, 21, "app-firefox-1234.scope")
        self.fake(300, "code", 100, 300, 30, "session.scope")
        self.fake(301, "code helper", 300, 100, 31, "session.scope")
        self.fake(400, "cargo", 100, 70, 40, "session.scope")
        (self.proc / "self").mkdir()
        procs = self.m.read_procs(str(self.proc))
        self.assertEqual(procs[201], (200, 21, 400, "Isolated Web Co", "app-firefox-1234.scope"))
        keys = self.m.group(procs, {300: "Code"})
        self.assertEqual((keys[200], keys[201]), ("app:firefox", "app:firefox"))
        # (the window's process and its helpers: the window's class)
        self.assertEqual((keys[300], keys[301]), ("class:Code", "class:Code"))
        # (below the session only: its own name)
        self.assertEqual(keys[400], "proc:cargo")

    def test_scopes(self):
        s = self.m.scope_app
        self.assertEqual(s("app-org.gnome.Nautilus-4567.scope"), "org.gnome.Nautilus")
        self.assertEqual(s("app-flatpak-com.spotify.Client-99.scope"), "com.spotify.Client")
        self.assertEqual(s("app-kitty@3.service"), "kitty")
        self.assertIsNone(s("session-2.scope"))

    def test_the_hours_deltas_and_top(self):
        m = self.m
        tick = m.TICK
        boot = time.time() - float(open("/proc/uptime").read().split()[0])
        since = time.time() - 300
        old_start = 1  # (running since boot: before the last look)
        new_start = int((since - boot + 60) * tick)  # (started a minute after the last look)
        procs = {10: (1, old_start, 1000, "a", ""), 11: (1, new_start, 200, "b", ""), 12: (1, old_start, 50, "c", ""),
                 13: (1, old_start + 5, 800, "d", "")}
        prev = {"10": [old_start, 400], "13": [old_start, 100]}
        keys = {10: "app:a", 11: "app:b", 12: "app:c", 13: "app:d"}
        d = m.deltas(prev, procs, keys, since)
        # seen before: its growth; new since: all of it; running before the
        # first look and never seen: nothing; a reused pid started before: nothing
        self.assertEqual(d, {"app:a": 600, "app:b": 200})
        now = time.time()
        ring = [{"t": now - 4000, "apps": {"app:old": 100 * tick}}, {"t": now - 600, "apps": {"app:a": 30 * tick, "app:b": 0.5 * tick}}]
        got = m.top(ring, {"app:a": 10 * tick, "app:c": 5 * tick}, now)
        # (older than the hour: out; under a second: out; the most first)
        self.assertEqual(got, [{"key": "app:a", "seconds": 40.0}, {"key": "app:c", "seconds": 5.0}])
        self.assertEqual(len(m.top([], {f"app:{i}": (i + 1) * tick for i in range(9)}, now)), 5)


class Conflicts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.bin = Path(self.tmp.name) / "bin"
        self.bin.mkdir()
        self.sleeper = None

    def tearDown(self):
        if self.sleeper:
            self.sleeper.kill()
            self.sleeper.wait()
        self.tmp.cleanup()

    def fake(self, name, out):
        f = self.bin / name
        f.write_text("#!/bin/sh\ncat <<'EOF'\n" + out + "\nEOF\n")
        f.chmod(0o755)

    def run_it(self):
        env = {"PATH": str(self.bin) + ":/usr/bin:/bin", "HOME": self.tmp.name}
        r = subprocess.run([sys.executable, "-I", str(SCRIPTS / "conflicts.py")], env=env, capture_output=True, text=True, timeout=30)
        self.assertEqual(r.returncode, 0, r.stderr)
        return {f["id"]: f for f in json.loads(r.stdout)}

    def test_a_notifier_a_bar_and_a_unit(self):
        self.sleeper = subprocess.Popen(["sleep", "30"])
        self.fake("busctl", f"u {self.sleeper.pid}")
        self.fake("hyprctl", json.dumps({"eDP-1": {"levels": {"2": [
            {"namespace": "waybar", "pid": 0}, {"namespace": "tessera:bar", "pid": 0}, {"namespace": "tessera-dock", "pid": 0},
            {"namespace": "waybar", "pid": 0}, {"namespace": "hyprpaper", "pid": 0}]}}}))
        units = {"waybar": "enabled", "mako": "enabled"}
        self.fake("systemctl", "\n".join(units.get(u, "disabled") for u in ("hypridle", "waybar", "cliphist", "swaync", "mako", "dunst", "ags", "hyprpanel")))
        got = self.run_it()
        self.assertIn("notifications:sleep", got)
        self.assertEqual(got["bar:waybar"]["short"], "Draws a second bar or shell")
        # (Tessera's own layers and a wallpaper daemon aren't bars; waybar once)
        self.assertFalse(any(k.startswith("bar:") and k != "bar:waybar" for k in got))
        # the running waybar is said once; mako only as a unit
        self.assertNotIn("unit:waybar", got)
        self.assertEqual(got["unit:mako"]["fix"], "systemctl --user disable --now mako")

    def test_nothing_found_is_an_empty_list(self):
        self.fake("busctl", "")
        self.fake("hyprctl", "{}")
        self.fake("systemctl", "")
        got = self.run_it()
        self.assertFalse(any(k.startswith(("notifications:", "bar:", "unit:")) for k in got))

    def test_duplicate_idlers_and_recorders(self):
        m = load("conflicts", "conflicts.py")
        procs = {1: ("hypridle", []), 2: ("hypridle", []), 3: ("wl-paste", ["wl-paste", "--type", "text", "--watch", "cliphist", "store"]),
                 4: ("wl-paste", ["wl-paste", "--type", "image", "--watch", "cliphist", "store"])}
        with mock.patch.object(m, "processes", lambda: iter(procs)), mock.patch.object(m, "comm", lambda p: procs[p][0]), \
                mock.patch.object(m, "cmdline", lambda p: procs[p][1]), mock.patch.object(m, "ours", lambda p: True):
            found = []
            m.duplicates(found)
            self.assertEqual([f["id"] for f in found], ["hypridle"])
            self.assertIn("2 times", found[0]["short"])
            # (one per kind is right; an untyped one next to a typed one saves twice)
            procs[5] = ("wl-paste", ["wl-paste", "--watch", "cliphist", "store"])
            procs.pop(2)
            found = []
            m.duplicates(found)
            self.assertEqual([f["id"] for f in found], ["cliphist"])


if __name__ == "__main__":
    unittest.main()
