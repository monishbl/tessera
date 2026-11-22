"""Dependencies on update: every package a Tessera version needs installed,
at a version it supports, before that version runs (docs/releases.md,
"Dependencies on update"). Shared by every way Tessera is updated:
Software Update (tools/tessera-update.py stage), `tessera update` (cli),
the restart and the login that install a staged update (finish), the
older full update (apply), bundles, and install.sh. Loaded by its path
(python -I leaves tools/ off sys.path), like tools/tessera_sources.py.

What a version needs, read as text from its tools/packages.sh (never run:
it may be an update's, not installed yet) and its compat.json:
  PACKAGES   installed (tools/tessera-packages.sh status: missing,
             unavailable, dropped)
  MINIMUM    optional, "name>=version" (also <=, <, >): the versions it
             supports. An installed package below one is upgraded (pacman
             -S --needed, to the repositories' version)
  compat.json  Hyprland's min and max ("0.57.x": any 0.57), Quickshell's
             min (Tessera's own Quickshell build stands in for an older one)
  AUR        the pointer theme: never installed by an update; a note

The decision (assess): install = missing + too old (when the repositories
have a version that fits); blocked = a needed package the repositories
don't have, or only at a version that doesn't fit (too old, or above
Hyprland's max): the update stops and says which and what to do. A
package installed above a max is a note (the version being installed is
the one most likely to support it), never a reason to stay behind.

    tessera_deps.py requirements --list F [--compat F]
        JSON {"outdated": [...], "blocked": [...], "newer": [...]}: names
        (tools/tessera-packages.sh adds "outdated" to its "missing", so
        Settings installs them with the rest, one password)
    tessera_deps.py check [--root DIR]
        install.sh, after its pacman -Syu: what's still not met, in words;
        exit 1 when something is
"""
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PKG = re.compile(r"^[a-z0-9@_+][a-z0-9@._+-]*$")  # (never "-…": it would pass for a pacman option)
REQ = re.compile(r"^([a-z0-9@_+][a-z0-9@._+-]*?)(>=|<=|<|>)((?:[0-9]+:)?[0-9A-Za-z._+~-]+)$")
MIN_OPS, MAX_OPS = (">=", ">"), ("<=", "<")
SYSTEM_FIX = "Update the system first (Settings → Software Update → System packages, or sudo pacman -Syu), then update Tessera again"


# ── Reading a version's lists (as text) ──────────────────────────────────

def block(text, array):
    """The words of a NAME=( … ) block: one or more per line, # comments,
    quotes taken off. The same rules as tools/tessera-packages.sh's names()."""
    out, on = [], False
    start = re.compile(r"^\s*" + re.escape(array) + r"=\(")
    for line in (text or "").splitlines():
        if not on:
            if not start.match(line):
                continue
            on = True
            line = start.sub("", line, count=1)
        line = line.split("#", 1)[0]
        done = ")" in line
        line = line.split(")", 1)[0]
        out += [w.strip("\"'") for w in line.split() if w.strip("\"'")]
        if done:
            on = False
    return out


def names(text, array):
    """Package names of a block (only well-formed ones)"""
    return [w for w in block(text, array) if PKG.match(w)]


def minimums(text):
    """packages.sh's MINIMUM=( "hyprland>=0.56.0" … ): {name: [(op, version)]}.
    Anything else in the block is ignored (an old reader, a typo: never a
    pacman argument)."""
    out = {}
    for w in block(text, "MINIMUM"):
        m = REQ.match(w)
        if m:
            out.setdefault(m.group(1), []).append((m.group(2), m.group(3)))
    return out


def max_limit(v):
    """compat.json's max: "0.57.x" is below 0.58; "0.57.1" is up to it"""
    v = (v or "").strip()
    if not v:
        return None
    if v.endswith(".x"):
        parts = v[:-2].split(".")
        if not parts or not parts[-1].isdigit():
            return None
        parts[-1] = str(int(parts[-1]) + 1)
        return ("<", ".".join(parts))
    return ("<=", v)


