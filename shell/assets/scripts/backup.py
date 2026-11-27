#!/usr/bin/python3 -I
"""Your whole setup in one file, and back (Settings → About → Back up and
restore, tessera-ui's pages/about/backup.rs):

  backup.py inventory --json [--config DIR] [--shell DIR]
        what a backup can hold, each part with its count and size
  backup.py create <folder or file> --include settings,hypr,wallpapers,login,apps
        [--config DIR] [--shell DIR]
  backup.py inspect <file>                 what's in a backup, its apps marked
                                           installed here or not
  backup.py restore-plan <file> [--include …] [--config DIR] [--shell DIR]
                                           what restoring would change
  backup.py restore <file> --include … [--config DIR] [--shell DIR] [--no-undo]
  backup.py undo <undo file>               puts back what a restore replaced
  backup.py apps-status <file>             which of its apps are here now
  backup.py apps-install <file> --ids a,b  installs the user-level ones (one
                                           JSON line per step, then the result)

The parts (each chosen, each with its count and size):
  settings    Tessera's settings (config.json without keys, tokens,
              passwords, accounts or this computer's displays: the same
              scrub as settings-backup.py), shortcuts and gestures
              (keybinds.json), Focus modes, and the UI state that's a
              preference (state.json's overlay layout, crosshair, widget
              stacks, shortcut buttons, muted conflicts)
  hypr        your own files in ~/.config/hypr: the ones Tessera doesn't ship
              (custom.lua, hypridle.conf, …), and the shipped ones you edited
              (restored beside Tessera's as <file>.from-backup, never over it)
  wallpapers  the pictures you chose that aren't Tessera's own (Tessera's go
              by name and are found in this install)
  login       the login screen's look (lock.login*: Tessera sends it to the
              login screen by itself)
  apps        the apps you installed, with where they came from and the
              command that installs each again: pacman's explicit installs
              (Arch's repositories, and foreign = the AUR, with the AUR
              helper), Flatpak apps with their remotes, fnm and nvm's Node
              versions, rustup's toolchains (components, targets, the
              default), cargo install, pipx, uv tools, npm -g, bun and deno
              globals, AppImages, and anything else in ~/.local/bin ("origin
              unknown"). Only managers that are here are asked, each with a
              timeout, never as root.
Never: passwords, keyrings, ssh or gpg keys, tokens, browser profiles,
personal documents (`never`: those folders and names are refused even when a
setting points at them).

The file ("Tessera backup YYYY-MM-DD.tar.zst", .tar.gz where Python has no
zstd; 0600) has a manifest.json: the format, when it was made, what's in it
(no host name; the home folder it came from, to move paths to this one).
Restore shows what will change first (restore-plan); before it writes, what
it replaces is kept in $XDG_STATE_HOME/tessera/backups/before-restore-<time>
(undo puts it back). Files are written whole (a temporary file, renamed).
Pacman and AUR apps are installed by Software Update's job (the root
helper, one password; the AUR helper), not here: apps-install leaves them
to it. Flatpak apps get Flathub added first when it's missing (another
remote only when it's already set up here); user-level managers run as you;
one that's missing makes its apps "needs <manager>" (installing the
manager's package comes first).

What Settings shows for each app is what runs (audit #704): inspect and
apps-status rebuild every app from its checked fields (checked_app), with
"command" = shown_command(install_steps(…)), the very argv apps-install
runs; the file's own "command" and "source" strings are never used. Names
are checked per source and sources come from an allow-list: a git or https
address in a backup is never followed (those apps are listed, refused, with
why). Restore writes only to an allow-list (allowed(): the three settings
files, Restored pictures, your ~/.config/hypr files; Tessera's own hypr
files only beside, as .from-backup), never through a link. A backup from
another computer or user (manifest "computer": a hash of the machine ID and
uid, no host name) is "elsewhere": apps-install, and a restore of Hyprland
files that run, need --confirm-elsewhere, which Settings passes only after
showing what would run and a second yes.

Prints one JSON object: {"ok": true, …} or {"ok": false, "error": "…"},
exit 0 or 1 (apps-install: a JSON line per step before it).
"""
import argparse
import copy
import fcntl
import hashlib
import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import time

FORMAT = 1
PARTS = ("settings", "hypr", "wallpapers", "login", "apps")
PART_NAMES = {"settings": "Tessera settings", "hypr": "Hyprland edits", "wallpapers": "Wallpapers",
              "login": "Login screen look", "apps": "Apps I installed"}
MAX_JSON = 8 * 1024 * 1024
MAX_HYPR = 1024 * 1024
MAX_PICTURE = 64 * 1024 * 1024
MAX_TOTAL = 1024 * 1024 * 1024
CMD_TIMEOUT = 20      # an inventory question (s)
ITEM_TIMEOUT = 1800   # one app's install (s): cargo builds take minutes
SETTINGS_FILES = ("config.json", "keybinds.json", "focus.json")
# state.json's keys that are a preference (common/UiState.qml), not a record
STATE_KEYS = ("overlay", "crosshair", "stacks", "shortcutButtons", "conflictsMuted")
PICTURE_EXT = (".jpg", ".jpeg", ".png", ".webp", ".jxl", ".avif", ".bmp", ".gif")
# What restore writes beside Tessera's own hypr file you had edited
BESIDE = ".from-backup"
# hypr files that are never yours to carry (another copy, a backup)
HYPR_SKIP = (".new", ".pre-tessera", ".bak", ".orig", ".tmp", ".swp", "~", BESIDE)
# Written by install.sh for this computer (tools/config-sync.py's GENERATED)
GENERATED = frozenset({"tessera/keyboard.lua", "tessera/gpu.lua", "tessera/vm.lua", "tessera/from-your-old-config.lua"})

# ── Never in a backup ───────────────────────────────────────────────────
NEVER_DIRS = (".ssh", ".gnupg", ".local/share/keyrings", ".password-store", ".pki", ".aws", ".kube", ".docker",
              ".config/gh", ".mozilla", ".thunderbird", ".librewolf", ".waterfox", ".config/google-chrome",
              ".config/chromium", ".config/BraveSoftware", ".config/vivaldi", ".config/microsoft-edge",
              ".config/opera", ".config/zen", ".var/app", "Documents", ".local/share/tessera/accounts")
NEVER_NAME = re.compile(r"(?i)(^id_(rsa|dsa|ecdsa|ed25519)|\.(pem|key|p12|pfx|kdbx|gpg|asc|keystore|jks)$|token|secret|passw|"
                        r"credential|keyring|cookie|^\.netrc$|git-credentials|ai-keys|^calendars\.json$)")


def never(path, home=None):
    """A file no backup takes: in a folder of keys, keyrings, browser
    profiles or documents, or named like a key, token or password (the path
    as given and where it really is)"""
    home = home or os.path.expanduser("~")
    for p in (os.path.abspath(path), os.path.realpath(path)):
        rel = os.path.relpath(p, home)
        if not rel.startswith(".."):
            if any(rel == d or rel.startswith(d + "/") for d in NEVER_DIRS):
                return True
        if NEVER_NAME.search(os.path.basename(p)):
            return True
    return False


# ── Names a restore acts on (never trusted from the file) ───────────────
PKG = re.compile(r"^[a-z0-9@._+-]+$")
FLATPAK_ID = re.compile(r"^[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]+){2,}$")
REMOTE = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
HTTPS = re.compile(r"^https://[A-Za-z0-9.-]+(:[0-9]{1,5})?(/[A-Za-z0-9._~%/+-]*)?$")
NODE = re.compile(r"^v?[0-9]{1,3}\.[0-9]{1,4}\.[0-9]{1,4}$")
TOOLCHAIN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,80}$")
CRATE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$")
FEATURE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_/-]{0,63}$")
PYPKG = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$")
NPM = re.compile(r"^(@[a-z0-9][a-z0-9._-]*/)?[a-z0-9][a-z0-9._-]{0,213}$")


def package_name(n):
    """A pacman or AUR package's name, as sysupdate.py and the root helper
    take one: ^[a-z0-9@._+-]+$, never starting with "-" (an option)"""
    return isinstance(n, str) and bool(PKG.match(n)) and not n.startswith(("-", "."))


def ok_name(rx, n):
    return isinstance(n, str) and bool(rx.match(n)) and not n.startswith("-")


class Refused(Exception):
    """A file or a request this won't act on; the message says how to fix it.
    `undo`: a restore's undo file, when one is left to offer."""

    def __init__(self, msg="", undo=""):
        super().__init__(msg)
        self.undo = undo


# ── Places ──────────────────────────────────────────────────────────────
HERE = os.path.dirname(os.path.realpath(__file__))


def xdg(var, rel):
    return os.environ.get(var) or os.path.join(os.path.expanduser("~"), rel)


def default_shell():
    return os.path.dirname(os.path.dirname(HERE))


def state_dir():
    return os.path.join(xdg("XDG_STATE_HOME", ".local/state"), "tessera")


def hypr_dir():
    return os.path.join(xdg("XDG_CONFIG_HOME", ".config"), "hypr")


def _load(name, file):
    import importlib.util
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, file))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# The settings file's scrub, merge and pictures (settings-backup.py: one
# place that knows what's secret or this computer's own)
SB = _load("settings_backup", "settings-backup.py")


class Ctx:
    """Where things are: the settings folder, the install (shell/), home"""

    def __init__(self, config=None, shell=None):
        self.home = os.path.expanduser("~")
        self.config = os.path.abspath(config or os.path.join(xdg("XDG_CONFIG_HOME", ".config"), "tessera"))
        self.shell = os.path.abspath(shell or default_shell())
        self.repo = os.path.dirname(self.shell)
        self.hypr = hypr_dir()
        self.state = state_dir()
        self.pictures = os.path.join(self.home, "Pictures", "Wallpapers", "Restored")


def read_json(path, default):
    return SB.read_json(path, default)


def write_whole(path, data, mode=0o600):
    SB.write_whole(path, data, mode)


def dump(v, indent=2):
    return json.dumps(v, indent=indent, ensure_ascii=False).encode()


