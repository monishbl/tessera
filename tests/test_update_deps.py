"""Dependencies on update (docs/releases.md): every way Tessera is updated
leaves the machine with every package the new version needs, at a version
it supports, before that version runs. No real pacman, pkexec or install:

  tools/tessera_deps.py     packages.sh's MINIMUM block and compat.json read
                            as text; the decision (missing, too old, too new,
                            not in the repositories, AUR) against a fake pacman
  tools/tessera-packages.sh status: the too-old ones join the missing (one
                            install upgrades them), a bundle's folder is read
  tools/tessera-update.py   each path through the shared step (deps_needs):
                            stage (Software Update, "auto"), a bundle's stage
                            (with the lock's run, one password), cli (asked
                            in the terminal), finish (the restart and the
                            login: checked only, never applied when unmet),
                            apply (the shell stopped: checked only)
  update-run.sh, launch-shell.sh, tools/tessera, install.sh: through them
"""
import importlib.machinery
import importlib.util
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
HAS_CRYPTO = importlib.util.find_spec("cryptography") is not None


def load(name, path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


D = load("deps_t", ROOT / "tools/tessera_deps.py")
B = load("deps_bundle_t", ROOT / "tools/tessera-bundle.py")  # (pacman's vercmp, exactly)


class FakePacman:
    """pacman -T/-Q/-Si and vercmp, from {name: version} installed and in
    the repositories; provides: {provided name: (package, version)}"""

    def __init__(self, installed=None, repo=None, provides=None):
        self.installed, self.repo, self.provides = installed or {}, repo or {}, provides or {}
        self.calls = []

    def satisfied(self, dep):
        m = re.match(r"^(.+?)(>=|<=|<|>|=)(.+)$", dep)
        name, op, want = (m.group(1), m.group(2), m.group(3)) if m else (dep, "", "")
        have = self.installed.get(name)
        if have is None and name in self.provides:
            have = self.provides[name][1]
        if have is None:
            return False
        if not op:
            return True
        c = B.vercmp(have, want)
        return {">=": c >= 0, ">": c > 0, "<=": c <= 0, "<": c < 0, "=": c == 0}[op]

    def __call__(self, cmd, **kw):
        self.calls.append(cmd)
        if cmd[0] == "vercmp":
            return subprocess.CompletedProcess(cmd, 0, str(B.vercmp(cmd[1], cmd[2])) + "\n", "")
        args = [a for a in cmd[2:] if a != "--"]
        if cmd[1] == "-T":
            out = [d for d in args if not self.satisfied(d)]
            return subprocess.CompletedProcess(cmd, 127 if out else 0, "".join(d + "\n" for d in out), "")
        if cmd[1] == "-Q":
            n = args[0]
            if n in self.installed:
                return subprocess.CompletedProcess(cmd, 0, f"{n} {self.installed[n]}\n", "")
            return subprocess.CompletedProcess(cmd, 1, "", f"error: package '{n}' was not found\n")
        if cmd[1] == "-Si":
            n = args[0]
            if n in self.repo:
                return subprocess.CompletedProcess(cmd, 0, f"Repository      : extra\nName            : {n}\nVersion         : {self.repo[n]}\n", "")
            return subprocess.CompletedProcess(cmd, 1, "", f"error: package '{n}' was not found\n")
        raise AssertionError(cmd)


LIST = """
PACKAGES=(
    # Compositor
    hyprland quickshell   # the shell
    jq "inter-font"
)
AUR=(bibata-cursor-theme-bin)
MINIMUM=(
    "jq>=1.7.1"     # (a comment)
    'python-cryptography>=42'
    not-a-requirement
    "--evil>=1"
)
DROPPED=(old-theme)
"""
COMPAT = json.dumps({"hyprland": {"min": "0.56.0", "max": "0.57.x"}, "quickshell": {"min": "0.3.2", "bundled": "0.3.2"}})


# ── Reading the lists ────────────────────────────────────────────────────

class Reading(unittest.TestCase):
    def test_minimum_block_is_read_as_text_and_only_well_formed_entries_count(self):
        self.assertEqual(D.minimums(LIST), {"jq": [(">=", "1.7.1")], "python-cryptography": [(">=", "42")]})
        self.assertEqual(D.names(LIST, "PACKAGES"), ["hyprland", "quickshell", "jq", "inter-font"])
        self.assertEqual(D.names(LIST, "AUR"), ["bibata-cursor-theme-bin"])
        self.assertEqual(D.minimums("PACKAGES=(a)\n"), {}, "no MINIMUM block: no minimums")
        self.assertEqual(D.minimums('MINIMUM=( "a>=1" "b<2" )\n'), {"a": [(">=", "1")], "b": [("<", "2")]}, "one line")

    def test_compat_ranges(self):
        lim, soft = D.compat_limits(COMPAT)
        self.assertEqual(lim["hyprland"], [(">=", "0.56.0"), ("<", "0.58")], "0.57.x: anything below 0.58")
        self.assertEqual(lim["quickshell"], [(">=", "0.3.2")])
        self.assertEqual(soft, {"quickshell"}, "Tessera's own Quickshell build can stand in")
        self.assertEqual(D.max_limit("0.57.1"), ("<=", "0.57.1"))
        self.assertEqual(D.compat_limits("not json"), ({}, set()))

    def test_requirements_merge_both_and_a_minimum_entry_makes_it_hard(self):
        r = D.requirements(LIST, COMPAT)
        self.assertEqual(set(r), {"jq", "python-cryptography", "hyprland", "quickshell"})
        self.assertTrue(r["quickshell"]["soft"])
        r = D.requirements('MINIMUM=( "quickshell>=0.4" )\n', COMPAT)
        self.assertFalse(r["quickshell"]["soft"])
        self.assertEqual(r["quickshell"]["limits"], [(">=", "0.4"), (">=", "0.3.2")])

    def test_the_real_list_parses_the_same_in_bash_and_python(self):
        text = (ROOT / "tools/packages.sh").read_text()
        self.assertIn("MINIMUM=(", text)
        self.assertGreater(len(D.names(text, "PACKAGES")), 50)
        for w in D.block(text, "MINIMUM"):
            self.assertRegex(w, D.REQ.pattern, "every MINIMUM entry is name>=version")
        # (sourcing it, as install.sh and setup-tty.sh do, still works)
        r = subprocess.run(["bash", "-c", 'source "$1"; echo "${#PACKAGES[@]} ${#MINIMUM[@]}"', "_", str(ROOT / "tools/packages.sh")],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(int(r.stdout.split()[0]), len(D.names(text, "PACKAGES")))


# ── The decision ─────────────────────────────────────────────────────────

class Decision(unittest.TestCase):
    OK = {"hyprland": "0.57.2-1", "quickshell": "0.3.2-1", "jq": "1.7.1-2", "inter-font": "4.0-1",
          "python-cryptography": "44.0.0-1", "bibata-cursor-theme-bin": "2.0.7-1"}

    def assess(self, installed=None, repo=None, status=None, current=LIST, own_qs=False, skip=(), provides=None, text=LIST):
        self.pac = FakePacman(dict(self.OK, **(installed or {})), repo or {}, provides)
        for k, v in (installed or {}).items():
            if v is None:
                del self.pac.installed[k]
        return D.assess(status or {"missing": [], "unavailable": [], "dropped": []}, text, COMPAT, current, self.pac, own_qs, skip)

    def test_all_there_nothing_to_do(self):
        p = self.assess()
        self.assertEqual((p["install"], p["blocked"], p["outdated"], p["newer"], p["aur"]), ([], [], [], [], []))

    def test_missing_ones_are_installed(self):
        p = self.assess(installed={"inter-font": None}, repo={"inter-font": "4.1-1"}, status={"missing": ["inter-font"], "unavailable": []})
        self.assertEqual(p["install"], ["inter-font"])
        self.assertEqual(p["blocked"], [])

    def test_too_old_is_upgraded_when_the_repositories_have_one_that_fits(self):
        p = self.assess(installed={"jq": "1.6-4"}, repo={"jq": "1.7.1-2"})
        self.assertEqual(p["install"], ["jq"])
        self.assertEqual([(o["name"], o["have"], o["repo"]) for o in p["outdated"]], [("jq", "1.6-4", "1.7.1-2")])
        self.assertIn("jq (1.6 → 1.7.1, needs 1.7.1 or newer)", D.needs_words(p))

    def test_too_old_with_nothing_newer_in_the_repositories_stops_it(self):
        p = self.assess(installed={"hyprland": "0.55.1-1"}, repo={"hyprland": "0.55.1-1"})
        self.assertEqual(p["install"], [])
        self.assertEqual([(b["name"], b["why"]) for b in p["blocked"]], [("hyprland", "too old")])
        msg = D.blocked_words(p, "1.4")
        for words in ("Tessera 1.4 can't be installed yet", "hyprland 0.56.0 or newer", "this computer has 0.55.1",
                      "your repositories have 0.55.1", "sudo pacman -Syu", "Nothing was changed"):
            self.assertIn(words, msg)

    def test_too_old_and_the_repositories_only_have_one_above_the_max_stops_it(self):
        p = self.assess(installed={"hyprland": "0.55.1-1"}, repo={"hyprland": "0.58.0-1"})
        self.assertEqual([(b["name"], b["why"]) for b in p["blocked"]], [("hyprland", "too new")])
        self.assertIn("isn't tested with", D.blocked_words(p))
        self.assertEqual(p["install"], [], "never a Hyprland above the max")

    def test_a_missing_one_whose_repository_version_does_not_fit_stops_it(self):
        p = self.assess(installed={"jq": None}, repo={"jq": "1.6-4"}, status={"missing": ["jq"], "unavailable": []})
        self.assertEqual(p["install"], [])
        self.assertEqual([(b["name"], b["why"]) for b in p["blocked"]], [("jq", "too old")])

    def test_installed_above_the_max_is_a_note_not_a_reason_to_stay_behind(self):
        p = self.assess(installed={"hyprland": "0.58.1-1"})
        self.assertEqual(p["blocked"], [])
        self.assertEqual([n["name"] for n in p["newer"]], ["hyprland"])
        self.assertTrue(any("newer than this Tessera is tested with" in l for l in D.notes_words(p)))

    def test_provided_by_another_package_counts(self):
        p = self.assess(installed={"hyprland": None}, provides={"hyprland": ("hyprland-git", "0.57.9")})
        self.assertEqual((p["install"], p["blocked"]), ([], []))

    def test_unavailable_new_requirement_stops_it_but_one_already_listed_stays_left_out(self):
        new = LIST.replace("jq \"inter-font\"", "jq \"inter-font\" brand-new")
        p = self.assess(text=new, installed={"brand-new": None}, status={"missing": [], "unavailable": ["brand-new"]})
        self.assertEqual([(b["name"], b["why"]) for b in p["blocked"]], [("brand-new", "unavailable")])
        self.assertIn("isn't in your repositories (pacman -Si brand-new)", D.blocked_words(p))
        p = self.assess(text=new, current=new, installed={"brand-new": None}, status={"missing": [], "unavailable": ["brand-new"]})
        self.assertEqual((p["blocked"], p["left_out"]), ([], ["brand-new"]), "the installed version ran without it")

    def test_unavailable_with_a_minimum_always_stops_it(self):
        p = self.assess(installed={"python-cryptography": None}, status={"missing": [], "unavailable": ["python-cryptography"]})
        self.assertEqual([b["name"] for b in p["blocked"]], ["python-cryptography"])

    def test_aur_ones_are_a_note_never_installed_by_an_update(self):
        p = self.assess(installed={"bibata-cursor-theme-bin": None})
        self.assertEqual((p["aur"], p["install"], p["blocked"]), (["bibata-cursor-theme-bin"], [], []))
        self.assertTrue(any("yay -S bibata-cursor-theme-bin" in l for l in D.notes_words(p)))

    def test_quickshell_too_old_is_fine_with_tesseras_own_build(self):
        p = self.assess(installed={"quickshell": "0.2.1-1"}, repo={"quickshell": "0.2.1-1"}, own_qs=True)
        self.assertEqual(p["blocked"], [])
        p = self.assess(installed={"quickshell": "0.2.1-1"}, repo={"quickshell": "0.2.1-1"}, own_qs=False)
        self.assertEqual([b["name"] for b in p["blocked"]], ["quickshell"])

    def test_a_bundles_held_packages_are_the_locks_to_move(self):
        p = self.assess(installed={"hyprland": "0.55.1-1"}, repo={"hyprland": "0.55.1-1"}, skip=["hyprland"])
        self.assertEqual(p["blocked"], [])

    def test_dropped_ones_pass_through(self):
        self.assertEqual(self.assess(status={"missing": [], "dropped": ["old-theme"]})["dropped"], ["old-theme"])


# ── tessera-packages.sh status, with a fake pacman ─────────────────────────

class PackagesScript(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        t = Path(self.tmp.name)
        self.bin = t / "bin"
        self.bin.mkdir()
        self.ver = t / "ver"
        (self.ver / "tools").mkdir(parents=True)
        (self.ver / "tools/packages.sh").write_text('PACKAGES=( hyprland jq )\nMINIMUM=( "jq>=1.7" )\nDROPPED=()\n')
        (self.ver / "compat.json").write_text(json.dumps({"hyprland": {"min": "0.56.0", "max": "0.57.x"}}))
        # jq 1.6 installed, 1.7.1 in the repositories; hyprland 0.57.2 (fine)
        self.fake("pacman", '''
            [ "$2" = -- ] && set -- "$1" "${@:3}"
            case "$1" in
                -T) shift; for d; do case "$d" in hyprland|"hyprland>=0.56.0"|"hyprland<0.58"|jq) ;; *) echo "$d" ;; esac; done ;;
                -Q) case "$2" in jq) echo "jq 1.6-4" ;; hyprland) echo "hyprland 0.57.2-1" ;; *) exit 1 ;; esac ;;
                -Qq) exit 1 ;;
                -Si) case "$2" in jq) printf 'Name : jq\\nVersion : 1.7.1-2\\n' ;; *) exit 1 ;; esac ;;
                -Sg) exit 1 ;;
            esac
        ''', bash=True)
        # (pacman's vercmp: tessera-bundle.py's exact copy of it)
        (t / "vercmp.py").write_text("import importlib.util, sys\n"
                                     f"spec = importlib.util.spec_from_file_location('b', {str(ROOT / 'tools/tessera-bundle.py')!r})\n"
                                     "B = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(B)\n"
                                     "print(B.vercmp(sys.argv[1], sys.argv[2]))\n")
        self.fake("vercmp", f'exec python3 -I "{t / "vercmp.py"}" "$1" "$2"\n')
        self.fake("systemctl", "exit 3\n")
        self.env = dict(os.environ, PATH=str(self.bin) + ":" + os.environ["PATH"], XDG_DATA_HOME=str(t / "data"))

    def tearDown(self):
        self.tmp.cleanup()

    def fake(self, name, script, bash=False):
        f = self.bin / name
        f.write_text(("#!/bin/bash\n" if bash else "#!/bin/sh\n") + textwrap.dedent(script))
        f.chmod(0o755)

    def test_a_too_old_one_joins_the_missing_and_a_folder_is_read(self):
        p = subprocess.run([str(ROOT / "tools/tessera-packages.sh"), "status", str(self.ver)], env=self.env, capture_output=True, text=True, timeout=60)
        out = json.loads(p.stdout)
        self.assertEqual(out["outdated"], ["jq"])
        self.assertIn("jq", out["missing"], "whatever installs the missing ones upgrades it (Settings, one password)")
        self.assertEqual(out["blocked"], [])


# ── tessera-update.py: every path through the shared step ───────────────────

bg = load("deps_bg_t", ROOT / "tests/test_background_update.py")


class Paths(unittest.TestCase):
    """A git install's updates (test_background_update's Flow fixture: a
    real remote and clone; its own tests stay there)"""
    commit, head, tearDown = bg.Flow.commit, bg.Flow.head, bg.Flow.tearDown

    def setUp(self):
        bg.Flow.setUp(self)
        self.deps = []
        self.root = []
        self.unmet = None

        def needs(rev, mode="auto", ask=None, latest="", skip=()):
            self.deps.append((rev, mode, ask))
            if self.unmet:
                raise self.m.DepsUnmet(self.unmet)
            return (["inter-font"], []) if mode != "check" else ([], [])
        self.m.deps_needs = needs
        self.m.packages_step = lambda vdir, system, say=None, install=(), lock=True, remove=(): \
            self.root.append((vdir, system, list(install), lock, list(remove)))

    def target(self):
        return self.m.check(str(self.config))["target"]

    def test_stage_installs_what_the_new_version_needs_before_making_it_ready(self):
        target = self.target()
        out = self.m.stage(str(self.config), {})
        self.assertEqual(out["state"], "ready")
        self.assertEqual(self.deps[0][:2], (target, "auto"), "the new version's list, not this one's")
        self.assertEqual(self.root, [(str(self.repo), False, ["inter-font"], False, [])], "sysupdate.py --install, one password")

    def test_stage_stops_and_stages_nothing_when_the_new_version_cant_run(self):
        self.target()
        self.unmet = "Tessera 2 can't be installed yet. It needs hyprland 0.56.0 or newer…"
        with self.assertRaises(SystemExit) as e:
            self.m.stage(str(self.config), {})
        self.assertIn("hyprland", str(e.exception))
        self.assertEqual(self.root, [])
        self.assertFalse(self.m.read_staged())
        self.assertEqual(self.head(), "two")

    def test_finish_checks_again_and_never_applies_an_update_whose_packages_went(self):
        self.target()
        self.m.stage(str(self.config), {})
        staged_to = self.m.read_staged()["to"]
        self.unmet = "Tessera 2 needs packages that aren't installed yet: inter-font. Open Settings → Software Update…"
        with self.assertRaises(SystemExit):
            self.m.finish(str(self.config), if_ready=True)
        self.assertEqual(self.deps[-1][:2], (staged_to, "check"), "the login and the restart can't ask a password")
        self.assertEqual(self.head(), "two", "not applied")
        self.assertFalse(self.m.read_staged(), "left for Software Update to offer again")
        st = json.loads((Path(self.m.job_dir()) / "status.json").read_text())
        self.assertEqual(st["state"], "error")
        self.assertIn("Software Update", st["error"])

    def test_finish_applies_when_everything_is_there(self):
        self.target()
        self.m.stage(str(self.config), {})
        r = self.m.finish(str(self.config))
        self.assertTrue(r["applied"], r)
        self.assertEqual(self.deps[-1][1], "check")

    def test_apply_with_the_shell_stopped_only_checks(self):
        self.target()
        self.unmet = "needs inter-font"
        with self.assertRaises(SystemExit):
            self.m.apply(str(self.config), {})
        self.assertEqual(self.deps[-1][1], "check")
        self.assertEqual(self.head(), "two")
        self.unmet = None
        r = self.m.apply(str(self.config), {})
        self.assertTrue(r["applied"], r)

    def test_the_cli_asks_in_the_terminal_and_unattended_it_cannot(self):
        got = []
        self.m.update_mode = lambda: "git"
        self.m.check = lambda *a, **k: {"available": True, "current": "1", "latest": "2", "channelName": "Beta", "conflicts": []}
        self.m.stage = lambda *a, **k: got.append(k) or {"staged": True}
        with mock.patch("sys.stdout", new_callable=io.StringIO), mock.patch("sys.stdin.isatty", return_value=True), \
                mock.patch("sys.stdout.isatty", return_value=True):
            self.assertEqual(self.m.cli_update(str(self.config)), 0)
        self.assertEqual((got[-1]["deps"], got[-1]["ask"]), ("ask", self.m.terminal_ask))
        with mock.patch("sys.stdin.isatty", return_value=False), mock.patch("sys.stdout", new_callable=io.StringIO):
            self.m.cli_update(str(self.config))
        self.assertEqual((got[-1]["deps"], got[-1]["ask"]), ("ask", None))


class Step(unittest.TestCase):
    """deps_needs itself: what each mode does with a plan"""

    def setUp(self):
        self.m = bg.load_update()
        self.plan = {"install": ["inter-font", "jq"], "missing": ["inter-font"], "blocked": [], "newer": [], "left_out": [], "aur": [],
                     "outdated": [{"name": "jq", "have": "1.6-4", "repo": "1.7.1-2", "need": "1.7.1 or newer"}],
                     "dropped": ["old-theme"], "reasons": {"inter-font": "Fonts"}}
        self.m.deps_plan = lambda rev, skip=(): self.plan

    def test_auto_returns_the_names(self):
        self.assertEqual(self.m.deps_needs("x", "auto"), (["inter-font", "jq"], []))

    def test_check_stops_with_what_to_do(self):
        with self.assertRaises(self.m.DepsUnmet) as e:
            self.m.deps_needs("x", "check", latest="2.1")
        for words in ("Tessera 2.1 needs", "inter-font", "jq (1.6 → 1.7.1", "Settings → Software Update", "Nothing was changed"):
            self.assertIn(words, str(e.exception))

    def test_ask_in_the_terminal(self):
        asked = []
        with mock.patch("sys.stdout", new_callable=io.StringIO) as out:
            got = self.m.deps_needs("x", "ask", ask=lambda q, d: asked.append((q, d)) or True)
        self.assertEqual(got, (["inter-font", "jq"], ["old-theme"]))
        self.assertEqual([d for _, d in asked], [True, False], "install: yes by default; removing: no by default")
        self.assertIn("inter-font  (Fonts)", out.getvalue())
        with mock.patch("sys.stdout", new_callable=io.StringIO), self.assertRaises(self.m.DepsUnmet):
            self.m.deps_needs("x", "ask", ask=lambda q, d: False)
        with self.assertRaises(self.m.DepsUnmet) as e:
            self.m.deps_needs("x", "ask", ask=None)
        self.assertIn("tessera update in a terminal", str(e.exception))
        with mock.patch("builtins.input", return_value=""):
            self.assertTrue(self.m.terminal_ask("? [Y/n] ", True))
            self.assertFalse(self.m.terminal_ask("? [y/N] ", False))

    def test_blocked_stops_every_mode(self):
        self.plan["blocked"] = [{"name": "hyprland", "have": "0.55.1-1", "repo": "0.55.1-1", "need": "0.56.0 or newer", "why": "too old"}]
        for mode in ("auto", "ask", "check"):
            with mock.patch("sys.stdout", new_callable=io.StringIO), self.assertRaises(self.m.DepsUnmet) as e:
                self.m.deps_needs("x", mode, ask=lambda q, d: True)
            self.assertIn("hyprland 0.56.0 or newer", str(e.exception))

    def test_no_list_in_the_version_nothing_to_check(self):
        m = bg.load_update()
        m.show = lambda rev, path: None
        self.assertEqual(m.deps_needs("x", "check"), ([], []))

    def test_the_real_plan_reads_the_versions_files(self):
        m = bg.load_update()
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "tools").mkdir()
            (Path(d) / "tools/packages.sh").write_text('PACKAGES=( jq )\nMINIMUM=( "jq>=1.7" )\n')
            seen = []
            m.deps_status = lambda rev: seen.append(rev) or {"missing": [], "unavailable": [], "dropped": []}
            pac = FakePacman({"jq": "1.6-1"}, {"jq": "1.7.1-1"})
            D2 = m._load_deps()
            real = D2.assess
            D2.assess = lambda *a, **k: real(*a[:4], run=pac, own_qs=False, skip=k.get("skip", ()))
            m._load_deps = lambda: D2
            plan = m.deps_plan("dir:" + d)
        self.assertEqual(seen, ["dir:" + d])
        self.assertEqual(plan["install"], ["jq"])


@unittest.skipUnless(HAS_CRYPTO, "needs python-cryptography")
class BundlePath(load("deps_tb_t", ROOT / "tests/test_bundles.py").Fixture):
    """A copy that runs from a bundle: the downloaded version's own list,
    installed in the lock's run (one password)"""

    def test_a_bundles_stage_installs_its_packages_and_a_blocked_one_stages_nothing(self):
        one = self.publish()[0]
        vid = B.fetch(one, "beta", base_list=self.bases(), pubkey=self.key.pub)["id"]
        B.activate(vid)
        cfgdir = self.config / "tessera"
        cfgdir.mkdir(parents=True)
        (cfgdir / "config.json").write_text(json.dumps({"update": {"channel": "beta"}}))
        two = self.commit_change(40, entry="Second")
        self.publish(two)
        U = load("deps_bundle_update_" + vid, self.data / "versions" / vid / "tools/tessera-update.py")
        seen, root = [], []

        def needs(rev, mode="auto", ask=None, latest="", skip=()):
            seen.append((rev, mode))
            if unmet:
                raise U.DepsUnmet(unmet)
            return ["inter-font"], []
        U.deps_needs = needs
        U.packages_step = lambda vdir, system, say=None, install=(), lock=True, remove=(): root.append((vdir, list(install), lock))
        unmet = "Tessera can't be installed yet. It needs hyprland…"
        with self.assertRaises(SystemExit):
            U.stage(str(cfgdir), {"setting:bar.height": "new"})
        self.assertFalse(U.read_staged())
        self.assertEqual(root, [])
        self.assertEqual(B.read_link(str(self.data / "current")), vid)
        unmet = None
        out = U.stage(str(cfgdir), {"setting:bar.height": "new"})
        self.assertEqual(out["state"], "ready")
        vdir = str(self.data / "versions" / two[:12])
        self.assertEqual(seen[-1], ("dir:" + vdir, "auto"), "the downloaded version's own files")
        self.assertEqual(root, [(vdir, ["inter-font"], False)])


class Wiring(unittest.TestCase):
    """The scripts that update reach the shared step"""

    def read(self, rel):
        return (ROOT / rel).read_text()

    def test_restart_and_login_go_through_finish_or_apply(self):
        run = self.read("shell/assets/scripts/update-run.sh")
        self.assertIn('UPDATE=(finish --config "$CONFIG")', run)
        self.assertIn("UPDATE=(apply --config", run)
        self.assertIn('tools/tessera-update.py" "${UPDATE[@]}"', run)
        self.assertIn("tessera-update.py\" finish --if-ready", self.read("shell/assets/scripts/launch-shell.sh"))

    def test_the_terminal_and_install_sh(self):
        t = self.read("tools/tessera")
        self.assertIn('tessera-update.py" cli', t)
        self.assertIn('install.sh" --yes', t, "a developer's checkout: install.sh, which installs and checks them")
        self.assertIn('tools/tessera_deps.py" check --root "$ROOT"', self.read("install.sh"))

    def test_every_stage_and_finish_calls_the_step(self):
        src = self.read("tools/tessera-update.py")
        for fn in ("def stage(", "def bundle_stage(", "def bundle_build_stage(", "def finish(", "def apply("):
            body = src[src.index(fn):]
            body = body[:body.index("\ndef ", 1)]
            self.assertIn("deps_needs(", body, fn)


if __name__ == "__main__":
    unittest.main()
