#!/usr/bin/env python3
"""Apple's macOS wallpapers, taken back out of an older install (24 Nov).

Until 3 November the installer offered Apple's wallpapers (yes by default),
fetched from a community mirror by tools/get-wallpapers.sh into
~/Pictures/Wallpapers/macOS: "<Name> Light.jpg" and "<Name> Dark.jpg" for
Tahoe, Sequoia, Sonoma, Ventura, Monterey, Big Sur, Catalina and Mojave, and
a "Tahoe Beach" folder of four pictures through the day. Settings then listed
them as wallpaper sets. They're Apple's (all rights reserved), so Tessera no
longer fetches them, and this removes the copies it put there.

What it removes, and nothing else: a file at exactly one of those paths
whose size and SHA-256 are the downloaded file's (KNOWN, below). A file
there that's different (yours, or another download), a link, or anything
else in the folders stays where it is; the two folders go only when that
leaves them empty. Settings that pointed at a picture or folder that's gone
(wallpaper.path, lightPath, darkPath, dynamicFolder) go back to empty, which
is Tessera's own built-in set (shell/assets/wallpapers, made by
tools/gen-wallpapers.sh); a time-of-day folder that's gone leaves the built-in
set through the day. Settings are written under the shell's own lock (an
flock on the settings folder, as cfgfile.rs).

Each start of the shell runs it (tessera-ui), and the installer and Software
Update; with nothing left to do it does nothing.

    apple-wallpapers.py [--dry-run]
prints what it removed or changed (one line each); never fails the caller.
"""
import argparse
import hashlib
import json
import os
import sys

FOLDER = os.path.join("Pictures", "Wallpapers", "macOS")  # (under your home)
TIME_SET = "Tahoe Beach"

# (path in FOLDER, size, SHA-256) of each picture get-wallpapers.sh fetched
KNOWN = [
    ("Big Sur Dark.jpg", 599907, "64b9e83e07ec66525d6586b4d52379fd03b293f60d6811fd29827c3f4ae78e1c"),
    ("Big Sur Light.jpg", 1083336, "2bef0fdc3b967cc9175aef9ed8b4b7b853fff004231dd3d5561fcbc4b315b1e7"),
    ("Catalina Dark.jpg", 593050, "cd485960cd978d348414bfb64e6809762602fc4e1175cd6f36302a858b72374e"),
    ("Catalina Light.jpg", 736414, "81240bc5a49ab99c4be42386891105a23a92591e29c52569358eccf2844974ed"),
    ("Mojave Dark.jpg", 211856, "4bb3eca34f0ea07d64ba41eea9eaa939a14d08e45df9c4ece6122814c49a802b"),
    ("Mojave Light.jpg", 225635, "3d285c39c65e8c0e97805e6d775e914ac17a91671342ecac78c301913092de71"),
    ("Monterey Dark.jpg", 107625, "93fd8a09599412d81d2ad2f6e7fc92c567e2e00863b716fb9d8e9e994cdb327c"),
    ("Monterey Light.jpg", 128162, "be465f72647e28439ae1eeafb7a5cd90ee1fbb30edd214fe8732dacef52501e7"),
    ("Sequoia Dark.jpg", 177463, "ca2537d9d1dd2250488429785c485a37b376def0a66d39f2948578462e452111"),
    ("Sequoia Light.jpg", 190087, "d069b4f2ea24fda8a78fb340a614a3a39f313929d6c153d4d2362891fa892943"),
    ("Sonoma Dark.jpg", 159761, "57f8801f0d74a4b1e6ac1c35f264a533774ec13a4e3aef263a4db129079f7223"),
    ("Sonoma Light.jpg", 191925, "56a4c280882884e22a1248d3e0bc0df692a40a0294ef72f83fd1ed290636afab"),
    ("Tahoe Beach/1 Dawn.jpg", 257112, "368b8c96772458d4a16e222ae28118fe40aac6eb5955ae1bc53de4d81cdf39ee"),
    ("Tahoe Beach/2 Day.jpg", 407141, "847ba5fa2e1bbf25bf77cd88f2afcf6dd76d8addaa2b533cdbf42349095d7e54"),
    ("Tahoe Beach/3 Dusk.jpg", 311023, "f40314464b656f93d5a6275de70cb8379d3f4638677abae29570a96ba8e41ada"),
    ("Tahoe Beach/4 Night.jpg", 146413, "1decd385cc819eb6b5d6ae33fbb8c4feb8c82d3168e2ee693fb86b7f703fc8fc"),
    ("Tahoe Dark.jpg", 195232, "65a9738b3853e7feebe488cf1db5ba0123acb556e112fb8d32365efeb6093723"),
    ("Tahoe Light.jpg", 219461, "3e4e9136d9d86391d290e961d991b4fa9cfe84fa17344c88dc8506f4b4f605d1"),
    ("Ventura Dark.jpg", 159354, "dbbfe059f951c4d24a2e2f1ab49b74ca547fb7e321636aa97e80a6ae84c0c0d5"),
    ("Ventura Light.jpg", 162025, "85fd5977ec9648629a1a0c86a3606fc11902d56a69fcccde721bf3a10e8a5fe5"),
]
PICTURE_KEYS = ("path", "lightPath", "darkPath")  # (wallpaper.*)