def size_words(n):
    if n < 1024:
        return "%d bytes" % n
    if n < 1024 * 1024:
        return "%d KB" % max(1, round(n / 1024))
    if n < 1024 ** 3:
        return "%.1f MB" % (n / 1024 / 1024)
    return "%.1f GB" % (n / 1024 ** 3)


def plural(n, one, many=None):
    return "%d %s" % (n, one if n == 1 else (many or one + "s"))


# ── Settings ────────────────────────────────────────────────────────────
def is_login_key(k):
    return isinstance(k, str) and k.startswith("login")


def login_of(config):
    lock = config.get("lock") if isinstance(config, dict) and isinstance(config.get("lock"), dict) else {}
    return {k: v for k, v in lock.items() if is_login_key(k)}


def settings_members(ctx):
    """settings/*.json: {member: bytes}, and how many changes they hold"""
    config = SB.scrub_config(read_json(os.path.join(ctx.config, "config.json"), {}))
    out = {"settings/config.json": dump(config)}
    keybinds = read_json(os.path.join(ctx.config, "keybinds.json"), None)
    if isinstance(keybinds, dict):
        out["settings/keybinds.json"] = dump(keybinds)
    focus = read_json(os.path.join(ctx.config, "focus.json"), None)
    if focus is not None:
        out["settings/focus.json"] = dump({"modes": SB.focus_modes(focus)})
    state = read_json(os.path.join(ctx.state, "state.json"), {})
    prefs = {k: state[k] for k in STATE_KEYS if isinstance(state, dict) and k in state}
    if prefs:
        out["settings/state.json"] = dump(prefs)
    return out, config, keybinds, focus


def leaves(v, prefix=""):
    """{"a.b.c": value} of a settings object"""
    out = {}
    if isinstance(v, dict):
        for k, x in v.items():
            out.update(leaves(x, prefix + k + ".") if isinstance(x, dict) else {prefix + k: x})
    return out


def shortcut_count(kb):
    if not isinstance(kb, dict):
        return 0
    n = len(kb.get("keybinds") or {}) + len(kb.get("shortcuts") or [])
    g = kb.get("gestures") if isinstance(kb.get("gestures"), dict) else {}
    return n + len(g.get("changes") or {}) + len(g.get("custom") or [])


# ── Wallpapers ──────────────────────────────────────────────────────────
def chosen_pictures(config, focus):
    """[(where, path)]: the wallpaper settings and each Focus mode's"""
    out = []
    wp = config.get("wallpaper") if isinstance(config.get("wallpaper"), dict) else {}
    for key in SB.WALLPAPER_KEYS:
        if isinstance(wp.get(key), str) and wp[key]:
            out.append(("wallpaper." + key, wp[key]))
    for m in SB.focus_modes(focus) if focus is not None else []:
        if isinstance(m.get("wallpaper"), str) and m["wallpaper"] and isinstance(m.get("id"), str):
            out.append(("focus." + m["id"], m["wallpaper"]))
    return out


def picture_plan(ctx, config, focus, carry=True):
    """(manifest "wallpapers" {where: {bundled}|{file, from}}, {member: path},
    [left out]): Tessera's own by name, the others carried"""
    where_to, files, left, seen, total = {}, {}, [], {}, 0
    for where, path in chosen_pictures(config, focus):
        name = SB.bundled_name(path)
        if name:
            where_to[where] = {"bundled": name}
            continue
        if not carry:
            continue
        if not path.lower().endswith(PICTURE_EXT) or never(path, ctx.home) or not os.path.isfile(path):
            left.append(path)
            continue
        real = os.path.realpath(path)
        if real in seen:
            where_to[where] = {"file": seen[real], "from": path}
            continue
        try:
            size = os.path.getsize(real)
        except OSError:
            left.append(path)
            continue
        if size > MAX_PICTURE or total + size > MAX_TOTAL:
            left.append(path)
            continue
        clean = re.sub(r"[^A-Za-z0-9 _.,()+-]", "_", os.path.basename(path))[:120] or "picture.jpg"
        member = "wallpapers/%d-%s" % (len(files), clean)
        files[member] = real
        seen[real] = member
        total += size
        where_to[where] = {"file": member, "from": path}
    return where_to, files, left


# ── Hyprland edits ──────────────────────────────────────────────────────
def shipped(ctx):
    """{rel: bytes} of what Tessera ships into ~/.config/hypr (hypr/'s
    hyprland.lua and tessera/*, as install.sh copies them)"""
    out = {}
    lua = os.path.join(ctx.repo, "hypr", "hyprland.lua")
    if os.path.isfile(lua):
        out["hyprland.lua"] = open(lua, "rb").read()
    d = os.path.join(ctx.repo, "hypr", "tessera")
    if os.path.isdir(d):
        for n in sorted(os.listdir(d)):
            p = os.path.join(d, n)
            if os.path.isfile(p) and not os.path.islink(p):
                out["tessera/" + n] = open(p, "rb").read()
    return out


def base_of(rel):
    """The shipped file as Tessera last put it there (config-sync.py's base)"""
    p = os.path.join(xdg("XDG_DATA_HOME", ".local/share"), "tessera", "hypr-base", rel)
    try:
        with open(p, "rb") as f:
            return f.read()
    except OSError:
        return None


def hypr_files(ctx):
    """([(rel, path)] your own, [(rel, path)] Tessera's that you edited)"""
    own, edited = [], []
    top = ctx.hypr
    if not os.path.isdir(top):
        return own, edited
    ships = shipped(ctx)
    for root, dirs, files in os.walk(top):
        depth = os.path.relpath(root, top).count("/") + (0 if root == top else 1)
        dirs[:] = sorted(d for d in dirs if not d.startswith(".") and depth < 3 and not os.path.islink(os.path.join(root, d)))
        for n in sorted(files):
            path = os.path.join(root, n)
            rel = os.path.relpath(path, top)
            if os.path.islink(path) or not os.path.isfile(path) or rel in GENERATED or n.endswith(HYPR_SKIP):
                continue
            if never(path, ctx.home) or not MEMBER_REL.match(rel):
                continue
            try:
                if os.path.getsize(path) > MAX_HYPR:
                    continue
                data = open(path, "rb").read()
            except OSError:
                continue
            if rel in ships:
                was = base_of(rel)
                if data != (was if was is not None else ships[rel]):
                    edited.append((rel, path))
            elif rel == "hyprland.lua" or rel.startswith("tessera/"):
                # (Tessera's folder without a checkout to compare with: left alone)
                if ships:
                    own.append((rel, path))
            else:
                own.append((rel, path))
    return own, edited


# ── Apps and where they came from ───────────────────────────────────────
EXTRA_BINS = {
    "fnm": (".local/share/fnm/fnm", ".fnm/fnm", ".cargo/bin/fnm"),
    "rustup": (".cargo/bin/rustup",), "cargo": (".cargo/bin/cargo",),
    "uv": (".local/bin/uv", ".cargo/bin/uv"), "pipx": (".local/bin/pipx",),
    "bun": (".bun/bin/bun",), "deno": (".deno/bin/deno",),
}


def find_bin(name):
    p = shutil.which(name)
    if p:
        return p
    home = os.path.expanduser("~")
    for rel in EXTRA_BINS.get(name, ()):
        c = os.path.join(home, rel)
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return c
    return None


def run(argv, timeout=CMD_TIMEOUT):
    """A manager's answer (stdout), None when it isn't here or failed. As
    you, with a timeout, in plain English and nothing on stdin"""
    exe = find_bin(argv[0])
    if not exe:
        return None
    env = dict(os.environ, LC_ALL="C", NO_COLOR="1")
    try:
        r = subprocess.run([exe] + list(argv[1:]), capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL, env=env)
    except (OSError, subprocess.SubprocessError):
        return None
    return r.stdout if r.returncode == 0 else None


# Each source: its name, the program it needs, and the package that brings
# that program (for "needs fnm": a pacman package, or "aur:<name>")
MANAGERS = {
    "pacman": {"name": "Arch repositories", "bin": "pacman", "package": ""},
    "aur": {"name": "AUR", "bin": "pacman", "package": ""},
    "flatpak": {"name": "Flatpak", "bin": "flatpak", "package": "flatpak"},
    "fnm": {"name": "fnm (Node.js versions)", "bin": "fnm", "package": "fnm"},
    "nvm": {"name": "nvm (Node.js versions)", "bin": "", "package": "aur:nvm"},
    "rustup": {"name": "rustup (Rust toolchains)", "bin": "rustup", "package": "rustup"},
    "cargo": {"name": "cargo install", "bin": "cargo", "package": "rustup"},
    "pipx": {"name": "pipx", "bin": "pipx", "package": "python-pipx"},
    "uv": {"name": "uv tools", "bin": "uv", "package": "uv"},
    "npm": {"name": "npm (global)", "bin": "npm", "package": "npm"},
    "bun": {"name": "bun (global)", "bin": "bun", "package": "aur:bun-bin"},
    "deno": {"name": "deno", "bin": "deno", "package": "deno"},
    "appimage": {"name": "AppImages", "bin": "", "package": ""},
    "local": {"name": "~/.local/bin", "bin": "", "package": ""},
}
ORDER = list(MANAGERS)


def _ver(v):
    return tuple(int(x) for x in v.lstrip("v").split("."))


def nvm_default(versions, default, nd=""):
    """The one installed version nvm's default alias means, or None: "20"
    the newest v20.x (not v2.x nor v200), "20.11" the newest v20.11.x, an
    alias (lts/iron, or one of yours) followed through nvm's alias folder,
    "node" or "stable" the newest of all"""
    d = (default or "").strip()
    for _ in range(5):  # (an alias naming an alias)
        f = os.path.join(nd, "alias", d) if nd and d and ".." not in d and not d.startswith("/") else ""
        if f and os.path.isfile(f) and not re.fullmatch(r"v?[0-9.]+", d):
            try:
                d = open(f).read().strip()
            except OSError:
                break
        else:
            break
    if not versions or not d:
        return None
    if d in ("node", "stable"):
        return max(versions, key=_ver)
    want = d.lstrip("v")
    if not re.fullmatch(r"[0-9]+(\.[0-9]+){0,2}", want):
        return None
    hits = [v for v in versions if v.lstrip("v") == want or v.lstrip("v").startswith(want + ".")]
    return max(hits, key=_ver) if hits else None


