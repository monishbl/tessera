#!/usr/bin/env python3
"""Keep desktop light/dark preferences and Dolphin's KDE palette in sync.

Independent of matugen and Match apps. --scheme applies a generated KDE
scheme instead; --keep-kde only updates the desktop light/dark preference.
Only palette keys are changed; fonts, icons, styles and other settings stay.

What each key held before Tessera first changed it (kdeglobals' palette and
ColorScheme, Dolphin's ColorScheme, qt6ct's custom_palette and
color_scheme_path) is recorded in $XDG_STATE_HOME/tessera/desktop-theme.json,
and `undo-kde` (the uninstaller) puts those back, unless you chose another
scheme since (audit #962).

  desktop-theme.py dark|light [--scheme FILE] [--keep-kde]
  desktop-theme.py undo-gtk | undo-kde      (uninstall)
  desktop-theme.py gtk-ours NAME            exit 0 when that GTK theme is one
                                            Tessera may switch (Material's
                                            app-theme.sh asks)

A GTK theme named MacTahoe... was Tessera's own once: it's switched away a
single time, the first sync after the update (marked done in the settings,
appearance.gtkMacTahoeMoved), and after that it's a theme you chose and is
left alone (audit #963).
"""
import argparse
import configparser
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


def update_ini(path, groups):
    """Update selected keys without rewriting unrelated settings or comments.

    Returns whether the file changed, so apps are only told to reload (which
    repaints every KDE window) when something really did.
    """
    path = Path(path).resolve()  # keep symlinked dotfiles linked to their source
    text = path.read_text() if path.exists() else ""
    for group, entries in groups.items():
        header = re.compile(r"^" + re.escape("[" + group + "]") + r"\s*$", re.M)
        match = header.search(text)
        if match:
            start = match.end()
            following = re.search(r"^\[", text[start:], re.M)
            end = start + following.start() if following else len(text)
            body = text[start:end]
            for key, value in entries.items():
                pattern = re.compile(r"^" + re.escape(key) + r"\s*=.*$", re.M)
                line = key + "=" + value
                if pattern.search(body):
                    body = pattern.sub(lambda _m: line, body)
                else:
                    body = body.rstrip("\n") + "\n" + line + "\n"
            text = text[:start] + body + text[end:]
        else:
            text = text.rstrip("\n") + "\n\n[" + group + "]\n"
            text += "".join(key + "=" + value + "\n" for key, value in entries.items())
    if path.exists() and path.read_text() == text:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".tessera-theme-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write(text.lstrip("\n"))
        if path.exists():
            shutil.copymode(path, temporary)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return True


def current(path, group, key):
    """A setting's value in an ini file, or "" when it isn't there."""
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    try:
        parser.read(path)
        return parser.get(group, key, fallback="")
    except configparser.Error:
        return ""


def state_path():
    base = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local/state")
    return Path(base) / "tessera" / "desktop-theme.json"


def load_record():
    try:
        r = json.loads(state_path().read_text())
        return r if isinstance(r, dict) else {}
    except (OSError, ValueError):
        return {}


def save_record(r):
    p = state_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".desktop-theme-", dir=p.parent)
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(r, f, indent=1, sort_keys=True)
        os.replace(tmp, p)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def current_or_none(path, group, key):
    """A setting's value in an ini file, or None when it isn't there."""
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    try:
        parser.read(path)
        return parser.get(group, key, fallback=None)
    except configparser.Error:
        return None


def remember(record, name, path, groups, mark):
    """Each key's value before Tessera's first write to it (None: it wasn't
    there), kept under record["before"][name]; a key already recorded keeps
    what it had then. True when anything new was recorded.

    `mark` (group, key) names the file's scheme: when what was recorded for
    it is Tessera's own (an install from before this record: Tessera had
    written them already), every key recorded is taken out at undo instead,
    the app's own defaults then"""
    before = record.setdefault("before", {}).setdefault(name, {})
    new = False
    for group, entries in groups.items():
        g = before.setdefault(group, {})
        for key in entries:
            if key not in g:
                g[key] = current_or_none(path, group, key)
                new = True
    was = before.get(mark[0], {}).get(mark[1])
    if new and isinstance(was, str) and Path(was).name.startswith("Tessera"):
        for g in before.values():
            for key in g:
                g[key] = None
    return new


