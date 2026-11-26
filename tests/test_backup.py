"""Back up and restore (shell/assets/scripts/backup.py), with fake managers
on PATH in a throwaway home: what each manager's answer becomes (name,
version, source, the command that installs it again), what never goes in a
backup, create → inspect → restore → undo, names checked before anything
runs, the user-level installs; and the health record's writer
(health_event.py) and the update scripts' use of it."""
import importlib.util
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "shell/assets/scripts/backup.py"
HEALTH = ROOT / "shell/assets/scripts/health_event.py"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


BK = load("backup", SCRIPT)

PACMAN = """case "$1" in
  -Qen) printf 'firefox 130.0-1\\ngit 2.47.0-1\\n' ;;
  -Qem) printf 'yay-bin 12.4.2-1\\n' ;;
  *) exit 1 ;;
esac
"""
FLATPAK = """case "$1" in
  list) printf 'org.mozilla.firefox\\tflathub\\t130.0\\tuser\\n' ;;
  remotes) [ -f "$HOME/no-remotes" ] || printf 'flathub\\thttps://dl.flathub.org/repo/\\tuser\\n' ;;
  *) echo "flatpak $*" >> "$HOME/ran.log" ;;
esac
"""
FNM = """case "$1" in
  list) printf '* v20.11.1 default\\n* v18.19.0\\n* system\\n' ;;
  *) echo "fnm $*" >> "$HOME/ran.log" ;;
esac
"""
RUSTUP = """case "$1 $2" in
  "toolchain list") printf 'stable-x86_64-unknown-linux-gnu (default)\\nnightly-x86_64-unknown-linux-gnu\\n' ;;
  "component list") printf 'cargo-x86_64-unknown-linux-gnu\\nclippy-x86_64-unknown-linux-gnu\\nrust-src\\nrustc-x86_64-unknown-linux-gnu\\nrust-std-x86_64-unknown-linux-gnu\\n' ;;
  "target list") printf 'x86_64-unknown-linux-gnu\\nwasm32-unknown-unknown\\n' ;;
  *) echo "rustup $*" >> "$HOME/ran.log" ;;
esac
"""
PIPX = """case "$1" in
  list) printf '%s' '{"venvs": {"httpie": {"metadata": {"main_package": {"package": "httpie", "package_version": "3.2.4", "package_or_url": "httpie", "apps": ["http", "https"]}}}}}' ;;
  *) echo "pipx $*" >> "$HOME/ran.log" ;;
esac
"""
UV = """case "$1 $2" in
  "tool list") printf 'ruff v0.6.9\\n- ruff\\nblack v24.1.0\\n- black\\n- blackd\\n' ;;
  *) echo "uv $*" >> "$HOME/ran.log" ;;
esac
"""
NPM = """case "$1" in
  prefix) echo "$HOME/.local" ;;
  ls) printf '%s' '{"dependencies": {"npm": {"version": "10.8.0"}, "typescript": {"version": "5.6.3"}, "@vue/cli": {"version": "5.0.8"}}}' ;;
  *) echo "npm $*" >> "$HOME/ran.log" ;;
esac
"""
CRATES2 = {"installs": {
    "ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)": {"bins": ["rg"], "features": [], "no_default_features": False},
    "tool 0.1.0 (git+https://github.com/someone/tool?branch=main#abc123)": {"bins": ["tool"], "features": ["fast"]},
    "mine 0.1.0 (path+file:///home/old/src/mine)": {"bins": ["mine"]},
}}


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        r = Path(self.tmp.name)
        self.home, self.bin = r / "home", r / "bin"
        self.cfg_home, self.state_home, self.data_home = self.home / ".config", self.home / ".local/state", self.home / ".local/share"
        self.config = self.cfg_home / "tessera"
        self.hypr = self.cfg_home / "hypr"
        for p in (self.bin, self.config, self.hypr / "tessera", self.state_home / "tessera", self.data_home):
            p.mkdir(parents=True, exist_ok=True)
        # An install: a checkout with its shell, its pictures and its hypr files
        self.repo = r / "tessera"
        self.shell = self.repo / "shell"
        (self.shell / "assets/wallpapers").mkdir(parents=True)
        (self.shell / "assets/wallpapers/tahoe-dusk.jpg").write_bytes(b"jpg")
        (self.repo / "hypr/tessera").mkdir(parents=True)
        (self.repo / "hypr/hyprland.lua").write_text("-- Tessera\n")
        (self.repo / "hypr/tessera/look.lua").write_text("look = 1\n")
        (self.repo / "hypr/tessera/rules.lua").write_text("rules = 1\n")
        self.env_was = {k: os.environ.get(k) for k in ("HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "PATH", "NVM_DIR", "CARGO_HOME")}
        self.env = {"HOME": str(self.home), "XDG_CONFIG_HOME": str(self.cfg_home), "XDG_STATE_HOME": str(self.state_home),
                    "XDG_DATA_HOME": str(self.data_home), "PATH": str(self.bin)}
        os.environ.update(self.env)
        os.environ.pop("NVM_DIR", None)
        os.environ.pop("CARGO_HOME", None)

    def tearDown(self):
        for k, v in self.env_was.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
        self.tmp.cleanup()

    def fake(self, name, body="exit 0\n"):
        f = self.bin / name
        f.write_text("#!/bin/sh\n" + body)
        f.chmod(0o755)

    def managers(self):
        self.fake("pacman", PACMAN)
        self.fake("paru")
        self.fake("flatpak", FLATPAK)
        self.fake("fnm", FNM)
        self.fake("rustup", RUSTUP)
        self.fake("cargo", 'echo "cargo $*" >> "$HOME/ran.log"\n')
        self.fake("pipx", PIPX)
        self.fake("uv", UV)
        self.fake("npm", NPM)
        (self.home / ".cargo").mkdir()
        (self.home / ".cargo/.crates2.json").write_text(json.dumps(CRATES2))
        (self.home / "Applications").mkdir()
        (self.home / "Applications/Obsidian.AppImage").write_bytes(b"\x7fELF")
        lb = self.home / ".local/bin"
        lb.mkdir(parents=True)
        for n in ("ruff", "http", "mystery", "rg"):
            (lb / n).write_text("#!/bin/sh\n")
        (lb / "tessera").symlink_to(self.repo / "shell")
        nvm = self.home / ".nvm"
        (nvm / "versions/node/v16.20.2").mkdir(parents=True)
        (nvm / "nvm.sh").write_text("nvm() { :; }\n")
        (nvm / "alias").mkdir()
        (nvm / "alias/default").write_text("16\n")

    def setup_settings(self):
        pics = self.home / "Pictures"
        pics.mkdir()
        (pics / "mine.jpg").write_bytes(b"my picture")
        (self.home / ".ssh").mkdir()
        (self.home / ".ssh/id_ed25519").write_text("PRIVATE KEY")
        (self.home / ".ssh/beach.jpg").write_bytes(b"in .ssh")
        (self.data_home / "keyrings").mkdir(parents=True)
        (self.data_home / "keyrings/login.keyring").write_text("SECRET")
        config = {
            "appearance": {"look": "material", "mode": "dark"},
            "wallpaper": {"path": str(pics / "mine.jpg"), "darkPath": str(self.home / ".ssh/beach.jpg"),
                          "lightPath": str(self.shell / "assets/wallpapers/tahoe-dusk.jpg")},
            "ai": {"apiKey": "sk-secret", "model": "x"},
            "google": {"account": "me@example.com"},
            "displays": {"eDP-1": {"scale": 1.5}},
            "sharing": {"name": "my-laptop", "on": True},
            "lock": {"loginStyle": "plain", "loginIntro": True, "clockFace": "big"},
            "dock": {"size": 48},
        }
        (self.config / "config.json").write_text(json.dumps(config))
        (self.config / "keybinds.json").write_text(json.dumps({"version": 1, "keybinds": {"term": {"keys": "SUPER+T"}}, "shortcuts": []}))
        (self.config / "focus.json").write_text(json.dumps({"modes": [{"id": "work", "name": "Work"}], "active": ""}))
        (self.config / "ai-keys.json").write_text(json.dumps({"openai": "sk-x"}))
        (self.config / "calendars.json").write_text(json.dumps([{"url": "https://cal/private-token"}]))
        (self.state_home / "tessera/state.json").write_text(json.dumps({"overlay": {"notes": {"open": True}}, "todos": [{"text": "buy milk"}],
                                                                        "clipPins": [{"text": "hunter2"}], "conflictsMuted": ["x"]}))
        # Hyprland: Tessera's files (one edited), yours, the installer's, a secret
        (self.hypr / "hyprland.lua").write_text("-- Tessera\n")
        (self.hypr / "tessera/look.lua").write_text("look = 2 -- mine\n")
        (self.hypr / "tessera/rules.lua").write_text("rules = 1\n")
        (self.hypr / "tessera/keyboard.lua").write_text("kb = 'us'\n")
        (self.hypr / "custom.lua").write_text("my = 1\n")
        (self.hypr / "hypridle.conf").write_text("general {}\n")
        (self.hypr / "api-token.conf").write_text("TOKEN\n")
        (self.hypr / "custom.lua.new").write_text("other\n")

    def run_bk(self, *args, ok=True):
        p = subprocess.run([sys.executable, "-I", str(SCRIPT), *args, "--shell", str(self.shell)], env=self.env, capture_output=True, text=True, timeout=120)
        lines = [l for l in p.stdout.splitlines() if l.strip()]
        out = json.loads(lines[-1]) if lines else {}
        self.assertEqual(out.get("ok"), ok, p.stdout + p.stderr)
        return out, lines

    def members(self, path):
        with tarfile.open(path, "r:*") as t:
            return {m.name: t.extractfile(m).read() for m in t if m.isreg()}


class Inventory(Base):
    def test_every_manager_that_is_here(self):
        self.managers()
        out, _ = self.run_bk("inventory", "--json")
        apps = {a["id"]: a for a in out["apps"]}
        self.assertEqual(apps["pacman:firefox"]["source"], "Arch repositories")
        self.assertEqual(apps["pacman:firefox"]["command"], "sudo pacman -S --needed firefox")
        self.assertEqual(apps["aur:yay-bin"]["command"], "paru -S --needed yay-bin")
        self.assertEqual(out["managers"]["aur"]["helper"], "paru")
        f = apps["flatpak:org.mozilla.firefox"]
        self.assertEqual((f["remote"], f["url"], f["installation"], f["version"]), ("flathub", "https://dl.flathub.org/repo/", "user", "130.0"))
        self.assertEqual(f["command"], "flatpak install --user flathub org.mozilla.firefox")
        self.assertEqual(out["managers"]["flatpak"]["remotes"][0]["name"], "flathub")
        self.assertTrue(apps["fnm:v20.11.1"]["default"])
        self.assertEqual(apps["fnm:v20.11.1"]["command"], "fnm install v20.11.1 && fnm default v20.11.1")
        self.assertFalse(apps["fnm:v18.19.0"]["default"])
        self.assertNotIn("fnm:system", apps)
        self.assertTrue(apps["nvm:v16.20.2"]["default"])
        r = apps["rustup:stable-x86_64-unknown-linux-gnu"]
        self.assertEqual((r["components"], r["targets"], r["default"]), (["clippy", "rust-src"], ["x86_64-unknown-linux-gnu", "wasm32-unknown-unknown"], True))
        self.assertIn("rustup default stable-x86_64-unknown-linux-gnu", r["command"])
        self.assertEqual(apps["cargo:ripgrep"]["command"], "cargo install ripgrep")
        self.assertEqual(apps["cargo:tool"]["command"], "cargo install --git https://github.com/someone/tool tool --features fast")
        self.assertFalse(apps["cargo:mine"]["installable"])
        self.assertEqual((apps["pipx:httpie"]["version"], apps["pipx:httpie"]["command"]), ("3.2.4", "pipx install httpie"))
        self.assertEqual((apps["uv:ruff"]["version"], apps["uv:black"]["command"]), ("0.6.9", "uv tool install black"))
        self.assertEqual(apps["npm:typescript"]["command"], "npm install -g typescript")
        self.assertIn("npm:@vue/cli", apps)
        self.assertNotIn("npm:npm", apps)
        self.assertEqual(apps["appimage:Obsidian.AppImage"]["source"], "AppImage, found, origin unknown")
        # ~/.local/bin: what a manager owns isn't "origin unknown"; Tessera's own isn't listed
        self.assertEqual(apps["local:mystery"]["source"], "In ~/.local/bin, origin unknown")
        for owned in ("local:ruff", "local:http", "local:rg", "local:tessera"):
            self.assertNotIn(owned, apps)
        part = {p["id"]: p for p in out["parts"]}["apps"]
        self.assertEqual(part["count"], len(out["apps"]))

    def test_only_managers_that_are_here(self):
        self.fake("pacman", PACMAN)
        out, _ = self.run_bk("inventory", "--json")
        self.assertEqual({a["group"] for a in out["apps"]}, {"pacman", "aur"})
        self.assertEqual(out["managers"]["aur"]["helper"], "")
        self.assertEqual({a["id"]: a for a in out["apps"]}["aur:yay-bin"]["command"], "yay -S --needed yay-bin")

    def test_a_manager_that_hangs_is_given_up_on(self):
        self.fake("pacman", "while :; do :; done\n")
        BK.CMD_TIMEOUT = 1
        try:
            self.assertIsNone(BK.run(["pacman", "-Qen"], timeout=1))
        finally:
            BK.CMD_TIMEOUT = 20

    def test_parsers(self):
        self.assertEqual(BK.parse_fnm("* v20.11.1 default\n* 18.19.0\n* system\n"), [("v20.11.1", True), ("v18.19.0", False)])
        self.assertEqual(BK.parse_rustup_toolchains("stable-x86_64-unknown-linux-gnu (active, default)\nbeta\n"),
                         [("stable-x86_64-unknown-linux-gnu", True), ("beta", False)])
        self.assertEqual(BK.parse_uv("ruff v0.6.9\n- ruff\n"), [("ruff", "0.6.9", ["ruff"])])
        self.assertEqual(BK.parse_npm('{"dependencies": {"corepack": {"version": "1"}, "x": {"version": "2"}, "../evil": {}}}'), [("x", "2")])
        apps, rem = BK.parse_flatpak("Application ID\tOrigin\norg.gnome.Maps\tflathub\t45\tsystem\nnot-an-id\tx\n", "flathub\thttps://x/\tsystem\n")
        self.assertEqual(apps, [("org.gnome.Maps", "flathub", "45", "system")])
        self.assertEqual(rem, {"flathub": ("https://x/", "system")})
        self.assertEqual(BK.parse_pacman("firefox 1-1\n-bad 1\nFirefox 2\n"), [("firefox", "1-1")])
        self.assertEqual(BK.parse_crates2("not json"), [])
        self.assertEqual(BK.parse_pipx("{}"), [])

    def test_nvms_default_is_one_version(self):
        # (audit #701: "20" marked every v20.x, "2" also v20 and v22)
        vs = ["v2.0.0", "v20.1.0", "v20.11.1", "v22.3.0"]
        self.assertEqual(BK.nvm_default(vs, "20"), "v20.11.1")
        self.assertEqual(BK.nvm_default(vs, "2"), "v2.0.0")
        self.assertEqual(BK.nvm_default(vs, "v20.1.0"), "v20.1.0")
        self.assertEqual(BK.nvm_default(vs, "node"), "v22.3.0")
        self.assertIsNone(BK.nvm_default(vs, "18"))
        self.assertIsNone(BK.nvm_default(vs, ""))
        nd = self.home / ".nvm-aliases"
        (nd / "alias/lts").mkdir(parents=True)
        (nd / "alias/lts/iron").write_text("v20.1.0\n")
        self.assertEqual(BK.nvm_default(vs, "lts/iron", str(nd)), "v20.1.0")
        self.assertIsNone(BK.nvm_default(vs, "lts/jod", str(nd)))


class NeverIncluded(Base):
    def test_nothing_secret_goes_in(self):
        self.setup_settings()
        out, _ = self.run_bk("create", str(self.home), "--include", "settings,hypr,wallpapers,login")
        m = self.members(out["file"])
        blob = b"\n".join(m.values())
        for secret in (b"PRIVATE KEY", b"SECRET", b"sk-secret", b"sk-x", b"me@example.com", b"my-laptop", b"TOKEN", b"private-token",
                       b"hunter2", b"buy milk", b"in .ssh"):
            self.assertNotIn(secret, blob, secret)
        config = json.loads(m["settings/config.json"])
        self.assertNotIn("google", config)
        self.assertNotIn("displays", config)
        self.assertNotIn("apiKey", config["ai"])
        self.assertEqual(config["dock"]["size"], 48)
        self.assertEqual(json.loads(m["settings/state.json"]), {"overlay": {"notes": {"open": True}}, "conflictsMuted": ["x"]})
        # Hyprland: yours and the edited one; not Tessera's unchanged, the installer's, copies, a secret
        self.assertIn("hypr/custom.lua", m)
        self.assertIn("hypr/hypridle.conf", m)
        self.assertIn("hypr-edited/tessera/look.lua", m)
        for gone in ("hypr/hyprland.lua", "hypr/tessera/rules.lua", "hypr/tessera/keyboard.lua", "hypr/api-token.conf", "hypr/custom.lua.new"):
            self.assertNotIn(gone, m)
        # Wallpapers: yours carried, Tessera's by name, one in ~/.ssh never
        pics = [n for n in m if n.startswith("wallpapers/")]
        self.assertEqual(len(pics), 1)
        self.assertEqual(m[pics[0]], b"my picture")
        manifest = json.loads(m["manifest.json"])
        self.assertEqual(manifest["wallpapers"]["wallpaper.lightPath"], {"bundled": "tahoe-dusk.jpg"})
        self.assertNotIn("hostname", json.dumps(manifest).lower())
        self.assertEqual(json.loads(m["login.json"]), {"lock": {"loginStyle": "plain", "loginIntro": True}})
        self.assertEqual(oct(os.stat(out["file"]).st_mode & 0o777), "0o600")
        self.assertTrue(out["file"].endswith((".tar.zst", ".tar.gz")))

    def test_the_never_rule(self):
        h = str(self.home)
        for p in (".ssh/config", ".gnupg/pubring.kbx", ".local/share/keyrings/x", ".mozilla/firefox/p/cookies.sqlite",
                  ".config/google-chrome/Default/Preferences", "Documents/taxes.pdf", "x/id_rsa", "x/server.pem", "x/github-token", "x/.netrc"):
            self.assertTrue(BK.never(os.path.join(h, p), h), p)
        for p in (".config/hypr/custom.lua", "Pictures/beach.jpg", ".config/hypr/keybinds.lua"):
            self.assertFalse(BK.never(os.path.join(h, p), h), p)
        # (a link from an allowed place into a secret one: where it really is counts)
        (self.home / ".ssh").mkdir()
        (self.home / ".ssh/key").write_text("x")
        (self.hypr / "link.conf").symlink_to(self.home / ".ssh/key")
        self.assertTrue(BK.never(str(self.hypr / "link.conf"), h))


class RoundTrip(Base):
    def test_create_inspect_restore_undo(self):
        self.setup_settings()
        self.managers()
        out, _ = self.run_bk("create", str(self.home))
        f = out["file"]
        self.assertEqual(out["parts"], ["settings", "hypr", "wallpapers", "login", "apps"])
        info, _ = self.run_bk("inspect", f)
        parts = {p["id"]: p for p in info["parts"]}
        self.assertEqual(set(parts), {"settings", "hypr", "wallpapers", "login", "apps"})
        self.assertEqual(parts["wallpapers"]["count"], 1)
        self.assertTrue(all(a["installed"] for a in info["apps"]), "everything's still here")

        # This computer changes since
        cfg = json.loads((self.config / "config.json").read_text())
        cfg["dock"]["size"] = 64
        cfg["appearance"]["look"] = "glass"
        cfg["lock"]["loginStyle"] = "text"
        cfg["ai"]["apiKey"] = "sk-new-here"
        (self.config / "config.json").write_text(json.dumps(cfg))
        (self.hypr / "custom.lua").unlink()
        (self.hypr / "hypridle.conf").write_text("changed\n")
        (self.home / "Pictures/mine.jpg").unlink()
        # (and Tessera updated its own file since)
        (self.hypr / "tessera/look.lua").write_text("look = 3\n")
        state = json.loads((self.state_home / "tessera/state.json").read_text())
        state["conflictsMuted"] = []
        (self.state_home / "tessera/state.json").write_text(json.dumps(state))

        # What would change
        plan, _ = self.run_bk("restore-plan", f, "--include", "settings,hypr,wallpapers")
        changes = {c["key"]: c for c in plan["parts"]["settings"]["changes"]}
        self.assertEqual((changes["dock.size"]["now"], changes["dock.size"]["then"]), ("64", "48"))
        self.assertIn("appearance.look", changes)
        self.assertNotIn("ai.apiKey", changes, "this computer's key stays")
        self.assertNotIn("lock.loginStyle", changes, "the login look wasn't chosen")
        acts = {x["path"]: x["action"] for x in plan["parts"]["hypr"]["files"]}
        self.assertEqual(acts["~/.config/hypr/custom.lua"], "add")
        self.assertEqual(acts["~/.config/hypr/hypridle.conf"], "replace")
        self.assertEqual(acts["~/.config/hypr/tessera/look.lua" + BK.BESIDE], "beside")
        self.assertEqual(plan["parts"]["wallpapers"]["count"], 1)
        self.assertEqual(plan["parts"]["settings"]["state"], ["conflictsMuted"])

        res, _ = self.run_bk("restore", f, "--include", "settings,hypr,wallpapers")
        self.assertEqual(res["look"], "material")
        cfg2 = json.loads((self.config / "config.json").read_text())
        self.assertEqual(cfg2["dock"]["size"], 48)
        self.assertEqual(cfg2["ai"]["apiKey"], "sk-new-here")
        self.assertEqual(cfg2["lock"]["loginStyle"], "text", "the login look stays this computer's")
        self.assertEqual(cfg2["sharing"]["name"], "my-laptop")
        restored_pic = cfg2["wallpaper"]["path"]
        self.assertTrue(restored_pic.startswith(str(self.home / "Pictures/Wallpapers/Restored")))
        self.assertEqual(Path(restored_pic).read_bytes(), b"my picture")
        self.assertEqual(cfg2["wallpaper"]["lightPath"], str(self.shell / "assets/wallpapers/tahoe-dusk.jpg"))
        self.assertEqual(cfg2["wallpaper"]["darkPath"], str(self.home / ".ssh/beach.jpg"), "never carried, but still here: kept")
        self.assertEqual((self.hypr / "custom.lua").read_text(), "my = 1\n")
        self.assertEqual((self.hypr / "hypridle.conf").read_text(), "general {}\n")
        self.assertEqual((self.hypr / "tessera/look.lua").read_text(), "look = 3\n", "Tessera's file isn't touched")
        self.assertEqual((self.hypr / ("tessera/look.lua" + BK.BESIDE)).read_text(), "look = 2 -- mine\n")
        self.assertEqual(json.loads((self.state_home / "tessera/state.json").read_text())["conflictsMuted"], ["x"])
        self.assertIn("todos", json.loads((self.state_home / "tessera/state.json").read_text()), "other state stays")

        # The same again changes nothing
        again, _ = self.run_bk("restore-plan", f, "--include", "settings,hypr,wallpapers")
        self.assertEqual(again["writes"], 0, again)

        # Undo: what was there before
        self.run_bk("undo", res["undo"])
        cfg3 = json.loads((self.config / "config.json").read_text())
        self.assertEqual((cfg3["dock"]["size"], cfg3["appearance"]["look"]), (64, "glass"))
        self.assertFalse((self.hypr / "custom.lua").exists(), "added by the restore: gone again")
        self.assertEqual((self.hypr / "hypridle.conf").read_text(), "changed\n")
        self.assertFalse((self.hypr / ("tessera/look.lua" + BK.BESIDE)).exists())
        self.assertFalse(Path(restored_pic).exists())
        self.assertEqual(json.loads((self.state_home / "tessera/state.json").read_text())["conflictsMuted"], [])

    def test_a_restore_that_fails_halfway_puts_things_back(self):
        # (audit #73: half restored, and the undo file never said)
        self.setup_settings()
        out, _ = self.run_bk("create", str(self.home))
        (self.home / "Pictures/mine.jpg").unlink()
        (self.hypr / "custom.lua").unlink()
        before = (self.config / "config.json").read_text()
        self.hypr.chmod(0o555)  # (a folder it can't write in)
        try:
            res, _ = self.run_bk("restore", out["file"], "--include", "settings,hypr,wallpapers", ok=False)
        finally:
            self.hypr.chmod(0o755)
        self.assertIn("Nothing was changed", res["error"])
        self.assertEqual((self.config / "config.json").read_text(), before)
        restored = self.home / "Pictures/Wallpapers/Restored"
        self.assertFalse(restored.exists() and any(restored.iterdir()), "the picture written first went again")

    def test_the_login_look_alone(self):
        self.setup_settings()
        out, _ = self.run_bk("create", str(self.home), "--include", "login")
        cfg = json.loads((self.config / "config.json").read_text())
        cfg["lock"]["loginStyle"] = "text"
        cfg["dock"]["size"] = 64
        (self.config / "config.json").write_text(json.dumps(cfg))
        plan, _ = self.run_bk("restore-plan", out["file"], "--include", "login")
        self.assertEqual(plan["parts"]["login"]["changes"], [{"key": "lock.loginStyle", "now": "text", "then": "plain"}])
        self.run_bk("restore", out["file"], "--include", "login")
        cfg2 = json.loads((self.config / "config.json").read_text())
        self.assertEqual((cfg2["lock"]["loginStyle"], cfg2["dock"]["size"]), ("plain", 64))

    def test_files_that_arent_backups_are_refused(self):
        bad = self.home / "bad.tar.gz"
        with tarfile.open(bad, "w:gz") as t:
            data = json.dumps({"app": "Tessera", "kind": "backup", "format": 1, "parts": ["hypr"]}).encode()
            for name in ("manifest.json", "hypr/../../.bashrc"):
                info = tarfile.TarInfo(name)
                info.size = len(data)
                import io
                t.addfile(info, io.BytesIO(data))
        out, _ = self.run_bk("inspect", str(bad), ok=False)
        self.assertIn("isn't a Tessera backup", out["error"])
        (self.home / "junk.tar.gz").write_bytes(b"not a tar")
        self.run_bk("inspect", str(self.home / "junk.tar.gz"), ok=False)
        newer = self.home / "newer.tar.gz"
        with tarfile.open(newer, "w:gz") as t:
            data = json.dumps({"app": "Tessera", "kind": "backup", "format": 99}).encode()
            info = tarfile.TarInfo("manifest.json")
            info.size = len(data)
            import io
            t.addfile(info, io.BytesIO(data))
        out, _ = self.run_bk("inspect", str(newer), ok=False)
        self.assertIn("newer Tessera", out["error"])
        # A damaged one of odd shapes: an answer, never a traceback (audit #705)
        odd = self.home / "odd.tar.gz"
        with tarfile.open(odd, "w:gz") as t:
            data = json.dumps({"app": "Tessera", "kind": "backup", "format": 1, "parts": 5,
                               "contents": {"settings": {"settings": [1]}}}).encode()
            info = tarfile.TarInfo("manifest.json")
            info.size = len(data)
            import io
            t.addfile(info, io.BytesIO(data))
        for cmd in ("inspect", "restore-plan"):
            p = subprocess.run([sys.executable, "-I", str(SCRIPT), cmd, str(odd), "--shell", str(self.shell)], env=self.env,
                               capture_output=True, text=True, timeout=120)
            self.assertNotIn("Traceback", p.stderr)
            self.assertFalse(json.loads(p.stdout.strip().splitlines()[-1])["ok"], p.stdout)


class AppsBack(Base):
    def backup_with(self, apps):
        """A backup whose apps.json is `apps` (as a tampered one could be)"""
        import io
        p = self.home / "b.tar.gz"
        with tarfile.open(p, "w:gz") as t:
            for name, obj in (("manifest.json", {"app": "Tessera", "kind": "backup", "format": 1, "parts": ["apps"]}), ("apps.json", {"apps": apps})):
                data = json.dumps(obj).encode()
                info = tarfile.TarInfo(name)
                info.size = len(data)
                t.addfile(info, io.BytesIO(data))
        return str(p)

    def test_names_are_checked_and_commands_never_taken_from_the_file(self):
        steps = BK.install_steps
        self.assertIsNone(steps({"group": "flatpak", "name": "--help", "remote": "flathub"}, {"flathub"}))
        self.assertIsNone(steps({"group": "flatpak", "name": "org.x.App", "remote": "evil", "url": "http://insecure/repo"}, set()))
        self.assertEqual(steps({"group": "flatpak", "name": "org.x.App", "remote": "flathub", "url": ""}, set())[0],
                         ["flatpak", "remote-add", "--if-not-exists", "--system", "flathub", BK.FLATHUB])
        # (audit #703: flathub-beta to its own repo, not stable Flathub; any
        # other remote only from a .flatpakrepo, which carries its key)
        self.assertEqual(steps({"group": "flatpak", "name": "org.x.App", "remote": "flathub-beta", "url": "https://dl.flathub.org/beta-repo/"},
                               set())[0][-1], "https://dl.flathub.org/beta-repo/flathub-beta.flatpakrepo")
        self.assertIsNone(steps({"group": "flatpak", "name": "org.x.App", "remote": "mine", "url": "https://dl.flathub.org/other/"}, set()))
        self.assertEqual(steps({"group": "flatpak", "name": "org.x.App", "remote": "mine", "url": "https://x.example/mine.flatpakrepo"},
                               set())[0][-1], "https://x.example/mine.flatpakrepo")
        self.assertIsNone(steps({"group": "cargo", "name": "tool", "git": "ssh://git@host/x"}, set()))
        self.assertIsNone(steps({"group": "cargo", "name": "../x"}, set()))
        self.assertIsNone(steps({"group": "npm", "name": "-g"}, set()))
        self.assertIsNone(steps({"group": "fnm", "version": "latest; rm -rf ~"}, set()))
        self.assertIsNone(steps({"group": "pipx", "name": "x", "spec": "x; rm"}, set()))
        self.assertEqual(steps({"group": "uv", "name": "ruff"}, set()), [["uv", "tool", "install", "ruff"]])
        self.assertEqual(steps({"group": "rustup", "name": "stable", "components": ["clippy", "-x"], "targets": [], "default": True}, set()),
                         [["rustup", "toolchain", "install", "stable", "--component", "clippy"], ["rustup", "default", "stable"]])
        for good in ("firefox", "lib32-glibc", "gtk+3", "yay-bin"):
            self.assertTrue(BK.package_name(good))
        for bad in ("-Syu", "Firefox", "a b", ".x", "", "a/b"):
            self.assertFalse(BK.package_name(bad))
        # A tampered backup: its "command" is never run
        self.fake("uv", 'echo "uv $*" >> "$HOME/ran.log"\n')
        f = self.backup_with([{"id": "uv:ruff", "group": "uv", "name": "ruff", "command": "touch $HOME/pwned"},
                              {"id": "pacman:firefox", "group": "pacman", "name": "firefox"}])
        out, _ = self.run_bk("apps-install", f, "--ids", "uv:ruff,pacman:firefox")
        self.assertEqual(out["results"]["uv:ruff"]["state"], "ok")
        self.assertEqual(out["results"]["pacman:firefox"]["state"], "system", "Software Update's job")
        self.assertFalse((self.home / "pwned").exists())
        self.assertEqual((self.home / "ran.log").read_text(), "uv tool install ruff\n")

    def test_user_level_installs_with_progress_and_missing_managers(self):
        self.fake("flatpak", "case \"$1\" in list|remotes) ;; *) echo \"flatpak $*\" >> \"$HOME/ran.log\" ;; esac\n")
        (self.home / "no-remotes").write_text("")
        f = self.backup_with([
            {"id": "flatpak:org.mozilla.firefox", "group": "flatpak", "name": "org.mozilla.firefox", "remote": "flathub",
             "url": "https://dl.flathub.org/repo/", "installation": "user"},
            {"id": "fnm:v20.11.1", "group": "fnm", "name": "node", "version": "v20.11.1", "default": True},
            {"id": "local:mystery", "group": "local", "name": "mystery", "installable": False},
        ])
        status, _ = self.run_bk("apps-status", f)
        by = {a["id"]: a for a in status["apps"]}
        self.assertEqual((by["fnm:v20.11.1"]["needs"], by["fnm:v20.11.1"]["needsPackage"]), ("fnm", "fnm"))
        self.assertEqual(by["flatpak:org.mozilla.firefox"]["needs"], "")
        self.assertFalse(by["flatpak:org.mozilla.firefox"]["installed"])
        out, lines = self.run_bk("apps-install", f, "--ids", "flatpak:org.mozilla.firefox,fnm:v20.11.1,local:mystery")
        r = out["results"]
        self.assertEqual(r["flatpak:org.mozilla.firefox"]["state"], "ok")
        self.assertEqual(r["fnm:v20.11.1"], {"state": "skipped", "why": "needs fnm"})
        self.assertEqual(r["local:mystery"]["state"], "skipped")
        self.assertEqual((self.home / "ran.log").read_text().splitlines(), [
            "flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo",
            "flatpak install -y --noninteractive --user flathub org.mozilla.firefox"])
        progress = [json.loads(l)["progress"] for l in lines[:-1]]
        self.assertEqual(progress[0], {"id": "flatpak:org.mozilla.firefox", "state": "running", "done": 0, "total": 3})
        self.assertEqual(progress[-1]["id"], "local:mystery")
        # Now fnm's here: it installs, and sets the default
        self.fake("fnm", FNM)
        out, _ = self.run_bk("apps-install", f, "--ids", "fnm:v20.11.1")
        self.assertEqual(out["results"]["fnm:v20.11.1"]["state"], "ok")
        self.assertIn("fnm install v20.11.1\nfnm default v20.11.1\n", (self.home / "ran.log").read_text())

    def test_a_failed_install_says_why(self):
        self.fake("uv", 'echo "error: no such tool" >&2; exit 2\n')
        f = self.backup_with([{"id": "uv:nope", "group": "uv", "name": "nope"}])
        out, _ = self.run_bk("apps-install", f, "--ids", "uv:nope")
        self.assertEqual(out["results"]["uv:nope"], {"state": "failed", "why": "error: no such tool"})
        self.assertEqual(out["failed"], 1)

    def test_never_as_root(self):
        if os.geteuid() == 0:
            self.skipTest("runs as root")
        orig = os.geteuid
        os.geteuid = lambda: 0
        try:
            import io
            import contextlib
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                self.assertEqual(BK.main(["inventory", "--json"]), 1)
            self.assertIn("never as root", buf.getvalue())
        finally:
            os.geteuid = orig


class HealthRecord(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.state = Path(self.tmp.name) / "state"
        self.was = os.environ.get("XDG_STATE_HOME")
        os.environ["XDG_STATE_HOME"] = str(self.state)
        self.he = load("health_event", HEALTH)

    def tearDown(self):
        if self.was is None:
            os.environ.pop("XDG_STATE_HOME", None)
        else:
            os.environ["XDG_STATE_HOME"] = self.was
        self.tmp.cleanup()

    def lines(self):
        return [json.loads(l) for l in (self.state / "tessera/health.jsonl").read_text().splitlines()]

    def test_the_same_line_format_as_the_shell(self):
        self.assertTrue(self.he.update("system", False, "pacman: a\nconflict", "system", now=1700000000))
        self.assertTrue(self.he.update("tessera", True, "ignored", "finish", now=1700000060))
        self.assertFalse(self.he.update("bogus", False))
        raw = (self.state / "tessera/health.jsonl").read_text()
        # (one compact object a line, its time first: core/src/health.rs reads "t" and "ev")
        self.assertTrue(raw.startswith('{"t":1700000000,"ev":"update",'))
        self.assertEqual(self.lines(), [{"t": 1700000000, "ev": "update", "what": "system", "ok": False, "why": "pacman: a conflict", "step": "system"},
                                        {"t": 1700000060, "ev": "update", "what": "tessera", "ok": True, "step": "finish"}])
        self.assertEqual(oct(os.stat(self.state / "tessera/health.jsonl").st_mode & 0o777), "0o600")
        # (the command line, as update-run.sh calls it)
        p = subprocess.run([sys.executable, "-I", str(HEALTH), "update", "native", "failed", "no network", "install"],
                           env=dict(os.environ, XDG_STATE_HOME=str(self.state)), capture_output=True)
        self.assertEqual(p.returncode, 0)
        self.assertEqual(self.lines()[-1]["what"], "native")

    def test_it_appends_never_rewrites(self):
        f = self.state / "tessera/health.jsonl"
        f.parent.mkdir(parents=True)
        f.write_text('{"t":1,"ev":"start","why":"normal"}\n')
        self.he.record("fallback", why="x" * 1000, now=2)
        got = self.lines()
        self.assertEqual(got[0]["ev"], "start")
        self.assertEqual(len(got[1]["why"]), self.he.MAX_TEXT)

    def test_native_install_leaves_the_updaters_failure_to_it(self):
        ni = load("native_install", ROOT / "tools/native-install.py")
        was = os.environ.pop("TESSERA_HEALTH_PARENT", None)
        try:
            ni.health_record({"ok": True, "changed": False})
            self.assertFalse((self.state / "tessera/health.jsonl").exists(), "nothing changed: nothing to say")
            ni.health_record({"ok": False, "why": "the signature didn't match"})
            self.assertEqual(self.lines()[-1], {"t": self.lines()[-1]["t"], "ev": "update", "what": "native", "ok": False,
                                                "why": "the signature didn't match", "step": "install"})
            os.environ["TESSERA_HEALTH_PARENT"] = "tessera-update"
            ni.health_record({"ok": False, "why": "again"})
            self.assertEqual(len(self.lines()), 1)
        finally:
            os.environ.pop("TESSERA_HEALTH_PARENT", None)
            if was is not None:
                os.environ["TESSERA_HEALTH_PARENT"] = was


if __name__ == "__main__":
    unittest.main()