def nvm_dir():
    return os.environ.get("NVM_DIR") or os.path.join(os.path.expanduser("~"), ".nvm")


def manager_here(group):
    if group == "nvm":
        return os.path.isfile(os.path.join(nvm_dir(), "nvm.sh"))
    b = MANAGERS.get(group, {}).get("bin")
    return True if not b else bool(find_bin(b))


def entry(group, name, version, source, command, installable=True, **more):
    e = {"id": "%s:%s" % (group, name if group not in ("fnm", "nvm") else version), "group": group, "name": name,
         "version": version or "", "source": source, "command": command, "installable": installable,
         "system": group in ("pacman", "aur")}
    e.update(more)
    return e


def parse_pacman(text):
    """[(name, version)] from `pacman -Qen` / `-Qem`"""
    out = []
    for line in (text or "").splitlines():
        parts = line.split()
        if len(parts) >= 2 and package_name(parts[0]):
            out.append((parts[0], parts[1]))
    return out


def parse_flatpak(apps, remotes):
    """([(app, origin, version, installation)], {remote: (url, installation)})"""
    rem = {}
    for line in (remotes or "").splitlines():
        f = line.split("\t")
        if len(f) >= 2 and REMOTE.match(f[0].strip()):
            opts = f[2] if len(f) > 2 else ""
            rem[f[0].strip()] = (f[1].strip(), "user" if "user" in opts else "system")
    out = []
    for line in (apps or "").splitlines():
        f = [x.strip() for x in line.split("\t")]
        if len(f) >= 2 and FLATPAK_ID.match(f[0]):
            out.append((f[0], f[1], f[2] if len(f) > 2 else "", f[3] if len(f) > 3 and f[3] else "system"))
    return out, rem


def parse_fnm(text):
    """[(version, default)] from `fnm list` ("* v20.11.1 default", "* system")"""
    out = []
    for line in (text or "").splitlines():
        t = line.replace("*", " ").split()
        if t and NODE.match(t[0]):
            out.append((t[0] if t[0].startswith("v") else "v" + t[0], "default" in t[1:]))
    return out


def parse_rustup_toolchains(text):
    """[(toolchain, default)] from `rustup toolchain list`"""
    out = []
    for line in (text or "").splitlines():
        t = line.split()
        if t and TOOLCHAIN.match(t[0]):
            out.append((t[0], "default" in line[len(t[0]):]))
    return out


HOST_SUFFIX = re.compile(r"-(x86_64|aarch64|i686|armv7|riscv64)[a-z0-9_-]*$")


def parse_rustup_components(text):
    """The components to ask for again (not rustc, cargo, rust-std, which
    come with any toolchain), without the host's triple"""
    out = []
    for line in (text or "").splitlines():
        n = HOST_SUFFIX.sub("", line.strip().split(" ")[0])
        if n and n not in ("rustc", "cargo", "rust-std", "rust-mingw") and TOOLCHAIN.match(n) and n not in out:
            out.append(n)
    return out


def parse_crates2(text):
    """[(crate, version, source kind, git url, features, no_default)] from
    ~/.cargo/.crates2.json"""
    try:
        d = json.loads(text or "")
    except ValueError:
        return []
    out = []
    for key, info in (d.get("installs") or {}).items() if isinstance(d, dict) else []:
        m = re.match(r"^(\S+) (\S+) \((\S+)\)$", key)
        if not m or not CRATE.match(m.group(1)):
            continue
        src, git = m.group(3), ""
        if src.startswith("registry+"):
            kind = "crates.io" if "crates.io-index" in src or "index.crates.io" in src else "registry"
        elif src.startswith("git+"):
            kind, git = "git", src[4:].split("?")[0].split("#")[0]
        else:
            kind = "path"
        info = info if isinstance(info, dict) else {}
        feats = [f for f in info.get("features") or [] if isinstance(f, str) and FEATURE.match(f)]
        out.append((m.group(1), m.group(2), kind, git, feats, bool(info.get("no_default_features")), list(info.get("bins") or [])))
    return out


def parse_pipx(text):
    """[(venv, package, version, spec, apps)] from `pipx list --json`"""
    try:
        d = json.loads(text or "")
    except ValueError:
        return []
    out = []
    for name, v in ((d.get("venvs") or {}).items() if isinstance(d, dict) else []):
        mp = ((v or {}).get("metadata") or {}).get("main_package") or {}
        pkg = mp.get("package") or name
        out.append((name, pkg, mp.get("package_version") or "", mp.get("package_or_url") or pkg, list(mp.get("apps") or [])))
    return out


def parse_uv(text):
    """[(tool, version, bins)] from `uv tool list`"""
    out = []
    for line in (text or "").splitlines():
        if line.startswith("-"):
            if out:
                out[-1][2].append(line[1:].strip())
            continue
        t = line.split()
        if t and PYPKG.match(t[0]):
            out.append((t[0], t[1].lstrip("v") if len(t) > 1 else "", []))
    return out


def parse_npm(text):
    """[(package, version)] from `npm ls -g --json --depth=0`"""
    try:
        d = json.loads(text or "")
    except ValueError:
        return []
    deps = d.get("dependencies") if isinstance(d, dict) else None
    return [(n, (v or {}).get("version", "")) for n, v in (deps or {}).items() if NPM.match(n) and n not in ("npm", "corepack")]


def is_appimage(name):
    return name.lower().endswith(".appimage")


def inventory_apps(ctx):
    """{"apps": [entries], "managers": {group: {present, …}}}: what's
    installed here, from each manager that's here"""
    apps, managers, bins = [], {}, set()
    home = ctx.home
    # pacman: explicit installs, Arch's repositories (native) and foreign (the AUR)
    if find_bin("pacman"):
        helper = "paru" if find_bin("paru") else "yay" if find_bin("yay") else ""
        managers["pacman"] = {"present": True}
        managers["aur"] = {"present": True, "helper": helper}
        for n, v in parse_pacman(run(["pacman", "-Qen"])):
            apps.append(entry("pacman", n, v, "Arch repositories", "sudo pacman -S --needed " + n))
        for n, v in parse_pacman(run(["pacman", "-Qem"])):
            apps.append(entry("aur", n, v, "AUR" + (" (with %s)" % helper if helper else ""), "%s -S --needed %s" % (helper or "yay", n)))
    # Flatpak: each app with its remote, the remotes with their addresses
    if find_bin("flatpak"):
        listed, remotes = parse_flatpak(run(["flatpak", "list", "--app", "--columns=application,origin,version,installation"]),
                                        run(["flatpak", "remotes", "--columns=name,url,options"]))
        managers["flatpak"] = {"present": True, "remotes": [{"name": n, "url": u, "installation": i} for n, (u, i) in remotes.items()]}
        for app, origin, ver, inst in listed:
            url = remotes.get(origin, ("", ""))[0]
            scope = "--user " if inst == "user" else ""
            apps.append(entry("flatpak", app, ver, "Flatpak, from " + origin, "flatpak install %s%s %s" % (scope, origin, app),
                              remote=origin, url=url, installation="user" if inst == "user" else "system"))
    # fnm and nvm: Node.js versions
    if find_bin("fnm"):
        managers["fnm"] = {"present": True}
        for v, default in parse_fnm(run(["fnm", "list"])):
            apps.append(entry("fnm", "node", v, "fnm" + (", the default" if default else ""), "fnm install %s%s" % (v, " && fnm default " + v if default else ""), default=default))
    nd = nvm_dir()
    if os.path.isfile(os.path.join(nd, "nvm.sh")):
        managers["nvm"] = {"present": True}
        default = ""
        try:
            default = open(os.path.join(nd, "alias", "default")).read().strip()
        except OSError:
            pass
        vd = os.path.join(nd, "versions", "node")
        versions = [v for v in sorted(os.listdir(vd)) if NODE.match(v)] if os.path.isdir(vd) else []
        chosen = nvm_default(versions, default, nd)
        for v in versions:
            is_default = v == chosen
            apps.append(entry("nvm", "node", v, "nvm" + (", the default" if is_default else ""), "nvm install " + v, default=is_default))
    # rustup: toolchains, their components and targets, the default
    if find_bin("rustup"):
        managers["rustup"] = {"present": True}
        for tc, default in parse_rustup_toolchains(run(["rustup", "toolchain", "list"])):
            comps = parse_rustup_components(run(["rustup", "component", "list", "--installed", "--toolchain", tc]))
            targets = [t.strip() for t in (run(["rustup", "target", "list", "--installed", "--toolchain", tc]) or "").splitlines() if TOOLCHAIN.match(t.strip())]
            cmd = "rustup toolchain install " + tc + (" --component " + ",".join(comps) if comps else "") + (" --target " + ",".join(targets) if targets else "")
            apps.append(entry("rustup", tc, "", "rustup" + (", the default" if default else ""), cmd + (" && rustup default " + tc if default else ""),
                              default=default, components=comps, targets=targets))
    # cargo install
    cargo_home = os.environ.get("CARGO_HOME") or os.path.join(home, ".cargo")
    crates = os.path.join(cargo_home, ".crates2.json")
    if os.path.isfile(crates):
        managers["cargo"] = {"present": bool(find_bin("cargo"))}
        try:
            text = open(crates).read(MAX_JSON)
        except OSError:
            text = ""
        for name, ver, kind, git, feats, nodef, cbins in parse_crates2(text):
            bins.update(cbins)
            flags = (" --features " + ",".join(feats) if feats else "") + (" --no-default-features" if nodef else "")
            if kind == "git" and HTTPS.match(git):
                apps.append(entry("cargo", name, ver, "cargo, from " + git, "cargo install --git %s %s%s" % (git, name, flags), git=git, features=feats, noDefault=nodef))
            elif kind in ("crates.io", "registry"):
                apps.append(entry("cargo", name, ver, "cargo, from crates.io", "cargo install %s%s" % (name, flags), features=feats, noDefault=nodef))
            else:
                apps.append(entry("cargo", name, ver, "cargo, from a folder on that computer (origin unknown)", "", installable=False))
    # pipx
    if find_bin("pipx"):
        managers["pipx"] = {"present": True}
        for venv, pkg, ver, spec, papps in parse_pipx(run(["pipx", "list", "--json"])):
            bins.update(papps)
            good = PYPKG.match(spec) or HTTPS.match(spec.replace("git+https://", "https://", 1))
            apps.append(entry("pipx", pkg, ver, "pipx", "pipx install " + spec if good else "", installable=bool(good), spec=spec if good else ""))
    # uv tools
    if find_bin("uv"):
        managers["uv"] = {"present": True}
        for tool, ver, ubins in parse_uv(run(["uv", "tool", "list"])):
            bins.update(ubins)
            apps.append(entry("uv", tool, ver, "uv tools", "uv tool install " + tool))
    # npm -g
    if find_bin("npm"):
        prefix = (run(["npm", "prefix", "-g"]) or "").strip()
        managers["npm"] = {"present": True, "prefix": prefix}
        for pkg, ver in parse_npm(run(["npm", "ls", "-g", "--json", "--depth=0"], timeout=40)):
            bins.add(pkg.rsplit("/", 1)[-1])
            apps.append(entry("npm", pkg, ver, "npm (global)" + (", in " + prefix.replace(home, "~", 1) if prefix else ""), "npm install -g " + pkg))
    # bun and deno globals
    bun_pkg = os.path.join(home, ".bun", "install", "global", "package.json")
    if find_bin("bun") or os.path.isfile(bun_pkg):
        managers["bun"] = {"present": bool(find_bin("bun"))}
        deps = read_json(bun_pkg, {}).get("dependencies") if isinstance(read_json(bun_pkg, {}), dict) else {}
        for pkg, ver in (deps or {}).items():
            if NPM.match(pkg):
                apps.append(entry("bun", pkg, str(ver).lstrip("^~"), "bun (global)", "bun add -g " + pkg))
    deno_bin = os.path.join(home, ".deno", "bin")
    if os.path.isdir(deno_bin):
        managers["deno"] = {"present": bool(find_bin("deno"))}
        for n in sorted(os.listdir(deno_bin)):
            if n != "deno" and not n.startswith("."):
                apps.append(entry("deno", n, "", "deno, found (where it came from isn't recorded)", "", installable=False))
    # AppImages and anything else in ~/.local/bin
    found = []
    for d in (os.path.join(home, "Applications"), os.path.join(home, ".local", "bin")):
        if os.path.isdir(d):
            for n in sorted(os.listdir(d)):
                if is_appimage(n) and os.path.isfile(os.path.join(d, n)) and n not in found:
                    found.append(n)
                    apps.append(entry("appimage", n, "", "AppImage, found, origin unknown", "", installable=False))
    lb = os.path.join(home, ".local", "bin")
    if os.path.isdir(lb):
        owned_dirs = [os.path.join(home, p) for p in (".local/share/pipx", ".local/pipx", ".local/share/uv", ".cargo")] + [os.path.realpath(ctx.repo)]
        for n in sorted(os.listdir(lb)):
            p = os.path.join(lb, n)
            if n.startswith(".") or is_appimage(n) or os.path.isdir(p) or n in bins or n.startswith("tessera"):
                continue
            real = os.path.realpath(p)
            if "/node_modules/" in real or any(real == o or real.startswith(o + "/") for o in owned_dirs):
                continue
            apps.append(entry("local", n, "", "In ~/.local/bin, origin unknown", "", installable=False))
    return {"apps": apps, "managers": managers}


