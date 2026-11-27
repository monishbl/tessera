#!/usr/bin/env python3
"""Tessera as one signed bundle per version (docs/UPDATE-PLAN.md, steps 3-6;
docs/releases.md, "Bundles"): the whole of a version, built on the release
builder, downloaded whole, checked, unpacked into a folder of its own and
swapped in with one rename. Nothing is compiled on the computer it goes to.

    tessera-bundle.py mode       [--json]   bundle | git | dev (how this copy updates)
    tessera-bundle.py status     [--json]   the versions here, current and previous
    tessera-bundle.py plan       --commit C [--channel CH] [--json]
                                             is there a bundle for C, how big, which packages it moves
    tessera-bundle.py latest     [--channel CH] [--json]   the channel's newest bundle (signed pointer)
    tessera-bundle.py fetch      --commit C [--channel CH] [--json]   download, check, unpack (not current yet)
    tessera-bundle.py activate   ID [--json]   that version becomes the current one (the previous kept)
    tessera-bundle.py undo       [--json]   back to the previous version (again: forward)
    tessera-bundle.py rollback   [--json]   launch-shell.sh: undo, and the version undone marked bad
    tessera-bundle.py ensure     [--json]   the links outside versions/ point at current (repairs a cut-off swap)
    tessera-bundle.py verify     [ID] [--json]   every file of a version against its manifest
    tessera-bundle.py prune      [--json]   versions other than current, previous and the staged one go
    tessera-bundle.py lock-check FILE [--json]   is FILE a valid package lock (the root helper's rules)
    tessera-bundle.py moves      FILE [--json]   what putting the packages at FILE's versions would do
    tessera-bundle.py pack       …           (the release builder, and Build now) make a bundle from a checkout
    tessera-bundle.py build-here --source DIR --commit C [--channel CH] [--json]
                                             Build now: build this version here (gently), as an unsigned bundle

Where things are ($XDG_DATA_HOME/tessera, usually ~/.local/share/tessera):

    versions/<id>/       one version, unpacked: shell/, material/, hypr/, tools/,
                         bin/tessera-ui, bin/tessera-events, plugin/tessera-glass.so,
                         and its manifest (bundle.json, bundle.json.minisig)
    current -> versions/<id>     swapped in one rename (a new link renamed over it)
    previous -> versions/<id>    the one before, kept for undo
    bin/tessera-ui -> ../current/bin/tessera-ui   (and tessera-events)
    tessera-glass.so -> current/plugin/tessera-glass.so   (Hyprland loads this path)
    source               "…/tessera/current": where the tessera command and the shell look
    ~/.config/quickshell/tessera -> …/tessera/current/shell

So the whole version, its programs and its glass plugin change in the one
rename of `current`; undo is the same rename back. A running shell keeps the
files it has open (the old version's folder stays as `previous`).

Settings and everything else in your home are never part of a bundle. Your
Hyprland config's Tessera files are merged three ways as before
(tools/config-sync.py: yours kept on a clash, the new one beside it as .new).

A bundle (tessera-<commit>-<variant>.bundle.tar.zst, x86-64-v3 or generic)
comes with one manifest for both variants (tessera-<commit>.bundle.json):
the commit, channel, each variant's archive (size, sha256) and every file in
it (sha256, size, mode), and the package lock: the system packages it was
built and tested with. The manifest is signed with minisign (the trusted
comment names the file and commit=<commit>), checked against
tools/tessera-ui.pub with tools/native-install.py's checks. A file in the
archive the manifest doesn't list, a missing one, a link that leads out of
the folder, or any checksum that differs, and nothing is installed.

The package lock: "held" packages (the Hyprland family the glass plugin's ABI
depends on, and Quickshell: HELD_FAMILY) are put at exactly the tested
version (its pkgver; from the repositories when they have it, else the Arch
Linux Archive) and held there (pacman's IgnorePkg) until the next Tessera
version moves them; the rest are minimum versions only (a system update
brings them). The root helper (shell/assets/scripts/tessera-root.py update
--lock) does the root part, checking every name and version itself; this
file only reads and explains.

How a copy updates (mode):
  bundle  it runs from versions/: updates are bundles
  git     a normal install still on git (setup.sh's ~/.local/share/tessera/repo,
          its shell copied, no edits of its own): its next update moves it to
          a bundle; with no bundle for the new version it can still update
          through git, building here, as before
  dev     a developer's checkout (install.sh --dev, any other folder, edits or
          commits of its own, or update.mode "dev"): git, as before
"""
import argparse
import fcntl
import hashlib
import importlib.machinery
import importlib.util
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

TOOLS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(TOOLS)
FORMAT = 1
NAME = "tessera-bundle"
VARIANTS = ("x86-64-v3", "generic")
PROGRAMS = ("tessera-ui", "tessera-events")
# (in a bundle when it built; never a reason to refuse one. None now: the
# passkeys daemon was removed, and an older bundle's copy isn't unpacked)
OPTIONAL = ()
CHANNELS = ("stable", "beta", "release", "local")
ROLLING = {"stable": ["tessera-ui-latest", "tessera-ui-beta"], "beta": ["tessera-ui-beta", "tessera-ui-latest"]}
LATEST = "tessera-latest.bundle.json"  # the rolling release's newest bundle manifest (the pointer)
MAX_MANIFEST = 4 * 1024 * 1024
MAX_BUNDLE = 512 * 1024 * 1024
MAX_FILE = 256 * 1024 * 1024
MAX_FILES = 20000
KEEP_VERSIONS = 2  # current and previous (plus a staged one)

# Packages put at exactly the tested version and held there: the glass
# plugin is built against Hyprland's headers (hypr/plugin/tessera-glass/
# Makefile: pkg-config hyprland, which pulls in aquamarine, hyprutils,
# hyprlang, hyprgraphics and hyprcursor's), and loads only into the Hyprland
# it was built for; hyprwayland-scanner generates the protocol headers it
# compiles. Quickshell: the QML shell (the Rust shell's fallback) is tested
# against one version of it. Everything else in a lock is a minimum.
HELD_FAMILY = ("hyprland", "aquamarine", "hyprutils", "hyprlang", "hyprgraphics", "hyprcursor", "hyprwayland-scanner",
               "quickshell")
# Held packages a system may not have (Tessera's own Quickshell build, the
# Rust shell alone): left alone when they aren't installed
HELD_OPTIONAL = ("quickshell", "hyprwayland-scanner")
# The minimum versions a bundle records (the builder's own versions): what
# its programs, helpers and shaders are built against
MINIMUM_FAMILY = ("glibc", "gcc-libs", "qt6-base", "qt6-declarative", "qt6-wayland", "qt6-svg", "qt6-shadertools",
                  "qt6-multimedia", "qt6-imageformats", "pipewire", "wayland", "libxkbcommon", "mesa", "openssl",
                  "python", "python-cryptography")