def compat_limits(text):
    """compat.json: {name: [(op, version)]}, and the names whose minimum
    Tessera's own build can meet instead (Quickshell)"""
    try:
        c = json.loads(text or "{}")
    except ValueError:
        return {}, set()
    out, soft = {}, set()
    h = c.get("hyprland") if isinstance(c.get("hyprland"), dict) else {}
    if h.get("min"):
        out.setdefault("hyprland", []).append((">=", str(h["min"])))
    mx = max_limit(str(h.get("max") or ""))
    if mx:
        out.setdefault("hyprland", []).append(mx)
    q = c.get("quickshell") if isinstance(c.get("quickshell"), dict) else {}
    if q.get("min"):
        out.setdefault("quickshell", []).append((">=", str(q["min"])))
        soft.add("quickshell")
    return out, soft


def requirements(list_text, compat_text=""):
    """MINIMUM and compat.json together: {name: {"limits": [(op, v)], "soft": bool}}.
    Both apply (pacman checks each), so the stricter one wins."""
    reqs = {}
    for n, lim in minimums(list_text).items():
        reqs.setdefault(n, {"limits": [], "soft": False})["limits"] += lim
    lim, soft = compat_limits(compat_text)
    for n, l in lim.items():
        # (a MINIMUM entry of its own makes it a real minimum: soft only from compat.json)
        r = reqs.setdefault(n, {"limits": [], "soft": n in soft})
        r["limits"] += [x for x in l if x not in r["limits"]]
    return reqs


# ── pacman, read-only (run is subprocess.run, or a test's fake) ──────────

def _out(run, cmd):
    try:
        r = run(cmd, capture_output=True, text=True, timeout=60, env=dict(os.environ, LANG="C", LC_ALL="C"))
    except (OSError, subprocess.SubprocessError):
        return None
    return r


def unsatisfied(run, deps):
    """pacman -T: the ones nothing installed satisfies (names or provides, with versions)"""
    if not deps:
        return []
    r = _out(run, ["pacman", "-T", "--", *deps])
    if r is None:
        return list(deps)
    return [l.strip() for l in (r.stdout or "").splitlines() if l.strip()]


def installed_version(run, name):
    r = _out(run, ["pacman", "-Q", "--", name])
    if r is None or r.returncode != 0:
        return ""
    p = (r.stdout or "").split()
    return p[1] if len(p) >= 2 and p[0] == name else ""


def repo_version(run, name):
    """The repositories' version (the package lists as last downloaded)"""
    r = _out(run, ["pacman", "-Si", "--", name])
    if r is None or r.returncode != 0:
        return ""
    for line in (r.stdout or "").splitlines():
        k, _, v = line.partition(":")
        if k.strip() == "Version":
            return v.strip()
    return ""


def _fallback_vercmp(a, b):
    def key(v):
        return [int(x) if x.isdigit() else x for x in re.split(r"[.:+~_-]", v)]
    ka, kb = key(a), key(b)
    try:
        return (ka > kb) - (ka < kb)
    except TypeError:
        return (a > b) - (a < b)


def vercmp(run, a, b):
    """pacman's vercmp (its own program, from pacman)"""
    r = _out(run, ["vercmp", a, b])
    try:
        return int((r.stdout or "").strip()) if r is not None and r.returncode == 0 else _fallback_vercmp(a, b)
    except ValueError:
        return _fallback_vercmp(a, b)


def fits(run, version, limits):
    for op, v in limits:
        c = vercmp(run, version, v)
        if not {">=": c >= 0, ">": c > 0, "<=": c <= 0, "<": c < 0}[op]:
            return False
    return True


def upstream(v):
    """1:0.57.1-2 -> 0.57.1 (said to people)"""
    v = re.sub(r"^[0-9]+:", "", v or "")
    return re.sub(r"-[0-9.]+$", "", v)


def range_words(limits):
    lo = [v for op, v in limits if op in MIN_OPS]
    hi = [(op, v) for op, v in limits if op in MAX_OPS]
    parts = [f"{lo[-1]} or newer"] if lo else []
    for op, v in hi:
        parts.append(f"older than {v}" if op == "<" else f"{v} at most")
    return ", ".join(parts)