def apps_summary(apps):
    groups = []
    for g in ORDER:
        n = sum(1 for a in apps if a["group"] == g)
        if n:
            groups.append("%d %s" % (n, MANAGERS[g]["name"]))
    return (plural(len(apps), "app") + (": " + ", ".join(groups) if groups else "")) if apps else "None found"


# ── Inventory: what a backup can hold ───────────────────────────────────
def inventory(ctx, with_apps=True):
    members, config, keybinds, focus = settings_members(ctx)
    focus_n = len(SB.focus_modes(focus)) if focus is not None else 0
    settings_bytes = sum(len(v) for v in members.values())
    s_bits = [plural(len(leaves(config)), "setting")]
    if shortcut_count(keybinds):
        s_bits.append(plural(shortcut_count(keybinds), "shortcut change"))
    if focus_n:
        s_bits.append(plural(focus_n, "Focus mode"))
    pics, files, left = picture_plan(ctx, read_json(os.path.join(ctx.config, "config.json"), {}), focus)
    pic_bytes = sum(os.path.getsize(p) for p in files.values())
    own, edited = hypr_files(ctx)
    hypr_bytes = sum(os.path.getsize(p) for _, p in own + edited)
    login = login_of(config)
    parts = [
        {"id": "settings", "name": PART_NAMES["settings"], "count": len(leaves(config)), "bytes": settings_bytes,
         "summary": ", ".join(s_bits) + " · " + size_words(settings_bytes)},
        {"id": "hypr", "name": PART_NAMES["hypr"], "count": len(own) + len(edited), "bytes": hypr_bytes,
         "summary": (", ".join(x for x in [plural(len(own), "file") + " of yours" if own else "", plural(len(edited), "edited Tessera file") if edited else ""] if x)
                     + " · " + size_words(hypr_bytes)) if own or edited else "Nothing of yours in ~/.config/hypr",
         "files": [r for r, _ in own] + [r + " (edited)" for r, _ in edited]},
        {"id": "wallpapers", "name": PART_NAMES["wallpapers"], "count": len(files), "bytes": pic_bytes,
         "summary": (plural(len(files), "picture") + " · " + size_words(pic_bytes)) if files else ("Tessera's own (they go by name)" if pics else "None chosen"),
         "left": left},
        {"id": "login", "name": PART_NAMES["login"], "count": len(login), "bytes": len(dump(login)),
         "summary": ("Style: " + str(login.get("loginStyle", "your look"))) if login else "As it comes"},
    ]
    out = {"ok": True, "parts": parts}
    if with_apps:
        inv = inventory_apps(ctx)
        parts.append({"id": "apps", "name": PART_NAMES["apps"], "count": len(inv["apps"]), "bytes": len(dump(inv)), "summary": apps_summary(inv["apps"])})
        out["apps"] = inv["apps"]
        out["managers"] = inv["managers"]
    return out


# ── Create ──────────────────────────────────────────────────────────────
def zstd_ok():
    try:
        import compression.zstd  # noqa: F401 (Python 3.14)
        return True
    except ImportError:
        return False


def out_path(dest):
    dest = os.path.abspath(os.path.expanduser(dest))
    ext = ".tar.zst" if zstd_ok() else ".tar.gz"
    if os.path.isdir(dest):
        if not os.access(dest, os.W_OK):
            raise Refused("Tessera can't write to that folder. Choose a folder in your home, like Documents.")
        return SB.unique(dest, "Tessera backup %s%s" % (time.strftime("%Y-%m-%d"), ext))
    folder = os.path.dirname(dest)
    if not os.path.isdir(folder):
        raise Refused("That folder isn't there any more. Choose another folder and try again.")
    if not dest.endswith((".tar.zst", ".tar.gz")):
        dest += ext
    if dest.endswith(".tar.zst") and not zstd_ok():
        dest = dest[:-len(".tar.zst")] + ".tar.gz"
    return dest


def pack(path, members, files):
    """The archive: members (bytes) and files (paths, read as they go in),
    written beside its place, then renamed: never half a file"""
    mode = "w:zst" if path.endswith(".zst") else "w:gz"
    d = os.path.dirname(path)
    tmp = os.path.join(d, ".%s.%d.tmp" % (os.path.basename(path), os.getpid()))
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, "wb") as raw, tarfile.open(fileobj=raw, mode=mode) as tar:
            now = int(time.time())

            def add(name, data):
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), 0o600, now
                tar.addfile(info, io.BytesIO(data))
            add("manifest.json", members["manifest.json"])
            for name in sorted(n for n in members if n != "manifest.json"):
                add(name, members[name])
            for name in sorted(files):
                with open(files[name], "rb") as f:
                    add(name, f.read(MAX_PICTURE + 1)[:MAX_PICTURE])
        os.chmod(tmp, 0o600)
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def create(dest, include, ctx):
    include = [p for p in PARTS if p in include]
    if not include:
        raise Refused("Choose at least one thing to back up.")
    path = out_path(dest)
    members, files, contents = {}, {}, {}
    smembers, config, keybinds, focus = settings_members(ctx)
    raw_config = read_json(os.path.join(ctx.config, "config.json"), {})
    pics, pic_files, left = picture_plan(ctx, raw_config, focus, carry="wallpapers" in include)
    if "settings" in include:
        members.update(smembers)
        contents["settings"] = {"settings": len(leaves(config)), "shortcuts": shortcut_count(keybinds),
                                "focusModes": len(SB.focus_modes(focus)) if focus is not None else 0,
                                "files": sorted(n.split("/", 1)[1] for n in smembers)}
    if "wallpapers" in include:
        files.update(pic_files)
        contents["wallpapers"] = {"pictures": len(pic_files), "left": left}
    if "login" in include:
        members["login.json"] = dump({"lock": login_of(raw_config)})
        contents["login"] = {"settings": len(login_of(raw_config))}
    if "hypr" in include:
        own, edited = hypr_files(ctx)
        for rel, p in own:
            files["hypr/" + rel] = p
        for rel, p in edited:
            files["hypr-edited/" + rel] = p
        contents["hypr"] = {"own": [r for r, _ in own], "edited": [r for r, _ in edited]}
    if "apps" in include:
        inv = inventory_apps(ctx)
        members["apps.json"] = dump(inv)
        contents["apps"] = {"apps": len(inv["apps"]), "groups": sorted({a["group"] for a in inv["apps"]})}
    manifest = {"app": "Tessera", "kind": "backup", "format": FORMAT, "created": time.strftime("%Y-%m-%dT%H:%M:%S"),
                "home": ctx.home, "computer": computer_tag(), "parts": include, "contents": contents, "wallpapers": pics}
    members["manifest.json"] = dump(manifest)
    pack(path, members, files)
    return {"file": path, "bytes": os.path.getsize(path), "parts": include}