def remove_keys(path, groups):
    """Keys taken out of an ini file (a group left empty goes too), the rest
    of the file as it was. Returns whether it changed"""
    path = Path(path).resolve()
    if not path.exists():
        return False
    text = path.read_text()
    out = text
    for group, keys in groups.items():
        header = re.compile(r"^" + re.escape("[" + group + "]") + r"[ \t]*\n?", re.M)
        match = header.search(out)
        if not match:
            continue
        start = match.end()
        following = re.search(r"^\[", out[start:], re.M)
        end = start + following.start() if following else len(out)
        body = out[start:end]
        for key in keys:
            body = re.sub(r"^" + re.escape(key) + r"[ \t]*=.*(\n|$)", "", body, flags=re.M)
        if body.strip():
            out = out[:start] + body + out[end:]
        else:
            out = out[:match.start()] + out[end:]
    if out == text:
        return False
    if out.strip():
        fd, temporary = tempfile.mkstemp(prefix=".tessera-theme-", dir=path.parent)
        try:
            with os.fdopen(fd, "w") as stream:
                stream.write(out.strip("\n") + "\n")
            shutil.copymode(path, temporary)
            os.replace(temporary, path)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
    else:
        path.unlink()
    return True


def put_back(path, before):
    """The recorded values back (None: the key taken out)"""
    sets = {g: {k: v for k, v in e.items() if v is not None} for g, e in before.items() if isinstance(e, dict)}
    gone = {g: [k for k, v in e.items() if v is None] for g, e in before.items() if isinstance(e, dict)}
    changed = False
    sets = {g: e for g, e in sets.items() if e}
    if sets:
        changed |= update_ini(path, sets)
    gone = {g: e for g, e in gone.items() if e}
    if gone:
        changed |= remove_keys(path, gone)
    return changed


# Schemes Tessera may switch between; anything else was chosen by the user and
# is left alone (a palette set in KDE's settings or qt6ct isn't ours to undo)
OURS = re.compile(r"^(BreezeDark|BreezeLight|Breeze|Tessera.*)?$")


def fallback_scheme(path, dark):
    bg, view, button, fg, muted = ("30,30,33", "24,24,27", "48,48,52", "235,235,240", "160,160,168") if dark else ("245,245,247", "255,255,255", "235,235,240", "30,30,33", "100,100,108")
    groups = {"General": {"Name": "Tessera Dark" if dark else "Tessera Light", "ColorScheme": path.stem}}
    for name, background in [("Window", bg), ("View", view), ("Button", button), ("Tooltip", button), ("Header", bg), ("Complementary", bg), ("Selection", "0,100,210")]:
        groups["Colors:" + name] = {
            "BackgroundNormal": background, "BackgroundAlternate": button,
            "ForegroundNormal": "255,255,255" if name == "Selection" else fg,
            "ForegroundInactive": "225,225,230" if name == "Selection" else muted,
            "ForegroundActive": fg, "ForegroundLink": "80,160,255" if dark else "0,90,190",
            "ForegroundVisited": "180,130,230", "ForegroundNegative": "230,80,80",
            "ForegroundNeutral": "200,140,30", "ForegroundPositive": "40,170,90",
            "DecorationFocus": "0,122,255", "DecorationHover": "0,122,255",
        }
    update_ini(path, groups)