def home():
    return os.path.expanduser("~")


def folder():
    return os.path.join(home(), FOLDER)


def config_path():
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(home(), ".config")
    return os.path.join(base, "tessera", "config.json")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def is_download(path, size, digest):
    """The file get-wallpapers.sh put there: a plain file (not a link) of
    that exact size and hash."""
    try:
        if os.path.islink(path) or not os.path.isfile(path) or os.path.getsize(path) != size:
            return False
        return sha256(path) == digest
    except OSError:
        return False


def remove_downloads(dry):
    """Removes the downloaded pictures; returns their paths."""
    gone = []
    base = folder()
    if not os.path.isdir(base) or os.path.islink(base):
        return gone
    for rel, size, digest in KNOWN:
        p = os.path.join(base, rel)
        if is_download(p, size, digest):
            if not dry:
                try:
                    os.remove(p)
                except OSError as e:
                    print(f"apple-wallpapers: can't remove {p}: {e}", file=sys.stderr)
                    continue
            gone.append(p)
    # (the folders only when nothing else is in them)
    for d in (os.path.join(base, TIME_SET), base):
        if not dry and os.path.isdir(d) and not os.path.islink(d):
            try:
                os.rmdir(d)
                gone.append(d)
            except OSError:
                pass
    return gone


def known_paths():
    """Every path get-wallpapers.sh made (its pictures and the two folders)."""
    base = folder()
    paths = {os.path.join(base, rel) for rel, _, _ in KNOWN}
    paths |= {base, os.path.join(base, TIME_SET)}
    return paths


def norm(p):
    if not isinstance(p, str) or not p:
        return ""
    return os.path.normpath(os.path.expanduser(p[7:] if p.startswith("file://") else p))


def fixed_settings(c, gone):
    """The settings in `c` (config.json) that point at a picture or folder
    of the download that isn't there any more, set back to the built-in
    set's empty value; returns the keys changed."""
    w = c.get("wallpaper") if isinstance(c, dict) else None
    if not isinstance(w, dict):
        return []
    known = known_paths()
    missing = lambda p: p in known and (p in gone or not os.path.exists(p))
    changed = []
    for k in PICTURE_KEYS:
        if missing(norm(w.get(k))):
            w[k] = ""
            changed.append("wallpaper." + k)
    if missing(norm(w.get("dynamicFolder"))):
        w["dynamicFolder"] = ""
        changed.append("wallpaper.dynamicFolder")
    return changed


def settings_file():
    """settings_file.py beside this: the shell's own settings lock and write"""
    import importlib.util
    here = os.path.dirname(os.path.realpath(__file__))
    spec = importlib.util.spec_from_file_location("settings_file", os.path.join(here, "settings_file.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def fix_settings(gone, dry):
    path = config_path()
    if not os.path.isfile(path):
        return []
    SF = settings_file()
    with SF.Lock(path):
        try:
            with open(path, encoding="utf-8") as f:
                c = json.load(f)
        except (OSError, ValueError):
            return []  # (a broken file is the shell's to deal with)
        if not isinstance(c, dict):
            return []
        changed = fixed_settings(c, gone)
        if changed and not dry:
            # (whole, through a temporary file; the shell watches it)
            SF.write_whole(path, SF.text(c))
    return changed


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    opts = ap.parse_args(argv)
    try:
        gone = remove_downloads(opts.dry_run)
        for p in gone:
            print(("would remove " if opts.dry_run else "removed ") + p)
        for k in fix_settings(set(gone), opts.dry_run):
            print(("would reset " if opts.dry_run else "reset ") + k + " (the built-in set)")
    except Exception as e:  # (never fails the shell's start or an update)
        print(f"apple-wallpapers: {e}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