# ── Reading a backup ────────────────────────────────────────────────────
NOT_BACKUP = "This isn't a Tessera backup, or it's damaged. Choose a file made with Back Up."
ELSEWHERE = "This backup is from another computer. Look at what it would run, then confirm in Settings."
SAFE = r"[A-Za-z0-9 _.,()+@=-]+"
MEMBER_REL = re.compile(r"^%s(/%s){0,3}$" % (SAFE, SAFE))
MEMBER = re.compile(r"^(manifest\.json|apps\.json|login\.json|settings/(config|keybinds|focus|state)\.json|"
                    r"wallpapers/[A-Za-z0-9 _.,()+-]{1,160}|(hypr|hypr-edited)/%s(/%s){0,3})$" % (SAFE, SAFE))
UNDO_MEMBER = re.compile(r"^(manifest\.json|files/[0-9]{1,6})$")


def limit_of(name):
    if name.startswith("wallpapers/"):
        return MAX_PICTURE
    if name.startswith(("hypr/", "hypr-edited/", "files/")):
        return MAX_PICTURE if name.startswith("files/") else MAX_HYPR
    return MAX_JSON


def unpack(path, undo=False):
    """The archive's members, checked: only names this writes, plain
    files, within the size limits; and its manifest"""
    pattern = UNDO_MEMBER if undo else MEMBER
    try:
        if os.path.getsize(path) > MAX_TOTAL + MAX_JSON * 8:
            raise Refused("That file is too large to be a Tessera backup.")
        tar = tarfile.open(path, mode="r:*")
    except tarfile.CompressionError:
        raise Refused("This backup is compressed with zstd, which this Python can't read. Update Python (3.14 or newer), then try again.")
    except (OSError, tarfile.TarError, EOFError):
        raise Refused(NOT_BACKUP)
    members, total = {}, 0
    with tar:
        try:
            for info in tar:
                name = info.name
                parts = name.split("/")
                if name.startswith(("/", "./")) or "\\" in name or any(p in ("", ".", "..") for p in parts):
                    raise Refused(NOT_BACKUP)
                if not pattern.match(name) or not info.isreg() or name in members:
                    raise Refused(NOT_BACKUP)
                limit = limit_of(name)
                if info.size > limit or total + info.size > MAX_TOTAL:
                    raise Refused("A part of that file is too large. Choose a file made with Back Up.")
                f = tar.extractfile(info)
                data = f.read(limit + 1) if f else b""
                if len(data) != info.size:
                    raise Refused(NOT_BACKUP)
                total += len(data)
                members[name] = data
        except (tarfile.TarError, EOFError, OSError):
            raise Refused(NOT_BACKUP)
    try:
        manifest = json.loads(members.get("manifest.json", b""))
    except ValueError:
        raise Refused(NOT_BACKUP)
    if not isinstance(manifest, dict) or manifest.get("app") != "Tessera" or manifest.get("kind") != ("undo" if undo else "backup"):
        raise Refused(NOT_BACKUP)
    if not isinstance(manifest.get("format"), int) or manifest["format"] > FORMAT:
        raise Refused("That backup was made by a newer Tessera. Update Tessera (Settings → Software Update), then try again.")
    return manifest, members


def json_member(members, name, default=None):
    if name not in members:
        return default
    try:
        return json.loads(members[name])
    except ValueError:
        raise Refused(NOT_BACKUP)


def backup_apps(members, remotes_here=None, helper=None):
    """The backup's apps as a restore sees them: each rebuilt from its
    checked fields only (checked_app), never the file's own "command" or
    "source" strings; one entry an id"""
    inv = json_member(members, "apps.json", {})
    apps = inv.get("apps") if isinstance(inv, dict) else None
    if remotes_here is None:
        remotes_here = flatpak_remotes_here()
    if helper is None:
        helper = aur_helper()
    out, seen = [], set()
    for i, a in enumerate(apps or []):
        if not isinstance(a, dict) or a.get("group") not in MANAGERS or not isinstance(a.get("name"), str):
            continue
        c = checked_app(a, remotes_here, helper, i)
        if c["id"] not in seen:
            seen.add(c["id"])
            out.append(c)
    return out


VERSION_TEXT = re.compile(r"^[A-Za-z0-9._+:~-]{0,64}$")
ID_KEY = re.compile(r"^[A-Za-z0-9@._+/-]{1,214}$")


def checked_app(a, remotes_here, helper="", index=0):
    """One app of a backup, with only the fields that pass their checks; its
    id, source and command made here from them. "command" is shown_command()
    of the very steps apps-install runs (audit #704: the file's own command
    string was shown while another one ran)"""
    g = a.get("group")
    n = a.get("name") if isinstance(a.get("name"), str) else ""
    v = a.get("version") if isinstance(a.get("version"), str) and VERSION_TEXT.match(a.get("version")) else ""
    c = {"group": g, "name": n, "version": v, "system": g in ("pacman", "aur")}
    if g == "flatpak":
        c["remote"] = a.get("remote") if isinstance(a.get("remote"), str) else ""
        c["installation"] = "user" if a.get("installation") == "user" else "system"
    if g in ("fnm", "nvm", "rustup"):
        c["default"] = a.get("default") is True
    if g == "rustup":
        c["components"] = [x for x in a.get("components") if ok_name(TOOLCHAIN, x)] if isinstance(a.get("components"), list) else []
        c["targets"] = [x for x in a.get("targets") if ok_name(TOOLCHAIN, x)] if isinstance(a.get("targets"), list) else []
    if g == "cargo":
        c["features"] = [x for x in a.get("features") if ok_name(FEATURE, x)] if isinstance(a.get("features"), list) else []
        c["noDefault"] = a.get("noDefault") is True
        if a.get("git") or a.get("url"):
            c["git"] = True  # (only that it had one: the address is never used)
    if g == "pipx":
        spec = a.get("spec")
        c["spec"] = spec if isinstance(spec, str) and ok_name(PYPKG, spec) else ("<address>" if spec else "")
    why = refused_why(c, remotes_here)
    key = v if g in ("fnm", "nvm") else n
    c["id"] = "%s:%s" % (g, key) if ID_KEY.match(key or "") and not key.startswith("-") else "%s:#%d" % (g, index)
    if c["system"]:
        c["installable"] = not why
        c["command"] = ("Software Update: " + ("pacman" if g == "pacman" else (helper or "yay")) + " -S --needed " + n) if not why else ""
    else:
        steps = install_steps(c, remotes_here)
        c["installable"] = bool(steps)
        c["command"] = shown_command(steps) if steps else ""
    c["why"] = why or ("" if c["installable"] else "it can't be installed again from here")
    c["source"] = source_words(c, helper)
    return c


def source_words(c, helper=""):
    """Where an app comes from, in words made here (never the file's)"""
    g = c["group"]
    if g == "pacman":
        return "Arch repositories"
    if g == "aur":
        return "AUR" + (" (with %s)" % helper if helper else "")
    if g == "flatpak":
        r = c.get("remote") or ""
        return "Flatpak, from " + (r if ok_name(REMOTE, r) else "an unknown remote")
    if g in ("fnm", "nvm", "rustup"):
        return g + (", the default" if c.get("default") else "")
    if g == "cargo":
        return "cargo, from a git repository" if c.get("git") else "cargo, from crates.io"
    if g in ("appimage", "local", "deno"):
        return MANAGERS[g]["name"] + ", origin unknown"
    return MANAGERS[g]["name"]


def shown_command(steps):
    """The commands as Settings shows them: each step's argv, quoted as a
    shell would read it, joined with &&. (nvm's one runs nvm.sh in bash:
    shown as the nvm commands it runs)"""
    out = []
    for st in steps or []:
        if st[:2] == ["bash", "-c"] and len(st) == 6 and st[3] == "nvm":
            out.append(st[2].replace('. "$1/nvm.sh" && ', "").replace('"$2"', shlex.quote(st[5])))
        else:
            out.append(shlex.join(st))
    return " && ".join(out)


def flatpak_remotes_here():
    if not find_bin("flatpak"):
        return set()
    return set(parse_flatpak("", run(["flatpak", "remotes", "--columns=name,url,options"]))[1])


def aur_helper():
    return "paru" if find_bin("paru") else "yay" if find_bin("yay") else ""


# ── Which computer a backup came from ───────────────────────────────────
MACHINE_ID_FILES = ("/etc/machine-id", "/var/lib/dbus/machine-id")


def computer_tag():
    """This computer and user, as a tag a backup carries (no host name: a
    hash of the machine ID and the user's number), "" when there's no
    machine ID"""
    for p in MACHINE_ID_FILES:
        try:
            with open(p) as f:
                mid = f.read(200).strip()
        except OSError:
            continue
        if mid:
            return hashlib.sha256(("tessera-backup\0%s\0%d" % (mid, os.getuid())).encode()).hexdigest()[:32]
    return ""


def origin_of(manifest, ctx):
    """"here" (made on this computer, by you), "elsewhere" (another computer
    or another user), or "unknown" (made before backups said which, or no
    machine ID here). Anything but "here" is shown as "From another
    computer" with what would run, and nothing that runs goes ahead without
    a second yes (--confirm-elsewhere)"""
    tag, here = manifest.get("computer"), computer_tag()
    home = manifest.get("home")
    if isinstance(home, str) and home and home != ctx.home:
        return "elsewhere"
    if not isinstance(tag, str) or not tag or not here:
        return "unknown"
    return "here" if tag == here else "elsewhere"


def needs(a):
    """The manager an app needs that isn't here ("" when it's here), and the
    package that brings it"""
    g = a["group"]
    if g in ("pacman", "aur", "appimage", "local") or manager_here(g):
        return "", ""
    return (MANAGERS[g]["bin"] or g), MANAGERS[g]["package"]