def own_quickshell(data=None):
    """Tessera's own Quickshell build (tools/build-quickshell.sh), which
    launch-shell.sh starts instead of the system's"""
    d = data or os.path.join(os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share"), "tessera")
    return os.access(os.path.join(d, "quickshell", "bin", "qs"), os.X_OK) and \
        os.path.isdir(os.path.join(d, "quickshell", "lib", "qt6", "qml", "Quickshell"))


# ── The decision ──────────────────────────────────────────────────────────

def check_requirements(reqs, packages, run=subprocess.run, own_qs=False, skip=()):
    """Each requirement against what's installed and the repositories:
    {"outdated": [{name, have, need, repo}], "blocked": [{name, have, need,
    repo, why}], "newer": [{name, have, need}]}. skip: names something
    else puts in place (a bundle's held packages)."""
    out = {"outdated": [], "blocked": [], "newer": []}
    packages = set(packages)
    for name, r in sorted(reqs.items()):
        limits = r["limits"]
        if name in skip or not limits:
            continue
        need = range_words(limits)
        lows = [(op, v) for op, v in limits if op in MIN_OPS]
        if unsatisfied(run, [name]):
            # Not installed: when it's one this version installs, the
            # repositories' version must fit (or it can't be installed)
            if name in packages and not (r["soft"] and own_qs):
                repo = repo_version(run, name)
                if repo and not fits(run, repo, limits):
                    out["blocked"].append({"name": name, "have": "", "need": need, "repo": repo,
                                           "why": "too new" if fits(run, repo, lows) else "too old"})
            continue
        if r["soft"] and own_qs:
            continue  # (Tessera's own Quickshell runs, not the system's)
        unmet = set(unsatisfied(run, [name + op + v for op, v in limits]))
        low = [(op, v) for op, v in limits if op in MIN_OPS and name + op + v in unmet]
        high = [(op, v) for op, v in limits if op in MAX_OPS and name + op + v in unmet]
        have = installed_version(run, name)
        if low:
            repo = repo_version(run, name)
            entry = {"name": name, "have": have, "need": need, "repo": repo}
            if repo and fits(run, repo, limits):
                out["outdated"].append(entry)
            else:
                entry["why"] = "too new" if repo and fits(run, repo, lows) else "too old"
                out["blocked"].append(entry)
        elif high:
            out["newer"].append({"name": name, "have": have, "need": need})
    return out


def assess(status, list_text, compat_text="", current_text=None, run=subprocess.run, own_qs=None, skip=()):
    """What a version needs on this computer. status: tools/tessera-packages.sh
    status for that version ({"missing", "unavailable", "dropped", "reasons"}).
    current_text: the installed version's packages.sh (a package it already
    listed and the repositories never had isn't new; one this version adds
    is). Returns {"install": names to install or upgrade, "missing",
    "outdated", "blocked", "newer", "left_out", "aur", "dropped", "reasons"}."""
    status = status if isinstance(status, dict) else {}
    pk = names(list_text, "PACKAGES")
    reqs = requirements(list_text, compat_text)
    if own_qs is None:
        own_qs = own_quickshell()
    got = check_requirements(reqs, pk, run, own_qs, skip)
    missing = [n for n in status.get("missing") or [] if PKG.match(n)]
    blocked = list(got["blocked"])
    stopped = {b["name"] for b in blocked}
    # Not in the repositories: one this version adds, or one with a minimum,
    # stops it; one the installed version already listed (and ran without)
    # stays left out, as install.sh does
    current = set(names(current_text, "PACKAGES")) if current_text is not None else set()
    left_out = []
    for n in status.get("unavailable") or []:
        if not PKG.match(n) or n in stopped:
            continue
        if (current_text is not None and n not in current) or n in reqs:
            blocked.append({"name": n, "have": "", "need": "", "repo": "", "why": "unavailable"})
            stopped.add(n)
        else:
            left_out.append(n)
    outdated = [o for o in got["outdated"] if o["name"] not in stopped]
    install = [n for n in missing if n not in stopped]
    install += [o["name"] for o in outdated if o["name"] not in install]
    aur = [n for n in names(list_text, "AUR") if unsatisfied(run, [n])]
    return {"install": install, "missing": [n for n in missing if n not in stopped], "outdated": outdated, "blocked": blocked,
            "newer": got["newer"], "left_out": left_out, "aur": aur,
            "dropped": [n for n in status.get("dropped") or [] if PKG.match(n)], "reasons": status.get("reasons") or {}}


# ── In words ──────────────────────────────────────────────────────────────

def blocked_words(plan, version=""):
    """Why the update stops, and what to do, for each blocked package"""
    lines = []
    for b in plan.get("blocked") or []:
        n, have, repo = b["name"], upstream(b.get("have")), upstream(b.get("repo"))
        if b["why"] == "unavailable":
            lines.append(f"It needs {n}, which isn't in your repositories (pacman -Si {n}). "
                         "Turn on the repository that carries it, then update Tessera again.")
        elif b["why"] == "too new":
            lines.append(f"It needs {n} {b['need']}, and your repositories have {repo}, which it isn't tested with. "
                         f"Wait for a Tessera version that supports {n} {repo}.")
        else:
            now = f"this computer has {have}" if have else f"{n} isn't installed"
            there = f"your repositories have {repo}" if repo else "your repositories don't have it"
            lines.append(f"It needs {n} {b['need']}; {now} and {there}. {SYSTEM_FIX}.")
    if not lines:
        return ""
    head = f"Tessera {version} can't be installed yet." if version else "This Tessera can't be installed yet."
    return head + " " + " ".join(lines) + " Nothing was changed."


def needs_words(plan):
    """"inter-font, hyprland (0.55.0, needs 0.56.0 or newer)": what installing does"""
    parts = list(plan.get("missing") or [])
    parts += [f"{o['name']} ({upstream(o['have']) or 'installed'} → {upstream(o['repo'])}, needs {o['need']})"
              for o in plan.get("outdated") or []]
    return ", ".join(parts)


def notes_words(plan):
    """What doesn't stop the update, said once"""
    out = []
    for n in plan.get("newer") or []:
        out.append(f"{n['name']} {upstream(n['have'])} is newer than this Tessera is tested with ({n['need']}).")
    if plan.get("left_out"):
        out.append("Not in your repositories, left out as before: " + ", ".join(plan["left_out"]) + ".")
    if plan.get("aur"):
        out.append("From the AUR, not installed: " + ", ".join(plan["aur"]) + " (yay -S " + " ".join(plan["aur"]) + ").")
    return out


# ── Command line ──────────────────────────────────────────────────────────

def _read(path):
    try:
        with open(path, errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def main(argv=None):
    a = list(sys.argv[1:] if argv is None else argv)

    def opt(flag, default=""):
        return a[a.index(flag) + 1] if flag in a and a.index(flag) + 1 < len(a) else default
    if a[:1] == ["requirements"]:
        text = _read(opt("--list"))
        got = check_requirements(requirements(text, _read(opt("--compat"))), names(text, "PACKAGES"), own_qs=own_quickshell())
        print(json.dumps({k: [x["name"] for x in v] for k, v in got.items()}))
        return 0
    if a[:1] == ["check"]:
        root = opt("--root", os.path.dirname(HERE))
        text = _read(os.path.join(root, "tools", "packages.sh"))
        got = check_requirements(requirements(text, _read(os.path.join(root, "compat.json"))), names(text, "PACKAGES"),
                                 own_qs=own_quickshell())
        # (after pacman -Syu: one still too old can't be upgraded any further)
        plan = {"blocked": got["blocked"] + [dict(o, why="too old") for o in got["outdated"]], "newer": got["newer"]}
        for line in notes_words(plan):
            print(line)
        msg = blocked_words(plan)
        if msg:
            print(msg, file=sys.stderr)
            return 1
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