def apply_kde(mode, scheme=None):
    cfg = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    if scheme is None:
        name = "BreezeDark" if mode == "dark" else "BreezeLight"
        roots = [data] + [Path(p) for p in os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(":") if p]
        scheme = next((p / "color-schemes" / (name + ".colors") for p in roots if (p / "color-schemes" / (name + ".colors")).is_file()), None)
        if scheme is None:
            scheme = data / "color-schemes" / ("TesseraDesktopDark.colors" if mode == "dark" else "TesseraDesktopLight.colors")
            fallback_scheme(scheme, mode == "dark")
    scheme = Path(scheme).resolve()
    # (strict=False: a scheme with a key twice is read, not a traceback)
    palette = configparser.ConfigParser(interpolation=None, strict=False)
    palette.optionxform = str
    palette.read(scheme)
    groups = {s: dict(palette[s]) for s in palette.sections() if s.startswith(("Colors:", "ColorEffects:"))}
    if "Colors:View" not in groups or "ForegroundNormal" not in groups["Colors:View"]:
        raise ValueError("Incomplete KDE color scheme: " + str(scheme))
    groups["General"] = {"ColorScheme": scheme.stem}
    changed = False
    # (what each key held before Tessera's first write: undo-kde puts it back)
    record = load_record()
    recorded = False
    if OURS.match(current(cfg / "kdeglobals", "General", "ColorScheme")):
        recorded |= remember(record, "kdeglobals", cfg / "kdeglobals", groups, ("General", "ColorScheme"))
        changed |= update_ini(cfg / "kdeglobals", groups)
    # KColorSchemeManager also reads Dolphin's per-app scheme, independently
    # of the Qt style and kdeglobals. Keep both on the same complete palette.
    if OURS.match(current(cfg / "dolphinrc", "UiSettings", "ColorScheme")):
        dolphin = {"UiSettings": {"ColorScheme": scheme.stem}}
        recorded |= remember(record, "dolphinrc", cfg / "dolphinrc", dolphin, ("UiSettings", "ColorScheme"))
        changed |= update_ini(cfg / "dolphinrc", dolphin)
    # Qt apps using qt6ct must use the same palette as KDE widgets.
    def color(group, key):
        # (a scheme without that group, Tooltip or Selection say: the
        # window's colours, then the view's)
        value = (groups.get("Colors:" + group, {}).get(key) or groups.get("Colors:Window", {}).get(key)
                 or groups["Colors:View"].get(key) or groups["Colors:View"]["ForegroundNormal"])
        if value.startswith("#"):
            return value
        return "#" + "".join(f"{int(v):02x}" for v in value.split(",")[:3])
    fg = color("Window", "ForegroundNormal")
    bg = color("Window", "BackgroundNormal")
    colors = [fg, color("Button", "BackgroundNormal"), bg, bg, bg, bg,
              color("View", "ForegroundNormal"), fg, color("Button", "ForegroundNormal"),
              color("View", "BackgroundNormal"), bg, "#000000",
              color("Selection", "BackgroundNormal"), color("Selection", "ForegroundNormal"),
              groups["Colors:View"].get("ForegroundLink", fg), groups["Colors:View"].get("ForegroundVisited", fg),
              color("View", "BackgroundAlternate"), "#000000", color("Tooltip", "BackgroundNormal"),
              color("Tooltip", "ForegroundNormal"), color("Window", "ForegroundInactive"), color("Selection", "BackgroundNormal")]
    # Link colors can be RGB triples too.
    for i, key in [(14, "ForegroundLink"), (15, "ForegroundVisited")]:
        if key in groups["Colors:View"]:
            colors[i] = color("View", key)
    disabled = colors.copy()
    for i in [0, 6, 8, 19]:
        disabled[i] = colors[20]
    qt_palette = cfg / "qt6ct/colors/TesseraDesktop.conf"
    # qt6ct: only when no palette is chosen, or one of ours (Tessera.conf from
    # Match apps, or this one)
    chosen = current(cfg / "qt6ct/qt6ct.conf", "Appearance", "color_scheme_path")
    if not chosen or Path(chosen).name.startswith("Tessera"):
        qt6ct = {"Appearance": {"custom_palette": "true", "color_scheme_path": str(qt_palette)}}
        recorded |= remember(record, "qt6ct", cfg / "qt6ct/qt6ct.conf", qt6ct, ("Appearance", "color_scheme_path"))
        changed |= update_ini(qt_palette, {"ColorScheme": {"active_colors": ", ".join(colors), "inactive_colors": ", ".join(colors), "disabled_colors": ", ".join(disabled)}})
        changed |= update_ini(cfg / "qt6ct/qt6ct.conf", qt6ct)
    if recorded:
        try:
            save_record(record)
        except OSError:
            pass  # (the sync goes on; an uninstall then leaves these as they are)
    # Existing KDE apps that watch the desktop palette can reload it.
    if changed and shutil.which("gdbus"):
        subprocess.run(["gdbus", "emit", "--session", "--object-path", "/KGlobalSettings", "--signal", "org.kde.KGlobalSettings.notifyChange", "0", "0"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, check=False)


# GTK's themes Tessera switches between; a theme of your own is left alone.
# GTK 3 has no "Adwaita-dark" theme of that name (21 Nov: the name GTK 3 was
# given, so every GTK 3 dialog, VS Code's and the pickers', came up light):
# its dark Adwaita is Adwaita plus gtk-application-prefer-dark-theme.
# adw-gtk3 (the package adw-gtk-theme) takes Tessera's colours from
# gtk-3.0/tessera-colors.css, so it's used when it's installed.
GTK_OURS = re.compile(r"^(|Adwaita|Adwaita-dark|adw-gtk3|adw-gtk3-dark)$")
# An older Tessera set MacTahoe (audit #963): switched away once, then yours
MACTAHOE = re.compile(r"^MacTahoe")
MOVED_KEY = "gtkMacTahoeMoved"


def tessera_config():
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "tessera" / "config.json"


def mactahoe_moved():
    """The once-only MacTahoe switch done (or never needed: no settings yet,
    a new install that never had an older Tessera's theme)"""
    try:
        c = json.loads(tessera_config().read_text())
    except (OSError, ValueError):
        return True
    a = c.get("appearance") if isinstance(c, dict) else None
    return isinstance(a, dict) and a.get(MOVED_KEY) is True


def mark_mactahoe_moved():
    """Marked in the settings, under the shell's own lock (settings_file.py)"""
    if not tessera_config().exists():
        return
    try:
        import importlib.util
        spec = importlib.util.spec_from_file_location("settings_file", Path(__file__).resolve().parent / "settings_file.py")
        sf = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(sf)
        sf.edit(lambda c: sf.put(c, "appearance." + MOVED_KEY, True), path=str(tessera_config()))
    except (OSError, ImportError, AttributeError):
        pass


def gtk_ours(name, moved=None):
    """A GTK theme Tessera may switch: one of its own, or (until the
    once-only switch is done) a MacTahoe one an older Tessera set"""
    if GTK_OURS.match(name):
        return True
    if MACTAHOE.match(name):
        return not (mactahoe_moved() if moved is None else moved)
    return False


def gtk_theme_dirs():
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    return [data / "themes", Path.home() / ".themes", Path(os.environ.get("TESSERA_SYSTEM_THEMES", "/usr/share/themes"))]


def gtk_theme(dark):
    if any((d / "adw-gtk3").is_dir() for d in gtk_theme_dirs()):
        return "adw-gtk3-dark" if dark else "adw-gtk3"
    return "Adwaita"


def apply_gtk(mode):
    """GTK 3 and 4 apps (and the GTK and GNOME pickers) in light or dark:
    the theme, and GTK's own settings.ini for apps that don't read gsettings"""
    dark = mode == "dark"
    cfg = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    got = gsettings("get", "org.gnome.desktop.interface", "gtk-theme")
    now = (got or "").strip().strip("'")
    ini3 = cfg / "gtk-3.0" / "settings.ini"
    mine = current(ini3, "Settings", "gtk-theme-name")
    moved = mactahoe_moved()
    if not moved:
        mark_mactahoe_moved()  # (the once-only switch: this sync)
    if not gtk_ours(now, moved) or not gtk_ours(mine, moved):
        return  # (a theme you picked: yours)
    theme = gtk_theme(dark)
    if got is not None and now != theme:
        gsettings("set", "org.gnome.desktop.interface", "gtk-theme", theme)
    prefer = "true" if dark else "false"
    update_ini(ini3, {"Settings": {"gtk-theme-name": theme, "gtk-application-prefer-dark-theme": prefer}})
    update_ini(cfg / "gtk-4.0" / "settings.ini", {"Settings": {"gtk-application-prefer-dark-theme": prefer}})


def gsettings(*args):
    """gsettings' answer (stdout), or None when it isn't there, fails or
    hangs (no session bus yet at login, a stuck dconf): the sync goes on
    without it instead of stopping with a traceback"""
    if not shutil.which("gsettings"):
        return None
    try:
        r = subprocess.run(["gsettings", *args], capture_output=True, text=True, timeout=5, check=False)
    except (OSError, subprocess.SubprocessError):
        return None
    return r.stdout if r.returncode == 0 else None


def undo_gtk():
    """tessera uninstall: GTK as it was before Tessera (its colour files and
    their imports gone, its theme and settings.ini keys back to GTK's
    defaults; a theme of your own stays)"""
    cfg = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    for v in ("gtk-3.0", "gtk-4.0"):
        d = cfg / v
        (d / "tessera-colors.css").unlink(missing_ok=True)
        css = d / "gtk.css"
        if css.exists():
            rest = "".join(l for l in css.read_text().splitlines(True) if "tessera-colors" not in l)
            if rest.strip():
                css.write_text(rest)
            else:
                css.unlink()
        ini = d / "settings.ini"
        if ini.exists() and gtk_ours(current(ini, "Settings", "gtk-theme-name")):
            text = re.sub(r"^(gtk-theme-name|gtk-application-prefer-dark-theme)=.*\n?", "", ini.read_text(), flags=re.M)
            if re.sub(r"\[Settings\]", "", text).strip():
                ini.write_text(text)
            else:
                ini.unlink()
    got = gsettings("get", "org.gnome.desktop.interface", "gtk-theme")
    if got is not None and gtk_ours(got.strip().strip("'")):
        gsettings("reset", "org.gnome.desktop.interface", "gtk-theme")


def undo_kde():
    """tessera uninstall: Qt and KDE apps' colours as they were before
    Tessera: each key it changed back to what it held (or taken out), in
    kdeglobals, dolphinrc and qt6ct.conf, unless another scheme was chosen
    since; its own qt6ct palette goes (audit #962)"""
    cfg = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    record = load_record()
    before = record.get("before") if isinstance(record.get("before"), dict) else {}
    changed = False
    k = before.get("kdeglobals")
    if isinstance(k, dict) and OURS.match(current(cfg / "kdeglobals", "General", "ColorScheme")):
        changed |= put_back(cfg / "kdeglobals", k)
    dl = before.get("dolphinrc")
    if isinstance(dl, dict) and OURS.match(current(cfg / "dolphinrc", "UiSettings", "ColorScheme")):
        changed |= put_back(cfg / "dolphinrc", dl)
    q = before.get("qt6ct")
    chosen = current(cfg / "qt6ct/qt6ct.conf", "Appearance", "color_scheme_path")
    if isinstance(q, dict) and (not chosen or Path(chosen).name.startswith("Tessera")):
        changed |= put_back(cfg / "qt6ct/qt6ct.conf", q)
    if not current(cfg / "qt6ct/qt6ct.conf", "Appearance", "color_scheme_path").endswith("TesseraDesktop.conf"):
        (cfg / "qt6ct/colors/TesseraDesktop.conf").unlink(missing_ok=True)
    record.pop("before", None)
    try:
        if record:
            save_record(record)
        else:
            state_path().unlink(missing_ok=True)
    except OSError:
        pass
    if changed and shutil.which("gdbus"):
        subprocess.run(["gdbus", "emit", "--session", "--object-path", "/KGlobalSettings", "--signal", "org.kde.KGlobalSettings.notifyChange", "0", "0"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, check=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["dark", "light", "undo-gtk", "undo-kde", "gtk-ours"])
    parser.add_argument("name", nargs="?", default="")
    parser.add_argument("--scheme", type=Path)
    parser.add_argument("--keep-kde", action="store_true")
    args = parser.parse_args()
    if args.mode == "undo-gtk":
        undo_gtk()
        return
    if args.mode == "undo-kde":
        undo_kde()
        return
    if args.mode == "gtk-ours":
        sys.exit(0 if gtk_ours(args.name) else 1)
    gsettings("set", "org.gnome.desktop.interface", "color-scheme", "prefer-" + args.mode)
    apply_gtk(args.mode)
    if not args.keep_kde:
        apply_kde(args.mode, args.scheme)


if __name__ == "__main__":
    main()