def with_status(apps, ctx):
    """The backup's apps, each marked installed here, and what it needs"""
    here = {a["id"] for a in inventory_apps(ctx)["apps"]}
    out = []
    for a in apps:
        a = dict(a)
        a["installed"] = a.get("id") in here
        a["needs"], a["needsPackage"] = needs(a)
        out.append(a)
    return out


def inspect(path, ctx):
    manifest, members = unpack(path)
    parts = []
    contents = manifest.get("contents") if isinstance(manifest.get("contents"), dict) else {}
    have = set(manifest.get("parts") or [])
    for p in PARTS:
        if p not in have:
            continue
        c = contents.get(p) if isinstance(contents.get(p), dict) else {}
        if p == "settings":
            bits = [plural(int(c.get("settings") or 0), "setting")]
            if c.get("shortcuts"):
                bits.append(plural(int(c["shortcuts"]), "shortcut change"))
            if c.get("focusModes"):
                bits.append(plural(int(c["focusModes"]), "Focus mode"))
            n, summary = int(c.get("settings") or 0), ", ".join(bits)
        elif p == "hypr":
            own = [m for m in members if m.startswith("hypr/")]
            ed = [m for m in members if m.startswith("hypr-edited/")]
            n, summary = len(own) + len(ed), ", ".join(x for x in [plural(len(own), "file") if own else "", plural(len(ed), "edited Tessera file") if ed else ""] if x) or "No files"
        elif p == "wallpapers":
            pics = [m for m in members if m.startswith("wallpapers/")]
            n, summary = len(pics), plural(len(pics), "picture") if pics else "Tessera's own pictures (found by name)"
        elif p == "login":
            login = (json_member(members, "login.json", {}) or {}).get("lock") or {}
            n, summary = len(login), ("Style: " + str(login.get("loginStyle", "your look"))) if login else "As it came"
        else:
            apps = backup_apps(members)
            n, summary = len(apps), apps_summary(apps)
        parts.append({"id": p, "name": PART_NAMES[p], "count": n, "summary": summary,
                      "bytes": sum(len(v) for k, v in members.items() if member_part(k) == p)})
    origin = origin_of(manifest, ctx)
    out = {"created": manifest.get("created", ""), "parts": parts, "origin": origin, "elsewhere": origin != "here"}
    if "apps" in have:
        out["apps"] = with_status(backup_apps(members), ctx)
        # (which managers that computer had: only their names)
        inv = json_member(members, "apps.json", {})
        mgrs = inv.get("managers") if isinstance(inv, dict) and isinstance(inv.get("managers"), dict) else {}
        out["managers"] = {k: {"present": True} for k in mgrs if k in MANAGERS}
        # (everything an install could run, as it would run it)
        out["runs"] = [{"id": a["id"], "command": a["command"]} for a in out["apps"] if a["installable"] and a["command"]]
    return out


def member_part(name):
    if name.startswith("settings/"):
        return "settings"
    if name.startswith(("hypr/", "hypr-edited/")):
        return "hypr"
    if name.startswith("wallpapers/"):
        return "wallpapers"
    return {"login.json": "login", "apps.json": "apps"}.get(name, "")


# ── What a restore writes ───────────────────────────────────────────────
def short(v):
    t = json.dumps(v, ensure_ascii=False) if not isinstance(v, str) else v
    return t if len(t) <= 60 else t[:57] + "…"


def plan(path, include, ctx):
    """What restoring these parts would do, computed once: the writes
    {path: bytes}, the state keys to set, and what changes for people"""
    manifest, members = unpack(path)
    have = set(manifest.get("parts") or [])
    include = [p for p in PARTS if p in include and p in have and p != "apps"]
    old_home = manifest.get("home") if isinstance(manifest.get("home"), str) else ""
    home = ctx.home
    writes, state, report, skipped = {}, {}, {}, []
    config_path = os.path.join(ctx.config, "config.json")
    current = read_json(config_path, {})
    current = current if isinstance(current, dict) else {}
    config = copy.deepcopy(current)

    # Pictures: Tessera's own found in this install, carried ones placed here
    placed = {}
    pic_report = []
    pictures = manifest.get("wallpapers") if isinstance(manifest.get("wallpapers"), dict) else {}
    taken = {}
    for where, how in pictures.items():
        if not isinstance(how, dict):
            continue
        if isinstance(how.get("bundled"), str):
            target = SB.find_bundled(ctx.shell, os.path.basename(how["bundled"]))
            if target:
                placed[where] = target
        elif "wallpapers" in include and how.get("file") in members:
            member = how["file"]
            if member not in taken:
                data = members[member]
                name = member.split("/", 1)[1].split("-", 1)[-1]
                target = same_or_new(ctx.pictures, name, data)
                # (two pictures with one name: the second gets its own, not
                # the first's place in this restore; audit #71)
                base, ext = os.path.splitext(name)
                n = 2
                while target in writes and writes[target] != data:
                    target = SB.unique(ctx.pictures, "%s %d%s" % (base, n, ext))
                    n += 1
                taken[member] = target
                if not os.path.exists(target):
                    writes[target] = data
                    pic_report.append(target)
            placed[where] = taken[member]
    if "wallpapers" in include:
        report["wallpapers"] = {"files": [tilde(p, home) for p in pic_report], "count": len(pic_report)}

    if "settings" in include:
        restored = json_member(members, "settings/config.json", {})
        if not isinstance(restored, dict):
            raise Refused(NOT_BACKUP)
        config = SB.merge_local(SB.rehome(SB.scrub_config(restored), old_home, home), current)
        wp = config.get("wallpaper")
        if isinstance(wp, dict):
            for key in SB.WALLPAPER_KEYS:
                if "wallpaper." + key in placed:
                    wp[key] = placed["wallpaper." + key]
                elif wp.get(key) and not (os.path.isfile(wp[key]) or wp[key] in writes):
                    wp[key] = ""  # (Tessera's own picture then)
            if wp.get("dynamicFolder") and not os.path.isdir(wp["dynamicFolder"]):
                skipped.append("the time-of-day wallpaper folder")
                wp["dynamicFolder"] = ""
                if wp.get("dynamic") == "time":
                    wp["dynamic"] = "off"
            if wp.get("live") and not os.path.isfile(wp["live"]):
                skipped.append("the live wallpaper (its video isn't on this computer)")
                wp["live"] = ""
            if wp.get("directory") and not os.path.isdir(wp["directory"]):
                wp["directory"] = ""
        if "login" not in include:
            # (the login screen's look stays this computer's)
            lock = config.get("lock") if isinstance(config.get("lock"), dict) else None
            if lock is not None:
                for k in [k for k in lock if is_login_key(k)]:
                    del lock[k]
                lock.update(login_of(current))
        keybinds = json_member(members, "settings/keybinds.json")
        if isinstance(keybinds, dict):
            writes[os.path.join(ctx.config, "keybinds.json")] = dump(SB.rehome(keybinds, old_home, home))
        focus = json_member(members, "settings/focus.json")
        if focus is not None:
            modes = SB.focus_modes(SB.rehome(focus, old_home, home))
            for m in modes:
                key = "focus." + str(m.get("id"))
                if key in placed:
                    m["wallpaper"] = placed[key]
                elif m.get("wallpaper") and not os.path.isfile(m["wallpaper"]):
                    m["wallpaper"] = ""
            here = read_json(os.path.join(ctx.config, "focus.json"), {})
            here = here if isinstance(here, dict) else {}
            keep = here.get("active") in [m.get("id") for m in modes]
            writes[os.path.join(ctx.config, "focus.json")] = dump({
                "modes": modes, "active": here.get("active", "") if keep else "", "until": here.get("until", 0) if keep else 0,
                "source": here.get("source", "") if keep else "", "restore": here.get("restore", {}) if keep else {},
                "skipScheduleUntil": here.get("skipScheduleUntil", 0)})
        prefs = json_member(members, "settings/state.json", {})
        if isinstance(prefs, dict):
            state = {k: prefs[k] for k in STATE_KEYS if k in prefs}
    if "login" in include:
        login = (json_member(members, "login.json", {}) or {}).get("lock") or {}
        login = {k: v for k, v in login.items() if is_login_key(k)} if isinstance(login, dict) else {}
        if login:
            if not isinstance(config.get("lock"), dict):
                config["lock"] = {}
            config["lock"].update(login)
        report["login"] = {"changes": [{"key": "lock." + k, "now": short(login_of(current).get(k)), "then": short(v)}
                                       for k, v in login.items() if login_of(current).get(k) != v]}

    if config != current:
        writes[config_path] = json.dumps(config, indent=4, ensure_ascii=False).encode()
    if "settings" in include:
        before, after = leaves(current), leaves(config)
        changes = [{"key": k, "now": short(before.get(k)), "then": short(after.get(k))}
                   for k in sorted(set(before) | set(after)) if before.get(k) != after.get(k) and not k.startswith("lock.login")]
        kb_path = os.path.join(ctx.config, "keybinds.json")
        shortcuts_change = kb_path in writes and read_bytes(kb_path) != writes[kb_path]
        focus_path = os.path.join(ctx.config, "focus.json")
        report["settings"] = {"changes": changes[:200], "count": len(changes), "shortcuts": shortcuts_change,
                              "focus": focus_path in writes and read_bytes(focus_path) != writes[focus_path],
                              "state": sorted(k for k, v in state.items() if read_state().get(k) != v)}

    if "hypr" in include:
        files = []
        # (Tessera's own files and the generated ones are never replaced from
        # a backup: what it carries for them goes beside, to compare. Hyprland
        # reloads its config by itself, so a restored file runs at once;
        # audit #72)
        ships = shipped(ctx)
        for name, data in sorted(members.items()):
            if name.startswith("hypr/"):
                rel = name[len("hypr/"):]
                target = os.path.join(ctx.hypr, rel)
                if os.path.islink(target):
                    skipped.append("%s (it's a link: left as it is)" % tilde(target, home))
                    continue
                now = read_bytes(target)
                if now == data:
                    files.append({"path": tilde(target, home), "action": "same"})
                    continue
                # (Tessera's own: hyprland.lua and tessera/*, whether or not
                # this install has a checkout to compare with; audit #704)
                if rel in ships or tessera_owned(rel) or rel.endswith(HYPR_SKIP):
                    if not rel.endswith(HYPR_SKIP):
                        writes[target + BESIDE] = data
                        files.append({"path": tilde(target + BESIDE, home), "action": "beside"})
                    continue
                writes[target] = data
                files.append({"path": tilde(target, home), "action": "replace" if now is not None else "add"})
            elif name.startswith("hypr-edited/"):
                rel = name[len("hypr-edited/"):]
                target = os.path.join(ctx.hypr, rel)
                if os.path.islink(target + BESIDE):
                    skipped.append("%s (it's a link: left as it is)" % tilde(target + BESIDE, home))
                    continue
                if read_bytes(target) == data:
                    files.append({"path": tilde(target, home), "action": "same"})
                    continue
                writes[target + BESIDE] = data
                files.append({"path": tilde(target + BESIDE, home), "action": "beside"})
        report["hypr"] = {"files": files, "runs": any(f["action"] in ("replace", "add") for f in files),
                          "note": "Hyprland files run commands on this computer as soon as they're restored: "
                                  "restore them only from a backup you made yourself."}
    # (what's the same as now isn't written, nor kept for Undo)
    writes = {k: v for k, v in writes.items() if read_bytes(k) != v}
    now_state = read_state()
    state = {k: v for k, v in state.items() if now_state.get(k) != v}
    # (only where a restore may write: settings, hypr, the restored pictures)
    for w in writes:
        if not allowed(w, ctx):
            raise Refused(NOT_BACKUP)
    return {"include": include, "writes": writes, "state": state, "report": report, "skipped": skipped,
            "look": (config.get("appearance") or {}).get("look", "glass") if isinstance(config.get("appearance"), dict) else "glass"}