PKG_NAME = re.compile(r"^[a-z0-9@_+][a-z0-9@._+-]*$")  # (never "-…": it would pass for a pacman option)
PKG_VERSION = re.compile(r"^(?:[0-9]+:)?[0-9A-Za-z._+~]+-[0-9]+(?:\.[0-9]+)?$")
PKG_ARCH = ("x86_64", "any")
ARCHIVE = "https://archive.archlinux.org/packages"
ARCHIVE_URL = re.compile(r"^https://archive\.archlinux\.org/packages/[a-z0-9]/[a-z0-9@._+-]+/[a-z0-9@._+:-]+\.pkg\.tar\.(zst|xz)$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
ID = re.compile(r"^[0-9a-f]{12}(-local)?$")
# What a bundle never carries from the checkout: the Rust sources (its
# programs are built), the website, the tests, git's own files
EXCLUDE_TOP = ("native/", "site/", "tests/", ".github/", ".gitignore", ".gitattributes", ".claude/")


class Failed(Exception):
    """Nothing changed; `why` says why. kind: missing (no bundle for this
    version yet), network, refused (a signature, checksum or file that isn't
    right), busy, bad (not something this can do)."""

    def __init__(self, why, kind="refused"):
        super().__init__(why)
        self.why = why
        self.kind = kind


def _load(name, file):
    path = os.path.join(TOOLS, file)
    loader = importlib.machinery.SourceFileLoader(name, path)
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


_NI = None


def ni():
    """tools/native-install.py: minisign, downloads, checksums, the processor,
    the release sources (loaded once, when first needed)"""
    global _NI
    if _NI is None:
        _NI = _load("tessera_native_install_for_bundles", "native-install.py")
    return _NI


# ── Where things are ─────────────────────────────────────────────────────

def _xdg(var, default):
    return os.environ.get(var) or os.path.expanduser(default)


def data_dir():
    return os.environ.get("TESSERA_BUNDLE_HOME") or os.path.join(_xdg("XDG_DATA_HOME", "~/.local/share"), "tessera")


def config_home():
    return _xdg("XDG_CONFIG_HOME", "~/.config")


def state_dir():
    return os.path.join(_xdg("XDG_STATE_HOME", "~/.local/state"), "tessera")


def versions_dir(data=None):
    return os.path.join(data or data_dir(), "versions")


def current_link(data=None):
    return os.path.join(data or data_dir(), "current")


def previous_link(data=None):
    return os.path.join(data or data_dir(), "previous")


def version_dir(vid, data=None):
    if not ID.match(vid or ""):
        raise Failed(f"not a version: {vid!r}", "bad")
    return os.path.join(versions_dir(data), vid)


def read_link(link):
    """The version a link (current, previous) points at, or None"""
    try:
        target = os.readlink(link)
    except OSError:
        return None
    vid = os.path.basename(target.rstrip("/"))
    return vid if ID.match(vid) and os.path.dirname(target.rstrip("/")) in ("versions", os.path.join(os.path.dirname(link), "versions")) else None


def manifest_name(commit):
    return f"tessera-{commit}.bundle.json"


def tarball_name(commit, variant):
    return f"tessera-{commit}-{variant}.bundle.tar.zst"


def version_id(commit, signed=True):
    return commit[:12] + ("" if signed else "-local")


def load_json(path, fallback=None):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return fallback


def write_json(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f"{path}.tmp-{secrets.token_hex(4)}"
    with open(tmp, "w") as f:
        json.dump(obj, f, indent=1)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def write_text(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f"{path}.tmp-{secrets.token_hex(4)}"
    with open(tmp, "w") as f:
        f.write(text)
    os.replace(tmp, path)


def _read_bytes(path):
    try:
        with open(path, "rb") as f:
            return f.read()
    except OSError:
        return None


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


class Lock:
    """One change to the versions at a time (fetch, activate, undo, prune)"""

    def __init__(self, data=None):
        d = data or data_dir()
        os.makedirs(d, exist_ok=True)
        self.f = open(os.path.join(d, ".bundle.lock"), "w")

    def __enter__(self):
        try:
            fcntl.flock(self.f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.f.close()
            raise Failed("another change to Tessera's versions is running", "busy") from None
        return self

    def __exit__(self, *exc):
        self.f.close()


# ── Package versions (pacman's vercmp, and the lock's rules) ─────────────

def _rpmvercmp(a, b):
    """libalpm's rpmvercmp, exactly: -1, 0, 1"""
    if a == b:
        return 0
    i = j = 0
    la, lb = len(a), len(b)
    while i < la and j < lb:
        si, sj = i, j
        while i < la and not a[i].isalnum():
            i += 1
        while j < lb and not b[j].isalnum():
            j += 1
        if i >= la or j >= lb:
            break
        # (more separators before a segment: newer)
        if (i - si) != (j - sj):
            return -1 if (i - si) < (j - sj) else 1
        p1, p2 = i, j
        if a[p1].isdigit():
            while p1 < la and a[p1].isdigit():
                p1 += 1
            while p2 < lb and b[p2].isdigit():
                p2 += 1
            isnum = True
        else:
            while p1 < la and a[p1].isalpha():
                p1 += 1
            while p2 < lb and b[p2].isalpha():
                p2 += 1
            isnum = False
        s1, s2 = a[i:p1], b[j:p2]
        if not s2:
            return 1 if isnum else -1
        if isnum:
            s1, s2 = s1.lstrip("0"), s2.lstrip("0")
            if len(s1) != len(s2):
                return 1 if len(s1) > len(s2) else -1
        if s1 != s2:
            return -1 if s1 < s2 else 1
        i, j = p1, p2
    if i >= la and j >= lb:
        return 0
    if (i >= la and not (j < lb and b[j].isalpha())) or (i < la and a[i].isalpha()):
        return -1
    return 1


def _evr(v):
    epoch, _, rest = v.partition(":") if ":" in v else ("0", "", v)
    if not epoch.isdigit():
        epoch, rest = "0", v
    ver, sep, rel = rest.rpartition("-")
    if not sep:
        ver, rel = rest, None
    return epoch or "0", ver, rel


def vercmp(a, b):
    """pacman's vercmp: epoch, then version, then release (when both have one)"""
    if a == b:
        return 0
    ea, va, ra = _evr(a)
    eb, vb, rb = _evr(b)
    r = _rpmvercmp(ea, eb) or _rpmvercmp(va, vb)
    if r == 0 and ra is not None and rb is not None:
        r = _rpmvercmp(ra, rb)
    return r


def upstream(v):
    """epoch:pkgver without the package release ("1:0.57.1-2" -> "1:0.57.1")"""
    e, ver, _ = _evr(v)
    return (e + ":" if e != "0" else "") + ver


def check_name(n):
    if not isinstance(n, str) or not PKG_NAME.match(n) or len(n) > 128:
        raise Failed(f"not a package name: {n!r}")
    return n


def check_version(v):
    if not isinstance(v, str) or not PKG_VERSION.match(v) or len(v) > 128:
        raise Failed(f"not a package version: {v!r}")
    return v


def check_lock(lock):
    """A package lock, strictly: {"held": {name: {"version", "arch"}},
    "minimum": {name: version}}. Held packages only from HELD_FAMILY. Returns
    it normalised; raises Failed."""
    if not isinstance(lock, dict) or set(lock) - {"held", "minimum", "built_on", "_"}:
        raise Failed("the package lock isn't one")
    held, minimum = lock.get("held", {}), lock.get("minimum", {})
    if not isinstance(held, dict) or not isinstance(minimum, dict) or len(held) > 32 or len(minimum) > 512:
        raise Failed("the package lock isn't one")
    out = {"held": {}, "minimum": {}}
    for n, e in sorted(held.items()):
        check_name(n)
        if n not in HELD_FAMILY:
            raise Failed(f"{n} isn't a package Tessera holds")
        if not isinstance(e, dict):
            raise Failed(f"{n}: not a held version")
        arch = e.get("arch", "x86_64")
        if arch not in PKG_ARCH:
            raise Failed(f"{n}: not an architecture: {arch!r}")
        out["held"][n] = {"version": check_version(e.get("version")), "arch": arch}
    for n, v in sorted(minimum.items()):
        check_name(n)
        if n in out["held"]:
            raise Failed(f"{n} is both held and a minimum")
        out["minimum"][n] = check_version(v)
    return out


def archive_url(name, version, arch):
    """The Arch Linux Archive's file for exactly that package, built here from
    checked parts only (never a URL from anywhere else)"""
    check_name(name)
    check_version(version)
    if arch not in PKG_ARCH:
        raise Failed(f"not an architecture: {arch!r}")
    url = f"{ARCHIVE}/{name[0]}/{name}/{name}-{version}-{arch}.pkg.tar.zst"
    if not ARCHIVE_URL.match(url):
        raise Failed(f"no archive address for {name} {version}")
    return url


def installed_packages(names, run=subprocess.run):
    """{name: version} of those installed (pacman -Q)"""
    names = [check_name(n) for n in names]
    if not names:
        return {}
    try:
        r = run(["pacman", "-Q", "--", *names], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.SubprocessError):
        return {}
    out = {}
    for line in (r.stdout or "").splitlines():
        p = line.split()
        if len(p) == 2 and p[0] in names:
            out[p[0]] = p[1]
    return out


def repo_versions(names, run=subprocess.run):
    """{name: version} the repositories have now (pacman -Si: the package
    lists as last downloaded; never refreshed here)"""
    names = [check_name(n) for n in names]
    if not names:
        return {}
    try:
        r = run(["pacman", "-Si", "--", *names], capture_output=True, text=True, timeout=30, env=dict(os.environ, LANG="C", LC_ALL="C"))
    except (OSError, subprocess.SubprocessError):
        return {}
    out, name = {}, None
    for line in (r.stdout or "").splitlines():
        k, _, v = line.partition(":")
        k, v = k.strip(), v.strip()
        if k == "Name":
            name = v
        elif k == "Version" and name in names and name not in out:
            out[name] = v
    return out


def package_moves(lock, installed, repo=None):
    """What putting the packages at LOCK's versions does, given what's
    INSTALLED ({name: version}) and what the repositories have (REPO):
    {"held": [{"name", "from", "to", "how", "url"}], "minimum": [{"name",
    "from", "need"}], "system": bool (a system update is needed first),
    "hold": [names to hold]}. A held package is right when its pkgver is
    the tested one (its package release may differ: a rebuild)."""
    lock = check_lock(lock)
    repo = repo or {}
    out = {"held": [], "minimum": [], "system": False, "hold": []}
    for n, e in lock["held"].items():
        have, want = installed.get(n), e["version"]
        if have is None and n in HELD_OPTIONAL:
            continue
        out["hold"].append(n)
        if have is not None and vercmp(upstream(have), upstream(want)) == 0:
            continue
        if repo.get(n) == want:
            out["held"].append({"name": n, "from": have or "", "to": want, "how": "repo", "url": ""})
        else:
            out["held"].append({"name": n, "from": have or "", "to": want, "how": "archive", "url": archive_url(n, want, e["arch"])})
    for n, need in lock["minimum"].items():
        have = installed.get(n)
        if have is None:
            continue  # (not one this computer uses)
        if vercmp(have, need) < 0:
            out["minimum"].append({"name": n, "from": have, "need": need})
    out["system"] = bool(out["minimum"])
    return out


def moves_words(m):
    """One line for Software Update: "Hyprland 0.57.1 → 0.57.2 (held)"…"""
    parts = [f"{x['name']} {upstream(x['from']) if x['from'] else '(new)'} → {upstream(x['to'])}" for x in m.get("held", [])]
    if m.get("minimum"):
        parts.append(f"{len(m['minimum'])} package{'s' if len(m['minimum']) != 1 else ''} updated first")
    return ", ".join(parts)


# ── The manifest and its signature ───────────────────────────────────────

def check_path(rel):
    """A path inside a bundle: relative, no "..", no empty part, no .git"""
    if not isinstance(rel, str) or not rel or len(rel) > 400 or "\0" in rel or "\\" in rel:
        raise Failed(f"a bundle holds a path that isn't allowed: {rel!r}")
    parts = rel.split("/")
    if rel.startswith("/") or any(p in ("", ".", "..") for p in parts) or ".git" in parts:
        raise Failed(f"a bundle holds a path that isn't allowed: {rel!r}")
    return rel


def link_ok(rel, target):
    """A link inside a bundle: relative, and it stays inside it"""
    if not isinstance(target, str) or not target or target.startswith("/") or "\0" in target:
        return False
    joined = os.path.normpath(os.path.join(os.path.dirname(rel), target))
    return not (joined == ".." or joined.startswith("../") or joined.startswith("/"))


def check_entry(variant, commit, e):
    if not isinstance(e, dict) or e.get("file") != tarball_name(commit, variant):
        raise Failed(f"the manifest's {variant} bundle isn't named for this version")
    if not isinstance(e.get("size"), int) or not 0 < e["size"] <= MAX_BUNDLE or not SHA256.match(str(e.get("sha256", ""))):
        raise Failed(f"the manifest's {variant} bundle has no size or checksum")
    files, links = e.get("files"), e.get("links") or {}
    if not isinstance(files, dict) or not files or len(files) > MAX_FILES or not isinstance(links, dict):
        raise Failed(f"the manifest doesn't list the {variant} bundle's files")
    for rel, f in files.items():
        check_path(rel)
        if not isinstance(f, dict) or not SHA256.match(str(f.get("sha256", ""))) or not isinstance(f.get("size"), int) \
                or not 0 <= f["size"] <= MAX_FILE or f.get("mode") not in (0o644, 0o755):
            raise Failed(f"the manifest's entry for {rel} isn't one")
    for rel, target in links.items():
        check_path(rel)
        if rel in files or not link_ok(rel, target) or any(f.startswith(rel + "/") for f in files):
            raise Failed(f"the bundle's link {rel} leads out of it")
    for name in PROGRAMS:
        if "bin/" + name not in files:
            raise Failed(f"the {variant} bundle has no {name}")
    if "shell/shell.qml" not in files:
        raise Failed(f"the {variant} bundle has no shell")
    return e


def check_manifest(m, commit=None):
    """A bundle manifest, strictly (raises Failed). Returns it."""
    if not isinstance(m, dict) or m.get("format") != FORMAT or m.get("name") != NAME:
        raise Failed("the signed manifest isn't a Tessera bundle's")
    c = m.get("commit")
    if not isinstance(c, str) or not COMMIT.match(c) or (commit and c != commit):
        raise Failed("the signed manifest is for another version")
    if m.get("channel") not in CHANNELS:
        raise Failed("the signed manifest names no channel")
    variants = m.get("variants")
    if not isinstance(variants, dict) or not variants or set(variants) - set(VARIANTS):
        raise Failed("the signed manifest has no bundles")
    for v, e in variants.items():
        check_entry(v, c, e)
    m["packages"] = check_lock(m.get("packages") or {})
    return m


def verify_manifest(data, sig_text, pubkey, commit=None, names=None):
    """DATA (a manifest's bytes) and its minisign signature, against PUBKEY:
    the signature, the signed comment (the file's name and commit=), then
    the manifest itself. Returns the manifest."""
    if len(data) > MAX_MANIFEST:
        raise Failed("the manifest is larger than any should be")
    trusted = ni().verify_minisign(pubkey, data, sig_text)
    try:
        m = json.loads(data)
    except ValueError:
        raise Failed("the signed manifest isn't JSON") from None
    c = m.get("commit") if isinstance(m, dict) else None
    words = trusted.split()
    allowed = set(names or ()) | ({manifest_name(c), LATEST} if isinstance(c, str) else set())
    if not words or words[0] not in allowed or f"commit={c}" not in words:
        raise Failed("the signature is for another file or version")
    if commit and c != commit:
        raise Failed("the signed manifest is for another version")
    return check_manifest(m, commit)


def pubkey_text():
    try:
        with open(os.environ.get("TESSERA_UI_PUBKEY") or os.path.join(REPO, "tools", "tessera-ui.pub")) as f:
            return f.read()
    except OSError:
        return ""


# ── Unpacking, checked ───────────────────────────────────────────────────

def unpack(tarball, into, entry):
    """The bundle TARBALL into INTO (a new, empty folder): exactly the files
    and links ENTRY lists, nothing else; files written with their listed
    mode, each one's sha256 checked; links made last, each inside the
    bundle. Raises Failed (INTO is then left for the caller to remove)."""
    files, links = entry["files"], entry.get("links") or {}
    plain = os.path.join(into, ".bundle.tar")
    r = subprocess.run(["zstd", "-q", "-d", "-f", "-o", plain, tarball], capture_output=True, text=True, timeout=900)
    if r.returncode != 0:
        raise Failed("the download isn't a zstd archive")
    seen, made_links = set(), {}
    try:
        with tarfile.open(plain, "r:") as tar:
            for m in tar:
                rel = m.name[2:] if m.name.startswith("./") else m.name
                rel = rel.rstrip("/")
                if not rel or rel == ".":
                    continue
                check_path(rel)
                if m.isdir():
                    continue
                if m.issym():
                    if links.get(rel) != m.linkname:
                        raise Failed(f"the bundle has a link its manifest doesn't list ({rel})")
                    made_links[rel] = m.linkname
                    continue
                if not m.isfile():
                    raise Failed(f"the bundle holds something that isn't a file ({rel})")
                f = files.get(rel)
                if f is None:
                    raise Failed(f"the bundle has a file its manifest doesn't list ({rel})")
                if rel in seen:
                    raise Failed(f"the bundle has {rel} twice")
                if m.size != f["size"]:
                    raise Failed(f"{rel} in the bundle isn't the size its manifest says")
                seen.add(rel)
                path = os.path.join(into, rel)
                parent = os.path.dirname(path)
                os.makedirs(parent, exist_ok=True)
                if os.path.realpath(parent) != os.path.normpath(os.path.join(os.path.realpath(into), os.path.dirname(rel))):
                    raise Failed(f"the bundle leads {rel} out of its folder")
                src = tar.extractfile(m)
                h = hashlib.sha256()
                with open(path, "xb") as out:
                    for block in iter(lambda: src.read(1 << 20), b""):
                        h.update(block)
                        out.write(block)
                os.chmod(path, f["mode"])
                if h.hexdigest() != f["sha256"]:
                    raise Failed(f"{rel} in the bundle doesn't match its signed checksum")
    except tarfile.TarError as e:
        raise Failed("the download isn't a tar archive (" + str(e) + ")") from None
    finally:
        try:
            os.remove(plain)
        except OSError:
            pass
    missing = sorted(set(files) - seen) + sorted(set(links) - set(made_links))
    if missing:
        raise Failed(f"the bundle is missing {missing[0]}" + (f" and {len(missing) - 1} more" if len(missing) > 1 else ""))
    for rel, target in sorted(made_links.items()):
        path = os.path.join(into, rel)
        if not link_ok(rel, target) or os.path.lexists(path):
            raise Failed(f"the bundle's link {rel} leads out of it")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        os.symlink(target, path)


def verify_tree(vdir, entry=None):
    """Every file of an unpacked version against its manifest (bundle.json).
    Returns [] when all are right, else what's wrong (at most 20)."""
    if entry is None:
        info = load_json(os.path.join(vdir, "bundle-info.json"), {}) or {}
        m = load_json(os.path.join(vdir, "bundle.json"), {}) or {}
        entry = ((m.get("variants") or {}).get(info.get("variant")) or {})
    bad = []
    for rel, f in sorted((entry.get("files") or {}).items()):
        p = os.path.join(vdir, rel)
        try:
            if os.path.islink(p) or os.path.getsize(p) != f["size"] or sha256_of(p) != f["sha256"]:
                bad.append(rel)
        except OSError:
            bad.append(rel)
        if len(bad) >= 20:
            break
    for rel, target in sorted((entry.get("links") or {}).items()):
        try:
            if os.readlink(os.path.join(vdir, rel)) != target:
                bad.append(rel)
        except OSError:
            bad.append(rel)
    return bad


def install_version(manifest_bytes, sig_text, tarball, variant, data=None, signed=True):
    """The checked bundle TARBALL unpacked as versions/<id> (not current
    yet). MANIFEST_BYTES must already be verified (verify_manifest), or be a
    bundle built here (signed=False). Returns the id."""
    m = check_manifest(json.loads(manifest_bytes))
    entry = m["variants"].get(variant)
    if entry is None:
        raise Failed(f"the bundle has no {variant} build", "missing")
    if os.path.getsize(tarball) != entry["size"] or sha256_of(tarball) != entry["sha256"]:
        raise Failed("the download doesn't match its signed checksum (damaged, or not what was published)")
    vid = version_id(m["commit"], signed)
    data = data or data_dir()
    dest = version_dir(vid, data)
    with Lock(data):
        if os.path.isdir(dest) and _read_bytes(os.path.join(dest, "bundle.json")) == manifest_bytes and not verify_tree(dest, entry):
            return vid  # (already here, whole)
        os.makedirs(versions_dir(data), exist_ok=True)
        tmp = tempfile.mkdtemp(dir=versions_dir(data), prefix=f".incoming-{vid}-")
        try:
            unpack(tarball, tmp, entry)
            with open(os.path.join(tmp, "bundle.json"), "wb") as f:
                f.write(manifest_bytes)
            if sig_text:
                write_text(os.path.join(tmp, "bundle.json.minisig"), sig_text)
            write_json(os.path.join(tmp, "bundle-info.json"), {"id": vid, "commit": m["commit"], "channel": m["channel"],
                                                               "variant": variant, "signed": bool(signed), "at": int(time.time())})
            for name in PROGRAMS + tuple(n for n in OPTIONAL if "bin/" + n in entry["files"]):
                # (what each program is, for native-install.py status and the login screen's check)
                rec = {"source": "bundle" if signed else "bundle-built", "variant": variant, "commit": m["commit"], "id": vid,
                       "tree": m.get("tree", ""), "sha256": entry["files"]["bin/" + name]["sha256"]}
                said = (entry.get("programs") or {}).get(name, {}).get("version")
                if said:
                    rec["build"] = said
                if signed:
                    # (verify_manifest checked it before this; Settings says
                    # "signed" only with this; audit #693)
                    rec["signature"] = "verified"
                if signed and name == "tessera-ui":
                    rec["manifest"] = manifest_bytes.decode()
                    rec["minisig"] = sig_text
                write_json(os.path.join(tmp, "bin", name + ".json"), rec)
            if os.path.lexists(dest):
                if vid in (read_link(current_link(data)), read_link(previous_link(data))):
                    # (the one in use, damaged: the good copy goes in beside it, then over it)
                    old = dest + ".old-" + secrets.token_hex(4)
                    os.replace(dest, old)
                    os.replace(tmp, dest)
                    shutil.rmtree(old, ignore_errors=True)
                    return vid
                shutil.rmtree(dest, ignore_errors=True)
            os.replace(tmp, dest)
        except BaseException:
            shutil.rmtree(tmp, ignore_errors=True)
            raise
    return vid


# ── Downloading ──────────────────────────────────────────────────────────

def bases(repo=None):
    """Where the bundles are, each a URL with {tag} in it (native-install.py's
    releases_bases: GitHub's releases, then the GitLab mirror's packages).
    TESSERA_BUNDLES sets the list (tests: file:// folders)."""
    if os.environ.get("TESSERA_BUNDLES", "").strip():
        return os.environ["TESSERA_BUNDLES"].split()
    origin = repo or REPO
    if os.path.isdir(os.path.join(origin, ".git")):
        return ni().releases_bases(origin)
    src = ni().SOURCES
    return [b for b in (src.release_base(u) for u in src.git_sources()) if b]


def tags_for(channel):
    return ROLLING.get(channel, ROLLING["stable"])


def fetch_manifest(commit, channel, work, *, base_list=None, pubkey=None, download=None):
    """The signed manifest for COMMIT into WORK (manifest.json and its
    .minisig), from the channel's rolling release first: (manifest, bytes,
    sig, url of its folder). Raises Failed(kind=missing) when no source has it."""
    download = download or ni().download
    pubkey = pubkey if pubkey is not None else pubkey_text()
    if not pubkey:
        raise Failed("this copy of Tessera has no release key (tools/tessera-ui.pub)", "missing")
    os.makedirs(work, exist_ok=True)
    mpath = os.path.join(work, "manifest.json")
    errors = []
    # (the rolling pointer only from the channel's own release: Stable's
    # missing for a moment, as every publish replaces it, gave Beta's;
    # audit #83. A commit's manifest may sit in either)
    tags = tags_for(channel) if commit else tags_for(channel)[:1]
    for b in (base_list if base_list is not None else bases()):
        for tag in tags:
            folder = b.replace("{tag}", tag)
            try:
                download(folder + "/" + (manifest_name(commit) if commit else LATEST), mpath, MAX_MANIFEST, timeout=60, retries=1, connect=5)
                download(folder + "/" + (manifest_name(commit) if commit else LATEST) + ".minisig", mpath + ".minisig", 64 * 1024, timeout=60, retries=1, connect=5)
            except ni().Failed as e:
                errors.append(Failed(e.why, e.kind))
                if e.kind == "missing":
                    continue
                break  # (this source doesn't answer: the next one)
            with open(mpath, "rb") as f:
                data = f.read()
            with open(mpath + ".minisig") as f:
                sig = f.read()
            try:
                m = verify_manifest(data, sig, pubkey, commit)
            except ni().Failed as e:
                raise Failed(e.why) from None
            return m, data, sig, folder
    refused = [e for e in errors if e.kind == "refused"]
    if refused:
        raise refused[0]
    if errors and all(e.kind == "missing" for e in errors):
        raise Failed("there's no download for this version yet", "missing")
    raise errors[-1] if errors else Failed("there's nowhere to download Tessera from", "missing")


def download_with_progress(url, path, size, say=None, interval=0.5):
    """curl into PATH, saying how far it is (say(bytes, size)) while it
    runs; resumes a cut-off one. Raises Failed."""
    local = url.startswith("file://")
    if not local and not url.startswith("https://"):
        raise Failed("only https downloads are allowed", "network")
    if os.path.exists(path) and os.path.getsize(path) > size:
        os.remove(path)
    cmd = ["curl", "--fail", "--silent", "--show-error", "--location", "--proto", "=file" if local else "=https",
           "--proto-redir", "=https", "--connect-timeout", "10", "--retry", "3", "--retry-delay", "2",
           "--max-filesize", str(MAX_BUNDLE), "--user-agent", "Tessera", "--output", path, url]
    if not local:
        cmd[1:1] = ["--continue-at", "-", "--retry-all-errors", "--speed-limit", "1024", "--speed-time", "60"]
    if os.path.exists(path) and os.path.getsize(path) == size:
        return
    errf = tempfile.TemporaryFile(mode="w+")
    try:
        p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=errf, text=True)
    except FileNotFoundError:
        errf.close()
        raise Failed("curl isn't installed", "network") from None
    while p.poll() is None:
        if say:
            try:
                say(os.path.getsize(path), size)
            except OSError:
                pass
        time.sleep(interval)
    errf.seek(0)
    err = errf.read()
    errf.close()
    if p.returncode != 0:
        if "404" in err or "410" in err or (local and p.returncode == 37):
            raise Failed("there's no download for this version yet", "missing")
        raise Failed("it couldn't be downloaded (" + (err.strip().splitlines() or ["curl " + str(p.returncode)])[-1].replace("curl: ", "") + ")", "network")
    if say:
        say(os.path.getsize(path), size)


def detect_variant():
    v = ni().detect_cpu()
    if v is None:
        raise Failed("there are no bundles for this processor", "missing")
    return v


def fetch(commit, channel, *, data=None, variant=None, say=None, base_list=None, pubkey=None):
    """The bundle for COMMIT downloaded, checked and unpacked as a version
    (not current yet). say(text, bytes, size) as it goes. Returns {"id",
    "manifest", "variant", "size"}; raises Failed."""
    data = data or data_dir()
    variant = variant or detect_variant()
    work = os.path.join(data, ".downloads")
    os.makedirs(work, exist_ok=True)
    m, mbytes, sig, folder = fetch_manifest(commit, channel, os.path.join(work, "m-" + commit[:12]), base_list=base_list, pubkey=pubkey)
    if variant not in m["variants"] and "generic" in m["variants"]:
        variant = "generic"  # (a tuned one missing: the generic one runs anywhere)
    entry = m["variants"].get(variant)
    if entry is None:
        raise Failed("there's no bundle of this version for this processor", "missing")
    vid = version_id(commit)
    dest = version_dir(vid, data)
    if os.path.isdir(dest) and _read_bytes(os.path.join(dest, "bundle.json")) == mbytes and not verify_tree(dest, entry):
        return {"id": vid, "manifest": m, "variant": variant, "size": 0, "kept": True}
    tpath = os.path.join(work, entry["file"])
    # (only this version's download is kept going)
    for old in os.listdir(work):
        if old.endswith(".bundle.tar.zst") and old != entry["file"]:
            try:
                os.remove(os.path.join(work, old))
            except OSError:
                pass
    if say:
        say("Downloading Tessera", 0, entry["size"])
    download_with_progress(folder + "/" + entry["file"], tpath, entry["size"],
                           (lambda got, size: say("Downloading Tessera", got, size)) if say else None)
    if os.path.getsize(tpath) != entry["size"] or sha256_of(tpath) != entry["sha256"]:
        os.remove(tpath)
        raise Failed("the download doesn't match its signed checksum (damaged, or not what was published)")
    if say:
        say("Checking every file", entry["size"], entry["size"])
    vid = install_version(mbytes, sig, tpath, variant, data)
    try:
        os.remove(tpath)
    except OSError:
        pass
    shutil.rmtree(os.path.join(work, "m-" + commit[:12]), ignore_errors=True)
    return {"id": vid, "manifest": m, "variant": variant, "size": entry["size"]}


def plan(commit, channel, *, base_list=None, pubkey=None, installed=None, repo=None, variant=None):
    """Software Update's check: is there a bundle for COMMIT ({"available":
    True, "size", "variant", "moves"}), or not yet ({"available": False,
    "why"}), or can't tell ({"available": None, "why"})."""
    with tempfile.TemporaryDirectory(prefix="tessera-bundle-plan-") as work:
        try:
            m, _, _, _ = fetch_manifest(commit, channel, work, base_list=base_list, pubkey=pubkey)
        except Failed as e:
            return {"available": False if e.kind == "missing" else None, "why": e.why, "kind": e.kind}
    try:
        variant = variant or detect_variant()
    except Failed as e:
        return {"available": False, "why": e.why, "kind": e.kind}
    if variant not in m["variants"] and "generic" in m["variants"]:
        variant = "generic"
    entry = m["variants"].get(variant)
    if entry is None:
        return {"available": False, "why": "there's no bundle of this version for this processor", "kind": "missing"}
    lock = m["packages"]
    names = list(lock["held"]) + list(lock["minimum"])
    have = installed if installed is not None else installed_packages(names)
    in_repo = repo if repo is not None else repo_versions(list(lock["held"]))
    moves = package_moves(lock, have, in_repo)
    return {"available": True, "size": entry["size"], "variant": variant, "commit": m["commit"], "channel": m["channel"],
            "moves": moves, "movesText": moves_words(moves), "date": m.get("date", "")}


def latest(channel, *, base_list=None, pubkey=None):
    """The channel's newest bundle manifest (the signed pointer), or raises"""
    with tempfile.TemporaryDirectory(prefix="tessera-bundle-latest-") as work:
        m, data, sig, folder = fetch_manifest("", channel, work, base_list=base_list, pubkey=pubkey)
    if channel in ROLLING and m.get("channel") != channel:
        raise Failed(f"the {channel} pointer names a {m.get('channel')!r} build", "refused")
    return m


# ── Current, previous: the swap ──────────────────────────────────────────

def _swap_link(link, target):
    """LINK points at TARGET, in one rename"""
    tmp = f"{link}.new-{secrets.token_hex(4)}"
    os.symlink(target, tmp)
    os.replace(tmp, link)


def _keep_aside(path, data):
    """A file or folder that a link takes the place of, kept (once) in the
    backups folder: what an install from before bundles had there"""
    dest = os.path.join(data, "backups", "before-bundles", os.path.basename(path))
    if os.path.lexists(dest):
        dest += "-" + time.strftime("%Y%m%d-%H%M%S")
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    os.replace(path, dest)
    return dest


def ensure_links(data=None, config=None):
    """Everything outside versions/ that the shell, Hyprland and the tessera
    command use, pointed through `current` (each link made in one rename;
    what was there before bundles kept in backups/before-bundles). Returns
    what it changed."""
    data = data or data_dir()
    config = config or config_home()
    changed = []
    cur = current_link(data)
    if read_link(cur) is None:
        return changed
    names = PROGRAMS + tuple(n for n in OPTIONAL if os.path.exists(os.path.join(cur, "bin", n)))
    want = [(os.path.join(data, "bin", n), os.path.join("..", "current", "bin", n)) for n in names]
    want += [(os.path.join(data, "bin", n + ".json"), os.path.join("..", "current", "bin", n + ".json")) for n in names]
    want += [(os.path.join(data, "tessera-glass.so"), os.path.join("current", "plugin", "tessera-glass.so")),
             (os.path.join(data, "tessera-glass.built-for"), os.path.join("current", "plugin", "tessera-glass.built-for")),
             (os.path.join(config, "quickshell", "tessera"), os.path.join(cur, "shell"))]
    for path, target in want:
        if os.path.islink(path) and os.readlink(path) == target:
            continue
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if os.path.lexists(path) and not os.path.islink(path):
            _keep_aside(path, data)
        _swap_link(path, target)
        changed.append(path)
    src = os.path.join(data, "source")
    try:
        with open(src) as f:
            had = f.read().strip()
    except OSError:
        had = ""
    if had != cur:
        write_text(src, cur + "\n")
        changed.append(src)
    # The tessera command (install.sh copies it to ~/.local/bin): this version's
    cmd_src, cmd = os.path.join(cur, "tools", "tessera"), os.path.join(os.path.expanduser("~"), ".local", "bin", "tessera")
    try:
        if os.path.isfile(cmd_src) and os.path.isfile(cmd) and not os.path.islink(cmd) and sha256_of(cmd_src) != sha256_of(cmd):
            tmp = cmd + ".new"
            shutil.copy2(cmd_src, tmp)
            os.chmod(tmp, 0o755)
            os.replace(tmp, cmd)
            changed.append(cmd)
    except OSError:
        pass
    return changed


def activate(vid, data=None, config=None, hypr=True):
    """VID becomes the current version (the one it replaces is kept as
    previous), then the links outside versions/ follow and ~/.config/hypr's
    Tessera files are merged with yours. Returns {"current", "previous",
    "changed", "config"}."""
    data = data or data_dir()
    vdir = version_dir(vid, data)
    if not os.path.isfile(os.path.join(vdir, "bundle.json")):
        raise Failed(f"version {vid} isn't here", "missing")
    with Lock(data):
        cur = read_link(current_link(data))
        if cur and cur != vid:
            _swap_link(previous_link(data), os.path.join("versions", cur))
        if cur != vid:
            _swap_link(current_link(data), os.path.join("versions", vid))  # (the commit point)
        changed = ensure_links(data, config)
        write_json(os.path.join(data, "bundle-state.json"), {"current": vid, "previous": read_link(previous_link(data)),
                                                             "at": int(time.time())})
    out = {"current": vid, "previous": read_link(previous_link(data)), "changed": changed}
    if hypr:
        out["config"] = sync_hypr(vdir)
    return out


def undo(data=None, config=None, hypr=True):
    """Back to the previous version (and the one undone becomes previous:
    undoing again goes forward). Returns {"current", "previous"}."""
    data = data or data_dir()
    with Lock(data):
        cur, prev = read_link(current_link(data)), read_link(previous_link(data))
        if not prev or not os.path.isfile(os.path.join(version_dir(prev, data), "bundle.json")):
            raise Failed("there's no previous version to go back to", "missing")
        _swap_link(current_link(data), os.path.join("versions", prev))
        if cur:
            _swap_link(previous_link(data), os.path.join("versions", cur))
        ensure_links(data, config)
        write_json(os.path.join(data, "bundle-state.json"), {"current": prev, "previous": cur, "at": int(time.time())})
    out = {"current": prev, "previous": cur}
    if hypr:
        out["config"] = sync_hypr(version_dir(prev, data))
    return out


def rollback(data=None, config=None):
    """launch-shell.sh, when the shell an update just put in didn't stay up:
    undo, the version undone marked bad (not offered again until a newer
    one), and a note for the shell ("Back on …")."""
    data = data or data_dir()
    gone = read_link(current_link(data))
    out = undo(data, config)
    m = load_json(os.path.join(version_dir(gone, data), "bundle.json"), {}) if gone else {}
    bad = os.path.join(state_dir(), "bad-bundles.json")
    d = load_json(bad, {}) or {}
    commits = [c for c in (d.get("commits") or []) if isinstance(c, str)][-20:]
    if m.get("commit") and m["commit"] not in commits:
        commits.append(m["commit"])
    write_json(bad, {"commits": commits})
    write_json(os.path.join(state_dir(), "rolled-back.json"), {"from": gone or "", "to": out["current"], "at": int(time.time())})
    return dict(out, bad=m.get("commit", ""))


def is_bad(commit):
    d = load_json(os.path.join(state_dir(), "bad-bundles.json"), {}) or {}
    return commit in (d.get("commits") or [])


def sync_hypr(vdir):
    """tools/config-sync.py's three-way merge of ~/.config/hypr's Tessera
    files from that version's hypr/ (never fatal: {"error"} on a failure)"""
    try:
        cs = _load("tessera_config_sync_for_bundles", "config-sync.py")
        rep = cs.sync(vdir, None, shell=False)
        h = rep.get("hypr") or {}
        return {"hypr": bool(h.get("any")), "notes": h.get("notes") or [], "new_files": h.get("new_files") or []}
    except Exception as e:  # (the version is in: this never undoes it)
        return {"error": str(e) or e.__class__.__name__}


def prune(data=None, keep=()):
    """Versions other than current, previous and KEEP removed (and any
    half-unpacked ones). Returns the ids removed."""
    data = data or data_dir()
    gone = []
    with Lock(data):
        keep = set(keep) | {read_link(current_link(data)), read_link(previous_link(data))}
        try:
            names = os.listdir(versions_dir(data))
        except OSError:
            return gone
        for n in names:
            p = os.path.join(versions_dir(data), n)
            if n.startswith(".incoming-") or ".old-" in n:
                shutil.rmtree(p, ignore_errors=True)
            elif ID.match(n) and n not in keep:
                shutil.rmtree(p, ignore_errors=True)
                gone.append(n)
    return gone


def staged_keep():
    """The version Software Update made ready for the next restart
    (tessera-update.py's update-staged/staged.json), kept by `prune`"""
    s = load_json(os.path.join(state_dir(), "update-staged", "staged.json"), {}) or {}
    vid = s.get("bundle") if isinstance(s, dict) else None
    return [vid] if isinstance(vid, str) and ID.match(vid) else []


def status(data=None):
    data = data or data_dir()
    out = {"current": read_link(current_link(data)), "previous": read_link(previous_link(data)), "versions": []}
    try:
        names = sorted(n for n in os.listdir(versions_dir(data)) if ID.match(n))
    except OSError:
        names = []
    for n in names:
        info = load_json(os.path.join(versions_dir(data), n, "bundle-info.json"), {}) or {}
        m = load_json(os.path.join(versions_dir(data), n, "bundle.json"), {}) or {}
        out["versions"].append({"id": n, "commit": m.get("commit", ""), "channel": m.get("channel", ""), "date": m.get("date", ""),
                                "variant": info.get("variant", ""), "signed": info.get("signed", False)})
    return out


# ── How this copy updates ────────────────────────────────────────────────

def _git(repo, *args):
    try:
        r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return None
    return r.stdout.strip() if r.returncode == 0 else None


def mode(repo=None, data=None, config=None, cfg=None):
    """bundle | git | dev (the top of this file). REPO: the copy asking
    (default: the one this file is in)."""
    forced = os.environ.get("TESSERA_UPDATE_MODE", "")
    if forced in ("bundle", "git", "dev"):
        return forced
    data = data or data_dir()
    config = config or config_home()
    repo = os.path.realpath(repo or REPO)
    c = cfg if cfg is not None else load_json(os.path.join(config, "tessera", "config.json"), {}) or {}
    up = c.get("update") if isinstance(c, dict) else None
    if isinstance(up, dict) and up.get("mode") == "dev":
        return "dev"
    vroot = os.path.realpath(versions_dir(data))
    if repo.startswith(vroot + os.sep) or os.path.isfile(os.path.join(repo, "bundle.json")):
        return "bundle"
    # A normal install still on git: setup.sh's folder, its shell copied
    # (not install.sh --dev's link), following main or beta, nothing of its own
    if os.path.realpath(os.path.join(data, "repo")) != repo or not os.path.isdir(os.path.join(repo, ".git")):
        return "dev"
    qs = os.path.join(config, "quickshell", "tessera")
    if os.path.islink(qs) and not os.path.realpath(qs).startswith(vroot + os.sep):
        return "dev"
    up = _git(repo, "rev-parse", "--abbrev-ref", "@{u}") or ""
    if up not in ("origin/main", "origin/beta"):
        return "dev"
    if _git(repo, "status", "--porcelain", "--untracked-files=no"):
        return "dev"  # (edits of its own: those are a developer's)
    if (_git(repo, "rev-list", "--count", "HEAD", "--not", "--remotes=origin") or "0") != "0":
        return "dev"
    return "git"


# ── Making a bundle (the release builder, and Build now) ─────────────────

def checkout_files(source):
    """What a bundle carries from the checkout at SOURCE: git's files, less
    EXCLUDE_TOP"""
    out = subprocess.run(["git", "-C", source, "ls-files", "-z"], capture_output=True, timeout=120)
    if out.returncode != 0:
        raise Failed("not a git checkout: " + source, "bad")
    names = [n for n in out.stdout.decode("utf-8", "surrogateescape").split("\0") if n]
    return [n for n in names if not any(n == e.rstrip("/") or n.startswith(e) for e in EXCLUDE_TOP)]


def built_files(source):
    """What the builds leave in the checkout that a bundle carries: the
    shaders (.qsb beside their sources) and the native helpers' build"""
    out = []
    for top in ("shell", "material"):
        for d, dirs, files in os.walk(os.path.join(source, top)):
            dirs[:] = [x for x in dirs if x not in (".git", "__pycache__")]
            for f in files:
                p = os.path.join(d, f)
                rel = os.path.relpath(p, source)
                if f.endswith(".qsb") or rel.startswith("shell/native/build/"):
                    out.append(rel)
    return out


def pack_variant(source, commit, variant, programs, plugin, out_dir, epoch=None):
    """One variant's bundle from the checkout SOURCE (built: shaders, native
    helpers), PROGRAMS ({name: path}) and PLUGIN (folder with
    tessera-glass.so and tessera-glass.built-for), into OUT_DIR. Returns its
    manifest entry."""
    stage = tempfile.mkdtemp(dir=out_dir, prefix=f".stage-{variant}-")
    try:
        files, links = {}, {}
        rels = sorted(set(checkout_files(source)) | set(built_files(source)))

        def add(rel, src):
            check_path(rel)
            dst = os.path.join(stage, rel)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            if os.path.islink(src):
                target = os.readlink(src)
                if not link_ok(rel, target):
                    raise Failed(f"{rel} is a link out of the checkout", "bad")
                os.symlink(target, dst)
                links[rel] = target
                return
            if not os.path.isfile(src):
                return  # (a file git lists that the checkout doesn't have: a sparse checkout)
            shutil.copyfile(src, dst)
            mode_ = 0o755 if os.stat(src).st_mode & 0o111 else 0o644
            os.chmod(dst, mode_)
            files[rel] = {"sha256": sha256_of(dst), "size": os.path.getsize(dst), "mode": mode_}
        for rel in rels:
            add(rel, os.path.join(source, rel))
        versions = {}
        for name in PROGRAMS + tuple(n for n in OPTIONAL if programs.get(n) and os.path.isfile(programs[n])):
            add("bin/" + name, programs[name])
            files["bin/" + name]["mode"] = 0o755
            os.chmod(os.path.join(stage, "bin", name), 0o755)
            versions[name] = ni().program_version(os.path.join(stage, "bin", name))
        for name in ("tessera-glass.so", "tessera-glass.built-for"):
            if plugin and os.path.isfile(os.path.join(plugin, name)):
                add("plugin/" + name, os.path.join(plugin, name))
        file = tarball_name(commit, variant)
        dest = os.path.join(out_dir, file)
        tmp = dest + ".tmp"
        epoch = epoch or int(time.time())
        tar = subprocess.Popen(["tar", "--sort=name", "--owner=0", "--group=0", "--numeric-owner", f"--mtime=@{epoch}",
                                "-C", stage, "-cf", "-", "."], stdout=subprocess.PIPE)
        z = subprocess.run(["nice", "-n", "10", "zstd", "-q", "-19", "-T0", "-f", "-o", tmp], stdin=tar.stdout)
        tar.stdout.close()
        if tar.wait() != 0 or z.returncode != 0:
            raise Failed(f"packing {file} failed", "bad")
        os.replace(tmp, dest)
        return {"file": file, "size": os.path.getsize(dest), "sha256": sha256_of(dest), "files": files, "links": links,
                "programs": {n: {"sha256": files["bin/" + n]["sha256"], "version": versions[n]} for n in versions}}
    finally:
        shutil.rmtree(stage, ignore_errors=True)


def system_lock(run=subprocess.run):
    """The package lock from the system this runs on (the build machine):
    the held family at exactly its versions, the minimum family at least
    theirs (pacman -Qi for the architecture)"""
    held, minimum = {}, {}
    names = list(HELD_FAMILY) + list(MINIMUM_FAMILY)
    have = installed_packages(names, run)
    arch = {}
    try:
        r = run(["pacman", "-Qi", "--", *[n for n in HELD_FAMILY if n in have]], capture_output=True, text=True, timeout=60,
                env=dict(os.environ, LANG="C", LC_ALL="C"))
        name = None
        for line in (r.stdout or "").splitlines():
            k, _, v = line.partition(":")
            if k.strip() == "Name":
                name = v.strip()
            elif k.strip() == "Architecture" and name:
                arch[name] = v.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    for n in HELD_FAMILY:
        if n in have and PKG_VERSION.match(have[n]):
            held[n] = {"version": have[n], "arch": arch.get(n) if arch.get(n) in PKG_ARCH else "x86_64"}
    for n in MINIMUM_FAMILY:
        if n in have and PKG_VERSION.match(have[n]):
            minimum[n] = have[n]
    return check_lock({"held": held, "minimum": minimum})


SETTINGS_TEXTS = ("shell/common/Config.qml", "shell/common/Theme.qml", "hypr/tessera/keymap.json",
                  "tools/settings-migrations.json", "tools/rust-defaults.json")


def make_manifest(source, commit, channel, entries, lock, tree="", tag="", version=""):
    """The manifest for both variants' entries: what installs check, the
    package lock, and what Software Update's check reads before anything is
    downloaded (the settings files whose defaults it compares, the changelog)"""
    texts = {}
    for rel in SETTINGS_TEXTS:
        try:
            with open(os.path.join(source, rel), encoding="utf-8") as f:
                texts[rel] = f.read()
        except (OSError, UnicodeDecodeError):
            pass
    try:
        with open(os.path.join(source, "CHANGELOG.md"), encoding="utf-8") as f:
            changelog = f.read()
    except OSError:
        changelog = ""
    date = _git(source, "log", "-1", "--format=%cs", commit) or time.strftime("%Y-%m-%d")
    ctime = _git(source, "log", "-1", "--format=%ct", commit) or "0"
    m = {"format": FORMAT, "name": NAME, "commit": commit, "id": commit[:12], "tree": tree, "channel": channel, "tag": tag,
         "version": version, "date": date, "time": int(ctime) if ctime.isdigit() else 0,
         "built": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "variants": entries, "packages": check_lock(lock),
         "texts": texts, "changelog": changelog_head(changelog)}
    return check_manifest(m, commit)


def changelog_head(text, entries=40):
    """The changelog's newest entries (its top), for What's new"""
    heads = [m.start() for m in re.finditer(r"^## ", text, re.M)]
    if len(heads) > entries:
        return text[:heads[entries]]
    return text


def build_here(source, commit, channel, *, data=None, say=None, run=subprocess.run, builder=None):
    """Build now: this version built here from the checkout SOURCE (tools/
    build-native.sh's gentle build: idle CPU and IO, half the cores, about
    ten minutes; the shaders, native helpers and glass plugin take less
    than a minute), packed as a bundle signed by nobody (versions/<id>-local)
    and unpacked as a version. Returns the id; raises Failed."""
    say = say or (lambda text: None)
    data = data or data_dir()
    variant = detect_variant()
    work = tempfile.mkdtemp(dir=data if os.path.isdir(data) else None, prefix=".build-here-")
    try:
        progs = os.path.join(work, "programs")
        os.makedirs(progs)
        say("Building tessera-ui and tessera-events here (gently; about ten minutes)")
        if builder:
            files = builder(progs, variant)
        else:
            files = ni().run_build(progs, False, True, lambda s: say(s[5:] if s.startswith("STEP ") else s), root=source)
        if not all(n in files for n in PROGRAMS):
            raise Failed("tessera-ui didn't build", "build")
        say("Building the shaders, native helpers and glass plugin")
        plugin = os.path.join(work, "plugin")
        os.makedirs(plugin)
        for cmd, env in (([os.path.join(source, "tools", "build-shaders.sh")], {}),
                         ([os.path.join(source, "shell", "native", "build.sh")], {}),
                         (["bash", os.path.join(source, "tools", "build-plugin.sh")], {"TESSERA_PLUGIN_DEST": plugin})):
            if builder:
                continue  # (tests: the programs are fakes, and so are the parts)
            r = run(["nice", "-n", "19", *cmd], capture_output=True, text=True, env=dict(os.environ, **env), timeout=1800)
            if r.returncode != 0 and "build-shaders" in cmd[-1]:
                raise Failed("the shaders didn't build: " + ((r.stdout + r.stderr).strip().splitlines() or ["no output"])[-1], "build")
        say("Packing")
        entry = pack_variant(source, commit, variant, files, plugin, work)
        lock = system_lock(run) if not builder else {"held": {}, "minimum": {}}
        m = make_manifest(source, commit, "local", {variant: entry}, lock)
        mbytes = json.dumps(m, indent=1).encode()
        return install_version(mbytes, "", os.path.join(work, entry["file"]), variant, data, signed=False)
    finally:
        shutil.rmtree(work, ignore_errors=True)


# ── Command line ─────────────────────────────────────────────────────────

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["mode", "status", "plan", "latest", "fetch", "activate", "undo", "rollback", "ensure",
                                        "verify", "prune", "lock-check", "moves", "pack", "build-here"])
    ap.add_argument("arg", nargs="?", default="")
    ap.add_argument("--commit", default="")
    ap.add_argument("--channel", default="stable", choices=["stable", "beta"])
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--repo", default="", help="mode: the copy asking")
    ap.add_argument("--source", default="", help="pack, build-here: the checkout (built)")
    ap.add_argument("--variant", default="", choices=["", *VARIANTS])
    ap.add_argument("--programs", default="", help="pack: the folder with tessera-ui and tessera-events")
    ap.add_argument("--plugin", default="", help="pack: the folder with tessera-glass.so")
    ap.add_argument("--out", default="", help="pack: where the bundle goes")
    a = ap.parse_args(argv)

    def emit(obj, text):
        print(json.dumps(obj, indent=1) if a.json else text)

    try:
        if a.command == "mode":
            m = mode(a.repo or None)
            emit({"mode": m}, m)
        elif a.command == "status":
            s = status()
            emit(s, "\n".join(f"{v['id']}{' (current)' if v['id'] == s['current'] else ' (previous)' if v['id'] == s['previous'] else ''}"
                              f" {v['channel']} {v['date']} {v['variant']}{'' if v['signed'] else ' built here'}" for v in s["versions"])
                 or "No bundles here")
        elif a.command == "plan":
            p = plan(a.commit, a.channel)
            emit(p, (f"Download {p['size'] / 1e6:.0f} MB" + (f"; packages: {p['movesText']}" if p["movesText"] else "")) if p["available"]
                 else "No: " + p["why"])
        elif a.command == "latest":
            m = latest(a.channel)
            emit({"commit": m["commit"], "date": m.get("date", ""), "channel": m["channel"]}, m["commit"])
        elif a.command == "fetch":
            r = fetch(a.commit, a.channel, say=(lambda t, got, size: print(f"PROGRESS {t} {got} {size}", file=sys.stderr, flush=True)))
            emit({"id": r["id"], "variant": r["variant"], "size": r["size"]}, "Ready: " + r["id"])
        elif a.command == "activate":
            r = activate(a.arg)
            emit(r, "Now on " + r["current"])
        elif a.command == "undo":
            r = undo()
            emit(r, "Back on " + r["current"])
        elif a.command == "rollback":
            r = rollback()
            emit(r, "Back on " + r["current"])
        elif a.command == "ensure":
            emit({"changed": ensure_links()}, "ok")
        elif a.command == "verify":
            vid = a.arg or read_link(current_link())
            bad = verify_tree(version_dir(vid)) if vid else ["(no version)"]
            emit({"id": vid, "bad": bad}, "All files are right" if not bad else "Wrong: " + ", ".join(bad))
            return 1 if bad else 0
        elif a.command == "prune":
            emit({"removed": prune(keep=staged_keep())}, "ok")
        elif a.command == "lock-check":
            lock = check_lock(load_json(a.arg, None))
            emit(lock, "ok")
        elif a.command == "moves":
            lock = check_lock(load_json(a.arg, None))
            names = list(lock["held"]) + list(lock["minimum"])
            m = package_moves(lock, installed_packages(names), repo_versions(list(lock["held"])))
            emit(m, moves_words(m) or "Nothing to move")
        elif a.command == "pack":
            if not (a.source and COMMIT.match(a.commit) and a.variant and a.programs and a.out):
                raise Failed("pack needs --source, --commit, --variant, --programs and --out", "bad")
            e = pack_variant(a.source, a.commit, a.variant, {n: os.path.join(a.programs, n) for n in PROGRAMS + OPTIONAL}, a.plugin or None, a.out)
            write_json(os.path.join(a.out, f"variant-{a.variant}.bundle.json"), e)
            emit({"file": e["file"], "size": e["size"], "sha256": e["sha256"]}, e["file"])
        elif a.command == "build-here":
            vid = build_here(a.source, a.commit, "local", say=lambda t: print("STEP " + t, file=sys.stderr, flush=True))
            emit({"id": vid}, "Built: " + vid)
        return 0
    except Failed as e:
        emit({"error": e.why, "kind": e.kind}, "No: " + e.why)
        return 1


if __name__ == "__main__":
    sys.exit(main())