def tessera_owned(rel):
    """A ~/.config/hypr file that's Tessera's own (shipped or written by the
    installer): never replaced from a backup, only put beside"""
    return rel == "hyprland.lua" or rel.startswith("tessera/") or rel in GENERATED


def inside(path, top):
    """path is under top, where both really are: no link in between leads
    out (the deepest folder that's there is followed; the file itself is
    never a link)"""
    if os.path.islink(path):
        return False
    real_top, real = real_of(top), real_of(os.path.dirname(os.path.abspath(path)))
    return real == real_top or real.startswith(real_top + "/")


def real_of(path):
    """Where a path really is, folders not made yet included: the deepest
    one that's there followed through its links, the rest as written"""
    d, rest = os.path.abspath(path), []
    while not os.path.lexists(d) and d != os.path.dirname(d):
        d, tail = os.path.split(d)
        rest.insert(0, tail)
    return os.path.join(os.path.realpath(d), *rest)


def allowed(path, ctx):
    """Where a restore (and its undo) may write, and only there: the three
    settings files, a picture straight in ~/Pictures/Wallpapers/Restored,
    and your files in ~/.config/hypr (Tessera's own only as <file>.from-
    backup). Never another file in ~/.config/tessera, Tessera's install,
    the root helper, PAM or greetd; never through a link out (audit #704)"""
    p = os.path.abspath(path)
    if never(p, ctx.home):
        return False
    config, hypr, pictures = (os.path.abspath(d) for d in (ctx.config, ctx.hypr, ctx.pictures))
    if p in (os.path.join(config, n) for n in SETTINGS_FILES):
        return inside(p, config)
    if os.path.dirname(p) == pictures:
        return p.lower().endswith(PICTURE_EXT) and inside(p, pictures)
    if p.startswith(hypr + "/"):
        rel = os.path.relpath(p, hypr)
        if rel.endswith(BESIDE):
            return bool(MEMBER_REL.match(rel[:-len(BESIDE)])) and inside(p, hypr)
        if not MEMBER_REL.match(rel) or tessera_owned(rel) or rel in shipped(ctx) or rel.endswith(HYPR_SKIP):
            return False
        return inside(p, hypr)
    return False


def tilde(p, home):
    return "~" + p[len(home):] if home and (p == home or p.startswith(home + "/")) else p


def read_bytes(p):
    try:
        with open(p, "rb") as f:
            return f.read()
    except OSError:
        return None


def same_or_new(folder, name, data):
    """Where a carried picture goes: the same picture already restored
    there, else a new name"""
    if os.path.isdir(folder):
        want = hashlib.sha256(data).digest()
        for n in sorted(os.listdir(folder)):
            p = os.path.join(folder, n)
            try:
                if os.path.isfile(p) and os.path.getsize(p) == len(data) and hashlib.sha256(open(p, "rb").read()).digest() == want:
                    return p
            except OSError:
                continue
    return SB.unique(folder, name)


# ── state.json, under the shell's lock ──────────────────────────────────
def read_state():
    s = read_json(os.path.join(state_dir(), "state.json"), {})
    return s if isinstance(s, dict) else {}


def merge_state(values):
    """Keys set (None: taken out) in state.json, under state-merge.py's
    lock (.state.lock), so the shell's own merge keeps them"""
    if not values:
        return
    d = state_dir()
    os.makedirs(d, mode=0o700, exist_ok=True)
    with open(os.path.join(d, ".state.lock"), "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        state = read_state()
        for k, v in values.items():
            if v is None:
                state.pop(k, None)
            else:
                state[k] = v
        write_whole(os.path.join(d, "state.json"), json.dumps(state, ensure_ascii=False).encode(), 0o600)


# ── Restore and undo ────────────────────────────────────────────────────
def restore_plan(path, include, ctx):
    p = plan(path, include, ctx)
    return {"include": p["include"], "parts": p["report"], "skipped": p["skipped"], "writes": len(p["writes"]) + (1 if p["state"] else 0)}


def write_undo(p, ctx):
    """What the restore replaces, kept first: the files it overwrites, the
    ones it adds (taken out again), the state keys it sets"""
    backups = os.path.join(ctx.state, "backups")
    os.makedirs(backups, mode=0o700, exist_ok=True)
    members, replaced, created = {}, [], []
    for i, target in enumerate(sorted(p["writes"])):
        now = read_bytes(target)
        if now is None:
            created.append(target)
        else:
            name = "files/%d" % i
            members[name] = now
            replaced.append({"path": target, "member": name, "mode": SB.mode_of(target)})
    state = read_state()
    manifest = {"app": "Tessera", "kind": "undo", "format": FORMAT, "created": time.strftime("%Y-%m-%dT%H:%M:%S"),
                "replaced": replaced, "created_files": created, "state": {k: state.get(k) for k in p["state"]}}
    members["manifest.json"] = dump(manifest)
    path = SB.unique(backups, "before-restore-%s.tar.gz" % time.strftime("%Y%m%d-%H%M%S"))
    pack(path, members, {})
    return path


def apply_writes(writes, ctx):
    # (the settings last: the shell reloads them as they change)
    config = os.path.join(ctx.config, "config.json")
    for target in sorted(writes, key=lambda t: t == config):
        os.makedirs(os.path.dirname(target), exist_ok=True)
        write_whole(target, writes[target], SB.mode_of(target))


def restore(path, include, ctx, undo=True, confirmed=False):
    p = plan(path, include, ctx)
    # (Hyprland files run commands as soon as they're written: from another
    # computer only after Settings showed them and you said yes again)
    if p["report"].get("hypr", {}).get("runs") and not confirmed:
        manifest, _ = unpack(path)
        if origin_of(manifest, ctx) != "here":
            raise Refused(ELSEWHERE)
    if not p["writes"] and not p["state"]:
        return {"restored": p["include"], "skipped": p["skipped"], "undo": "", "look": p["look"], "changed": 0}
    undo_file = write_undo(p, ctx) if undo else ""
    try:
        apply_writes(p["writes"], ctx)
    except OSError as err:
        # (half restored: put back at once from what was kept; and if even
        # that fails, say where it is, so Undo can still be offered; audit #73)
        why = err.strerror or str(err)
        if undo_file:
            try:
                globals()["undo"](undo_file, ctx)
            except (OSError, Refused):
                raise Refused("Couldn't restore it (%s), and putting things back failed too. What you had is kept in %s."
                              % (why, tilde(undo_file, ctx.home)), undo=undo_file) from None
        raise Refused("Couldn't restore it (%s). Nothing was changed." % why) from None
    merge_state(p["state"])
    return {"restored": p["include"], "skipped": p["skipped"], "undo": undo_file, "look": p["look"], "changed": len(p["writes"])}


def undo(path, ctx):
    manifest, members = unpack(path, undo=True)
    for r in manifest.get("replaced") or []:
        target = r.get("path") if isinstance(r, dict) else None
        if not isinstance(target, str) or r.get("member") not in members or not allowed(target, ctx):
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        mode = r.get("mode") if isinstance(r.get("mode"), int) and 0 <= r["mode"] <= 0o777 else 0o600
        write_whole(target, members[r["member"]], mode)
    for target in manifest.get("created_files") or []:
        if isinstance(target, str) and allowed(target, ctx) and os.path.isfile(target) and not os.path.islink(target):
            os.unlink(target)
    st = manifest.get("state") if isinstance(manifest.get("state"), dict) else {}
    merge_state({k: v for k, v in st.items() if k in STATE_KEYS})
    config = read_json(os.path.join(ctx.config, "config.json"), {})
    ap = config.get("appearance") if isinstance(config, dict) and isinstance(config.get("appearance"), dict) else {}
    return {"undone": True, "look": ap.get("look", "glass")}


# ── Apps back ───────────────────────────────────────────────────────────
FLATHUB = "https://dl.flathub.org/repo/flathub.flatpakrepo"
# (Flathub's own remotes by name: each to its own .flatpakrepo, which carries
# its signing key; any other remote is added again only from a .flatpakrepo)
FLATHUB_REMOTES = {"flathub": FLATHUB, "flathub-beta": "https://dl.flathub.org/beta-repo/flathub-beta.flatpakrepo"}


def refused_why(a, remotes_here):
    """Why an app from a backup isn't installed from here ("" when it can
    be): only names that pass their source's pattern, and only from the
    allow-list of sources (Arch's repositories, the AUR, Flathub or a
    Flatpak remote already set up here, crates.io, PyPI, npm, the Node.js
    and Rust releases). A git or https address in a backup is never
    followed: anyone's backup could name any repository (audit #704)"""
    g, n, v = a.get("group"), a.get("name"), a.get("version") or ""
    if g not in MANAGERS:
        return "it's from an unknown source"
    if g in ("pacman", "aur"):
        return "" if package_name(n) else "its name isn't a package name"
    if g in ("appimage", "local") or (g == "deno"):
        return "where it came from isn't known"
    if g == "flatpak":
        if not ok_name(FLATPAK_ID, n):
            return "its name isn't a Flatpak app ID"
        remote = a.get("remote")
        if not ok_name(REMOTE, remote):
            return "its Flatpak remote has an odd name"
        if remote not in remotes_here and remote not in FLATHUB_REMOTES:
            return "it's from the Flatpak remote “%s”, which isn't Flathub: add that remote yourself if you trust it, then look again" % remote
        return ""
    if g in ("fnm", "nvm"):
        return "" if ok_name(NODE, v) else "its Node.js version isn't a version number"
    if g == "rustup":
        return "" if ok_name(TOOLCHAIN, n) else "its toolchain's name is odd"
    if g == "cargo":
        if a.get("git") or a.get("url"):
            return "it was built from a git repository; install it yourself if you trust it"
        return "" if ok_name(CRATE, n) else "its name isn't a crate name"
    if g == "pipx":
        spec = a.get("spec") or n
        if not ok_name(PYPKG, n) or not ok_name(PYPKG, spec) or pep503(spec) != pep503(n):
            return "it was installed from an address, not by name from PyPI; install it yourself if you trust it"
        return ""
    if g == "uv":
        return "" if ok_name(PYPKG, n) else "its name isn't a Python package name"
    if g in ("npm", "bun"):
        return "" if ok_name(NPM, n) else "its name isn't an npm package name"
    return "it can't be installed again from here"


def pep503(n):
    return re.sub(r"[-_.]+", "-", n).lower() if isinstance(n, str) else ""


def install_steps(a, remotes_here):
    """The commands that install an app again, built from its checked
    name, version and source (never a command from the file), or None.
    Settings shows exactly these (shown_command), so what's shown is what
    runs"""
    if refused_why(a, remotes_here):
        return None
    g, n, v = a.get("group"), a.get("name"), a.get("version") or ""
    if g == "flatpak":
        remote = a.get("remote")
        scope = "--user" if a.get("installation") == "user" else "--system"
        steps = []
        if remote not in remotes_here:
            # (Flathub's own .flatpakrepo, which carries its key: never an
            # address from the file)
            steps.append(["flatpak", "remote-add", "--if-not-exists", scope, remote, FLATHUB_REMOTES[remote]])
        steps.append(["flatpak", "install", "-y", "--noninteractive", scope, remote, n])
        return steps
    if g == "fnm":
        if not ok_name(NODE, v):
            return None
        return [["fnm", "install", v]] + ([["fnm", "default", v]] if a.get("default") is True else [])
    if g == "nvm":
        if not ok_name(NODE, v):
            return None
        script = '. "$1/nvm.sh" && nvm install "$2"' + (' && nvm alias default "$2"' if a.get("default") is True else "")
        return [["bash", "-c", script, "nvm", nvm_dir(), v]]
    if g == "rustup":
        if not ok_name(TOOLCHAIN, n):
            return None
        cmd = ["rustup", "toolchain", "install", n]
        comps = [c for c in a.get("components") or [] if ok_name(TOOLCHAIN, c)]
        targets = [t for t in a.get("targets") or [] if ok_name(TOOLCHAIN, t)]
        if comps:
            cmd += ["--component", ",".join(comps)]
        if targets:
            cmd += ["--target", ",".join(targets)]
        return [cmd] + ([["rustup", "default", n]] if a.get("default") is True else [])
    if g == "cargo":
        if not ok_name(CRATE, n):
            return None
        cmd = ["cargo", "install", "--locked"]
        feats = [f for f in a.get("features") or [] if ok_name(FEATURE, f)]
        if feats:
            cmd += ["--features", ",".join(feats)]
        if a.get("noDefault") is True:
            cmd.append("--no-default-features")
        return [cmd + [n]]
    if g == "pipx":
        return [["pipx", "install", n]]
    if g == "uv":
        return [["uv", "tool", "install", n]] if ok_name(PYPKG, n) else None
    if g == "npm":
        return [["npm", "install", "-g", n]] if ok_name(NPM, n) else None
    if g == "bun":
        return [["bun", "add", "-g", n]] if ok_name(NPM, n) else None
    return None


def say(obj):
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def run_step(argv):
    """(ok, the last lines it said) of one install command, as you"""
    exe = find_bin(argv[0])
    if not exe:
        return False, "%s isn't installed" % argv[0]
    try:
        r = subprocess.run([exe] + argv[1:], capture_output=True, text=True, timeout=ITEM_TIMEOUT, stdin=subprocess.DEVNULL,
                           env=dict(os.environ, LC_ALL="C", NO_COLOR="1"))
    except subprocess.TimeoutExpired:
        return False, "it took over %d minutes" % (ITEM_TIMEOUT // 60)
    except OSError as e:
        return False, str(e)
    tail = [line for line in (r.stderr + "\n" + r.stdout).splitlines() if line.strip()][-3:]
    return r.returncode == 0, " ".join(tail)[:300]


def apps_install(path, ids, ctx, confirmed=False):
    """The chosen user-level apps (and Flatpak's), one at a time: a line per
    step ({"progress": {id, state, why}}), then the results. Pacman and AUR
    ones are Software Update's job: left to it ("system"). A backup from
    another computer only with --confirm-elsewhere (Settings asks first,
    showing every command)"""
    manifest, members = unpack(path)
    if origin_of(manifest, ctx) != "here" and not confirmed:
        raise Refused(ELSEWHERE)
    remotes = flatpak_remotes_here()
    apps = {a["id"]: a for a in backup_apps(members, remotes)}
    results = {}
    for i in ids:
        a = apps.get(i)
        if a is None:
            continue
        if a["group"] in ("pacman", "aur"):
            results[i] = {"state": "system"}
            continue
        missing, _pkg = needs(a)
        steps = install_steps(a, remotes)
        if missing:
            results[i] = {"state": "skipped", "why": "needs " + missing}
        elif not steps:
            results[i] = {"state": "skipped", "why": a.get("why") or "it can't be installed again from here"}
        else:
            say({"progress": {"id": i, "state": "running", "done": len(results), "total": len(ids)}})
            ok, why = True, ""
            for step in steps:
                ok, why = run_step(step)
                if not ok:
                    break
            if ok and a["group"] == "flatpak":
                remotes.add(a.get("remote"))
            results[i] = {"state": "ok"} if ok else {"state": "failed", "why": why}
        say({"progress": dict(results[i], id=i, done=len(results), total=len(ids))})
    return {"results": results, "installed": sum(1 for r in results.values() if r["state"] == "ok"),
            "failed": sum(1 for r in results.values() if r["state"] == "failed")}


def apps_status(path, ctx):
    _, members = unpack(path)
    return {"apps": with_status(backup_apps(members), ctx)}


# ── Command line ────────────────────────────────────────────────────────
def parts_arg(v):
    return [p for p in (v or "").split(",") if p in PARTS]


def main(argv):
    ap = argparse.ArgumentParser(description="Back up and restore your Tessera setup")
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("inventory", "create", "inspect", "restore-plan", "restore", "undo", "apps-status", "apps-install"):
        c = sub.add_parser(name)
        if name in ("create", "inspect", "restore-plan", "restore", "undo", "apps-status", "apps-install"):
            c.add_argument("file")
        c.add_argument("--config")
        c.add_argument("--shell")
        c.add_argument("--json", action="store_true")
        c.add_argument("--include", default=",".join(PARTS))
        c.add_argument("--ids", default="")
        c.add_argument("--no-undo", action="store_true")
        c.add_argument("--no-apps", action="store_true")
        c.add_argument("--confirm-elsewhere", action="store_true")
    a = ap.parse_args(argv)
    try:
        if os.geteuid() == 0:
            raise Refused("Backups run as you, never as root. Open Settings as yourself, then try again.")
        ctx = Ctx(a.config, a.shell)
        if a.cmd == "inventory":
            out = inventory(ctx, with_apps=not a.no_apps)
        elif a.cmd == "create":
            out = create(a.file, parts_arg(a.include), ctx)
        elif a.cmd == "inspect":
            out = inspect(a.file, ctx)
        elif a.cmd == "restore-plan":
            out = restore_plan(a.file, parts_arg(a.include), ctx)
        elif a.cmd == "restore":
            out = restore(a.file, parts_arg(a.include), ctx, undo=not a.no_undo, confirmed=a.confirm_elsewhere)
        elif a.cmd == "undo":
            out = undo(a.file, ctx)
        elif a.cmd == "apps-status":
            out = apps_status(a.file, ctx)
        else:
            out = apps_install(a.file, [i for i in a.ids.split(",") if i], ctx, confirmed=a.confirm_elsewhere)
        out = dict(out)
        out.pop("ok", None)
        say(dict(ok=True, **out))
        return 0
    except Refused as err:
        say(dict({"ok": False, "error": str(err)}, **({"undo": err.undo} if getattr(err, "undo", "") else {})))
    except OSError as err:
        say({"ok": False, "error": "Couldn't %s: %s. Check there's free space and the folder is yours, then try again." % (
            {"create": "save the backup", "restore": "restore it", "undo": "put things back"}.get(a.cmd, "read the backup"), err.strerror or err)})
    except Exception as err:  # noqa: BLE001
        # (a damaged or odd file: an answer Settings can show, never a
        # traceback and "the backup helper didn't answer")
        say({"ok": False, "error": NOT_BACKUP if a.cmd not in ("inventory", "create") else
             "Something went wrong (%s). Try again; if it keeps happening, Settings → Health has the details." % type(err).__name__})
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
