#!/usr/bin/python3 -I
"""Everything Tessera does as root, as one fixed program: installed (by the
next update, or the installer) to /usr/lib/tessera/tessera-root, owned by root,
with what it runs next to it, so nothing it starts lives in your home.
Started through pkexec; each subcommand is a polkit action of its own
(org.tessera.root.<subcommand>, matched on the first argument:
shell/assets/polkit/org.tessera.root.policy).

Without a password, for an administrator (wheel) at this computer
(shell/assets/polkit/49-tessera.rules):
  wallpaper <path>   copies one image (JPEG, PNG or WebP, up to 64 MB, read
                     as you) to /usr/share/tessera/wallpaper.jpg, the login
                     screen's copy
  login-look <json>  the login screen's look (Settings → Lock Screen → Login
                     screen) as /usr/share/tessera/greeter.json, which it can
                     read (your home it can't): a style, the look, light or
                     dark, Material's colours and the clock's style, each
                     checked against its few values; nothing else is taken
  fix <id>          one of the fixed fixes below (system services,
                     Bluetooth's pairing prompts); nothing free-form ever
                     runs. (fix firewall, from shells before sharing-ports:
                     the same as sharing-ports open)
  sharing-ports open [<set>...] | close
                     the firewall's way in for nearby sharing, only while
                     it's needed (services/SharingPorts.qml): open lets in
                     exactly these of the fixed sets in PORT_SETS (all of
                     them without one), close none. Runtime rules where the
                     firewall has them, so a restart starts closed; both
                     also take out the permanent rules the fix before this
                     left
  wifi-helper        Quick Share's Wi-Fi helper (qs-wifi-helper.py, next to
                     this file): JSON on stdin and stdout
  priority on|off    the shell's priority with ananicy-cpp: its fixed rule
                     in /etc/ananicy.d (on), or none and a marker in
                     /etc/tessera that keeps updates from putting it back (off)
With the password:
  fingerprint on|off polkit|sudo|login
      a finger is (or isn't) accepted for password prompts (polkit), sudo, or
      the login screen (greetd, ly, SDDM): one marked line, `auth sufficient
      pam_fprintd.so`, first in that PAM file (greetd, ly, SDDM, LightDM or
      LXDM for the login screen; a vendor file in /usr/lib/pam.d
      is copied to /etc/pam.d first). "sufficient": no finger in time, or no
      pam_fprintd, and the password is asked as before. The login screen
      takes a finger only after a password login this boot, while that
      keyring still runs (/usr/lib/tessera/fingerprint-login), so the
      keyring is always opened by the password. Tessera's own lock
      screen needs none of this (its PAM file is in the shell).
  face setup <camera> | add | clear | list
      face unlock for Tessera's lock screen, with Howdy (howdy-next): setup
      downloads Howdy's face models (once) and sets the camera, infrared or
      regular (a /dev/v4l/by-path, /dev/v4l/by-id or /dev/videoN path that
      exists) in /etc/howdy/config.ini; add, clear and list the faces of the
      person who asked (never anyone else's; clear also ends admin below).
      Tessera's lock screen uses its own PAM file; no system PAM file is
      touched.
  face admin on <camera> ir | face admin off
      opt-in, infrared cameras only: your face approves administrator
      prompts too. Refused unless Howdy's camera is that camera and it's
      infrared (a regular camera never approves an administrator). A drop-in
      for polkit's helper service (polkit-agent-helper@.service.d/50-tessera-
      face.conf) gives it the camera and shows it a copy of its PAM file with
      `auth sufficient pam_howdy.so` first, the rest (a fingerprint, the
      password) as it was. polkit's own PAM file isn't changed; off removes
      the drop-in, and so does setting up a regular camera.
  unhold
      Tessera's package hold gone (update --lock's IgnorePkg): /etc/pacman.d/
      tessera-hold.conf, its two marked lines in /etc/pacman.conf and the
      pacman.conf.pre-tessera copy. uninstall.sh runs it; the next update
      with a lock holds them again.
  phonecam on|off
      a virtual camera for an Android phone's camera (Settings → Phone): the
      v4l2loopback module with one device, /dev/video42 "Phone camera",
      loaded now and at every start (modprobe.d and modules-load.d drop-ins
      named tessera-phonecam.conf); off removes them and unloads it when no app
      is using it.
  login-switch on <shell folder> <look json> | login-switch off [<before>]
      Tessera's login screen instead of the one this computer has (SDDM,
      GDM, LightDM, ly or none), from the next start: greetd, the greeter
      user, the login screen's copy in /usr/share/tessera with the look,
      /etc/greetd/config.toml (kept as .pre-tessera), the keyring at login,
      then the one before off and greetd on. The one before is kept in
      /etc/tessera/login-switch.json; if Tessera's fails at the next start
      (greetd fails, or the login screen doesn't show), the one before comes
      back by itself (login-fallback, login-check: systemd runs them). off:
      the one before again (<before>: the one install.sh noted, when there's
      no record). See "login-switch" below.
  boot-check [--repair]
      will the next login work? (the end of every update runs it, and
      Settings → Health's Fix, as fix login): the login screen's PAM files
      (its session starts, pam_setcred only; the password's path, read),
      greetd's config, its command and user, Hyprland, Tessera's login
      screen's files. --repair: Tessera's own part in anything wrong undone
      (its marked PAM lines out, or the file before its edit back; greetd's
      text login instead of a login screen that can't start), checked
      again. JSON on stdout, kept in /var/lib/tessera/boot-check.json
  rescue [undo|status]
      from a console when there's no login screen (Ctrl+Alt+F2, log in,
      sudo /usr/lib/tessera/tessera-root rescue): Tessera's lines out of the
      login screens' PAM files, greetd's text login (agreety) instead of
      Tessera's login screen, greetd started again. undo: all of it back,
      once the real Linux-PAM finds no problem with it (the Restore button
      the shell shows); status: the note (/var/lib/tessera/rescue/note.json)
  login-rescue
      systemd's own (tessera-login-rescue.service, when greetd.service
      fails for good): one step further each time in a boot, Tessera's
      lines out, then greetd's text login, then a console login on tty1
With the password, once, at the start:
  update [--snapshot snapper|timeshift] [--system] [--ignore a,b]
         [--install a,b] [--remove a,b] [--downgrade f,g] [--mirrors l,m] [--setup <shell folder>]
         [--lock <file>] [--broker]
      the whole system part of an update (sysupdate.py starts it): snapshot,
      this helper's own files (--setup), pacman -Syu, Tessera's packages,
      dropped ones. --mirrors: these mirror lists are behind (sysupdate.py
      asks tools/pac.sh), so before pacman -Syu they're re-ranked by fixed
      commands (cachyos-rate-mirrors, else reflector), each old list kept
      as <list>.tessera-backup; never a reason to stop. Only Arch's
      /etc/pacman.d/mirrorlist and CachyOS's cachyos*-mirrorlist are taken.
      "@@step" lines say where it is; exit codes say which
      step failed (20 snapshot, 21 system, 22 install, 23 remove, 24
      downgrade). --downgrade: going back to before an update, pacman -U of
      earlier package files, only from pacman's cache, your yay/paru cache
      or Arch's archive (archive.archlinux.org). Then
      "@@root-done", and with --broker it stays for the AUR step: the AUR
      helper builds as you and runs `tessera-sudo pacman …` for the root
      parts, which sysupdate.py passes here over stdin (a socket only the
      update has). Only pacman runs, from /usr/bin, its arguments checked
      (check_pacman); output and exit code go back the same way. It ends
      when sysupdate.py closes the socket, so nothing outlives the update.
      --lock <file>: the package lock of the Tessera version being installed
      (a bundle's, tools/tessera-bundle.py: read as you, every name and
      version checked here): the held packages (only the Hyprland family
      and Quickshell, HELD_FAMILY) put at exactly their tested version, from
      the repositories when they have it, else Arch's archive
      (archive.archlinux.org, the address made here from the checked name
      and version), then held there (IgnorePkg, in /etc/pacman.d/tessera-
      hold.conf, which /etc/pacman.conf's [options] includes, one marked
      line) until the next Tessera version moves them. Minimum versions are
      a system update's (--system). Written first, so the system update
      never moves a held package; exit 25 when the moves fail.
      --features <ids> [the features' options below]: after the AUR step
      (sysupdate.py asks for it over the same socket, {"features": true}),
      or straight after the packages without one, the features below; the
      same password
  features <ids> [--face-camera <camera>] [--fprintd-wait]
           [--login-look <json>] [--shell <folder>]
      what root does for Tessera's features (features.py; the installer's
      Features step, with sudo): a comma list of fingerprint (pam_fprintd
      for password prompts, sudo and the login screen; --fprintd-wait: the
      "fprintd" fix first), face (Howdy's models and that camera, as face
      setup), bluetooth (bluetoothd's Experimental = true in main.conf, kept
      once as .pre-tessera, and the "btpairing" fix), power
      (power-profiles-daemon on), quickshare (quickshare-trust on, Avahi
      on), login (login-switch on, with --shell and --login-look) and
      bootsplash (Tessera's first boot animation, where Plymouth is in the
      boot image). Each on its own: "@@feature <id> ok" or "@@feature <id>
      failed <why>", the rest going on; exit 1 when any failed
  quickshare-trust on|off
      Quick Share's Wi-Fi helper starts without the password for you (a
      polkit rule naming you, at this computer, in the session on screen);
      off removes it. /etc/tessera/quickshare-trusted says it's on
  bootmenu on|off
      snapshots in GRUB's boot menu: grub-btrfs's daemon (grub-btrfsd), on
      or off; on also writes the submenu now
  bootsplash breathe|orbit|frost|bloom|system|none
      the boot animation (Plymouth's theme; Settings → Lock & Login Screen):
      one of Tessera's (copied from /usr/lib/tessera/plymouth, where setup put
      them, to /usr/share/plymouth/themes/tessera-<id>), the system's own (the
      theme set before Tessera first changed it, kept in
      /etc/tessera/bootsplash.json) or none (Plymouth's text theme); then the
      boot image is rebuilt (mkinitcpio -P). The boot images are copied
      first: if the rebuild fails, they and the theme setting are put back,
      and the error says why
  setup <shell folder>
      the same files as --setup, on their own (install.sh, with sudo), and
      the boot animations (assets/plymouth) into /usr/lib/tessera/plymouth
      Both also refresh the login screen's own copy of Tessera
      (/usr/share/tessera/shell and material, and greeter.lua; only where
      ./install.sh --greeter made one) from the same version: the shell
      folder's checkout, or the update's staged one (<folder>/tree)
      Tessera's old name: setup also moves what the helper installed under
      it to Tessera's names (the section "Tessera's old name" below, the
      one place here that knows it)

Files are read as you (the person who started pkexec or sudo), never as
root: a symlink in your home can't make root read anything you can't.
"""
import base64
import contextlib
import glob
import hashlib
import importlib.util
import json
import os
import pwd
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

# ── Tessera's old name ───────────────────────────────────────────────────
# Tessera was called iRice until 4 November 2026. This section is the one
# place in the root helper that knows that name: what the helper installed
# under it, which setup moves to Tessera's names once (it puts Tessera's
# helper in place first, so there's never a moment without one), and what
# other subcommands still recognise until then. (The helper is one file,
# installed as root's own: it never loads the user side's migrate-names.py.)
# REMOVE AFTER 2027-02-01, with migrate-names.py, old-names.sh, core's
# old_names.rs and common/OldNames.qml (docs/rename-to-tessera.md).
OLD_LOGIN_SHARE = "/usr/share/irice"  # (until ./install.sh --only=login installs Tessera's)
OLD_ANANICY_RULE = "/etc/ananicy.d/irice.rules"
OLD_DROPIN = "/etc/systemd/system/bluetooth.service.d/irice-no-rap.conf"
OLD_FP_MARK = "# iRice fingerprint"  # (a PAM line marked under the old name: still ours)
OLD_PORTS_TAG = "irice sharing"  # (sharing ports opened under the old name: closed alike)
UFW_OLD = ("iRice sharing", "iRice sharing (LocalSend)", "iRice Quick Share hotspot (DHCP)")
OLD_FIREWALLD_ADDED = "/run/irice/sharing-ports"
NFT_OLD = ("/etc/nftables.d/irice.nft", "/etc/nftables.d/irice.conf")
OLD_BOOTSPLASH_RECORD = "/etc/irice/bootsplash.json"
OLD_PLYMOUTH_PREFIX = "irice-"
OLD_SESSION_ENTRY = "/usr/share/wayland-sessions/irice.desktop"
SESSION_ENTRY = "/usr/share/wayland-sessions/tessera.desktop"
OLD_BACKUPS = ("/etc/greetd/config.toml.pre-irice", "/etc/default/grub.pre-irice", "/etc/pam.d/*.pre-irice")
OLD_LIB = "/usr/lib/irice"
OLD_FILES = ("/usr/share/polkit-1/actions/org.irice.root.policy", "/etc/polkit-1/rules.d/49-irice.rules",
             "/etc/dbus-1/system.d/zzz-irice-quickshare.conf")  # (that one: left by a Wi-Fi helper that died)
OLD_QS_TRUST_RULE = "/etc/polkit-1/rules.d/49-irice-quickshare.rules"
OLD_FOLDERS = (("/etc/irice", "/etc/tessera"), ("/var/lib/irice", "/var/lib/tessera"))
# Drop-ins and module options: the same content under Tessera's name
RENAMED = (
    (OLD_DROPIN, "/etc/systemd/system/bluetooth.service.d/tessera-no-rap.conf"),
    ("/etc/systemd/system/fprintd.service.d/irice-timeout.conf", "/etc/systemd/system/fprintd.service.d/tessera-timeout.conf"),
    ("/etc/systemd/system/plymouth-quit.service.d/irice.conf", "/etc/systemd/system/plymouth-quit.service.d/tessera.conf"),
    ("/etc/modprobe.d/irice-phonecam.conf", "/etc/modprobe.d/tessera-phonecam.conf"),
    ("/etc/modules-load.d/irice-phonecam.conf", "/etc/modules-load.d/tessera-phonecam.conf"),
)


def move_old_folders(prefix):
    """/etc/irice and /var/lib/irice into Tessera's (what's in both stays
    Tessera's). Returns what it moved."""
    done = []
    for old, new in OLD_FOLDERS:
        o, n = (os.path.join(prefix, p.lstrip("/")) for p in (old, new))
        if os.path.islink(o) or not os.path.isdir(o):
            continue
        os.makedirs(n, mode=0o755, exist_ok=True)
        for name in os.listdir(o):
            if not os.path.lexists(os.path.join(n, name)):
                os.rename(os.path.join(o, name), os.path.join(n, name))
        shutil.rmtree(o, ignore_errors=True)
        done.append(new)
    return done


def migrate_old_names(prefix, run=subprocess.run, src=None):
    """Everything else the helper left under the old name, now that
    Tessera's is installed. Returns what it wrote or removed."""
    def at(path):
        return os.path.join(prefix, path.lstrip("/"))
    done = session_entry(src, prefix)
    # Backups the installer kept (greetd's, GRUB's, PAM's): the new suffix,
    # unless Tessera's own is there already
    for pattern in OLD_BACKUPS:
        for p in glob.glob(at(pattern)):
            n = p[:-len(".pre-irice")] + ".pre-tessera"
            if not os.path.lexists(n) and not os.path.islink(p):
                os.rename(p, n)
                done.append(n)
    # Quick Share's trust rule, for the same person, for Tessera's helper
    old_rule = at(OLD_QS_TRUST_RULE)
    if os.path.exists(old_rule):
        user = (read_text(at(QS_TRUST_MARK)) or "").strip()
        if not user:
            m = re.search(r'subject\.user === "([^"]+)"', read_text(old_rule) or "")
            user = m.group(1) if m else ""
        with contextlib.suppress(Refused):
            write_root(at(QS_TRUST_RULE), quickshare_trust_rule(user).encode(), 0o644)
            write_root(at(QS_TRUST_MARK), (user + "\n").encode(), 0o644)
            done.append(QS_TRUST_RULE)
        os.unlink(old_rule)
        done.append(OLD_QS_TRUST_RULE)
    # Drop-ins and module options: renamed, same content
    reload = False
    for old, new in RENAMED:
        o, n = at(old), at(new)
        if os.path.isfile(o) and not os.path.islink(o):
            if not os.path.exists(n):
                with open(o, "rb") as f:
                    write_root(n, f.read().replace(b"iRice", b"Tessera").replace(b"irice uninstall", b"tessera uninstall"), 0o644)
                done.append(new)
            os.unlink(o)
            done.append(old)
            reload = reload or "/systemd/" in old
    # Fingerprint lines in PAM files: marked with Tessera's name
    for names in FP_FILES.values():
        for name in names:
            p = os.path.join(at(PAM_ETC), name)
            text = read_text(p)
            if text and OLD_FP_MARK in text:
                text = text.replace(OLD_FP_MARK, FP_MARK)
                write_root(p, text.encode(), 0o644)
                done.append(os.path.join(PAM_ETC, name))
    # The login screen's finger from before 22 Nov (any time, so the
    # keyring stayed locked): now only after a password login this boot;
    # and every marked line as this helper writes it today
    done += fingerprint_refresh(prefix)
    # The boot animation in use, under its new name (the boot image keeps
    # the old copy, the same animation, until it's next rebuilt)
    current = current_theme(prefix)
    if current.startswith(OLD_PLYMOUTH_PREFIX) and current[len(OLD_PLYMOUTH_PREFIX):] in BOOTSPLASH_IDS:
        target = "tessera-" + current[len(OLD_PLYMOUTH_PREFIX):]
        src = at(os.path.join(PLYMOUTH_LIB, target))
        try:
            files = theme_files(src)
        except (OSError, Refused):
            files = {}
        conf = at(PLYMOUTH_CONF)
        text = read_text(conf) or ""
        if f"{target}.plymouth" in files and re.search(r"^\s*Theme\s*=\s*" + re.escape(current) + r"\s*$", text, re.M):
            write_tree(files, at(os.path.join(PLYMOUTH_THEMES, target)))
            write_root(conf, re.sub(r"^(\s*Theme\s*=\s*)" + re.escape(current) + r"(\s*)$", r"\g<1>" + target + r"\g<2>", text, flags=re.M).encode(), 0o644)
            done += [os.path.join(PLYMOUTH_THEMES, target), PLYMOUTH_CONF]
            try:
                record = json.loads(read_text(at(BOOTSPLASH_RECORD)) or "null")
            except ValueError:
                record = None
            if isinstance(record, dict) and record.get("applied") == current:
                record["applied"] = target
                write_root(at(BOOTSPLASH_RECORD), json.dumps(record).encode(), 0o644)
    # The old name's boot animations, unless one is still the one in use
    current = current_theme(prefix)
    for id in BOOTSPLASH_IDS:
        old = at(os.path.join(PLYMOUTH_THEMES, OLD_PLYMOUTH_PREFIX + id))
        if OLD_PLYMOUTH_PREFIX + id != current and os.path.isdir(old) and not os.path.islink(old):
            shutil.rmtree(old, ignore_errors=True)
            done.append(os.path.join(PLYMOUTH_THEMES, OLD_PLYMOUTH_PREFIX + id))
    # The old helper, its polkit action and rule, its ananicy rule
    for old in OLD_FILES + (OLD_ANANICY_RULE,):
        if os.path.exists(at(old)):
            os.unlink(at(old))
            done.append(old)
    if os.path.isdir(at(OLD_LIB)) and not os.path.islink(at(OLD_LIB)):
        shutil.rmtree(at(OLD_LIB), ignore_errors=True)
        done.append(OLD_LIB)
    if reload and prefix == "/":
        with contextlib.suppress(OSError, subprocess.SubprocessError):
            run(["systemctl", "daemon-reload"], timeout=60, check=False)
    return done


def session_entry(src, prefix):
    """The login screen's entry for a Tessera run from a checkout (tools/
    setup-session.sh): the old name's (irice.desktop, starting
    sandbox/irice-session.sh) becomes Tessera's, and an entry that starts the
    checkout through its old folder name (~/iRice, a link to ~/tessera once
    moved) starts it by its own path. Only for the checkout setup reads from
    (<src>/..), and only an entry that already starts that same checkout's
    session script: nothing new is offered at the login screen. Returns what
    it wrote or removed."""
    if not src:
        return []
    checkout = os.path.dirname(os.path.realpath(src))
    script = os.path.join(checkout, "sandbox", "tessera-session.sh")
    if not os.path.isfile(script) or "\n" in script or "\r" in script:
        return []
    done = []
    for entry in (OLD_SESSION_ENTRY, SESSION_ENTRY):
        p = os.path.join(prefix, entry.lstrip("/"))
        text = read_text(p)
        m = re.search(r"^Exec=(.*)$", text or "", re.M)
        if not m:
            continue
        exe = m.group(1).strip()
        if os.path.basename(exe) not in ("tessera-session.sh", "irice-session.sh"):
            continue
        if os.path.realpath(os.path.dirname(exe)) != os.path.dirname(script):
            continue  # (another checkout's: not this setup's to change)
        if entry == SESSION_ENTRY and exe == script:
            continue
        new = os.path.join(prefix, SESSION_ENTRY.lstrip("/"))
        write_root(new, ("[Desktop Entry]\nName=Tessera\n"
                         f"Comment=Tessera, the macOS-style Hyprland desktop, from {checkout}\n"
                         f"Exec={script}\nType=Application\nDesktopNames=Hyprland\n").encode(), 0o644)
        done.append(SESSION_ENTRY)
        if entry == OLD_SESSION_ENTRY:
            os.unlink(p)
            done.append(OLD_SESSION_ENTRY)
    return done


LIB = "/usr/lib/tessera"
PACMAN = "/usr/bin/pacman"
LOGIN_SHARE = "/usr/share/tessera"  # the login screen's (./install.sh --greeter)
LOGIN_WALLPAPER = LOGIN_SHARE + "/wallpaper.jpg"
LOGIN_LOOK = LOGIN_SHARE + "/greeter.json"


def login_share(prefix="/"):
    """The login screen's copy in use: Tessera's, else the old name's (still
    the one greetd starts until the login screen is installed again)."""
    new, old = (os.path.join(prefix, p.lstrip("/")) for p in (LOGIN_SHARE, OLD_LOGIN_SHARE))
    return old if not os.path.isdir(new) and os.path.isdir(old) else new
MAX_IMAGE = 64 * 1024 * 1024
MAX_SOURCE = 1024 * 1024
PREFIX = "/"  # where FILES go (tests: a folder)

# What setup installs: from the shell's folder (the repo's shell/, or its
# installed copy) → where, with which mode
FILES = [
    ("assets/scripts/tessera-root.py", LIB + "/tessera-root", 0o755),
    ("assets/scripts/quickshare/qs-wifi-helper.py", LIB + "/qs-wifi-helper.py", 0o644),
    ("assets/polkit/org.tessera.root.policy", "/usr/share/polkit-1/actions/org.tessera.root.policy", 0o644),
    ("assets/polkit/49-tessera.rules", "/etc/polkit-1/rules.d/49-tessera.rules", 0o644),
]
# ananicy-cpp's rule that keeps the shell ("tessera-shell", started so by
# launch-shell.sh) interactive, like GNOME Shell and KWin (CachyOS files "qs"
# as a background service otherwise). Fixed here, never read from a folder:
# `priority on` runs without a password, so it writes nothing it's given.
# Installed where ananicy-cpp is (its /etc/ananicy.d), unless turned off in
# Settings → Battery & Power (the marker).
ANANICY_RULE = "/etc/ananicy.d/tessera.rules"
ANANICY_TEXT = b"""# Tessera's shell: interactive, so panels and animations stay smooth while
# apps are busy (Tessera's root helper; Settings > Battery & Power turns it off).
# tessera-ui: the shell itself since it runs standalone (rust.standalone)
{ "name": "tessera-shell", "type": "LowLatency_RT" }
{ "name": "tessera-ui", "type": "LowLatency_RT" }
"""
PRIORITY_OFF = "/etc/tessera/priority-off"


# The boot animations (tools/make-bootsplash.py): setup copies each theme's
# folder from the shell's assets/plymouth to LIB/plymouth, root's own, the
# only place bootsplash copies a theme from
BOOTSPLASH_IDS = ("breathe", "orbit", "frost", "bloom")
PLYMOUTH_LIB = LIB + "/plymouth"
THEME_FILE = re.compile(r"^([a-z0-9]+@[12]x\.png|tessera-[a-z]+\.(script|plymouth))$")
MAX_THEME_FILES = 400


class Refused(Exception):
    """Arguments this helper won't act on; the message says why."""


REFUSED_EXIT = 64  # (an older helper without that subcommand or fix: callers ask as before)


# ── The fixes (System health, Settings → Sharing) ───────────────────────
# BlueZ plugins that make phones ask to pair for Quick Share (sharing-check.py)
# rap: Ranging; vcp: volume (with volume offset, audio input); micp: microphone;
# mcp: media control; bass: broadcast audio scan
PLUGINS = ("rap", "vcp", "micp", "mcp", "bass")
DROPIN = "/etc/systemd/system/bluetooth.service.d/tessera-no-rap.conf"
PAIRING_SCRIPT = r'''set -e
NOPLUGIN=@PLUGINS@
exe=$(systemctl show -p ExecStart --value bluetooth | sed -n 's/.*path=\([^ ;]*\).*/\1/p')
[ -x "$exe" ] || exe=/usr/lib/bluetooth/bluetoothd
mkdir -p /etc/systemd/system/bluetooth.service.d
printf '# Tessera: no Ranging or LE Audio control profiles, which make phones ask to pair for Quick Share\n[Service]\nExecStart=\nExecStart=%s --noplugin=%s\n' "$exe" "$NOPLUGIN" > /etc/systemd/system/bluetooth.service.d/tessera-no-rap.conf
systemctl daemon-reload
systemctl restart bluetooth
'''
PAIRING_SCRIPT = PAIRING_SCRIPT.replace("@PLUGINS@", ",".join(PLUGINS))
PAIRING_SHOWN = f"bluetoothd --noplugin={','.join(PLUGINS)} (a drop-in in {os.path.dirname(DROPIN)}), then restart Bluetooth"

# NetworkManager, unless iwd or systemd-networkd runs the network instead:
# switching over is a choice (the installer offers it), not a fix
NETWORK_SCRIPT = r'''set -e
if ! systemctl is-enabled --quiet NetworkManager; then
    for s in iwd systemd-networkd; do
        if systemctl is-active --quiet "$s"; then
            echo "$s runs your network; Tessera's installer can switch it to NetworkManager" >&2
            exit 1
        fi
    done
fi
systemctl enable --now NetworkManager
'''
NETWORK_SHOWN = "systemctl enable --now NetworkManager"

# fprintd stopped while Broadcom's driver updates the reader's firmware (about
# a minute, on its first start): longer to start, then started (this waits
# for it)
FPRINTD_SCRIPT = r'''set -e
mkdir -p /etc/systemd/system/fprintd.service.d
printf '%s\n' '# Tessera: a fingerprint reader may update its firmware as fprintd starts' \
    '[Service]' 'TimeoutStartSec=5min' 'TimeoutStopSec=2min' > /etc/systemd/system/fprintd.service.d/tessera-timeout.conf
systemctl daemon-reload
systemctl reset-failed fprintd || true
systemctl start fprintd
'''

FIXES = {
    "networkmanager": ["/bin/sh", "-c", NETWORK_SCRIPT],
    "bluetooth": ["systemctl", "enable", "--now", "bluetooth"],
    "avahi": ["systemctl", "enable", "--now", "avahi-daemon"],
    "upower": ["systemctl", "enable", "--now", "upower"],
    "profiles": ["systemctl", "enable", "--now", "power-profiles-daemon"],
    "btpairing": ["/bin/sh", "-c", PAIRING_SCRIPT],
    "fprintd": ["/bin/sh", "-c", FPRINTD_SCRIPT],
    # (Settings → Health → Login screen: the boot check, and Tessera's own
    # part in anything wrong undone; boot_check_and_repair)
    "login": [LIB + "/tessera-root", "boot-check", "--repair"],
}


# ── Acting as the person who asked ──────────────────────────────────────
def caller_uid():
    """Who started pkexec (or sudo, for install.sh); root if neither."""
    for key in ("PKEXEC_UID", "SUDO_UID"):
        v = os.environ.get(key, "")
        if v.isdigit():
            return int(v)
    return 0


@contextlib.contextmanager
def as_user(uid):
    """Reads files with that person's rights (effective ids only, given back
    afterwards). Unprivileged (tests), it changes nothing."""
    if os.geteuid() != 0 or uid == 0:
        yield
        return
    pw = pwd.getpwuid(uid)
    groups = os.getgroups()
    os.setgroups(os.getgrouplist(pw.pw_name, pw.pw_gid))
    os.setegid(pw.pw_gid)
    os.seteuid(uid)
    try:
        yield
    finally:
        os.seteuid(0)
        os.setegid(0)
        os.setgroups(groups)


def read_as(uid, path, limit):
    """A regular file's bytes, read as uid (never a FIFO or a device)."""
    with as_user(uid):
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        if not (st.st_mode & 0o170000 == 0o100000):
            raise Refused(f"{path} isn't a regular file")
        if st.st_size > limit:
            raise Refused(f"{path} is too big")
        data = b""
        while len(data) <= limit:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            data += chunk
        if len(data) > limit:
            raise Refused(f"{path} is too big")
        return data
    finally:
        os.close(fd)


def write_root(dest, data, mode):
    """Replaces dest whole (a temporary file next to it, then a rename), so
    nothing ever reads half a file; parents are made 0755 when missing."""
    d = os.path.dirname(dest)
    os.makedirs(d, mode=0o755, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=d, prefix=".tessera-")
    try:
        os.fchmod(fd, mode)
        os.write(fd, data)
        os.fsync(fd)
        os.close(fd)
        fd = -1
        os.rename(tmp, dest)
    except BaseException:
        if fd >= 0:
            os.close(fd)
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise


# ── fingerprint (Settings → Lock Screen → Fingerprint) ─────────────────
FP_MARK = "# Tessera fingerprint"
# max-tries / timeout: a finger, or the password after a few seconds (PAM
# asks one thing at a time; the login screen waits longest). Five tries: some
# readers (Broadcom's) answer "no match" at once for a finger still resting
# on them, which used up two tries before a real touch
# Every line Tessera adds to a PAM file starts with "-": Linux-PAM then
# doesn't log a module that isn't installed (fprintd, pam-u2f or Howdy
# removed later). The dash changes only the logging: a missing module gives
# PAM_MODULE_UNKNOWN either way, so each line's control must already make
# that harmless ("sufficient", "optional", or a gate's default=1 skip), and
# the password's own lines, which follow, decide (pam_problem checks it with
# the real Linux-PAM before any edit is written)
FP_LINES = {"polkit": "-auth      sufficient   pam_fprintd.so max-tries=5 timeout=10   " + FP_MARK,
            "sudo": "-auth      sufficient   pam_fprintd.so max-tries=5 timeout=10   " + FP_MARK,
            "login": "-auth      sufficient   pam_fprintd.so max-tries=5 timeout=15   " + FP_MARK}
# The login screen takes a finger only once your password has opened the
# keyring this boot (a finger gives PAM no password, so the login keyring
# stayed locked and apps asked for it: 22 Nov). FP_GATE checks: a password
# login since boot (its mark in /run, made as the session opens) and your
# keyring still running (a full logout ends it: the password again). Asked
# first; no: the finger's line is skipped and the password is asked.
FP_GATE = "/usr/lib/tessera/fingerprint-login"
FP_GATE_TEXT = b"""#!/bin/sh
# Tessera (tessera-root.py fingerprint on login): the login screen's finger,
# only after a password login this boot, while that keyring still runs.
#   check: may a finger log PAM_USER in?   mark: a session opened
uid=$(/usr/bin/id -u -- "$PAM_USER" 2>/dev/null) || exit 1
case "$1" in
  check) [ -e "/run/tessera-login/$uid" ] && [ -S "/run/user/$uid/keyring/control" ] ;;
  mark) /usr/bin/mkdir -p -m 0700 /run/tessera-login && : > "/run/tessera-login/$uid" ;;
  *) exit 1 ;;
esac
"""
# A gate's control: skip the next line unless the gate says yes. Only while
# authenticating (type=auth), and "ignore=ignore": pam_setcred without an
# authentication first (greetd starting its greeter, autologin) otherwise
# came back PAM_IGNORE through the skip, and greetd wouldn't start the login
# screen at all (22 Nov: system1 couldn't log out or boot; pam_start_confdir
# reproduces it, tests/test_root_helper.py)
GATE_CONTROL = "-auth      [success=ignore ignore=ignore default=1] pam_exec.so quiet type=auth "
# (the line before 22 Nov's fix, rewritten on update: migrate_old_names;
# and every marked line in the form before the dash: pam_refresh)
OLD_GATE_CONTROL = "auth       [success=ignore default=1] pam_exec.so quiet "
FP_LOGIN_HEAD = [GATE_CONTROL + FP_GATE + " check   " + FP_MARK]
FP_LOGIN_TAIL = ["-session   optional     pam_exec.so quiet " + FP_GATE + " mark   " + FP_MARK]
# (GDM isn't here: it asks for a finger by itself, gdm-fingerprint, once one is added)
FP_FILES = {"polkit": ["polkit-1"], "sudo": ["sudo"], "login": ["greetd", "ly", "sddm", "lightdm", "lxdm"]}
PAM_ETC, PAM_VENDOR = "/etc/pam.d", "/usr/lib/pam.d"
PAM_FPRINTD = ("/usr/lib/security/pam_fprintd.so", "/lib/security/pam_fprintd.so")


def pam_without_fingerprint(text):
    return "".join(l for l in text.splitlines(keepends=True) if FP_MARK not in l and OLD_FP_MARK not in l)


def pam_with_fingerprint(text, line, head=(), tail=()):
    """The line (after HEAD's) first among the PAM file's rules (after its
    header comments and blank lines), TAIL's last, any earlier ones of ours
    gone."""
    lines = pam_without_fingerprint(text).splitlines(keepends=True)
    at = 0
    while at < len(lines) and (lines[at].strip() == "" or lines[at].lstrip().startswith("#")):
        at += 1
    if lines and not lines[-1].endswith("\n"):
        lines[-1] += "\n"
    return "".join(lines[:at] + [l + "\n" for l in (*head, line)] + lines[at:] + [l + "\n" for l in tail])


def pam_login_fingerprint(text):
    return pam_with_fingerprint(text, FP_LINES["login"], FP_LOGIN_HEAD, FP_LOGIN_TAIL)


PAM_SUCCESS, PAM_SYSTEM_ERR, PAM_PERM_DENIED = 0, 4, 6
PAM_IGNORE, PAM_MODULE_UNKNOWN = 25, 28
PAM_WORDS = {4: "a system error", 6: "permission denied", 25: "PAM_IGNORE (nothing decided)", 28: "a module that isn't there"}
PAM_CHECK_TIMEOUT = 30  # s; a module that hangs is "couldn't tell"


def pam_confdir(d, etc, vendor=PAM_VENDOR):
    """A copy of the PAM files as PAM reads them: the package's
    (/usr/lib/pam.d), then /etc's over them"""
    for src_dir in (vendor, etc):
        try:
            names = os.listdir(src_dir)
        except OSError:
            continue
        for f in names:
            src = os.path.join(src_dir, f)
            if os.path.isfile(src):
                shutil.copy(src, os.path.join(d, f))


# Unprivileged (tests), PAM runs only when a test turns this on, and only on
# PAM files whose every module is a stand-in or isn't installed at all
PAM_STAND_INS = False
STAND_IN_MODULES = ("pam_permit.so", "pam_deny.so", "pam_exec.so")


def stand_ins_only(d):
    """Every module the PAM files in D name is a stand-in (pam_exec only
    running /bin/true or /bin/false) or isn't installed"""
    for f in os.listdir(d):
        text = read_text(os.path.join(d, f)) or ""
        for kind, control, module, _ in pam_rules(text):
            if control in ("include", "substack"):
                continue
            base = os.path.basename(module)
            if base not in STAND_IN_MODULES and any(os.path.exists(os.path.join(p, base)) for p in (
                    "/usr/lib/security", "/lib/security", "/usr/lib64/security", "/lib64/security",
                    "/usr/lib/x86_64-linux-gnu/security")):
                return False
        for line in text.splitlines():
            line = line.split("#", 1)[0]
            if "pam_exec.so" in line and not re.search(r"pam_exec\.so(\s+(quiet|type=\w+))*\s+/(usr/)?bin/(true|false)(\s|$)", line):
                return False
    return True


def pam_setcred_code(etc, name, text, user="root", vendor=PAM_VENDOR, timeout=PAM_CHECK_TIMEOUT):
    """The real Linux-PAM's pam_setcred, with no authentication first
    (greetd starting its greeter, autologin), on a copy of the PAM files
    with TEXT as NAME (pam_start_confdir, Linux-PAM 1.4+), in a child
    process (a module that crashes or hangs can't take this one with it).
    Only pam_setcred, ever: no password, finger, key or face is asked, no
    failure is counted (pam_authenticate never runs here). Returns PAM's
    code, or None when it couldn't be tried."""
    import ctypes
    if os.geteuid() != 0 and not PAM_STAND_INS:
        return None  # (not root: a test, which runs it only on stand-ins)
    try:
        ctypes.CDLL("libpam.so.0").pam_start_confdir
    except (OSError, AttributeError):
        return None
    with tempfile.TemporaryDirectory(prefix="tessera-pam-") as d:
        try:
            pam_confdir(d, etc, vendor)
            with open(os.path.join(d, name), "w") as f:
                f.write(text)
        except OSError:
            return None
        if os.geteuid() != 0 and not stand_ins_only(d):
            raise AssertionError("a test's PAM files name a real module: stand-ins only (pam_permit, pam_deny, pam_exec)")
        r, w = os.pipe()
        pid = os.fork()
        if pid == 0:  # (the child: PAM, its code down the pipe, nothing else)
            try:
                os.close(r)
                devnull = os.open(os.devnull, os.O_RDWR)
                for fd in (0, 1, 2):
                    os.dup2(devnull, fd)
                os.write(w, str(_pam_setcred_child(d, name, user)).encode())
            finally:
                os._exit(0)
        os.close(w)
        import select
        out = b""
        deadline = time.monotonic() + timeout
        try:
            while time.monotonic() < deadline:
                ready, _, _ = select.select([r], [], [], max(0.0, deadline - time.monotonic()))
                if not ready:
                    break
                chunk = os.read(r, 64)
                if not chunk:
                    break
                out += chunk
        finally:
            os.close(r)
            with contextlib.suppress(OSError):
                if os.waitpid(pid, os.WNOHANG) == (0, 0):
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
        try:
            code = int(out)
        except ValueError:
            return None
        return None if code < 0 else code


def _pam_setcred_child(d, name, user):
    import ctypes
    pam = ctypes.CDLL("libpam.so.0")

    class Conv(ctypes.Structure):
        _fields_ = [("conv", ctypes.c_void_p), ("appdata", ctypes.c_void_p)]
    cb_t = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)
    cb = cb_t(lambda n, m, r, a: 19)  # PAM_CONV_ERR: nobody to ask, nothing is asked
    conv = Conv(ctypes.cast(cb, ctypes.c_void_p), None)
    h = ctypes.c_void_p()
    if pam.pam_start_confdir(name.encode(), user.encode(), ctypes.byref(conv), d.encode(), ctypes.byref(h)) != 0:
        return -1
    code = pam.pam_setcred(h, 2)  # PAM_ESTABLISH_CRED
    pam.pam_end(h, code)
    return code


def login_screen_starts(etc, name, text, user="root", vendor=PAM_VENDOR):
    """Whether TEXT, as /etc/pam.d/NAME, still lets a login screen start
    its session without a password first (greetd's greeter, autologin):
    pam_setcred on a copy of the PAM files. False only on a clear failure;
    True when it can't be tried."""
    r = pam_setcred_code(etc, name, text, user, vendor)
    # 25 PAM_IGNORE (the 22 Nov fault), 4 PAM_SYSTEM_ERR, 6 PAM_PERM_DENIED
    return r not in (PAM_IGNORE, PAM_SYSTEM_ERR, PAM_PERM_DENIED, PAM_MODULE_UNKNOWN)


# ── the password's path, read (never run) ───────────────────────────────
# Whether a PAM file still gets to the password, worked out from the text
# alone: its auth lines (includes and substacks followed, as Linux-PAM lays
# them out) walked by Linux-PAM's own rules for each control, with every
# Tessera-marked line taking each answer it could give (success, failure,
# not installed, PAM_IGNORE, an error) and everything else succeeding.
# Unless one of Tessera's lines let the person in (a finger in time: its
# "sufficient" success), the walk must go through exactly the lines it goes
# through without Tessera's, no failure of Tessera's counted on the way:
# Tessera's lines are then invisible to the password. Nothing is executed,
# so nothing is asked, woken or counted (pam_faillock).
TESSERA_MARKS = (FP_MARK, OLD_FP_MARK, "# Tessera passkey", "# Tessera face")
PAM_CONTROLS = {
    "required": {"success": "ok", "new_authtok_reqd": "ok", "ignore": "ignore", "default": "bad"},
    "requisite": {"success": "ok", "new_authtok_reqd": "ok", "ignore": "ignore", "default": "die"},
    "sufficient": {"success": "done", "new_authtok_reqd": "done", "default": "ignore"},
    "optional": {"success": "ok", "new_authtok_reqd": "ok", "default": "ignore"},
}
TESSERA_ANSWERS = ("success", "auth_err", "module_unknown", "ignore", "authinfo_unavail", "system_err")
PASSWORD_MODULES = ("pam_unix.so", "pam_sss.so", "pam_ldap.so", "pam_krb5.so", "pam_systemd_home.so")


def pam_rules(text):
    """(type, control, module, marked) for each rule in a PAM file's text
    (continued lines joined, comments out, a bracketed control kept whole)"""
    out = []
    after_mark = False  # (the keyring's line: its mark is the comment above it)
    for raw in re.sub(r"\\\n", " ", text).splitlines():
        marked = any(m in raw for m in TESSERA_MARKS) or (after_mark and "pam_gnome_keyring" in raw)
        after_mark = "# tessera: unlock the keyring" in raw
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("@include"):
            parts = line.split()
            if len(parts) > 1:
                out.append(("auth", "include", parts[1], marked))
            continue
        m = re.match(r"^-?(\w+)\s+(\[[^\]]*\]|\S+)\s+(\S+)", line)
        if m:
            out.append((m.group(1).lower(), m.group(2), m.group(3), marked))
    return out


def pam_auth_chain(name, texts, depth=0, marked=False):
    """NAME's auth rules with includes and substacks laid out in place:
    [(control dict, module, marked)]. texts: name → text."""
    if depth > 16:
        raise ValueError("includes nest too deep")
    chain = []
    for kind, control, module, mark in pam_rules(texts.get(name) or ""):
        if kind != "auth":
            continue
        if control in ("include", "substack"):
            chain += pam_auth_chain(module, texts, depth + 1, marked or mark)
            continue
        if control.startswith("["):
            c = {}
            for pair in control[1:-1].split():
                k, _, v = pair.partition("=")
                c[k.lower()] = v.lower()
        else:
            c = PAM_CONTROLS.get(control.lower())
            if c is None:
                raise ValueError(f"an unknown control: {control}")
        chain.append((c, os.path.basename(module), marked or mark))
    return chain


def pam_walk(chain, answers):
    """Linux-PAM's walk of an auth chain (each rule answering
    answers.get(i, "success")): (the plain rules it goes through, whether a
    Tessera rule failed it, whether a Tessera rule's success ended it)"""
    seen, failed, i = [], False, 0
    while i < len(chain):
        c, module, marked = chain[i]
        answer = answers.get(i, "success")
        action = c.get(answer, c.get("default", "bad"))
        if not marked:
            seen.append(i)
        if action in ("bad", "die"):
            failed = failed or marked
            if action == "die":
                break
        elif action == "done":
            if answer == "success" and marked:
                return seen, failed, True
            break
        elif action.isdigit():
            i += int(action) + 1
            continue
        i += 1
    return seen, failed, False


def password_path_problem(etc, name, text, vendor=PAM_VENDOR):
    """Why the password could be cut off by Tessera's lines in TEXT (as
    ETC/NAME), or "" (see above: read, never run)"""
    texts = {}
    for d in (vendor, etc):
        try:
            for f in os.listdir(d):
                p = os.path.join(d, f)
                if os.path.isfile(p):
                    texts[f] = read_text(p) or ""
        except OSError:
            continue
    texts[name] = text
    try:
        chain = pam_auth_chain(name, texts)
    except ValueError as e:
        return f"{name}: {e}"
    ours = [i for i, (_, _, marked) in enumerate(chain) if marked]
    if not ours:
        return ""
    plain = [i for i, (_, _, marked) in enumerate(chain) if not marked]
    # (what everything is compared to: the walk without Tessera's lines)
    want = [plain[j] for j in pam_walk([chain[i] for i in plain], {})[0]]
    if any(chain[i][1] in PASSWORD_MODULES for i in plain) and not any(chain[i][1] in PASSWORD_MODULES for i in want):
        return ""  # (the system's own stack never gets to it: not Tessera's to judge)
    import itertools
    for combo in itertools.product(TESSERA_ANSWERS, repeat=min(len(ours), 5)):
        answers = dict(zip(ours, combo))
        seen, failed, let_in = pam_walk(chain, answers)
        if let_in:
            continue
        if failed:
            return f"{name}: one of Tessera's lines would count as a failed password"
        if seen != want:
            return f"{name}: Tessera's lines would skip some of the password's own lines"
    return ""


def password_reached(etc, name, text=None, vendor=PAM_VENDOR):
    """Whether NAME's auth rules, read (never run), get to a password
    module (pam_unix and the like) when everything before it succeeds and
    Tessera's lines don't let anyone in: the boot check's "a password still
    works". None when the file can't be read."""
    texts = {}
    for d in (vendor, etc):
        with contextlib.suppress(OSError):
            for f in os.listdir(d):
                if os.path.isfile(os.path.join(d, f)):
                    texts[f] = read_text(os.path.join(d, f)) or ""
    if text is not None:
        texts[name] = text
    if name not in texts:
        return None
    try:
        chain = pam_auth_chain(name, texts)
    except ValueError:
        return False
    if not any(module in PASSWORD_MODULES for _, module, marked in chain if not marked):
        return None  # (no module this knows asks for it: not for this to judge)
    answers = {i: "module_unknown" for i, c in enumerate(chain) if c[2]}
    # (a password module that succeeds ends the walk too: each one's place
    # is what counts, so each is tried as the first to answer)
    for i, (c, module, marked) in enumerate(chain):
        if module in PASSWORD_MODULES and not marked:
            if i in pam_walk(chain, answers)[0]:
                return True
            answers[i] = "auth_err"
    return False


def pam_problem(etc, name, text, login=None, user="root", vendor=PAM_VENDOR):
    """Why TEXT, as /etc/pam.d/NAME, would lock people out, or "" when it
    wouldn't (or it can't be told). Every file: the password's path, read
    from the text (password_path_problem). A login screen's (login: NAME is
    one by default) also: its session must start without a password first
    (greetd's greeter, autologin): pam_setcred, the one call made to the
    real Linux-PAM (nothing is asked by it)."""
    login = name in FP_FILES["login"] + ["greetd-greeter"] if login is None else login
    if login:
        r = pam_setcred_code(etc, name, text, user, vendor)
        if r in (PAM_IGNORE, PAM_SYSTEM_ERR, PAM_PERM_DENIED, PAM_MODULE_UNKNOWN):
            return f"{name}: starting a login screen's session gave {PAM_WORDS.get(r, r)}"
    return password_path_problem(etc, name, text, vendor)


PAM_BACKUP = "/var/lib/tessera/pam-backup"


def pam_backup_dir(etc):
    """Where the copies from before Tessera's edits of ETC's files go:
    /var/lib/tessera/pam-backup (under a test's folder, beside its etc)"""
    etc = etc.rstrip("/")
    if etc.endswith("/etc/pam.d"):
        return _at(etc[:-len("/etc/pam.d")] or "/", PAM_BACKUP)
    return os.path.join(os.path.dirname(etc), "pam-backup")


def write_pam(etc, name, text, vendor=PAM_VENDOR, check=True):
    """TEXT as ETC/NAME, only if it locks no one out (pam_problem: refused
    otherwise), the file before kept first: <backup>/NAME (the one before
    the latest edit) and <backup>/NAME.first (before Tessera's first, kept
    for good). Returns the destination."""
    dest = os.path.join(etc, name)
    if check:
        why = pam_problem(etc, name, text, vendor=vendor)
        if why:
            raise Refused(f"{why}: so the change wasn't made")
    before = read_text(dest)
    if before is None:
        before = read_text(os.path.join(vendor, name))
    if before is not None and before != text:
        b = pam_backup_dir(etc)
        os.makedirs(b, mode=0o700, exist_ok=True)
        if not os.path.exists(os.path.join(b, name + ".first")):
            write_root(os.path.join(b, name + ".first"), before.encode(), 0o600)
        write_root(os.path.join(b, name), before.encode(), 0o600)
    write_root(dest, text.encode(), 0o644)
    return dest


def refresh_pam_file(etc, name, text, new, vendor=PAM_VENDOR):
    """An update's rewrite of Tessera's marked lines in ETC/NAME (TEXT now,
    NEW as this helper writes them): only when the real Linux-PAM finds no
    problem with NEW (else it stays as it is, and the boot check after the
    update decides about it). Returns True when written."""
    if new == text:
        return False
    try:
        write_pam(etc, name, new, vendor)
        return True
    except Refused as e:
        print(f"tessera-root: {e}", file=sys.stderr)
        return False


def fingerprint_refresh(prefix="/"):
    """setup's part, at each update: the fingerprint's marked lines (and the
    login screen's gate) as this helper writes them today, wherever they're
    on: the lines from before 22 Nov's gate, and those before the dash.
    Each file tried with the real Linux-PAM first (write_pam). Returns what
    it wrote."""
    etc = _at(prefix, PAM_ETC)
    done = []
    for kind, names in FP_FILES.items():
        for name in names:
            text = read_text(os.path.join(etc, name))
            if not text or FP_MARK not in text:
                continue
            new = pam_login_fingerprint(text) if kind == "login" else pam_with_fingerprint(text, FP_LINES[kind])
            if new == text:
                continue
            if kind == "login" and write_fp_gate(_at(prefix, FP_GATE)):
                done.append(FP_GATE)
            if refresh_pam_file(etc, name, text, new, _at(prefix, PAM_VENDOR)):
                done.append(os.path.join(PAM_ETC, name))
    return done


def write_fp_gate(gate=FP_GATE):
    """FP_GATE as it should be; True when written."""
    try:
        with open(gate, "rb") as f:
            if f.read() == FP_GATE_TEXT:
                return False
    except OSError:
        pass
    write_root(gate, FP_GATE_TEXT, 0o755)
    return True


# Features that need hardware never touch the login without it: a reader
# (fingerprint on). Read from /sys and the files the
# drivers keep, never by asking the device (nothing is woken).
FP_HWDB = "/usr/lib/udev/hwdb.d/60-autosuspend-fingerprint-reader.hwdb"
FP_VENDORS = ("27c6", "06cb", "138a", "10a5", "1c7a", "2808", "147e", "08ff", "0a5c")  # (fingerprint.py's makers, and Broadcom's)
FP_STORE = "/var/lib/fprint"


def fingerprint_reader(prefix="/"):
    """A fingerprint reader is plugged in: a USB device systemd's
    fingerprint hwdb lists, or one by a reader maker, or fingers fprintd
    has enrolled (a reader whose driver isn't in libfprint)"""
    ids = set()
    for d in glob.glob(_at(prefix, "/sys/bus/usb/devices/*")):
        v, p = read_text(os.path.join(d, "idVendor")), read_text(os.path.join(d, "idProduct"))
        if v and p:
            ids.add((v.strip().lower(), p.strip().lower()))
    known = {(v.lower(), p.lower()) for v, p in re.findall(r"^usb:v([0-9A-Fa-f]{4})p([0-9A-Fa-f]{4})", read_text(_at(prefix, FP_HWDB)) or "", re.M)}
    if ids & known or any(v in FP_VENDORS for v, _ in ids):
        return True
    return any(files for _, _, files in os.walk(_at(prefix, FP_STORE)))


def fingerprint(on, where, etc=PAM_ETC, vendor=PAM_VENDOR, module=PAM_FPRINTD, gate=FP_GATE, reader=None):
    """Returns the files changed."""
    if where not in FP_FILES:
        raise Refused("fingerprint: polkit, sudo or login")
    if on and not any(os.path.exists(m) for m in module):
        raise Refused("pam_fprintd isn't installed (fprintd)")
    if on and not (fingerprint_reader() if reader is None else reader):
        raise Refused("there's no fingerprint reader here, so no PAM file is changed")
    changed = []
    if on and where == "login" and write_fp_gate(gate):
        changed.append(gate)
    for name in FP_FILES[where]:
        dest = os.path.join(etc, name)
        src = dest if os.path.exists(dest) else os.path.join(vendor, name)
        if not os.path.exists(src):
            continue  # (that login screen isn't installed)
        with open(src) as f:
            text = f.read()
        new = (pam_login_fingerprint(text) if where == "login" else pam_with_fingerprint(text, FP_LINES[where])) if on \
            else pam_without_fingerprint(text)
        # Off, and /etc's copy is the package's file again (it was copied
        # here to turn this on): removed, so the package's updates apply
        vsrc = os.path.join(vendor, name)
        if not on and src == dest and os.path.exists(vsrc):
            with open(vsrc) as f:
                if f.read() == new:
                    os.unlink(dest)
                    changed.append(dest)
                    continue
        if new != text or (on and src != dest):
            # (tried with the real Linux-PAM first: the login screen still
            # starts, a password still reaches the password's check)
            changed.append(write_pam(etc, name, new, vendor))
    return changed


# ── passkeys: removed (Nov 2026) ─────────────────────────────────────────
# Tessera Passkeys (a virtual security key, native/tessera-passkeys) is gone:
# its "sudo and login" lines let any program running as the user approve its
# own sudo (the daemon's check read a PAM file in the user's runtime folder).
# What an older Tessera wrote is taken out at every setup (passkeys_retire):
# the marked pam_u2f lines and their gate in sudo's and the login screens'
# PAM files (each file tried with the real Linux-PAM first), the key list,
# the gate script and the udev rule. tpm2-tools and pam-u2f stay installed
# (other things may use them). The user's sealed passkeys
# (~/.local/share/tessera/passkeys) are never touched here.
PK_RULE = "/etc/udev/rules.d/70-tessera-passkeys.rules"
PK_MARK = "# Tessera passkey"
PK_KEYS = "/etc/tessera/passkeys-u2f"
PK_GATE = "/usr/lib/tessera/passkeys-gate"
PK_FILES = ["sudo"] + FP_FILES["login"]


def pam_without_passkey(text):
    return "".join(l for l in text.splitlines(keepends=True) if PK_MARK not in l)


def passkeys_retire(prefix="/", run=subprocess.run):
    """setup's part, at each update: everything an older Tessera wrote for
    Passkeys taken out. Returns what it removed or rewrote."""
    etc = _at(prefix, PAM_ETC)
    done = []
    for name in PK_FILES:
        text = read_text(os.path.join(etc, name))
        if text and PK_MARK in text and refresh_pam_file(etc, name, text, pam_without_passkey(text), _at(prefix, PAM_VENDOR)):
            done.append(os.path.join(PAM_ETC, name))
    for path in (PK_KEYS, PK_GATE, PK_RULE):
        with contextlib.suppress(FileNotFoundError):
            os.unlink(_at(prefix, path))
            done.append(path)
    if PK_RULE in done and prefix == "/":
        for cmd in (["udevadm", "control", "--reload"], ["udevadm", "trigger", "--action=change", "--subsystem-match=misc", "--sysname-match=uhid"]):
            with contextlib.suppress(OSError, subprocess.SubprocessError):
                run(cmd, capture_output=True, timeout=30)
    return done


# ── face (Settings → Lock Screen → Face) ─────────────────────────────────
HOWDY_CONFIG = "/etc/howdy/config.ini"
CAMERA_PATH = re.compile(r"^/dev/(v4l/by-(path|id)/[\w:.\-]+|video\d{1,3})$")


def howdy_device(text, device):
    """config.ini with [video] device_path set, everything else as it was."""
    lines = text.splitlines()
    out, section, done = [], "", False
    for line in lines:
        m = re.match(r"^\s*\[(.+)\]\s*$", line)
        if m:
            if section == "video" and not done:
                out.append(f"device_path = {device}")
                done = True
            section = m.group(1).strip().lower()
        elif section == "video" and re.match(r"^\s*device_path\s*=", line):
            if not done:
                out.append(f"device_path = {device}")
                done = True
            continue
        out.append(line)
    if not done:
        if section != "video":
            out += ["", "[video]"]
        out.append(f"device_path = {device}")
    return "\n".join(out) + "\n"


def face(args, uid, run=subprocess.run, config=HOWDY_CONFIG, exists=os.path.exists, which=shutil.which,
         models="/usr/share/howdy/models"):
    """Returns the exit status; howdy's own output goes straight through."""
    howdy = which("howdy")
    if not howdy:
        raise Refused("Howdy isn't installed (howdy-next)")
    user = pwd.getpwuid(uid).pw_name
    if args[:1] == ["setup"] and len(args) == 2:
        device = args[1]
        if not CAMERA_PATH.match(device) or not exists(device):
            raise Refused(f"not a camera: {device}")
        if not glob.glob(os.path.join(models, "*.onnx")):
            r = run([howdy, "download-models"])
            if r.returncode != 0:
                raise Refused("Howdy's face models couldn't be downloaded (check the internet connection)")
        try:
            with open(config) as f:
                text = f.read()
        except FileNotFoundError:
            text = ""
        write_root(config, howdy_device(text, device).encode(), 0o644)
        if not is_infrared(device):
            # (a regular camera now: it never approves an administrator)
            with contextlib.suppress(OSError):
                face_admin_off(uid, run=run)
        return 0
    if args == ["add"]:
        return run([howdy, "-U", user, "--plain", "-y", "add", "Tessera " + time.strftime("%Y-%m-%d %H.%M")]).returncode
    if args == ["clear"]:
        with contextlib.suppress(OSError):
            face_admin_off(uid, run=run)
        return run([howdy, "-U", user, "-y", "clear"]).returncode
    if args == ["list"]:
        return run([howdy, "-U", user, "--plain", "list"]).returncode
    if args[:2] == ["admin", "on"] and len(args) == 4:
        for d in face_admin_on(uid, args[2], args[3], exists=exists, which=which):
            print("changed", d)
        return 0
    if args == ["admin", "off"]:
        for d in face_admin_off(uid):
            print("changed", d)
        return 0
    raise Refused("face: setup <camera> | add | clear | list | admin on <camera> ir | admin off")


# ── face for administrator prompts (opt-in: Settings → Face) ───────────
# Infrared cameras only: a regular camera never approves an administrator
# (a video of you could fool it), whatever is asked. polkit's PAM file isn't
# changed. polkit asks through its helper, a systemd service started per
# request (polkit-agent-helper@.service); a drop-in shows that service alone
# our copy of its PAM file, with Howdy first and everything else as it was
# (a fingerprint line, the password: BindReadOnlyPaths, a view, not an
# edit; removing the drop-in is the whole undo).
FACE_ADMIN_DIR = "/var/lib/tessera/face"
FACE_DROPIN = "/etc/systemd/system/polkit-agent-helper@.service.d/50-tessera-face.conf"
POLKIT_HELPER_UNITS = ("/usr/lib/systemd/system/polkit-agent-helper@.service", "/etc/systemd/system/polkit-agent-helper@.service")
POLKIT_PAM = ("/etc/pam.d/polkit-1", "/usr/lib/pam.d/polkit-1")  # (PAM's own order)
FACE_MARK = "# Tessera face"
VIDIOC_ENUM_FMT = 0xC0405602
GREY_FORMATS = {int.from_bytes(c.encode().ljust(4)[:4], "little") for c in ("GREY", "Y8  ", "Y10 ", "Y12 ", "Y16 ", "Y10B", "Y8I ", "Y12I")}
NOT_INFRARED = ("A regular camera never approves administrator prompts (a video of you could fool it): "
                "only an infrared camera, a fingerprint or your password")


def polkit_face_line():
    """Howdy with the infrared camera: a face in time approves; none, and
    what follows (a finger, the password) is asked as before"""
    return f"-auth      sufficient                       pam_howdy.so   {FACE_MARK}"


def polkit_with_face(text):
    """polkit's PAM file with the face first (before its first auth line),
    any face lines of ours from before taken out; nothing else changed"""
    lines = [l for l in text.splitlines() if FACE_MARK not in l]
    at = next((i for i, l in enumerate(lines) if re.match(r"^\s*-?auth\b", l)), len(lines))
    return "\n".join(lines[:at] + [polkit_face_line()] + lines[at:]) + "\n"


def face_dropin(overlay, target):
    return ("# Tessera: face unlock (infrared camera) for administrator prompts (Settings → Face).\n"
            "# The camera, and polkit's PAM file with the face first; delete this file to undo.\n"
            "[Service]\nPrivateDevices=no\nDeviceAllow=char-video4linux rw\n"
            f"BindReadOnlyPaths={overlay}:{target}\n")


def camera_formats(dev):
    """The pixel formats a camera gives (VIDIOC_ENUM_FMT: nothing switched on)"""
    import fcntl
    import struct
    out = []
    try:
        fd = os.open(dev, os.O_RDONLY | os.O_NONBLOCK)
    except OSError:
        return out
    try:
        for i in range(32):
            buf = bytearray(struct.pack("II", i, 1) + bytes(56))
            try:
                fcntl.ioctl(fd, VIDIOC_ENUM_FMT, buf)
            except OSError:
                break
            out.append(struct.unpack_from("I", buf, 44)[0])
    finally:
        os.close(fd)
    return out


def is_infrared(device, sys_root="/sys/class/video4linux", formats=camera_formats):
    """face.py's test, here as root: its name says IR, or it gives only greyscale"""
    node = os.path.join(sys_root, os.path.basename(os.path.realpath(device)))
    label = ""
    for f in ("device/interface", "name"):
        with contextlib.suppress(OSError):
            with open(os.path.join(node, f)) as fh:
                label += " " + fh.read().strip()
    if re.search(r"\bIR\b|infrared", label, re.I):
        return True
    fmts = formats(device)
    return bool(fmts) and all(f in GREY_FORMATS for f in fmts)


def howdy_camera(config=HOWDY_CONFIG):
    """The camera Howdy (so pam_howdy) uses"""
    try:
        with open(config) as f:
            m = re.search(r"^\s*device_path\s*=\s*(\S+)", f.read(), re.M)
    except OSError:
        return ""
    return m.group(1) if m and m.group(1).lower() != "none" else ""


def face_admin_on(uid, device, kind, exists=os.path.exists, which=shutil.which, prefix="/", run=subprocess.run,
                  config=HOWDY_CONFIG, infrared=is_infrared):
    if kind != "ir":
        raise Refused(NOT_INFRARED)
    if not CAMERA_PATH.match(device) or not exists(device):
        raise Refused(f"not a camera: {device}")
    # (pam_howdy uses Howdy's camera: that one must be the infrared one, whatever was asked)
    if howdy_camera(config) != device:
        raise Refused("Howdy is set to another camera: set face unlock up again")
    if not infrared(device):
        raise Refused(NOT_INFRARED)
    root = lambda p: os.path.join(prefix, p.lstrip("/"))
    if not which("howdy"):
        raise Refused("Howdy isn't installed (howdy-next)")
    if not any(os.path.exists(root(u)) for u in POLKIT_HELPER_UNITS):
        raise Refused("this polkit asks with its setuid helper, which can't be given a camera: administrator prompts keep asking for your password")
    target = next((p for p in POLKIT_PAM if os.path.exists(root(p))), "")
    if not target:
        raise Refused("polkit has no PAM file (polkit-1)")
    with open(root(target)) as f:
        text = f.read()
    overlay = f"{FACE_ADMIN_DIR}/polkit-1"
    why = pam_problem(os.path.dirname(root(target)), "polkit-1", polkit_with_face(text), login=False)
    if why:
        raise Refused(f"{why}: so faces don't approve administrator prompts")
    write_root(root(overlay), polkit_with_face(text).encode(), 0o644)
    write_root(root(FACE_DROPIN), face_dropin(overlay, target).encode(), 0o644)
    if prefix == "/":
        run(["systemctl", "daemon-reload"], timeout=30, check=False)
    return [root(overlay), root(FACE_DROPIN)]


def face_admin_refresh(prefix="/", run=subprocess.run):
    """polkit's PAM file changed (a fingerprint turned on or off for
    administrator prompts) while faces approve them: our copy is made again
    from it, so the helper sees that change too (it sees only the copy), and
    the drop-in follows the file PAM reads now (/etc's, or the package's
    again once /etc's copy went). Nothing when face admin is off. Returns
    the files written."""
    root = lambda p: os.path.join(prefix, p.lstrip("/"))
    if not os.path.exists(root(FACE_DROPIN)):
        return []
    target = next((p for p in POLKIT_PAM if os.path.exists(root(p))), "")
    if not target:
        # (no PAM file for polkit at all: nothing to show the helper instead)
        return face_admin_off(prefix=prefix, run=run)
    with open(root(target)) as f:
        text = f.read()
    overlay = f"{FACE_ADMIN_DIR}/polkit-1"
    written = []
    for path, data in ((overlay, polkit_with_face(text)), (FACE_DROPIN, face_dropin(overlay, target))):
        try:
            with open(root(path)) as f:
                same = f.read() == data
        except OSError:
            same = False
        if not same:
            write_root(root(path), data.encode(), 0o644)
            written.append(root(path))
    if prefix == "/" and root(FACE_DROPIN) in written:
        run(["systemctl", "daemon-reload"], timeout=30, check=False)
    return written


def face_admin_off(uid=None, prefix="/", run=subprocess.run):
    """Faces no longer approve administrator prompts: the drop-in and our
    copy go (polkit sees its own PAM file again)"""
    root = lambda p: os.path.join(prefix, p.lstrip("/"))
    gone = []
    for p in (FACE_DROPIN, FACE_ADMIN_DIR + "/polkit-1"):
        if os.path.exists(root(p)):
            os.unlink(root(p))
            gone.append(root(p))
    if prefix == "/" and gone:
        run(["systemctl", "daemon-reload"], timeout=30, check=False)
    return gone


# ── phonecam (Settings → Phone → Use as webcam) ─────────────────────────
PHONECAM_DEV = 42
PHONECAM_OPTIONS = f'options v4l2loopback devices=1 video_nr={PHONECAM_DEV} card_label="Phone camera" exclusive_caps=1\n'
PHONECAM_FILES = ("/etc/modprobe.d/tessera-phonecam.conf", "/etc/modules-load.d/tessera-phonecam.conf")


def phonecam(on, etc="/etc", run=subprocess.run, dev_dir="/dev"):
    """Returns what changed. The module itself comes with the kernel or
    v4l2loopback-dkms (Software Update installs it)."""
    probe, load = (os.path.join(etc, f[len("/etc/"):]) for f in PHONECAM_FILES)
    changed = []
    if on:
        if run(["modinfo", "v4l2loopback"], capture_output=True).returncode != 0:
            raise Refused("v4l2loopback isn't installed (v4l2loopback-dkms)")
        for path, text in ((probe, PHONECAM_OPTIONS), (load, "v4l2loopback\n")):
            write_root(path, text.encode(), 0o644)
            changed.append(path)
        if not os.path.exists(os.path.join(dev_dir, f"video{PHONECAM_DEV}")):
            # loaded before with other options: again, with ours (only when no app has it open)
            run(["modprobe", "-r", "v4l2loopback"], capture_output=True)
            r = run(["modprobe", "v4l2loopback"], capture_output=True, text=True)
            if r.returncode != 0:
                raise Refused("another app is using a virtual camera: close it (or restart), then try again")
            # (modprobe says yes when it was loaded already, with other
            # options, by an app still using it: the camera is checked for)
            run(["udevadm", "settle", "--timeout=5"], capture_output=True)
            if not os.path.exists(os.path.join(dev_dir, f"video{PHONECAM_DEV}")):
                raise Refused("another app's virtual camera is in the way (OBS?): close it, or restart, then try again")
    else:
        for path in (probe, load):
            if os.path.exists(path):
                os.unlink(path)
                changed.append(path)
        run(["modprobe", "-r", "v4l2loopback"], capture_output=True)
    return changed


# ── quickshare-trust (Settings → Sharing → Fast transfers without asking) ─
# Quick Share's Wi-Fi helper (it makes and joins Wi-Fi Direct links) asks
# for the password each time it starts. On: a polkit rule lets it start
# without asking, for the person who turned it on, at this computer, in the
# session on screen; nothing else changes (updates and the rest still ask).
# A marker readable by all says it's on (polkit's rules.d is root's only).
QS_TRUST_RULE = "/etc/polkit-1/rules.d/49-tessera-quickshare.rules"
QS_TRUST_MARK = "/etc/tessera/quickshare-trusted"


def quickshare_trust_rule(user):
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,31}\$?", user or ""):
        raise Refused("unusable user name")
    return f"""// Tessera: Quick Share's Wi-Fi helper starts without the password for {user}
// (Settings → Sharing → Fast transfers without asking; off removes this file)
polkit.addRule(function (action, subject) {{
    if (action.id !== "org.tessera.root.wifi-helper")
        return polkit.Result.NOT_HANDLED;
    if (action.lookup("program") !== "/usr/lib/tessera/tessera-root")
        return polkit.Result.NOT_HANDLED;
    var words = String(action.lookup("command_line") || "").split(" ");
    if (words[0] !== "/usr/lib/tessera/tessera-root" || words[1] !== "wifi-helper" || words.length !== 2)
        return polkit.Result.NOT_HANDLED;
    if (subject.user === "{user}" && subject.active && subject.local)
        return polkit.Result.YES;
    return polkit.Result.NOT_HANDLED;
}});
"""


def quickshare_trust(on, uid, prefix=None):
    """Returns the files changed."""
    prefix = PREFIX if prefix is None else prefix
    rule = os.path.join(prefix, QS_TRUST_RULE.lstrip("/"))
    mark = os.path.join(prefix, QS_TRUST_MARK.lstrip("/"))
    if on:
        try:
            user = pwd.getpwuid(uid).pw_name
        except KeyError:
            raise Refused("unknown user") from None
        if uid == 0:
            raise Refused("turn it on as yourself, not as root")
        write_root(rule, quickshare_trust_rule(user).encode(), 0o644)
        write_root(mark, (user + "\n").encode(), 0o644)
        return [rule, mark]
    changed = []
    for path in (rule, mark):
        if os.path.exists(path):
            os.unlink(path)
            changed.append(path)
    return changed


# ── bootmenu (Settings → Software Update → Going back) ──────────────────
GRUB_BTRFS_SCRIPT = "/etc/grub.d/41_snapshots-btrfs"


def bootmenu(on, run=subprocess.run, which=shutil.which, exists=os.path.exists):
    """Snapshots in GRUB's boot menu: grub-btrfs's daemon (grub-btrfsd)
    keeps a "snapshots" submenu up to date as snapper makes them. On also
    writes the submenu now. Returns what it did."""
    if not which("grub-btrfsd") or not exists(GRUB_BTRFS_SCRIPT):
        raise Refused("grub-btrfs isn't installed")
    if on:
        if run(["systemctl", "enable", "--now", "grub-btrfsd.service"], capture_output=True).returncode != 0:
            raise Refused("grub-btrfsd didn't start")
        run([GRUB_BTRFS_SCRIPT], capture_output=True)
        return ["enabled grub-btrfsd"]
    run(["systemctl", "disable", "--now", "grub-btrfsd.service"], capture_output=True)
    return ["disabled grub-btrfsd"]


def priority(on, prefix=None, run=subprocess.run):
    """The shell's priority with ananicy-cpp (Settings → Battery & Power):
    on writes its fixed rule, off removes it and leaves a marker so updates
    (setup) don't put it back. Returns what changed."""
    prefix = PREFIX if prefix is None else prefix
    rule = os.path.join(prefix, ANANICY_RULE.lstrip("/"))
    marker = os.path.join(prefix, PRIORITY_OFF.lstrip("/"))
    if not os.path.isdir(os.path.dirname(rule)):
        raise Refused("ananicy-cpp isn't installed (its /etc/ananicy.d is missing)")
    changed = []
    old = os.path.join(prefix, OLD_ANANICY_RULE.lstrip("/"))
    if os.path.exists(old):
        os.remove(old)
        changed.append(old)
    if on:
        write_root(rule, ANANICY_TEXT, 0o644)
        changed.append(rule)
        if os.path.exists(marker):
            os.remove(marker)
            changed.append(marker)
    else:
        os.makedirs(os.path.dirname(marker), exist_ok=True)
        write_root(marker, "Turned off in Settings → Battery & Power (Keep Tessera smooth when busy)\n".encode(), 0o644)
        changed.append(marker)
        if os.path.exists(rule):
            os.remove(rule)
            changed.append(rule)
    # ananicy-cpp reads its rules as it starts
    if prefix == "/":
        with contextlib.suppress(OSError, subprocess.SubprocessError):
            run(["systemctl", "try-restart", "ananicy-cpp.service"], timeout=20, check=False)
    return changed


# ── sharing-ports (Settings → Sharing: while you're visible) ────────────
# The firewall's way in for nearby sharing, open only while something needs
# it (services/SharingPorts.qml), closed otherwise: never for good. Fixed
# sets, asked for by name; each (protocol, first port, last port)
PORT_SETS = {
    # Quick Share's port and its faster link's (nearby/wifi.py UPGRADE_PORT),
    # and DHCP for phones joining its hotspot
    "quickshare": (("tcp", 53318, 53319), ("udp", 67, 67)),
    # LocalSend's HTTPS port and its multicast discovery (devices answer an
    # announcement on either)
    "localsend": (("tcp", 53317, 53317), ("udp", 53317, 53317)),
    # mDNS: phones find this computer, and it finds them, on the network
    "mdns": (("udp", 5353, 5353),),
    # A QR-code share (link-share.py: 53320, else 53319) and Cast's file (53320)
    "links": (("tcp", 53319, 53320),),
}
PORTS_TAG = "tessera sharing"  # (nftables' and iptables' comment on ours)
PORTS_TAGS = "(?:" + PORTS_TAG + "|" + OLD_PORTS_TAG + ")"
# ufw keeps its rules through a restart (it has no runtime ones), so ours
# carry a comment and close takes out exactly those (and the tags under the
# old name: UFW_OLD)
UFW_TAG = "Tessera sharing"
UFW_RULE = re.compile(r"^\[\s*(\d+)\]\s+(\S+)(?: \(v6\))?\s+ALLOW(?: IN)?\s.*?(?:#\s*(.*?))?\s*$")
# firewalld's ports have no comment: the ones we added, noted where a
# restart clears it, as it clears them
FIREWALLD_ADDED = "/run/tessera/sharing-ports"
# The fix before this, kept for good: firewalld's ports and services (the
# services only where the zone doesn't come with them), nftables' drop-in
FIREWALLD_OLD = ("53317-53320/tcp", "53317/udp")
FIREWALLD_OLD_SERVICES = ("mdns", "dhcp")


def _cmd(run, *cmd):
    """(exit code, output) of a command; (None, "") when it can't run."""
    try:
        p = run(list(cmd), capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.SubprocessError):
        return None, ""
    return p.returncode, p.stdout or ""


def firewall_kind(run=subprocess.run):
    """The firewall that's on: ufw, firewalld, nftables, iptables or None."""
    if re.search(r"^Status: active", _cmd(run, "ufw", "status")[1], re.M):
        return "ufw"
    if _cmd(run, "firewall-cmd", "--state")[0] == 0:
        return "firewalld"
    for kind in ("nftables", "iptables"):
        if _cmd(run, "systemctl", "is-active", "--quiet", kind)[0] == 0:
            return kind
    return None


def _ports_ufw(wanted, run, did):
    want = {f"{lo}:{hi}/{p}" if hi != lo else f"{lo}/{p}" for p, lo, hi in wanted}
    rules = [(int(m[1]), m[2], m[3] or "") for m in map(UFW_RULE.match, _cmd(run, "ufw", "status", "numbered")[1].splitlines()) if m]
    # Ours that aren't wanted, last first (the numbers after one move up)
    for n, to, note in sorted(rules, reverse=True):
        if (note in (UFW_TAG,) + UFW_OLD or (note == "mDNS" and to == "5353/udp")) and to not in want:
            did(f"closed {to}", "ufw", "--force", "delete", str(n))
    # (a port someone allowed already is left to them)
    for to in sorted(want - {to for _n, to, _note in rules}):
        did(f"opened {to}", "ufw", "allow", to, "comment", UFW_TAG)


def _ports_firewalld(wanted, run, did, prefix):
    want = {f"{lo}-{hi}/{p}" if hi != lo else f"{lo}/{p}" for p, lo, hi in wanted}
    # The fix before this: out of the permanent set, and out of this boot's
    if _cmd(run, "firewall-cmd", "--permanent", f"--query-port={FIREWALLD_OLD[0]}")[0] == 0:
        zone = _cmd(run, "firewall-cmd", "--get-default-zone")[1].strip()
        shipped = ""
        if re.fullmatch(r"[\w-]+", zone):
            with contextlib.suppress(OSError), open(os.path.join(prefix, f"usr/lib/firewalld/zones/{zone}.xml")) as f:
                shipped = f.read()
        old = [f"--remove-port={p}" for p in FIREWALLD_OLD] + \
              [f"--remove-service={s}" for s in FIREWALLD_OLD_SERVICES if f'name="{s}"' not in shipped]
        did("removed the permanent sharing rules", "firewall-cmd", "--permanent", *old)
        did("closed the permanent sharing rules for now", "firewall-cmd", *old)
    path = os.path.join(prefix, FIREWALLD_ADDED.lstrip("/"))
    added = set()
    for p in (path, os.path.join(prefix, OLD_FIREWALLD_ADDED.lstrip("/"))):
        try:
            with open(p) as f:
                added |= set(f.read().split())
        except OSError:
            pass
    with contextlib.suppress(OSError):
        os.remove(os.path.join(prefix, OLD_FIREWALLD_ADDED.lstrip("/")))
    for port in sorted(added - want):
        if did(f"closed {port}", "firewall-cmd", f"--remove-port={port}"):
            added.discard(port)
    for port in sorted(want):
        # (one there already, ours or the zone's own, stays as it is)
        if _cmd(run, "firewall-cmd", f"--query-port={port}")[0] != 0 and did(f"opened {port}", "firewall-cmd", f"--add-port={port}"):
            added.add(port)
    if added:
        write_root(path, "".join(p + "\n" for p in sorted(added)).encode(), 0o644)
    elif os.path.exists(path):
        os.remove(path)


def _nft_input_chains(text):
    """qs-wifi-helper.py's list of nftables' input chains (next to this file
    once installed; in quickshare/ in the shell's folder)."""
    here = os.path.dirname(os.path.realpath(__file__))
    path = next(p for p in (os.path.join(here, "qs-wifi-helper.py"), os.path.join(here, "quickshare/qs-wifi-helper.py"))
                if os.path.exists(p))
    spec = importlib.util.spec_from_file_location("qs_wifi_helper", path)
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper.nft_input_chains(text)


def _ports_nft(wanted, run, did):
    # An accept in a table of our own wouldn't do: nftables runs every base
    # chain on the input hook, and a drop in any of them is final. So ours go
    # first in each input chain (iptables' own INPUT chains are left to it)
    want = {f"{p} dport {lo}-{hi}" if hi != lo else f"{p} dport {lo}" for p, lo, hi in wanted}
    for f, t, c in _nft_input_chains(_cmd(run, "nft", "list", "chains")[1]):
        kept = set()
        for line in _cmd(run, "nft", "-a", "list", "chain", f, t, c)[1].splitlines():
            m = re.search(r'^\s*(.*?) accept comment "' + PORTS_TAGS + r'" # handle (\d+)', line)
            if m and (m[1] not in want or m[1] in kept):
                did(f"closed {m[1]} ({f} {t} {c})", "nft", "delete", "rule", f, t, c, "handle", m[2])
            elif m:
                kept.add(m[1])
        for rule in sorted(want - kept):
            did(f"opened {rule} ({f} {t} {c})", "nft", "insert", "rule", f, t, c, *rule.split(), "accept", "comment", f'"{PORTS_TAG}"')


def _ports_iptables(wanted, run, did):
    want = {f"{p} {lo}:{hi}" if hi != lo else f"{p} {lo}" for p, lo, hi in wanted}
    for ipt in ("iptables", "ip6tables"):
        if ipt == "ip6tables" and _cmd(run, "systemctl", "is-active", "--quiet", "ip6tables")[0] != 0:
            continue
        ours = []
        for line in _cmd(run, ipt, "-L", "INPUT", "-n", "--line-numbers")[1].splitlines():
            m = re.match(r"^(\d+)\s.*\b(tcp|udp) dpts?:(\S+).*/\* " + PORTS_TAGS + r" \*/", line)
            if m:
                ours.append((int(m[1]), f"{m[2]} {m[3]}"))
        # Ours that aren't wanted, last first (the numbers after one move up)
        for n, rule in sorted(ours, reverse=True):
            if rule not in want:
                did(f"closed {rule} ({ipt})", ipt, "-D", "INPUT", str(n))
        for rule in sorted(want - {r for _n, r in ours}):
            p, port = rule.split()
            did(f"opened {rule} ({ipt})", ipt, "-I", "INPUT", "-p", p, "--dport", port,
                "-m", "comment", "--comment", PORTS_TAG, "-j", "ACCEPT")


def sharing_ports(sets, run=subprocess.run, prefix=None):
    """Lets in exactly these sets' ports (none: closes them all), and takes
    out what the permanent fix before this left. Returns (what it did, what
    failed)."""
    for s in sets:
        if s not in PORT_SETS:
            raise Refused("sharing-ports: " + " | ".join(PORT_SETS))
    prefix = PREFIX if prefix is None else prefix
    wanted = sorted({p for s in sets for p in PORT_SETS[s]})
    done, failed = [], []

    def did(what, *cmd):
        ok = _cmd(run, *cmd)[0] == 0
        (done if ok else failed).append(what)
        return ok

    for old in NFT_OLD:
        path = os.path.join(prefix, old.lstrip("/"))
        if os.path.exists(path):
            os.remove(path)
            done.append(f"removed {old}")
    kind = firewall_kind(run)
    if kind == "ufw":
        _ports_ufw(wanted, run, did)
    elif kind == "firewalld":
        _ports_firewalld(wanted, run, did, prefix)
    elif kind == "nftables":
        _ports_nft(wanted, run, did)
    elif kind == "iptables":
        _ports_iptables(wanted, run, did)
    else:
        done.append("no firewall is turned on: nothing to open or close")
    return done, failed
# ── bootsplash (Settings → Lock & Login Screen → Boot animation) ───────
PLYMOUTH_THEMES = "/usr/share/plymouth/themes"
PLYMOUTH_CONF = "/etc/plymouth/plymouthd.conf"
PLYMOUTH_DEFAULTS = "/usr/share/plymouth/plymouthd.defaults"
BOOTSPLASH_RECORD = "/etc/tessera/bootsplash.json"  # {"original": the system's theme, "applied": Tessera's}
INITRAMFS_BACKUP = "/var/lib/tessera/initramfs-backup"
THEME_NAME = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")


def theme_files(folder):
    """A theme folder's files ({name: (bytes, mode)}): only the names a theme
    of ours has, regular files, no links."""
    if os.path.islink(folder) or not os.path.isdir(folder):
        return {}
    files = read_tree(folder)
    bad = [n for n in files if not THEME_FILE.match(n)]
    if bad or len(files) > MAX_THEME_FILES:
        raise Refused(f"{folder} holds files a boot animation doesn't: " + ", ".join(sorted(bad)[:3]))
    return files


def install_themes(src, uid, prefix):
    """setup's part: each boot animation's folder from the shell's
    assets/plymouth (read as you) to LIB/plymouth, whole, when it changed.
    Returns the folders written."""
    written = []
    for id in BOOTSPLASH_IDS:
        name = "tessera-" + id
        top = os.path.join(src, "assets", "plymouth", name)
        with as_user(uid):
            if not os.path.isdir(top):
                continue  # (a shell folder from before them)
            names = sorted(os.listdir(top))
        if len(names) > MAX_THEME_FILES or not all(THEME_FILE.match(n) for n in names):
            raise Refused(f"{top} holds files a boot animation doesn't")
        files = {n: (read_as(uid, os.path.join(top, n), MAX_SOURCE), 0o644) for n in names}
        dest = os.path.join(prefix, PLYMOUTH_LIB.lstrip("/"), name)
        try:
            same = theme_files(dest) == files
        except (OSError, Refused):
            same = False
        if not same:
            write_tree(files, dest)
            written.append(dest)
    return written


def ini_value(text, key):
    """The last `key=value` in a Plymouth config (its one section, [Daemon])."""
    value = ""
    for line in text.splitlines():
        m = re.match(rf"^\s*{key}\s*=\s*(.*?)\s*$", line)
        if m:
            value = m.group(1)
    return value


def read_text(path):
    try:
        with open(path) as f:
            return f.read()
    except (OSError, UnicodeDecodeError):
        return None


def current_theme(prefix):
    """Plymouth's theme now: its config's, else the distribution's default."""
    for path in (PLYMOUTH_CONF, PLYMOUTH_DEFAULTS):
        value = ini_value(read_text(os.path.join(prefix, path.lstrip("/"))) or "", "Theme")
        if value:
            return value
    return ""


def mkinitcpio_hooks(prefix):
    """The HOOKS mkinitcpio builds with (its config, then conf.d's; the last wins)."""
    hooks = []
    paths = [os.path.join(prefix, "etc/mkinitcpio.conf")] + sorted(glob.glob(os.path.join(prefix, "etc/mkinitcpio.conf.d/*.conf")))
    for path in paths:
        for line in (read_text(path) or "").splitlines():
            m = re.match(r"^\s*HOOKS(\+?)=\((.*)\)", line)
            if m:
                hooks = (hooks if m.group(1) else []) + m.group(2).split()
    return hooks


def boot_images(prefix):
    """The boot images mkinitcpio -P writes (each preset's image and UKI),
    and any initramfs in /boot: the ones copied before a rebuild."""
    found = set(glob.glob(os.path.join(prefix, "boot/initramfs-*.img")))
    for preset in glob.glob(os.path.join(prefix, "etc/mkinitcpio.d/*.preset")):
        for line in (read_text(preset) or "").splitlines():
            m = re.match(r"""^\s*[A-Za-z0-9]+_(image|uki)=(["']?)(/[^"'\s]+)\2\s*$""", line)
            if m:
                found.add(os.path.join(prefix, m.group(3).lstrip("/")))
    return sorted(p for p in found if os.path.isfile(p) and not os.path.islink(p))


def restore_file(src, dest):
    """dest put back from src: copy_file, or, when there's no room for its
    temporary copy beside dest (a full /boot), in place: the broken dest
    removed first, then src written straight to it and synced."""
    try:
        copy_file(src, dest)
        return
    except OSError:
        pass
    with contextlib.suppress(FileNotFoundError):
        os.unlink(dest)
    with open(dest, "wb") as out, open(src, "rb") as f:
        shutil.copyfileobj(f, out, 1 << 20)
        out.flush()
        os.fsync(out.fileno())
    shutil.copystat(src, dest)


def copy_file(src, dest):
    """dest replaced whole by a copy of src (a temporary file, then a rename)."""
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(dest), prefix=".tessera-")
    try:
        with os.fdopen(fd, "wb") as out, open(src, "rb") as f:
            shutil.copyfileobj(f, out, 1 << 20)
            out.flush()
            os.fsync(out.fileno())
        shutil.copystat(src, tmp)
        os.rename(tmp, dest)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise


def bootsplash(choice, prefix=None, run=subprocess.run, which=shutil.which):
    """Sets the boot animation and rebuilds the boot image; on any failure
    the theme setting and the boot images are as they were. Returns what it
    did, a line each."""
    prefix = PREFIX if prefix is None else prefix

    def at(path):
        return os.path.join(prefix, path.lstrip("/"))
    if choice not in BOOTSPLASH_IDS + ("system", "none"):
        raise Refused("bootsplash: " + " | ".join(BOOTSPLASH_IDS) + " | system | none")
    if not which("plymouth-set-default-theme") or not os.path.isdir(at(PLYMOUTH_THEMES)):
        raise Refused("Plymouth isn't installed. Install the plymouth package, add plymouth to HOOKS in /etc/mkinitcpio.conf "
                      "and splash to the kernel's command line, then try again")
    if not which("mkinitcpio"):
        if which("dracut"):
            raise Refused("this computer builds its boot image with dracut, which Tessera doesn't rebuild. "
                          "Set the theme in a terminal: sudo plymouth-set-default-theme -R <theme>")
        if which("booster"):
            raise Refused("this computer builds its boot image with booster, which doesn't show Plymouth's animations")
        raise Refused("there's no mkinitcpio to rebuild the boot image with")
    if "plymouth" not in mkinitcpio_hooks(prefix):
        raise Refused("Plymouth isn't in the boot image. Add plymouth to HOOKS in /etc/mkinitcpio.conf (after kms), then try again")

    current = current_theme(prefix)
    try:
        record = json.loads(read_text(at(BOOTSPLASH_RECORD)) or read_text(at(OLD_BOOTSPLASH_RECORD)) or "null")
    except ValueError:
        record = None
    if not isinstance(record, dict) or not all(THEME_NAME.match(str(record.get(k, ""))) for k in ("original", "applied")):
        record = None
    # The system's own: the theme before Tessera first changed it, unless
    # someone has set another since (then that one)
    original = record["original"] if record and record["applied"] == current else current
    if not THEME_NAME.match(original or "") or original.startswith(("tessera-", OLD_PLYMOUTH_PREFIX)):
        original = ini_value(read_text(at(PLYMOUTH_DEFAULTS)) or "", "Theme") or "text"
    target = {"system": original, "none": "text"}.get(choice, "tessera-" + choice)

    theme_dir = at(os.path.join(PLYMOUTH_THEMES, target))
    files = {}
    if choice in BOOTSPLASH_IDS:
        src = at(os.path.join(PLYMOUTH_LIB, target))
        if os.path.islink(src) or not os.path.isdir(src) or (os.geteuid() == 0 and os.stat(src).st_uid != 0):
            raise Refused("Tessera's boot animations aren't installed yet. Update Tessera (Settings → Software Update), then try again")
        files = theme_files(src)
        if f"{target}.plymouth" not in files or f"{target}.script" not in files:
            raise Refused(f"{src} is incomplete. Update Tessera, then try again")
    elif not os.path.isfile(os.path.join(theme_dir, target + ".plymouth")):
        raise Refused(f"the theme {target} isn't installed any more. Reinstall it, or pick another")
    fresh = bool(files) and theme_files(theme_dir) != files
    if target == current and not fresh:
        return [f"already {target}"]

    # Copies of the boot images first: a failed rebuild must never leave the
    # computer unable to start
    images = boot_images(prefix)
    backup = at(INITRAMFS_BACKUP)
    shutil.rmtree(backup, ignore_errors=True)
    os.makedirs(backup, mode=0o700)
    need = sum(os.path.getsize(p) for p in images)
    if shutil.disk_usage(backup).free < need + 64 * 1024 * 1024:
        os.rmdir(backup)
        raise Refused(f"there isn't room to keep a copy of the boot images while they're rebuilt "
                      f"({need // (1024 * 1024) + 64} MB free in {os.path.dirname(INITRAMFS_BACKUP)}). Free some space, then try again")
    # (and on the boot images' own filesystem: a full /boot is the commonest
    # reason a rebuild fails. mkinitcpio rewrites each image in place, so
    # only a margin is asked for: a new image a little larger than the old)
    for where in sorted({os.path.dirname(p) for p in images}):
        if shutil.disk_usage(where).free < 16 * 1024 * 1024:
            os.rmdir(backup)
            raise Refused(f"{where} is nearly full ({shutil.disk_usage(where).free // (1024 * 1024)} MB free), so the boot image "
                          f"can't be rebuilt safely. Remove an old kernel or its fallback image there, then try again")
    conf = at(PLYMOUTH_CONF)
    try:
        with open(conf, "rb") as f:
            conf_before = f.read()
    except FileNotFoundError:
        conf_before = None
    had_dir = os.path.isdir(theme_dir)
    kept = []

    not_back = []

    def undo():
        # (every step on its own: one that fails never stops the rest; audit #13)
        try:
            if conf_before is None:
                with contextlib.suppress(FileNotFoundError):
                    os.unlink(conf)
            else:
                write_root(conf, conf_before, 0o644)
        except OSError as e:
            print(f"tessera-root: {conf} couldn't be put back: {e}", file=sys.stderr)
        for p, copy in kept:
            try:
                restore_file(copy, p)
            except OSError as e:
                not_back.append((p, copy, e))
        if files and not had_dir:
            shutil.rmtree(theme_dir, ignore_errors=True)

    done = []
    try:
        for n, p in enumerate(images):
            copy = os.path.join(backup, f"{n}.img")
            shutil.copy2(p, copy)
            kept.append((p, copy))
        if fresh:
            write_tree(files, theme_dir)
            done.append(f"installed {theme_dir}")
        r = run(["plymouth-set-default-theme", target], capture_output=True, text=True)
        if r.returncode != 0:
            raise Refused(f"Plymouth didn't take the theme {target}, so nothing changed: " + (r.stderr or r.stdout).strip()[-400:])
        r = run(["mkinitcpio", "-P"], capture_output=True, text=True)
        if r.returncode != 0:
            out = (r.stdout or "") + (r.stderr or "")
            said = [l.strip() for l in out.splitlines() if "ERROR" in l] or out.strip().splitlines()
            raise Refused("rebuilding the boot image failed, so nothing changed: " + " / ".join(said[-3:])[-600:])
    except BaseException as e:
        undo()
        if not_back:
            # (the copies stay where they are: the only good images now)
            lines = "; ".join(f"{p} (its copy: {c}; {err.strerror or err})" for p, c, err in not_back)
            raise Refused("DON'T RESTART: the boot image rebuild failed and these boot images couldn't be put back: " + lines
                          + ". Free space there, then copy each one back (sudo cp <copy> <image>)") from e
        raise
    finally:
        if not not_back:
            shutil.rmtree(backup, ignore_errors=True)
    done.append(f"theme {target}, boot image rebuilt")

    # Remembered: the system's own theme (for "system"), and what Tessera set
    rec = at(BOOTSPLASH_RECORD)
    if target == original:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(rec)
    else:
        write_root(rec, json.dumps({"original": original, "applied": target}).encode(), 0o644)
    with contextlib.suppress(FileNotFoundError):
        os.unlink(at(OLD_BOOTSPLASH_RECORD))
    # Tessera's other themes, no longer used (and under the old name)
    for id in BOOTSPLASH_IDS:
        for name in ("tessera-" + id, OLD_PLYMOUTH_PREFIX + id):
            other = at(os.path.join(PLYMOUTH_THEMES, name))
            if name != target and os.path.isdir(other) and not os.path.islink(other):
                shutil.rmtree(other, ignore_errors=True)
    return done


# ── wallpaper ───────────────────────────────────────────────────────────
def image_kind(data):
    if data[:3] == b"\xff\xd8\xff":
        return "jpeg"
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        return "png"
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "webp"
    return ""


def check_image_path(path):
    if not isinstance(path, str) or not path.startswith("/") or len(path) > 4096 or "\0" in path or "\n" in path:
        raise Refused("give the image's full path")
    return path


def wallpaper(path, uid, dest=LOGIN_WALLPAPER):
    path = check_image_path(path)
    data = read_as(uid, path, MAX_IMAGE)
    if not data or not image_kind(data):
        raise Refused("that isn't a JPEG, PNG or WebP image")
    d = os.path.dirname(dest)
    if not os.path.isdir(d) or os.path.islink(d):
        raise Refused("the Tessera login screen isn't installed (./install.sh --greeter)")
    write_root(dest, data, 0o644)


# ── login-look: what the login screen looks like ──────────────────────
LOOK_CHOICES = {
    "style": ("look", "text", "plain"),
    "look": ("glass", "material"),
    "mode": ("dark", "light"),
    "materialStyle": ("tonalSpot", "vibrant", "expressive", "neutral", "content", "fidelity", "monochrome"),
    "clockFace": ("dynamic", "stacked", "wide", "overlap", "analog", "weather"),
}
LOOK_RANGES = {"clockWeight": (100, 1000), "clockWidth": (25, 151)}
LOOK_KEYS = set(LOOK_CHOICES) | set(LOOK_RANGES) | {"seed", "clockRound"}
LOOK_OPTIONAL = {"clock24"}  # (true or false; a shell from before it sends none)
SEED = re.compile(r"^#[0-9a-fA-F]{6}$")


def check_login_look(text):
    """The look, as JSON with exactly LOOK_KEYS, each one of its values."""
    if not isinstance(text, str) or len(text) > 2048:
        raise Refused("give the look as one short JSON object")
    try:
        d = json.loads(text)
    except ValueError:
        raise Refused("that isn't JSON") from None
    if not isinstance(d, dict) or not LOOK_KEYS <= set(d) <= LOOK_KEYS | LOOK_OPTIONAL:
        raise Refused("give exactly: " + ", ".join(sorted(LOOK_KEYS)) + " (and clock24)")
    for k, values in LOOK_CHOICES.items():
        if not isinstance(d[k], str) or d[k] not in values:
            raise Refused(f"{k} is one of: " + ", ".join(values))
    for k, (lo, hi) in LOOK_RANGES.items():
        if type(d[k]) is not int or not lo <= d[k] <= hi:
            raise Refused(f"{k} is a whole number from {lo} to {hi}")
    if not isinstance(d["seed"], str) or not SEED.match(d["seed"]):
        raise Refused("seed is a colour as #rrggbb")
    for k in ("clockRound", "clock24"):
        if k in d and type(d[k]) is not bool:
            raise Refused(f"{k} is true or false")
    return d


def login_look(text, dest=LOGIN_LOOK):
    d = check_login_look(text)
    folder = os.path.dirname(dest)
    if not os.path.isdir(folder) or os.path.islink(folder):
        raise Refused("the Tessera login screen isn't installed (./install.sh --greeter)")
    # (one line, keys sorted: the shell compares it with what it would send)
    write_root(dest, json.dumps(d, sort_keys=True, separators=(",", ":")).encode(), 0o644)


# ── The login screen's copy of Tessera (setup) ──────────────────────────
TREE_SKIP = {".git", "__pycache__", "build", "node_modules"}
MAX_TREE_FILE = 32 * 1024 * 1024
MAX_TREE = 256 * 1024 * 1024
MAX_TREE_FILES = 20000
VM_MARK = b"\n-- virtual machine ("  # (what install.sh adds to greeter.lua in one)


def login_tree(src):
    """The checkout the login screen's copy comes from: the shell folder's
    own (install.sh, setup) or the update's staged one, else None."""
    src = src.rstrip("/")
    if os.path.isfile(os.path.join(src, "shell.qml")):
        return os.path.dirname(src)
    staged = os.path.join(src, "tree")
    if os.path.isfile(os.path.join(staged, "shell", "shell.qml")):
        return staged
    return None


def read_tree(top, uid=None):
    """{relative path: (bytes, mode)} of a folder's regular files (links,
    devices and TREE_SKIP folders left out), read as uid when one's given."""
    out, total = {}, 0
    with as_user(uid) if uid is not None else contextlib.nullcontext():
        for d, dirs, names in os.walk(top):
            dirs[:] = sorted(x for x in dirs if x not in TREE_SKIP and not os.path.islink(os.path.join(d, x)))
            for n in sorted(names):
                p = os.path.join(d, n)
                st = os.lstat(p)
                if n.endswith(".pyc") or st.st_mode & 0o170000 != 0o100000:
                    continue
                if uid is not None:
                    data = read_as(uid, p, MAX_TREE_FILE)
                else:
                    with open(p, "rb") as f:
                        data = f.read()
                total += len(data)
                if total > MAX_TREE or len(out) >= MAX_TREE_FILES:
                    raise Refused(f"{top} is too big for the login screen's copy")
                out[os.path.relpath(p, top)] = (data, 0o755 if st.st_mode & 0o111 else 0o644)
    return out


def write_tree(files, dest):
    """Puts files ({path: (bytes, mode)}) at dest whole: built next to it,
    then swapped in, so the login screen never starts from half a copy."""
    new, old = dest + ".new", dest + ".old"
    for p in (new, old):
        if os.path.islink(p):
            os.unlink(p)
        shutil.rmtree(p, ignore_errors=True)
    os.makedirs(new, mode=0o755)
    for rel, (data, mode) in files.items():
        p = os.path.join(new, rel)
        os.makedirs(os.path.dirname(p), mode=0o755, exist_ok=True)
        fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
        try:
            os.fchmod(fd, mode)
            view = memoryview(data)
            while view:
                view = view[os.write(fd, view):]
            # (on disk before the swap: a power cut just after an update
            # mustn't leave the login screen's files empty)
            os.fsync(fd)
        finally:
            os.close(fd)
    for d, _, _ in os.walk(new):
        _fsync_dir(d)
    if os.path.islink(dest):
        os.unlink(dest)
    elif os.path.exists(dest):
        os.rename(dest, old)
    os.rename(new, dest)
    _fsync_dir(os.path.dirname(dest) or ".")
    shutil.rmtree(old, ignore_errors=True)


def _fsync_dir(path):
    """The folder's entries (a rename) on disk; never fails the caller"""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    except OSError:
        return
    try:
        os.fsync(fd)
    except OSError:
        pass
    finally:
        os.close(fd)


# The login screen's keyboard layout: the system's (localectl writes it to
# 00-keyboard.conf, the file the console and other login screens read),
# written as Hyprland Lua next to greeter.lua, which loads it. Without one,
# a German or French password was typed as US at the login screen and
# refused (audit #7). No layout set: no file (Hyprland's "us").
XKB_CONF = "/etc/X11/xorg.conf.d/00-keyboard.conf"
XKB_FIELDS = (("XkbLayout", "kb_layout"), ("XkbVariant", "kb_variant"), ("XkbModel", "kb_model"), ("XkbOptions", "kb_options"))
XKB_VALUE = re.compile(r"^[A-Za-z0-9_,:+().-]+$")


def greeter_keyboard_lua(prefix="/"):
    """keyboard.lua's bytes for the system's layout, or None when none is set
    (or a value isn't one a layout is made of: tools/keyboard-lua.sh's rule)"""
    text = read_text(_at(prefix, XKB_CONF)) or ""
    found = {}
    for key, _ in XKB_FIELDS:
        m = re.search(r'^\s*Option\s+"' + key + r'"\s+"([^"]*)"', text, re.M)
        if m and m.group(1):
            found[key] = m.group(1)
    if not found.get("XkbLayout") or not all(XKB_VALUE.match(v) for v in found.values()):
        return None
    fields = ", ".join(f'{lua} = "{found[key]}"' for key, lua in XKB_FIELDS if key in found)
    return ("-- Written by Tessera's root helper from the system's keyboard layout\n"
            f"-- ({XKB_CONF}: localectl set-x11-keymap changes it; the next update copies it here)\n"
            f"hl.config({{ input = {{ {fields} }} }})\n").encode()


def write_greeter_keyboard(share, prefix="/"):
    """SHARE/keyboard.lua as the system's layout is now. Returns what changed."""
    dest = os.path.join(share, "keyboard.lua")
    want = greeter_keyboard_lua(prefix)
    try:
        with open(dest, "rb") as f:
            have = f.read()
    except OSError:
        have = None
    if want == have:
        return []
    if want is None:
        os.unlink(dest)
    else:
        write_root(dest, want, 0o644)
    return [dest]


def refresh_login(src, uid, prefix=None):
    """The login screen's copy of Tessera, from the same checkout as setup's
    files, where ./install.sh --greeter made one. Returns what it wrote."""
    prefix = PREFIX if prefix is None else prefix
    # (Tessera's only: the old name's, while greetd still starts that one, is
    # left as it was installed, whole, until ./install.sh --only=login replaces it)
    share = os.path.join(prefix, LOGIN_SHARE.lstrip("/"))
    if os.path.islink(share) or os.path.islink(os.path.join(share, "shell")) or not os.path.isdir(os.path.join(share, "shell")):
        return []
    repo = login_tree(src)
    if repo is None:
        return []
    written = []
    for name in ("shell", "material"):
        top = os.path.join(repo, name)
        if not os.path.isfile(os.path.join(top, "shell.qml")):
            continue  # (a version without the Material look)
        files = read_tree(top, uid)
        dest = os.path.join(share, name)
        try:
            same = os.path.isdir(dest) and not os.path.islink(dest) and read_tree(dest) == files
        except (OSError, Refused):
            same = False
        if not same:
            write_tree(files, dest)
            written.append(dest)
    lua = os.path.join(repo, "hypr", "greeter.lua")
    if os.path.isfile(lua):
        data = read_as(uid, lua, MAX_SOURCE)
        dest = os.path.join(share, "greeter.lua")
        try:
            with open(dest, "rb") as f:
                before = f.read()
        except OSError:
            before = b""
        # (a virtual machine's software cursor, which install.sh added, stays)
        i = before.find(VM_MARK)
        data = data.rstrip(b"\n") + b"\n" + (before[i:] if i >= 0 else b"")
        if data != before:
            write_root(dest, data, 0o644)
            written.append(dest)
        written += write_greeter_keyboard(share, prefix)
    # The Rust login screen, where the switch put one (greeter-start.sh
    # prefers it): the one you run now. Only the QML copy was refreshed, so
    # the login screen stayed the program from the day of the switch. Only
    # Tessera's signed build or one built here (read_login_ui); anything
    # else is refused, and the login screen falls back to the QML one (its
    # bin/ stays: a later refresh with one that's accepted puts it back)
    bindir = os.path.join(share, "bin")
    ui = os.path.join(bindir, "tessera-ui")
    if os.path.isdir(bindir) and not os.path.islink(bindir) and not os.path.islink(ui):
        try:
            data, how = read_login_ui(uid)
        except (OSError, Refused) as e:
            data, how = None, None
            if os.path.lexists(ui):
                os.unlink(ui)
                written.append(ui)
            if not isinstance(e, FileNotFoundError):  # (none of yours at all: nothing to say)
                login_ui_refused(e)
        if data is not None:
            try:
                with open(ui, "rb") as f:
                    same = f.read() == data
            except OSError:
                same = False
            if not same:
                write_root(ui, data, 0o755)
                written.append(ui)
                login_ui_said(how)
    return written


# ── login-switch: Tessera's login screen instead of another ─────────────
# Settings → Lock & Login Screen ("Use Tessera's login screen") and
# ./install.sh (--greeter, --only=login) both switch through here: greetd
# (installed when missing), the greeter user, the login screen's copy of
# Tessera in /usr/share/tessera with its look, /etc/greetd/config.toml (the
# one before kept as .pre-tessera), the keyring unlocked by the login
# password, then the login manager before it turned off and greetd on, last,
# so a failure before leaves the one you had.
# The login manager before is kept in LOGIN_RECORD. A bad switch would leave
# no graphical way in, so after the restart:
#   - greetd failing (systemd gives up restarting it) starts
#     tessera-login-fallback.service (OnFailure, a drop-in for greetd), and
#   - tessera-login-check.timer checks the login screen showed: greeter-
#     start.sh writes this boot's id to GREETER_SEEN once it has been up 10 s
#     or someone logged in through it; a session from greetd counts too.
#     Without either within 90 s of greetd starting (10 min of the boot at
#     most), or with greetd failed, it switches back.
# Switching back turns greetd off, the one before on and starts it now; the
# record says why (state "reverted"), and the shell tells you after you log in.
# `login-switch off` goes back by choice (state "off"). Exactly one login
# manager is ever on: the old one is turned off before greetd goes on, and
# on again when greetd can't be.
LOGIN_RECORD = "/etc/tessera/login-switch.json"
GREETER_SEEN = "/var/lib/tessera/greeter/seen"
GREETD_CONFIG = "/etc/greetd/config.toml"
GREETD_TOML = b'[terminal]\nvt = 1\n\n[default_session]\ncommand = "/usr/share/tessera/greeter-session.sh"\nuser = "greeter"\n'
DM_LINK = "/etc/systemd/system/display-manager.service"
UNIT_DIRS = ("/etc/systemd/system", "/usr/lib/systemd/system", "/lib/systemd/system")
LOGIN_UNITS = {
    "/etc/systemd/system/greetd.service.d/tessera-fallback.conf":
        "# Tessera: when the login screen fails, the one you had before comes back\n"
        "# (tessera-root login-fallback; Settings > Lock & Login Screen)\n"
        "[Unit]\nOnFailure=tessera-login-fallback.service\n",
    "/etc/systemd/system/tessera-login-fallback.service":
        "[Unit]\nDescription=Tessera: the login screen before, when Tessera's fails\n\n"
        "[Service]\nType=oneshot\nExecStart=/usr/lib/tessera/tessera-root login-fallback\n",
    "/etc/systemd/system/tessera-login-check.service":
        "[Unit]\nDescription=Tessera: check the new login screen showed\n\n"
        "[Service]\nType=oneshot\nExecStart=/usr/lib/tessera/tessera-root login-check\n",
    "/etc/systemd/system/tessera-login-check.timer":
        "[Unit]\nDescription=Tessera: check the new login screen showed, after the restart\n\n"
        "[Timer]\nOnBootSec=60s\nOnUnitActiveSec=20s\nAccuracySec=5s\n\n[Install]\nWantedBy=timers.target\n",
}
CHECK_TIMER = "tessera-login-check.timer"
DM_NAME = re.compile(r"^[a-z0-9][a-z0-9_.-]{0,40}$")
PAM_KEYRING = ("/usr/lib/security/pam_gnome_keyring.so",)
KEYRING_MARK = "# tessera: unlock the keyring with the login password"
SEEN_AFTER = 90  # s greetd runs without the login screen showing: back
GIVE_UP = 600  # s after the boot: back, whatever greetd is doing
DEFAULT_WALLPAPER = "shell/assets/wallpapers/tahoe-dusk.jpg"
MAX_UI = 256 * 1024 * 1024


def pam_with_keyring(text):
    """A login's PAM file with your keyring unlocked by the login password:
    gnome-keyring's lines after its last auth and its last session line,
    each under KEYRING_MARK (uninstall.sh takes them out). None when there's
    nothing to add: the keyring's lines are there, or KWallet's (it unlocks
    its own), or the file has no auth or session lines."""
    if re.search(r"pam_gnome_keyring\.so|pam_kwallet[0-9]*\.so", text):
        return None
    lines = text.splitlines()
    auth = [i for i, x in enumerate(lines) if re.match(r"^-?auth\s", x)]
    sess = [i for i, x in enumerate(lines) if re.match(r"^-?session\s", x)]
    if not auth or not sess:
        return None
    out = []
    for i, x in enumerate(lines):
        out.append(x)
        if i == auth[-1]:
            out += [KEYRING_MARK, "-auth      optional     pam_gnome_keyring.so"]
        if i == sess[-1]:
            out += [KEYRING_MARK, "-session   optional     pam_gnome_keyring.so auto_start"]
    return "\n".join(out) + "\n"


def _at(prefix, path):
    return os.path.join(prefix, path.lstrip("/"))


def current_dm(prefix="/"):
    """The login manager the computer starts with (display-manager.service's
    unit), "" for none (a console login)."""
    try:
        target = os.readlink(_at(prefix, DM_LINK))
    except OSError:
        return ""
    name = os.path.basename(target)
    return name[:-len(".service")] if name.endswith(".service") else name


def tessera_greetd(prefix="/"):
    """greetd starts Tessera's login screen (its config, Tessera's copy)"""
    return "/usr/share/tessera/" in (read_text(_at(prefix, GREETD_CONFIG)) or "") and os.path.isdir(_at(prefix, LOGIN_SHARE + "/shell"))


def check_dm(name, prefix="/"):
    """A login manager's name, only one whose unit is installed"""
    if not isinstance(name, str) or not DM_NAME.match(name):
        raise Refused("that isn't a login manager's name")
    if not any(os.path.isfile(_at(prefix, f"{d}/{name}.service")) for d in UNIT_DIRS):
        raise Refused(f"{name} isn't installed")
    return name


def read_record(prefix="/"):
    try:
        r = json.loads(read_text(_at(prefix, LOGIN_RECORD)) or "{}")
    except ValueError:
        r = {}
    return r if isinstance(r, dict) else {}


def write_record(prefix, **fields):
    rec = {k: v for k, v in fields.items() if v is not None}
    rec["time"] = int(time.time())
    rec["boot"] = boot_id(prefix)
    write_root(_at(prefix, LOGIN_RECORD), (json.dumps(rec, sort_keys=True) + "\n").encode(), 0o644)
    return rec


def boot_id(prefix="/"):
    return (read_text(_at(prefix, "/proc/sys/kernel/random/boot_id")) or "").strip()


def uptime(prefix="/"):
    try:
        return float((read_text(_at(prefix, "/proc/uptime")) or "0").split()[0])
    except (ValueError, IndexError):
        return 0.0


def _systemctl(run, *args):
    """(exit code, what it said: output and errors) of systemctl"""
    try:
        p = run(["systemctl", *args], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return None, ""
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def set_dm(new, old, prefix="/", run=subprocess.run):
    """display-manager.service from old to new ("" for none): old off first
    (another can't take the name while it holds it), then new on; when new
    can't be, old goes back on. Never both, never neither."""
    if new == old:
        return
    if old:
        code, out = _systemctl(run, "disable", f"{old}.service")
        if code != 0 or current_dm(prefix) == old:
            raise Refused(f"couldn't turn off {old}: {out.strip() or 'systemctl failed'}")
    if new:
        code, out = _systemctl(run, "enable", f"{new}.service")
        if code != 0 or current_dm(prefix) != new:
            if old:
                _systemctl(run, "enable", f"{old}.service")
            raise Refused(f"couldn't turn on {new}: {out.strip().splitlines()[-1] if out.strip() else 'systemctl failed'}")


def ensure_greeter_user(run=subprocess.run, getpw=pwd.getpwnam):
    """greetd's greeter user (its package makes it; else sysusers, else useradd)"""
    for attempt in (None, ["systemd-sysusers"], ["useradd", "--system", "--no-create-home", "--home-dir", "/var/lib/greetd",
                                                  "--shell", "/bin/bash", "--groups", "video", "greeter"]):
        if attempt:
            _cmd(run, *attempt)
        try:
            return getpw("greeter")
        except KeyError:
            continue
    raise Refused("couldn't make greetd's greeter user")


def caller_home(uid):
    try:
        return pwd.getpwuid(uid).pw_dir
    except KeyError:
        return ""


LOGIN_UI_FROM = ".local/share/tessera/bin/tessera-ui"  # (in the caller's home)
LOGIN_UI_RECORD = LOGIN_UI_FROM + ".json"  # (tools/native-install.py's record of where it came from)
MAX_RECORD = 1024 * 1024
# Tessera's release key (tools/tessera-ui.pub; tests/test_root_helper.py
# keeps the two the same). Root's own copy: the one in your checkout is
# yours to change, so it can't vouch for a program root installs
RELEASE_KEY = """untrusted comment: minisign public key 13D1783A15B8915C
RWRckbgVOnjREzjnJuW7OPl+iF+D6YISTVCnN/qCIIylPZtHNlNTXoPB
"""
# How a tessera-ui accepted for the login screen was vouched for
LOGIN_UI_HOW = {"signed": "Tessera's signed release build", "built": "built on this computer"}


def _b64(text):
    try:
        return base64.b64decode(text.strip(), validate=True)
    except ValueError:
        raise Refused("the signature isn't minisign's format") from None


def verify_minisign(pubkey_text, data, sig_text):
    """Raises Refused unless SIG_TEXT is a good minisign signature of DATA
    by the key PUBKEY_TEXT; returns its trusted comment. (The same check as
    tools/native-install.py's, which root can't import: it's in your home.)"""
    keys = [l for l in pubkey_text.splitlines() if l.strip() and not l.startswith("untrusted comment:")]
    raw = _b64(keys[0]) if keys else b""
    if len(raw) != 42 or raw[:2] != b"Ed":
        raise Refused("the release key isn't a minisign public key")
    keyid, key = raw[2:10], raw[10:]
    lines = (sig_text or "").splitlines()
    if len(lines) < 4 or not lines[0].startswith("untrusted comment:") or not lines[2].startswith("trusted comment: "):
        raise Refused("the signature isn't minisign's format")
    sig = _b64(lines[1])
    if len(sig) != 74 or sig[:2] not in (b"Ed", b"ED"):
        raise Refused("the signature isn't minisign's format")
    if sig[2:10] != keyid:
        raise Refused("it's signed with another key than Tessera's release key")
    trusted = lines[2][len("trusted comment: "):]
    message = hashlib.blake2b(data, digest_size=64).digest() if sig[:2] == b"ED" else data
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError:
        if not shutil.which("minisign"):
            raise Refused("nothing here can check signatures (python-cryptography or minisign)") from None
        with tempfile.TemporaryDirectory(prefix="tessera-root-verify-") as d:
            for name, content in (("key.pub", pubkey_text.encode()), ("file", data), ("file.minisig", sig_text.encode())):
                with open(os.path.join(d, name), "wb") as f:
                    f.write(content)
            r = subprocess.run(["minisign", "-V", "-q", "-p", os.path.join(d, "key.pub"), "-m", os.path.join(d, "file")],
                               capture_output=True, timeout=30)
        if r.returncode != 0:
            raise Refused("its signature doesn't match: it isn't what the project published") from None
        return trusted
    pk = Ed25519PublicKey.from_public_bytes(key)
    try:
        pk.verify(sig[10:], message)
        pk.verify(_b64(lines[3]), sig[10:] + trusted.encode())
    except InvalidSignature:
        raise Refused("its signature doesn't match: it isn't what the project published") from None
    return trusted


def login_ui_vouched(data, record, key=None):
    """How the tessera-ui DATA is vouched for ("signed" or "built"), from
    its record (tools/native-install.py's tessera-ui.json); Refused when it
    isn't. A ready-made build only when its sha256 is the one a release
    manifest gives it and that manifest's signature checks out against
    Tessera's release key; a build of yours only while its sha256 is the
    one recorded right after it was built. Anything else is refused."""
    if not isinstance(record, dict):
        raise Refused("there's no record of where tessera-ui came from")
    sha = hashlib.sha256(data).hexdigest()
    src = record.get("source")
    if src == "ready-made":
        manifest, sig = record.get("manifest"), record.get("minisig")
        if not isinstance(manifest, str) or not isinstance(sig, str) or not manifest or not sig:
            raise Refused("tessera-ui's signed manifest wasn't kept (an older install): install it again to use it at the login screen")
        trusted = verify_minisign(RELEASE_KEY if key is None else key, manifest.encode(), sig)
        try:
            m = json.loads(manifest)
        except ValueError:
            raise Refused("tessera-ui's signed manifest isn't JSON") from None
        tree = m.get("tree") if isinstance(m, dict) else None
        if not isinstance(tree, str) or not tree or "tree=" + tree not in trusted.split():
            raise Refused("tessera-ui's signed manifest is for another version")
        variants = m.get("variants") if isinstance(m.get("variants"), dict) else {}
        signed = {((v.get("programs") or {}).get("tessera-ui") or {}).get("sha256") for v in variants.values() if isinstance(v, dict)}
        if sha not in signed:
            raise Refused("tessera-ui isn't the program its signed manifest names (its checksum differs)")
        return "signed"
    if src == "bundle":
        # (a version installed as a bundle, tools/tessera-bundle.py: its
        # manifest lists every file's sha256, signed as a whole)
        manifest, sig = record.get("manifest"), record.get("minisig")
        if not isinstance(manifest, str) or not isinstance(sig, str) or not manifest or not sig:
            raise Refused("tessera-ui's signed bundle manifest wasn't kept")
        trusted = verify_minisign(RELEASE_KEY if key is None else key, manifest.encode(), sig)
        try:
            m = json.loads(manifest)
        except ValueError:
            raise Refused("tessera-ui's signed manifest isn't JSON") from None
        commit = m.get("commit") if isinstance(m, dict) else None
        if not isinstance(commit, str) or not commit or "commit=" + commit not in trusted.split() or m.get("name") != "tessera-bundle":
            raise Refused("tessera-ui's signed manifest is for another version")
        variants = m.get("variants") if isinstance(m.get("variants"), dict) else {}
        signed = {((v.get("files") or {}).get("bin/tessera-ui") or {}).get("sha256") for v in variants.values() if isinstance(v, dict)}
        if sha not in signed:
            raise Refused("tessera-ui isn't the program its signed manifest names (its checksum differs)")
        return "signed"
    if src in ("built", "built-tuned", "bundle-built"):
        want = record.get("sha256")
        if not isinstance(want, str) or not want:
            raise Refused("tessera-ui was built here before its checksum was recorded: build it again to use it at the login screen")
        if want != sha:
            raise Refused("tessera-ui changed after it was built here (its checksum differs from the build's record)")
        return "built"
    raise Refused("there's no record of where tessera-ui came from")


def resolve_in_home(path, home):
    """PATH, or where its links lead when it's a link (run as you): only to a
    file in HOME (else Refused). A link that leads nowhere stays as it is."""
    if not os.path.islink(path):
        return path
    real = os.path.realpath(path)
    if not real.startswith(os.path.realpath(home).rstrip("/") + "/"):
        raise Refused("tessera-ui leads out of your home: the login screen's copy isn't refreshed")
    return real


def read_login_ui(uid, home=None, key=None):
    """The Rust login screen to copy for greetd: your installed tessera-ui,
    read as you, and how it's vouched for (login_ui_vouched: "signed" or
    "built"). Refused unless it's a program (ELF) that only you (or root)
    could have written: it, its record and their folder owned by you or
    root, not writable by the group or others; so another account can't
    plant the program that reads every password at the login screen. And
    refused unless it's Tessera's signed build or yours, as its record says."""
    home = caller_home(uid) if home is None else home
    if not home:
        raise Refused("there's no home folder to take the login screen from")
    path, rec_path = os.path.join(home, LOGIN_UI_FROM), os.path.join(home, LOGIN_UI_RECORD)
    with as_user(uid):
        # (a bundle install's are links into its current version: the files
        # they lead to are checked, and must be in your home)
        path, rec_path = resolve_in_home(path, home), resolve_in_home(rec_path, home)
        dst = os.lstat(os.path.dirname(path))
        fst = os.lstat(path)
        try:
            rst = os.lstat(rec_path)
        except FileNotFoundError:
            rst = None
    for what, st in (("its folder", dst), ("tessera-ui", fst), ("its record", rst)):
        if st is not None and (st.st_uid not in (uid, 0) or st.st_mode & 0o022):
            raise Refused(f"{what} could be changed by another account: the login screen's copy isn't refreshed")
    data = read_as(uid, path, MAX_UI)
    if not data.startswith(b"\x7fELF"):
        raise Refused("tessera-ui isn't a program")
    if rst is None:
        raise Refused("there's no record of where tessera-ui came from")
    try:
        record = json.loads(read_as(uid, rec_path, MAX_RECORD))
    except ValueError:
        raise Refused("tessera-ui's record isn't JSON") from None
    return data, login_ui_vouched(data, record, key)


def login_ui_refused(why):
    """What's said when the login screen can't have your tessera-ui"""
    print(f"tessera-root: the login screen uses the QML login screen: tessera-ui wasn't accepted ({why})", file=sys.stderr)


def login_ui_said(how):
    print(f"tessera-root: the login screen's tessera-ui is {LOGIN_UI_HOW.get(how, how)}", file=sys.stderr)


def login_wallpaper_from(uid, home, repo):
    """The wallpaper you use (your settings' dark one, else the one), read as
    you; Tessera's own when it can't be read"""
    try:
        cfg = json.loads(read_as(uid, os.path.join(home, ".config/tessera/config.json"), MAX_SOURCE))
        w = cfg.get("wallpaper", {}) if isinstance(cfg, dict) else {}
        path = check_image_path(w.get("darkPath") or w.get("path") or "")
        data = read_as(uid, path, MAX_IMAGE)
        if image_kind(data):
            return data
    except (OSError, ValueError, Refused, AttributeError):
        pass
    return read_as(uid, os.path.join(repo, DEFAULT_WALLPAPER), MAX_IMAGE)


LOGIN_UI_LAST = {"ui": "", "why": ""}  # (how the last login copy's tessera-ui was vouched for: login-switch on says)


def install_login_copy(repo, look, uid, prefix, run, chown=os.chown):
    """/usr/share/tessera: the shell (and Material) from the checkout, the
    greeter's compositor config and start script, the Rust login screen when
    you have it, the look, the wallpaper. Returns what it wrote."""
    share = _at(prefix, LOGIN_SHARE)
    if os.path.islink(share):
        raise Refused(f"{LOGIN_SHARE} is a link")
    os.makedirs(share, mode=0o755, exist_ok=True)
    os.chmod(share, 0o755)
    done = []
    for name in ("shell", "material"):
        if os.path.isfile(os.path.join(repo, name, "shell.qml")):
            write_tree(read_tree(os.path.join(repo, name), uid), os.path.join(share, name))
            done.append(os.path.join(share, name))
    lua = read_as(uid, os.path.join(repo, "hypr/greeter.lua"), MAX_SOURCE).rstrip(b"\n") + b"\n"
    code, virt = _cmd(run, "systemd-detect-virt", "--vm")
    virt = virt.strip()
    if code == 0 and virt and virt != "none" and re.match(r"^[a-z0-9_-]+$", virt):
        lua += VM_MARK + virt.encode() + b"): software cursor\nhl.config({ cursor = { no_hardware_cursors = true } })\n"
    files = [("greeter.lua", lua, 0o644),
             ("greeter-session.sh", read_as(uid, os.path.join(repo, "hypr/greeter-session.sh"), MAX_SOURCE), 0o755),
             ("greeter.json", json.dumps(look, sort_keys=True, separators=(",", ":")).encode(), 0o644),
             ("wallpaper.jpg", login_wallpaper_from(uid, caller_home(uid), repo), 0o644)]
    # The login screen in Rust (tessera-ui --greeter), when you have it and
    # it's Tessera's signed build or one built here; greeter-start.sh starts
    # the QML one without it (its bin/ folder is made either way: a refresh
    # puts in one that's accepted later)
    os.makedirs(os.path.join(share, "bin"), mode=0o755, exist_ok=True)
    try:
        data, how = read_login_ui(uid)
        files.append(("bin/tessera-ui", data, 0o755))
        LOGIN_UI_LAST.update(ui=how, why="")
        login_ui_said(how)
    except (OSError, Refused) as e:
        # (none of yours at all: the QML one, nothing to say)
        missing = isinstance(e, FileNotFoundError)
        LOGIN_UI_LAST.update(ui="", why="" if missing else str(e))
        old_ui = os.path.join(share, "bin", "tessera-ui")
        if os.path.lexists(old_ui) and not os.path.isdir(old_ui):
            os.unlink(old_ui)
        if not missing:
            login_ui_refused(e)
    for rel, data, mode in files:
        write_root(os.path.join(share, rel), data, mode)
        done.append(os.path.join(share, rel))
    done += write_greeter_keyboard(share, prefix)
    # Where the login screen says it showed (greeter-start.sh, as greeter)
    seen = os.path.dirname(_at(prefix, GREETER_SEEN))
    os.makedirs(seen, mode=0o755, exist_ok=True)
    if os.geteuid() == 0:
        pw = ensure_greeter_user(run)
        chown(seen, pw.pw_uid, pw.pw_gid)
    return done


def login_switch_on(src, look_text, uid, prefix=None, run=subprocess.run, which=shutil.which):
    """Tessera's login screen from the next start (see above). Returns the
    record."""
    prefix = PREFIX if prefix is None else prefix
    src = check_folder(src)
    look = check_login_look(look_text)
    repo = login_tree(src)
    if repo is None or repo != os.path.dirname(src.rstrip("/")) or not all(
            os.path.isfile(os.path.join(repo, "hypr", f)) for f in ("greeter.lua", "greeter-session.sh")):
        raise Refused("that folder has no login screen to install: update Tessera, then try again")
    dm, rec = current_dm(prefix), read_record(prefix)
    already = dm == "greetd" and tessera_greetd(prefix)
    if already:
        previous = rec.get("previous", "") if isinstance(rec.get("previous"), str) else ""
    else:
        previous = dm
    # The helper the fallback units start (install.sh, before its own setup)
    if not os.path.isfile(_at(prefix, LIB + "/tessera-root")):
        setup(src, uid, prefix, run)
    if which("greetd") is None and not os.path.isfile(_at(prefix, "/usr/bin/greetd")):
        try:
            p = run([PACMAN, "-S", "--needed", "--noconfirm", "greetd"], capture_output=True, text=True, timeout=900)
            code, out = p.returncode, (p.stdout or "") + (p.stderr or "")
        except (OSError, subprocess.SubprocessError):
            code, out = None, ""
        if code != 0:
            last = out.strip().splitlines()[-1] if out.strip() else "pacman failed"
            raise Refused(f"couldn't install greetd: {last}")
    if os.geteuid() == 0:
        ensure_greeter_user(run)
    install_login_copy(repo, look, uid, prefix, run)
    conf = _at(prefix, GREETD_CONFIG)
    if os.path.isfile(conf) and not os.path.lexists(conf + ".pre-tessera"):
        copy_file(conf, conf + ".pre-tessera")
    write_root(conf, GREETD_TOML, 0o644)
    # (the copy under the old name: greetd starts Tessera's now)
    old = _at(prefix, OLD_LOGIN_SHARE)
    if os.path.isdir(old) and not os.path.islink(old):
        shutil.rmtree(old, ignore_errors=True)
    pam = _at(prefix, "/etc/pam.d/greetd")
    if any(os.path.exists(_at(prefix, m)) for m in PAM_KEYRING) and os.path.isfile(pam):
        new = pam_with_keyring(read_text(pam) or "")
        if new is not None:
            if not os.path.lexists(pam + ".pre-tessera"):
                copy_file(pam, pam + ".pre-tessera")
            # (refused, the login screen keeps its file: the keyring then asks once)
            try:
                write_pam(os.path.dirname(pam), "greetd", new, _at(prefix, PAM_VENDOR))
            except Refused as e:
                print(f"tessera-root: {e}", file=sys.stderr)
    for path, text in LOGIN_UNITS.items():
        write_root(_at(prefix, path), text.encode(), 0o644)
    # (greetd may have come just now: its login rescue too)
    for path, text in RESCUE_UNITS.items():
        write_root(_at(prefix, path), text.encode(), 0o644)
    _systemctl(run, "daemon-reload")
    if already:
        return write_record(prefix, previous=previous, state=rec.get("state") if rec.get("state") in ("trial", "active") else "active")
    # Recorded before switching: a switch cut short still knows the way back
    record = write_record(prefix, previous=previous, state="trial")
    if dm == "greetd":
        pass  # (greetd with another greeter: on already, only its config changed)
    else:
        try:
            set_dm("greetd", previous, prefix, run)
        except Refused:
            write_record(prefix, previous=previous, state="off")
            raise
    _systemctl(run, "enable", CHECK_TIMER)
    return record


def login_back(prefix, run, state, reason=None, now=False):
    """greetd off, the login manager before on (now too, at the start)"""
    rec = read_record(prefix)
    previous = rec.get("previous")
    if not isinstance(previous, str) or (previous and not DM_NAME.match(previous)):
        raise Refused("there's no record of the login screen before")
    if current_dm(prefix) == "greetd":
        if previous == "greetd":
            # (greetd with another greeter before: its config back)
            conf = _at(prefix, GREETD_CONFIG)
            if os.path.isfile(conf + ".pre-tessera"):
                copy_file(conf + ".pre-tessera", conf)
        else:
            set_dm(previous, "greetd", prefix, run)
    _systemctl(run, "disable", CHECK_TIMER)
    out = write_record(prefix, previous=previous, state=state, reason=reason)
    if now:
        # (the boot goes on with the one before; nothing was logged in through greetd)
        if previous == "greetd":
            # (greetd reads its config only as it starts: running Tessera's
            # greeter, a "start" did nothing until the next boot. A restart
            # brings the greeter before back now, and starts a failed greetd)
            _systemctl(run, "restart", "--no-block", "greetd.service")
        else:
            _systemctl(run, "stop", "--no-block", "greetd.service")
            _systemctl(run, "start", "--no-block", f"{previous}.service" if previous else "getty@tty1.service")
    return out


def login_switch_off(hint=None, prefix=None, run=subprocess.run):
    """Back to the login manager before, from the next start. hint: the one
    install.sh noted before there was a record (checked: an installed unit)."""
    prefix = PREFIX if prefix is None else prefix
    rec = read_record(prefix)
    if not isinstance(rec.get("previous"), str):
        if hint is None:
            raise Refused("there's no record of the login screen before")
        write_record(prefix, previous=check_dm(hint, prefix) if hint else "", state="active")
    return login_back(prefix, run, "off")


def greetd_session(run):
    """Someone is logged in through greetd"""
    code, out = _cmd(run, "loginctl", "list-sessions", "--no-legend")
    if code != 0:
        return False
    for line in out.splitlines():
        sid = line.split()[0] if line.split() else ""
        if not re.match(r"^[A-Za-z0-9_-]+$", sid):
            continue
        c, props = _cmd(run, "loginctl", "show-session", sid, "-p", "Service", "-p", "Class")
        if c == 0 and "Service=greetd" in props.split() and "Class=user" in props.split():
            return True
    return False


def greetd_state(run):
    """(ActiveState, seconds since it became active, or None)"""
    code, out = _systemctl(run, "show", "greetd.service", "-p", "ActiveState", "-p", "ActiveEnterTimestampMonotonic")
    props = dict(line.split("=", 1) for line in out.splitlines() if "=" in line)
    try:
        since = int(props.get("ActiveEnterTimestampMonotonic", "0")) / 1e6
    except ValueError:
        since = 0
    return props.get("ActiveState", ""), since


def login_fallback(prefix=None, run=subprocess.run):
    """greetd failed (OnFailure): back to the one before, if Tessera switched"""
    prefix = PREFIX if prefix is None else prefix
    rec = read_record(prefix)
    if rec.get("state") not in ("trial", "active") or current_dm(prefix) != "greetd" or not isinstance(rec.get("previous"), str):
        return "nothing"
    login_back(prefix, run, "reverted", "greetd stopped with an error as the computer started", now=True)
    return "reverted"


def login_check(prefix=None, run=subprocess.run):
    """After the restart (tessera-login-check.timer): did the login screen
    show? Returns what it decided: waiting, confirmed, reverted or done."""
    prefix = PREFIX if prefix is None else prefix
    rec = read_record(prefix)
    if rec.get("state") != "trial" or current_dm(prefix) != "greetd" or not isinstance(rec.get("previous"), str):
        _systemctl(run, "disable", "--now", CHECK_TIMER)
        return "done"
    me = boot_id(prefix)
    if not me or rec.get("boot") == me:
        return "waiting"  # (not restarted since the switch)
    if (read_text(_at(prefix, GREETER_SEEN)) or "").strip() == me or greetd_session(run):
        write_record(prefix, previous=rec["previous"], state="active")
        _systemctl(run, "disable", "--now", CHECK_TIMER)
        return "confirmed"
    state, since = greetd_state(run)
    up = uptime(prefix)
    if state == "failed":
        why = "greetd stopped with an error as the computer started"
    elif state == "active" and since and up - since >= SEEN_AFTER:
        why = "the login screen didn't appear"
    elif up >= GIVE_UP:
        why = "the login screen didn't start within 10 minutes"
    else:
        return "waiting"
    login_back(prefix, run, "reverted", why, now=True)
    return "reverted"


# ── boot readiness: the login screen will start ─────────────────────────
# "It must not fail at boot at all" (22 Nov). After every update (the end of
# `update`, whatever failed), as root: the things a login needs, read and
# tried without logging anyone in:
#   the login screen's PAM files (greetd's greeter service, greetd-greeter
#     or greetd, and the person's, greetd's general.service): pam_setcred
#     starts its session, and the password's path is intact (read, never
#     run: pam_problem); any other login screen's file Tessera marked
#   greetd's config parses; its default_session command is there and runs;
#     its user exists
#   Hyprland runs (--version), and Tessera's login screen's files are there
# A problem in Tessera's own edits is undone straight away (repair: the
# marked lines out, or the file before Tessera's edit back), checked again,
# and recorded in BOOT_CHECK, which Software Update and Health read: they
# say it loudly, before anyone logs out. Settings → Health's Fix runs the
# same (fix login).
BOOT_CHECK = "/var/lib/tessera/boot-check.json"
RESCUE_DIR = "/var/lib/tessera/rescue"
RESCUE_NOTE = RESCUE_DIR + "/note.json"
RESCUE_STAGE = "/run/tessera/login-rescue"
TEXT_GREETER = "/etc/greetd/tessera-text-greeter.toml"
TEXT_DROPIN = "/etc/systemd/system/greetd.service.d/zz-tessera-text-greeter.conf"
LOGIN_PAMS = {"greetd": "greetd", "ly": "ly", "sddm": "sddm", "lightdm": "lightdm", "lxdm": "lxdm"}
# greetd.service fails for good (its start limit): the rescue, as root
RESCUE_UNITS = {
    "/etc/systemd/system/greetd.service.d/tessera-rescue.conf":
        "# Tessera: when greetd keeps failing, the login rescue (tessera-root login-rescue)\n"
        "# brings a way in back: Tessera's PAM lines out, then greetd's text login, then a\n"
        "# console login on tty1. Delete this file to undo (uninstall.sh does).\n"
        "[Unit]\nOnFailure=tessera-login-rescue.service\nStartLimitIntervalSec=30\nStartLimitBurst=5\n",
    "/etc/systemd/system/tessera-login-rescue.service":
        "# Tessera: started only by greetd.service failing (its drop-in tessera-rescue.conf)\n"
        "[Unit]\nDescription=Tessera: bring a login back when the login screen keeps failing\n"
        "After=tessera-login-fallback.service\n\n"
        "[Service]\nType=oneshot\nExecStart=/usr/lib/tessera/tessera-root login-rescue\nTimeoutStartSec=120\n"
        "# (the helper itself can't run: a console login, whatever happened)\n"
        "ExecStopPost=/bin/sh -c '[ \"$SERVICE_RESULT\" = success ] || systemctl start --no-block getty@tty1.service'\n",
}


def greetd_config(prefix="/", path=None):
    """greetd's config as a dict, or None when it doesn't parse"""
    text = read_text(path or _at(prefix, GREETD_CONFIG))
    if text is None:
        return None
    try:
        import tomllib
        return tomllib.loads(text)
    except ImportError:
        pass
    except ValueError:
        return None
    # (Python before 3.11: the few keys read here)
    conf, table = {}, None
    for line in text.splitlines():
        line = line.strip()
        m = re.match(r"^\[([A-Za-z_]+)\]$", line)
        if m:
            table = conf.setdefault(m.group(1), {})
            continue
        m = re.match(r'^([A-Za-z_]+)\s*=\s*(?:"((?:[^"\\]|\\.)*)"|(\d+))\s*(#.*)?$', line)
        if m and table is not None:
            table[m.group(1)] = m.group(2) if m.group(2) is not None else int(m.group(3))
        elif line and not line.startswith("#"):
            return None
    return conf


def command_program(command, prefix="/"):
    """The program a greetd command runs (its first word that isn't an
    environment setting), found as sh would: a path, or on PATH"""
    try:
        import shlex
        words = shlex.split(command or "")
    except ValueError:
        return ""
    words = [w for w in words if not re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", w)]
    if words and words[0] == "env":
        words = [w for w in words[1:] if not w.startswith("-")]
    if not words:
        return ""
    if "/" in words[0]:
        return _at(prefix, words[0])
    for d in ("/usr/local/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"):
        if os.path.isfile(_at(prefix, f"{d}/{words[0]}")):
            return _at(prefix, f"{d}/{words[0]}")
    return ""


def greeter_pam(etc, vendor, conf):
    """The PAM services greetd uses: (the greeter's, the person's)"""
    general = conf.get("general", {}) if isinstance(conf, dict) else {}
    service = general.get("service") if isinstance(general.get("service"), str) else "greetd"
    greeter = "greetd-greeter" if any(os.path.isfile(os.path.join(d, "greetd-greeter")) for d in (etc, vendor)) else service
    return greeter, service


def pam_text(etc, vendor, name):
    text = read_text(os.path.join(etc, name))
    return text if text is not None else read_text(os.path.join(vendor, name))


def has_marks(text):
    return bool(text) and (any(m in text for m in TESSERA_MARKS) or KEYRING_MARK in text)


def boot_check(prefix="/", run=subprocess.run, which=shutil.which, hyprland=True):
    """What would stop the next login (see above): {"ok", "problems":
    [{"id", "what", "file"}], "dm"}. Reads and tries only: nothing changes."""
    etc, vendor = _at(prefix, PAM_ETC), _at(prefix, PAM_VENDOR)
    problems = []

    def problem(id, what, file=""):
        problems.append({"id": id, "what": what, "file": file})
    dm = current_dm(prefix)
    names = []
    if dm == "greetd":
        conf = greetd_config(prefix)
        if conf is None:
            problem("greetd-config", f"greetd's config ({GREETD_CONFIG}) is missing or doesn't parse", GREETD_CONFIG)
            conf = {}
        session = conf.get("default_session") if isinstance(conf.get("default_session"), dict) else {}
        command = session.get("command") if isinstance(session.get("command"), str) else ""
        if conf and not command:
            problem("greetd-config", "greetd's config has no default_session command", GREETD_CONFIG)
        elif command:
            prog = command_program(command, prefix)
            if not prog or not os.access(prog, os.X_OK):
                problem("greeter-command", f"the login screen's command isn't there: {command}", GREETD_CONFIG)
            if "/usr/share/tessera/" in command:
                for rel in ("greeter-session.sh", "greeter.lua", "shell/assets/scripts/greeter-start.sh"):
                    if not os.path.isfile(_at(prefix, f"{LOGIN_SHARE}/{rel}")):
                        problem("tessera-greeter", f"Tessera's login screen is missing {LOGIN_SHARE}/{rel}", LOGIN_SHARE)
                        break
        user = session.get("user") if isinstance(session.get("user"), str) else "greeter"
        if prefix == "/":
            try:
                pwd.getpwnam(user)
            except KeyError:
                problem("greeter-user", f"the login screen's user ({user}) doesn't exist")
        if hyprland and ("/usr/share/tessera/" in command or "Hyprland" in command or "hyprland" in command):
            exe = which("Hyprland") or ""
            code, _ = _cmd(run, exe, "--version") if exe else (None, "")
            if code != 0:
                problem("hyprland", "Hyprland doesn't run (Hyprland --version failed)" if exe else "Hyprland isn't installed")
        names += list(dict.fromkeys(greeter_pam(etc, vendor, conf)))
    elif dm in LOGIN_PAMS:
        names.append(LOGIN_PAMS[dm])
    # (every other login screen's file Tessera marked: it may be the next one used)
    names += [n for n in FP_FILES["login"] if n not in names and has_marks(read_text(os.path.join(etc, n)))]
    for name in names:
        text = pam_text(etc, vendor, name)
        if text is None:
            if name in ("greetd", "greetd-greeter") and dm == "greetd":
                problem("pam:" + name, f"there's no PAM file for {name}", os.path.join(PAM_ETC, name))
            continue
        why = pam_problem(etc, name, text, login=True, vendor=vendor)
        if why:
            problem("pam:" + name, why, os.path.join(PAM_ETC, name))
        elif name != "greetd-greeter" and password_reached(etc, name, text, vendor) is False:
            # (the system's own stack, not Tessera's lines: said, never "repaired")
            problem("password:" + name, f"{name}: nothing in it gets to the password", os.path.join(PAM_ETC, name))
    for name in ("sudo", "polkit-1"):
        text = pam_text(etc, vendor, name)
        if has_marks(text):
            why = password_path_problem(etc, name, text, vendor)
            if why:
                problem("pam:" + name, why, os.path.join(PAM_ETC, name))
    return {"ok": not problems, "problems": problems, "dm": dm}


def pam_without_tessera(text):
    """Every line Tessera added out: the fingerprint's, passkeys', face's,
    and the keyring's (the marked comment and the line under it)"""
    out, drop_next = [], False
    for line in text.splitlines(keepends=True):
        if drop_next and "pam_gnome_keyring" in line:
            drop_next = False
            continue
        drop_next = False
        if KEYRING_MARK in line:
            drop_next = True
            continue
        if any(m in line for m in TESSERA_MARKS):
            continue
        out.append(line)
    return "".join(out)


def keep_for_rescue(prefix, path, stamp):
    """A copy of PATH (as it is) in the rescue folder; returns where"""
    text = read_text(path)
    if text is None:
        return ""
    d = _at(prefix, f"{RESCUE_DIR}/{stamp}")
    os.makedirs(d, mode=0o755, exist_ok=True)
    dest = os.path.join(d, os.path.basename(path))
    write_root(dest, text.encode(), 0o644)
    return dest


def repair_pam(prefix, name, stamp, strip_only=False):
    """NAME's PAM file without Tessera's lines, else the copy from before
    Tessera's edit, whichever the real Linux-PAM finds no problem with
    first. Returns (what was done, the copy of the file before) or ("", "")."""
    etc, vendor = _at(prefix, PAM_ETC), _at(prefix, PAM_VENDOR)
    path = os.path.join(etc, name)
    text = read_text(path)
    if text is None:
        return "", ""
    tries = [("took Tessera's lines out of", pam_without_tessera(text))]
    if not strip_only:
        backups = pam_backup_dir(etc)
        for b in (os.path.join(backups, name), os.path.join(backups, name + ".first"), path + ".pre-tessera"):
            t = read_text(b)
            if t is not None:
                tries.append((f"put back {b} as", t))
    for how, new in tries:
        if new == text or not new.strip():
            continue
        if pam_problem(etc, name, new, login=name in LOGIN_PAMS.values() or name == "greetd-greeter", vendor=vendor):
            continue
        saved = keep_for_rescue(prefix, path, stamp)
        write_root(path, new.encode(), 0o644)
        return f"{how} {os.path.join(PAM_ETC, name)}", saved
    return "", ""


def text_greeter_possible(prefix="/", which=shutil.which):
    return bool(which("agreety") or os.path.isfile(_at(prefix, "/usr/bin/agreety"))) and \
        bool(which("greetd") or os.path.isfile(_at(prefix, "/usr/bin/greetd")))


def text_greeter_on(prefix="/", run=subprocess.run, which=shutil.which, hyprland_runs=True):
    """greetd's own text login (agreety) instead of the login screen in its
    config, from a config and a drop-in of their own (the config itself is
    left as it is: text_greeter_off is two files gone). The session it
    starts: Hyprland as the login screen's sessions start it."""
    conf = greetd_config(prefix) or {}
    vt = (conf.get("terminal") or {}).get("vt", 1) if isinstance(conf.get("terminal"), dict) else 1
    if not (isinstance(vt, int) or (isinstance(vt, str) and re.match(r"^[a-z0-9]+$", vt))):
        vt = 1
    session = "/bin/bash -l"  # (no Hyprland that runs: a shell, to fix it from)
    if hyprland_runs:
        session = "start-hyprland" if which("start-hyprland") else "Hyprland" if which("Hyprland") else session
    try:
        pwd.getpwnam("greeter")
        user = "greeter"
    except KeyError:
        user = "root" if prefix == "/" else "greeter"
    agreety = which("agreety") or "/usr/bin/agreety"
    greetd = which("greetd") or "/usr/bin/greetd"
    toml = (f"# Tessera's login rescue: greetd's own text login while the login screen can't start\n"
            f"# (tessera-root rescue undo, or the Restore button, goes back)\n"
            f"[terminal]\nvt = {vt if isinstance(vt, int) else json.dumps(vt)}\n\n"
            f"[default_session]\ncommand = \"{agreety} --cmd '{session}'\"\nuser = \"{user}\"\n")
    write_root(_at(prefix, TEXT_GREETER), toml.encode(), 0o644)
    write_root(_at(prefix, TEXT_DROPIN), (
        "# Tessera's login rescue: greetd starts its own text login (tessera-text-greeter.toml).\n"
        "# Delete this file (or press Restore in Tessera) to go back to the login screen.\n"
        f"[Service]\nExecStart=\nExecStart={greetd} --config {TEXT_GREETER}\n").encode(), 0o644)
    if prefix == "/":
        _systemctl(run, "daemon-reload")
    return [TEXT_GREETER, TEXT_DROPIN]


def text_greeter_off(prefix="/", run=subprocess.run):
    gone = []
    for p in (TEXT_DROPIN, TEXT_GREETER):
        if os.path.lexists(_at(prefix, p)):
            os.unlink(_at(prefix, p))
            gone.append(p)
    if gone and prefix == "/":
        _systemctl(run, "daemon-reload")
    return gone


def read_note(prefix="/"):
    try:
        n = json.loads(read_text(_at(prefix, RESCUE_NOTE)) or "{}")
    except ValueError:
        n = {}
    return n if isinstance(n, dict) else {}


def add_to_note(prefix, why, did, saved=None, text_greeter=None, log=None):
    """The note the shell shows at the next login: what broke, what was
    rolled back, and what Restore puts back"""
    n = read_note(prefix)
    n.setdefault("why", [])
    n.setdefault("did", [])
    n.setdefault("saved", {})
    n["why"] = list(dict.fromkeys(n["why"] + list(why)))
    n["did"] = n["did"] + list(did)
    n["saved"].update({k: v for k, v in (saved or {}).items() if v and k not in n["saved"]})
    if text_greeter is not None:
        n["textGreeter"] = bool(text_greeter) or bool(n.get("textGreeter"))
    if log:
        n["log"] = log[-40:]
    n["time"] = int(time.time())
    n["boot"] = boot_id(prefix)
    write_root(_at(prefix, RESCUE_NOTE), (json.dumps(n, sort_keys=True, indent=1) + "\n").encode(), 0o644)
    return n


def boot_repair(check, prefix="/", run=subprocess.run, which=shutil.which, stamp=None):
    """Undoes Tessera's own part in what check found. Returns (did, saved,
    text greeter turned on)."""
    stamp = stamp or str(int(time.time()))
    did, saved, text = [], {}, False
    for p in check["problems"]:
        if p["id"].startswith("pam:"):
            name = p["id"][4:]
            how, copy = repair_pam(prefix, name, stamp)
            if how:
                did.append(how)
                saved[name] = copy
        elif p["id"] == "greeter-user" and prefix == "/" and os.geteuid() == 0:
            with contextlib.suppress(Refused):
                ensure_greeter_user(run)
                did.append("made greetd's greeter user again")
    ids = {p["id"] for p in check["problems"]}
    if ids & {"greetd-config", "greeter-command", "tessera-greeter", "hyprland"} and not os.path.exists(_at(prefix, TEXT_DROPIN)) \
            and text_greeter_possible(prefix, which):
        did += [f"greetd's text login (agreety) instead of the login screen, for now ({TEXT_DROPIN})"]
        text_greeter_on(prefix, run, which, hyprland_runs="hyprland" not in ids)
        text = True
    return did, saved, text


def boot_check_and_repair(prefix="/", run=subprocess.run, which=shutil.which, hyprland=True, repair=True):
    """The check after an update (and Health's Fix): check, undo Tessera's
    part in what's wrong, check again; recorded in BOOT_CHECK (and the
    rescue note, when anything was undone). Returns the record."""
    before = boot_check(prefix, run, which, hyprland)
    did, saved, text = boot_repair(before, prefix, run, which) if repair and not before["ok"] else ([], {}, False)
    after = boot_check(prefix, run, which, hyprland) if did else before
    rec = {"ok": after["ok"], "problems": after["problems"], "found": before["problems"], "did": did,
           "time": int(time.time()), "boot": boot_id(prefix)}
    with contextlib.suppress(OSError):
        write_root(_at(prefix, BOOT_CHECK), (json.dumps(rec, sort_keys=True) + "\n").encode(), 0o644)
    if did:
        with contextlib.suppress(OSError):
            add_to_note(prefix, [p["what"] for p in before["problems"]], did, saved, text)
    return rec


def boot_check_said(rec, say):
    """The check's result as the update's lines (sysupdate.py reads
    BOOT_CHECK too)"""
    if rec["ok"] and not rec["did"]:
        say("@@boot-check ok")
        return
    for d in rec["did"]:
        say("@@boot-check undid " + d)
    for p in rec["problems"]:
        say("@@boot-check problem " + p["what"])
    if rec["ok"]:
        say("@@boot-check ok")


# ── the login rescue: greetd keeps failing as the computer starts ───────
# tessera-login-rescue.service (OnFailure of greetd.service, after its start
# limit), one step further each time it's started in a boot (RESCUE_STAGE,
# in /run: the next boot starts over):
#   1  what's wrong in Tessera's part undone (boot_check_and_repair); found
#      nothing: Tessera's lines taken out of greetd's PAM files anyway
#   2  greetd's own text login (agreety) instead of the login screen
#   3  a console login on tty1 (getty), Tessera's lines out of every login
#      screen's PAM file
# Each step but the last starts greetd again (reset-failed, restart). What it
# did goes in the note the shell shows at the next login, with Restore
# (tessera-root rescue undo). It never raises: anything unexpected, getty.
def journal_lines(run, n=30):
    code, out = _cmd(run, "journalctl", "-b", "-u", "greetd.service", "-n", str(n), "--no-pager", "-o", "short-monotonic")
    return out.splitlines()[-n:] if code == 0 else []


def rescue_stage(prefix, bump=True):
    p = _at(prefix, RESCUE_STAGE)
    try:
        stage = int((read_text(p) or "0").split()[0])
    except (ValueError, IndexError):
        stage = 0
    if bump:
        stage += 1
        os.makedirs(os.path.dirname(p), mode=0o755, exist_ok=True)
        with open(p, "w") as f:
            f.write(f"{stage}\n")
    return stage


def login_rescue(prefix=None, run=subprocess.run, which=shutil.which, say=print):
    """systemd's (see above). Returns what it did: repaired, text-login,
    console, elsewhere"""
    prefix = PREFIX if prefix is None else prefix
    try:
        return _login_rescue(prefix, run, which, say)
    except Exception as e:  # (whatever it was: a way in)
        say(f"tessera-root login-rescue: {e!r}: a console login on tty1")
        _systemctl(run, "start", "--no-block", "getty@tty1.service")
        return "console"


def _login_rescue(prefix, run, which, say):
    stage = rescue_stage(prefix)
    log = journal_lines(run)
    say(f"Tessera login rescue, step {stage}: greetd keeps failing")
    for line in log[-10:]:
        say("  " + line)
    if current_dm(prefix) != "greetd":
        # (login-fallback switched back to the login screen before; only
        # Tessera's lines out of the one it starts now, if they're a problem)
        rec = boot_check_and_repair(prefix, run, which, hyprland=False)
        say("Another login screen starts now; " + ("; ".join(rec["did"]) or "nothing of Tessera's to undo"))
        return "elsewhere"
    stamp = f"{int(time.time())}-boot"
    if stage == 1:
        rec = boot_check_and_repair(prefix, run, which, hyprland=False)
        did = rec["did"]
        if not did:
            saved = {}
            for name in ("greetd", "greetd-greeter"):
                how, copy = repair_pam(prefix, name, stamp, strip_only=True)
                if how:
                    did.append(how)
                    saved[name] = copy
            if did:
                add_to_note(prefix, ["greetd kept failing as the computer started"], did, saved, log=log)
        if did:
            for d in did:
                say("  " + d)
            restart_greetd(run)
            return "repaired"
        stage = rescue_stage(prefix)
    if stage == 2:
        if text_greeter_possible(prefix, which) and not os.path.exists(_at(prefix, TEXT_DROPIN)):
            text_greeter_on(prefix, run, which)
            add_to_note(prefix, ["greetd kept failing as the computer started"],
                        ["greetd's text login (agreety) instead of the login screen"], text_greeter=True, log=log)
            say("  greetd's text login (agreety) instead of the login screen")
            restart_greetd(run)
            return "text-login"
        stage = rescue_stage(prefix)
    # 3 (and after): a console login, and Tessera's lines out everywhere
    did, saved = [], {}
    for name in FP_FILES["login"] + ["greetd-greeter", "login"]:
        how, copy = repair_pam(prefix, name, stamp, strip_only=True)
        if how:
            did.append(how)
            saved[name] = copy
    did.append("a console login on tty1 (getty)")
    add_to_note(prefix, ["greetd kept failing as the computer started, its text login too"], did, saved, log=log)
    _systemctl(run, "start", "--no-block", "getty@tty1.service")
    say("  a console login on tty1")
    return "console"


def restart_greetd(run):
    _systemctl(run, "reset-failed", "greetd.service")
    _systemctl(run, "restart", "--no-block", "greetd.service")


def rescue(args, prefix=None, run=subprocess.run, which=shutil.which, say=print):
    """tessera-root rescue: from a console (Ctrl+Alt+F2, then sudo), a login
    now: Tessera's part in the login undone at once (every step of the
    login rescue but the console), greetd started again. rescue undo: what
    a rescue took out back, once the real Linux-PAM finds no problem with
    it (the Restore button). rescue status: the note, as JSON."""
    prefix = PREFIX if prefix is None else prefix
    if args == ["status"]:
        say(json.dumps(read_note(prefix), sort_keys=True))
        return 0
    if args == ["undo"]:
        return rescue_undo(prefix, run, say)
    if args:
        raise Refused("usage: tessera-root rescue [undo|status]")
    stamp = f"{int(time.time())}-rescue"
    rec = boot_check_and_repair(prefix, run, which)
    did, saved = list(rec["did"]), {}
    for name in FP_FILES["login"] + ["greetd-greeter"]:
        how, copy = repair_pam(prefix, name, stamp, strip_only=True)
        if how:
            did.append(how)
            saved[name] = copy
    text = False
    if current_dm(prefix) == "greetd" and tessera_greetd(prefix) and text_greeter_possible(prefix, which) \
            and not os.path.exists(_at(prefix, TEXT_DROPIN)):
        text_greeter_on(prefix, run, which)
        did.append("greetd's text login (agreety) instead of the login screen")
        text = True
    if did:
        add_to_note(prefix, ["a rescue was asked for from a console"], did, saved, text_greeter=text, log=journal_lines(run))
    for d in did:
        say(d)
    if current_dm(prefix) == "greetd":
        # (run from a terminal inside a desktop greetd started, a restart
        # would end that session and every unsaved document in it: the
        # repairs stay, the restart waits for the next login)
        if greetd_session(run):
            say("Someone is logged in through the login screen, so greetd wasn't restarted: log out or restart to use the repaired login")
        else:
            restart_greetd(run)
            say("greetd started again: the login is on tty1 (Ctrl+Alt+F1)")
    say("Nothing of Tessera's to undo" if not did else "Once you're in, Tessera offers to put it back (or run: sudo /usr/lib/tessera/tessera-root rescue undo)")
    return 0


def rescue_undo(prefix, run, say):
    """What the rescues took out, back (only what the real Linux-PAM finds
    no problem with), and the login screen instead of the text login (only
    when its files are there). The note goes once all of it is back."""
    note = read_note(prefix)
    etc, vendor = _at(prefix, PAM_ETC), _at(prefix, PAM_VENDOR)
    left = {}
    for name, copy in (note.get("saved") or {}).items():
        if not isinstance(name, str) or not PAM_NAME.match(name) or not isinstance(copy, str):
            continue
        copy_path = copy
        if not os.path.realpath(copy_path).startswith(os.path.realpath(_at(prefix, RESCUE_DIR)) + "/"):
            continue  # (only a copy the rescue made)
        text = read_text(copy_path)
        if text is None:
            continue
        if text == read_text(os.path.join(etc, name)):
            continue
        why = pam_problem(etc, name, text, login=name in LOGIN_PAMS.values() or name == "greetd-greeter", vendor=vendor)
        if why:
            left[name] = copy
            say(f"Not put back: {why}")
            continue
        write_pam(etc, name, text, vendor, check=False)
        say(f"Put back {os.path.join(PAM_ETC, name)}")
    if note.get("textGreeter") or os.path.exists(_at(prefix, TEXT_DROPIN)):
        chk = boot_check(prefix, run, hyprland=True)
        bad = [p for p in chk["problems"] if p["id"] in ("greetd-config", "greeter-command", "tessera-greeter", "hyprland")]
        if bad:
            say("The login screen stays greetd's text login: " + bad[0]["what"])
            left["textGreeter"] = True
        else:
            for p in text_greeter_off(prefix, run):
                say(f"Removed {p}: the login screen is back from the next start")
    if left:
        n = dict(note, saved={k: v for k, v in left.items() if k != "textGreeter"}, textGreeter=bool(left.get("textGreeter")))
        write_root(_at(prefix, RESCUE_NOTE), (json.dumps(n, sort_keys=True, indent=1) + "\n").encode(), 0o644)
        return 1
    with contextlib.suppress(FileNotFoundError):
        os.unlink(_at(prefix, RESCUE_NOTE))
    return 0


PAM_NAME = re.compile(r"^[a-z0-9][a-z0-9_.-]{0,40}$")


def rescue_units(prefix="/", run=subprocess.run):
    """setup's part: the login rescue's unit and greetd's drop-in, wherever
    greetd is installed (gone with it). Returns what it wrote or removed."""
    have = any(os.path.isfile(_at(prefix, f"{d}/greetd.service")) for d in UNIT_DIRS)
    done = []
    for path, text in RESCUE_UNITS.items():
        p = _at(prefix, path)
        if have and read_text(p) != text:
            write_root(p, text.encode(), 0o644)
            done.append(path)
        elif not have and os.path.exists(p):
            os.unlink(p)
            done.append(path)
    if done and prefix == "/":
        _systemctl(run, "daemon-reload")
    return done


# ── fix ─────────────────────────────────────────────────────────────────
def fix_command(id):
    if id not in FIXES:
        raise Refused("there's no fix called that")
    return FIXES[id]


# ── setup: this helper's own files ──────────────────────────────────────
def check_folder(src):
    if not isinstance(src, str) or not src.startswith("/") or "\0" in src or "\n" in src:
        raise Refused("give the shell's folder as a full path")
    return src


def setup(src, uid, prefix=None, run=subprocess.run):
    """Installs FILES from the shell's folder, only the ones that changed,
    and ananicy-cpp's rule where it's installed (unless turned off). Returns
    the destinations written."""
    src = check_folder(src)
    prefix = PREFIX if prefix is None else prefix
    written = []
    for rel, dest, mode in FILES:
        data = read_as(uid, os.path.join(src, rel), MAX_SOURCE)
        dest = os.path.join(prefix, dest.lstrip("/"))
        try:
            with open(dest, "rb") as f:
                same = f.read() == data and (os.stat(dest).st_mode & 0o7777) == mode
        except OSError:
            same = False
        if not same:
            write_root(dest, data, mode)
            written.append(dest)
    # (the old name's /etc folder first: its priority-off marker counts below)
    written += move_old_folders(prefix)
    rule = os.path.join(prefix, ANANICY_RULE.lstrip("/"))
    if os.path.isdir(os.path.dirname(rule)) and not os.path.exists(os.path.join(prefix, PRIORITY_OFF.lstrip("/"))):
        try:
            with open(rule, "rb") as f:
                same = f.read() == ANANICY_TEXT
        except OSError:
            same = False
        if not same:
            write_root(rule, ANANICY_TEXT, 0o644)
            written.append(rule)
    written += install_themes(src, uid, prefix)
    written += refresh_login(src, uid, prefix)
    written += migrate_old_names(prefix, run, src)
    # (Passkeys are gone: an older Tessera's pam_u2f lines, gate, key list and udev rule out)
    written += [os.path.join(prefix, d.lstrip("/")) for d in passkeys_retire(prefix, run)]
    # (the login rescue, wherever greetd is installed: tessera-login-rescue.service)
    written += [os.path.join(prefix, d.lstrip("/")) for d in rescue_units(prefix, run)]
    # ananicy-cpp reads its rules as it starts
    if prefix == "/" and any(d.startswith("/etc/ananicy.d/") or d == OLD_ANANICY_RULE for d in written):
        with contextlib.suppress(OSError, subprocess.SubprocessError):
            run(["systemctl", "try-restart", "ananicy-cpp.service"], timeout=20, check=False)
    return written


# ── update: the system part, and the broker for the AUR step ────────────
# The pacman steps as one inline script, as before (sysupdate.py's protocol):
# $1 snapshot tool, $2 held packages, $3 to install, $4 to remove,
# $5 1 = pacman -Syu, $6 Tessera's hold file (HOLD_CONF), $7 a folder of
# this run's own (a file "dropped" there: the hold was lifted, see below)
# A system update the hold (IgnorePkg) blocks, because a library a held
# package needs changed (pacman: "breaks dependency", "could not satisfy
# dependencies"), runs once more with Tessera's hold lifted for that one
# transaction, said loudly; the hold file comes back after it either way,
# and the lock step leaves the held packages where that update put them
# (audit #17: else Hyprland could only stay broken or block every update)
UPDATE_SCRIPT = r"""
syu() {
    echo "@@step system"
    log="$7/syu.log"
    { if [ -n "$2" ]; then pacman -Syu --noconfirm --ignore "$2"; else pacman -Syu --noconfirm; fi; echo $? >"$log.rc"; } 2>&1 | tee "$log"
    rc=$(cat "$log.rc" 2>/dev/null || echo 1)
    [ "$rc" = 0 ] && return 0
    if [ -n "$6" ] && grep -q '^IgnorePkg' "$6" 2>/dev/null && \
        grep -q -e 'breaks dependency' -e 'could not satisfy dependencies' -e 'unable to satisfy dependency' "$log"; then
        echo "Tessera's held packages (Hyprland and its libraries) block this update: a library they need changed."
        echo "Updating them too, this once, past the versions this Tessera was tested with."
        cp -p "$6" "$7/hold.conf" || return 1
        sed -i '/^IgnorePkg/d' "$6"
        : >"$7/dropped"
        if [ -n "$2" ]; then pacman -Syu --noconfirm --ignore "$2"; else pacman -Syu --noconfirm; fi
        rc=$?
        cp -p "$7/hold.conf" "$6"
        return $rc
    fi
    return "$rc"
}
if [ "$5" = 1 ]; then
    syu "$@" || exit 21
fi
if [ -n "$3" ]; then
    # One already installed is an upgrade: never on its own (Arch has no
    # partial upgrades: it could need newer libraries than the rest has), so
    # the whole system is updated first, in this same run
    if [ "$5" != 1 ]; then
        for p in $(echo "$3" | tr , ' '); do
            if pacman -Q -- "$p" >/dev/null 2>&1; then
                syu "$@" || exit 21
                break
            fi
        done
    fi
    echo "@@step packages"
    pacman -S --needed --noconfirm -- $(echo "$3" | tr , ' ') || exit 22
fi
if [ -n "$4" ]; then
    echo "@@step remove"
    pacman -Rns --noconfirm -- $(echo "$4" | tr , ' ') || exit 23
fi
exit 0
"""
SNAPSHOT = {
    "snapper": ["snapper", "-c", "root", "create", "-d", "Before Tessera update", "-c", "number"],
    "timeshift": ["timeshift", "--create", "--comments", "Before Tessera update", "--scripted"],
}
# The mirror lists --mirrors may re-rank: Arch's, CachyOS's
MIRRORLIST = re.compile(r"^/etc/pacman\.d/(mirrorlist|cachyos(-v[0-9])?-mirrorlist)$")


def refresh_mirrors(lists, say, out, prefix="/", run=subprocess.run, which=shutil.which):
    """The mirror lists that are behind, re-ranked before pacman -Syu
    (tools/pac.sh's pac_mirrors_refresh, as root: fixed commands only). Each
    old list kept once as <list>.tessera-backup. Returns True when re-ranked;
    on any failure the lists stay as they are."""
    at = lambda f: os.path.join(prefix, f.lstrip("/"))
    for f in lists:
        if os.path.isfile(at(f)) and not os.path.exists(at(f) + ".tessera-backup"):
            with contextlib.suppress(OSError):
                shutil.copy2(at(f), at(f) + ".tessera-backup")
    say("Re-ranking the mirrors first (" + ", ".join(os.path.basename(f) for f in lists) + " behind; the old lists kept as *.tessera-backup)")
    try:
        if which("cachyos-rate-mirrors"):
            return run(["cachyos-rate-mirrors"], stdout=out, stderr=subprocess.STDOUT, timeout=600).returncode == 0
        if "/etc/pacman.d/mirrorlist" not in lists:
            say("CachyOS's mirror lists are behind; cachyos-rate-mirrors (pacman -S cachyos-rate-mirrors) ranks them")
            return False
        if not which("reflector") and run([PACMAN, "-S", "--needed", "--noconfirm", "reflector"], stdout=out,
                                          stderr=subprocess.STDOUT, timeout=600).returncode != 0:
            say("reflector couldn't be installed, so the mirror list stays as it is")
            return False
        return run(["reflector", "--latest", "20", "--protocol", "https", "--sort", "rate", "--age", "12",
                    "--save", at("/etc/pacman.d/mirrorlist")], stdout=out, stderr=subprocess.STDOUT, timeout=600).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


# A package name: never starting with "-" (it would pass as a pacman option)
NAME = re.compile(r"^[a-z0-9@_+][a-z0-9@._+-]*$")


# Going back: an earlier package's file, from where earlier packages are kept
PKGFILE = re.compile(r"^[a-z0-9@_+][a-z0-9@._+-]*-[^-/]+-[^-/]+-[a-z0-9_]+\.pkg\.tar\.(zst|xz|gz|bz2)$")
ARCHIVE_URL = re.compile(r"^https://archive\.archlinux\.org/packages/[a-z0-9]/[a-z0-9@._+-]+/[a-z0-9@._+:-]+\.pkg\.tar\.(zst|xz)$")  # (":": an epoch)


# ── The package lock (update --lock: a Tessera version's tested packages) ─
# The same rules as tools/tessera-bundle.py's check_lock, kept here: this
# helper is one file, and never loads anything from your home.
HELD_FAMILY = ("hyprland", "aquamarine", "hyprutils", "hyprlang", "hyprgraphics", "hyprcursor", "hyprwayland-scanner",
               "quickshell")
HELD_OPTIONAL = ("quickshell", "hyprwayland-scanner")  # (left alone when not installed)
PKG_VERSION = re.compile(r"^(?:[0-9]+:)?[0-9A-Za-z._+~]+-[0-9]+(?:\.[0-9]+)?$")
PKG_ARCH = ("x86_64", "any")
MAX_LOCK = 8 * 1024 * 1024  # (a bundle's whole manifest may be passed)
HOLD_CONF = "/etc/pacman.d/tessera-hold.conf"
PACMAN_CONF = "/etc/pacman.conf"
HOLD_MARK = "# tessera: the packages Tessera holds at its tested versions (tessera-root update --lock)"
HOLD_INCLUDE = "Include = " + HOLD_CONF
VERCMP = "/usr/bin/vercmp"


def check_lock(doc):
    """{"held": {name: {"version", "arch"}}, "minimum": {name: version}}
    from a lock file, or a bundle manifest's "packages"; every name and
    version checked, held ones only from HELD_FAMILY. Raises Refused."""
    if isinstance(doc, dict) and isinstance(doc.get("packages"), dict) and doc.get("name") == "tessera-bundle":
        doc = doc["packages"]
    if not isinstance(doc, dict):
        raise Refused("not a package lock")
    held, minimum = doc.get("held", {}), doc.get("minimum", {})
    if not isinstance(held, dict) or not isinstance(minimum, dict) or len(held) > 32 or len(minimum) > 512:
        raise Refused("not a package lock")
    out = {"held": {}, "minimum": {}}
    for n, e in sorted(held.items()):
        if not isinstance(n, str) or not NAME.match(n) or n not in HELD_FAMILY:
            raise Refused(f"not a package Tessera holds: {n!r}")
        if not isinstance(e, dict) or not isinstance(e.get("version"), str) or not PKG_VERSION.match(e["version"]):
            raise Refused(f"{n}: not a package version")
        arch = e.get("arch", "x86_64")
        if arch not in PKG_ARCH:
            raise Refused(f"{n}: not an architecture")
        out["held"][n] = {"version": e["version"], "arch": arch}
    for n, v in sorted(minimum.items()):
        if not isinstance(n, str) or not NAME.match(n) or not isinstance(v, str) or not PKG_VERSION.match(v):
            raise Refused(f"not a package and version: {n!r}")
        out["minimum"][n] = v
    return out


def lock_archive_url(name, version, arch):
    """Arch's archive file for exactly that package: made here, from checked
    parts (never an address from the lock)"""
    if not NAME.match(name) or not PKG_VERSION.match(version) or arch not in PKG_ARCH:
        raise Refused(f"no archive address for {name} {version}")
    # (a CachyOS rebuild, pkgrel "1.1", is Arch's "1" rebuilt: the archive
    # only has Arch's; the held check already matches on pkgver)
    version = re.sub(r"-(\d+)\.\d+$", r"-\1", version)
    url = f"https://archive.archlinux.org/packages/{name[0]}/{name}/{name}-{version}-{arch}.pkg.tar.zst"
    if not ARCHIVE_URL.match(url):
        raise Refused(f"no archive address for {name} {version}")
    return url


def _pkg_upstream(v):
    """epoch:pkgver, without the package release"""
    head, sep, _ = v.rpartition("-")
    return head if sep else v


def _versions(args, run):
    """{name: version} from pacman -Q or -Si (LANG=C)"""
    try:
        r = run([PACMAN, *args], capture_output=True, text=True, timeout=60, env=dict(os.environ, LANG="C", LC_ALL="C"))
    except (OSError, subprocess.SubprocessError):
        return {}
    out, name = {}, None
    for line in (r.stdout or "").splitlines():
        if args[0] == "-Q":
            f = line.split()
            if len(f) == 2:
                out[f[0]] = f[1]
            continue
        k, _, v = line.partition(":")
        if k.strip() == "Name":
            name = v.strip()
        elif k.strip() == "Version" and name and name not in out:
            out[name] = v.strip()
    return out


def same_version(a, b, run=subprocess.run):
    """pacman's vercmp says a == b"""
    if a == b:
        return True
    try:
        r = run([VERCMP, a, b], capture_output=True, text=True, timeout=10)
        return r.stdout.strip() == "0"
    except (OSError, subprocess.SubprocessError):
        return False


def lock_moves(lock, run=subprocess.run):
    """What the lock moves now: {"repo": [names], "archive": [urls], "hold":
    [names], "words": [...]}: a held package whose pkgver isn't the tested
    one is moved (the repositories' copy when it's exactly that version,
    else the archive's); one not installed is installed, unless optional"""
    names = list(lock["held"])
    have = _versions(["-Q", "--", *names], run) if names else {}
    repo = _versions(["-Si", "--", *names], run) if names else {}
    out = {"repo": [], "archive": [], "hold": [], "words": []}
    for n, e in lock["held"].items():
        if n not in have and n in HELD_OPTIONAL:
            continue
        out["hold"].append(n)
        want = e["version"]
        if n in have and same_version(_pkg_upstream(have[n]), _pkg_upstream(want), run):
            continue
        out["words"].append(f"{n} {have.get(n, '(not installed)')} -> {want}")
        if repo.get(n) == want:
            out["repo"].append(n)
        else:
            out["archive"].append(lock_archive_url(n, want, e["arch"]))
    return out


def hold_text(names):
    return ("# Written by Tessera (tessera-root update --lock): these packages stay at the\n"
            "# versions this Tessera was tested with, until the next Tessera version moves\n"
            "# them. If pacman -Syu says one of them breaks a dependency, update with\n"
            "# `tessera update` (it lifts the hold for that update), or stop holding them:\n"
            "# sudo /usr/lib/tessera/tessera-root unhold\n"
            + ("IgnorePkg = " + " ".join(names) + "\n" if names else ""))


def with_hold_include(text):
    """pacman.conf with the marked Include line first in [options], or None
    when it's there already. Raises Refused without an [options] section."""
    lines = text.splitlines(keepends=True)
    if any(l.strip() == HOLD_INCLUDE for l in lines):
        return None
    for i, l in enumerate(lines):
        if l.strip() == "[options]":
            return "".join(lines[:i + 1] + [HOLD_MARK + "\n", HOLD_INCLUDE + "\n"] + lines[i + 1:])
    raise Refused("/etc/pacman.conf has no [options] section")


def write_hold(names, prefix="/"):
    """The held packages written down (HOLD_CONF), and pacman.conf including
    it (kept once as pacman.conf.pre-tessera before the first change)"""
    at = lambda f: os.path.join(prefix, f.lstrip("/"))
    for n in names:
        if not NAME.match(n) or n not in HELD_FAMILY:
            raise Refused(f"not a package Tessera holds: {n!r}")
    write_root(at(HOLD_CONF), hold_text(sorted(set(names))).encode(), 0o644)
    with open(at(PACMAN_CONF)) as f:
        text = f.read()
    new = with_hold_include(text)
    if new is not None:
        if not os.path.exists(at(PACMAN_CONF) + ".pre-tessera"):
            shutil.copy2(at(PACMAN_CONF), at(PACMAN_CONF) + ".pre-tessera")
        write_root(at(PACMAN_CONF), new.encode(), 0o644)
    return "Held at their tested versions: " + (", ".join(sorted(set(names))) or "nothing")


def without_hold_include(text):
    """pacman.conf without the marked Include line (and its mark)"""
    return "".join(l for l in text.splitlines(keepends=True) if l.strip() not in (HOLD_MARK, HOLD_INCLUDE))


def unhold(prefix="/"):
    """Tessera's hold gone: HOLD_CONF, and the two marked lines out of
    pacman.conf (only those: a restore of pacman.conf.pre-tessera would drop
    later edits), and that copy. Returns what changed (audit #10)."""
    at = lambda f: os.path.join(prefix, f.lstrip("/"))
    done = []
    text = read_text(at(PACMAN_CONF))
    if text is not None:
        new = without_hold_include(text)
        if new != text:
            write_root(at(PACMAN_CONF), new.encode(), 0o644)
            done.append(PACMAN_CONF)
    for f in (HOLD_CONF, PACMAN_CONF + ".pre-tessera"):
        with contextlib.suppress(FileNotFoundError):
            os.unlink(at(f))
            done.append(f)
    return done


def apply_lock(lock, say, out, run=subprocess.run):
    """The held packages to their tested versions (lock_moves): one pacman
    transaction for the repositories' and one for the archive's. True when
    everything is where the lock says (or nothing had to move)."""
    m = lock_moves(lock, run)
    if not m["repo"] and not m["archive"]:
        say("The held packages are at their tested versions")
        return True
    for w in m["words"]:
        say("Moving " + w)
    env = dict(os.environ, LANG="C", LC_ALL="C")
    if m["repo"] and run([PACMAN, "-S", "--needed", "--noconfirm", "--", *m["repo"]], stdout=out, stderr=subprocess.STDOUT,
                         env=env).returncode != 0:
        say("The held packages couldn't be moved (pacman -S)")
        return False
    if m["archive"] and run([PACMAN, "-U", "--needed", "--noconfirm", "--", *m["archive"]], stdout=out, stderr=subprocess.STDOUT,
                            env=env).returncode != 0:
        say("The held packages couldn't be moved from Arch's archive (archive.archlinux.org)")
        return False
    return True


def package_dirs(uid, run=subprocess.run):
    dirs = []
    try:
        r = run(["pacman-conf", "CacheDir"], capture_output=True, text=True, timeout=10)
        dirs = [l.strip() for l in r.stdout.splitlines() if l.strip()]
    except (OSError, subprocess.SubprocessError):
        pass
    try:
        home = pwd.getpwuid(uid).pw_dir
    except KeyError:
        home = ""
    return (dirs or ["/var/cache/pacman/pkg/"]) + ([os.path.join(home, ".cache/yay"), os.path.join(home, ".cache/paru/clone")] if home else [])


def check_package_file(v, dirs):
    if ARCHIVE_URL.match(v):
        return v
    path = os.path.realpath(v)
    if not v.startswith("/") or not PKGFILE.match(os.path.basename(path)) or not os.path.isfile(path):
        raise Refused(f"not a package file: {v}")
    if not any(path.startswith(os.path.realpath(d).rstrip("/") + "/") for d in dirs):
        raise Refused(f"not from a package cache: {v}")
    return path


def user_cache_packages_foreign(files, uid, run=subprocess.run):
    """A package file from the user's own folders (yay's, paru's) only for a
    package that is foreign (pacman -Qm: built from the AUR there). For one
    from the repositories, a file the user (or a program of theirs) put in
    those folders would be installed as root (audit #74): pacman's own
    cache or Arch's archive only. Raises Refused."""
    try:
        home = pwd.getpwuid(uid).pw_dir.rstrip("/")
    except KeyError:
        home = ""
    mine = [f for f in files if home and not ARCHIVE_URL.match(f) and f.startswith(home + "/")]
    if not mine:
        return
    try:
        r = run([PACMAN, "-Qmq"], capture_output=True, text=True, timeout=60, env=dict(os.environ, LANG="C", LC_ALL="C"))
        foreign = set(r.stdout.split())
    except (OSError, subprocess.SubprocessError):
        foreign = set()
    for f in mine:
        name = os.path.basename(f).rsplit("-", 3)[0]
        if name not in foreign:
            raise Refused(f"{name} is from the repositories, so its earlier version is only taken from pacman's cache or Arch's archive, not {f}")


def parse_update(args, dirs=None):
    """update's arguments, strictly: known flags only, package names checked."""
    p = {"snapshot": "", "system": False, "ignore": [], "install": [], "remove": [], "downgrade": [], "mirrors": [], "setup": "",
         "broker": False, "features": None, "lock": ""}
    feat = []
    i = 0
    while i < len(args):
        a = args[i]
        if a in ("--system", "--broker"):
            p[a[2:]] = True
            i += 1
            continue
        # (the features to set up once the packages are in: `features`'
        # own arguments, checked by it)
        if a == "--fprintd-wait":
            feat.append(a)
            i += 1
            continue
        if a in ("--features", "--face-camera", "--login-look", "--shell") and i + 1 < len(args):
            if a == "--features":
                feat.insert(0, args[i + 1])
            else:
                feat += [a, args[i + 1]]
            i += 2
            continue
        if a not in ("--snapshot", "--ignore", "--install", "--remove", "--downgrade", "--mirrors", "--setup", "--lock") or i + 1 >= len(args):
            raise Refused(f"unknown argument: {a}")
        v = args[i + 1]
        i += 2
        if a == "--snapshot":
            if v not in SNAPSHOT:
                raise Refused("unknown snapshot tool")
            p["snapshot"] = v
        elif a == "--setup":
            p["setup"] = check_folder(v)
        elif a == "--lock":
            if not v.startswith("/") or "\0" in v:
                raise Refused("--lock: a file's full path")
            p["lock"] = v
        elif a == "--downgrade":
            p["downgrade"] = [check_package_file(f, dirs or []) for f in v.split(",") if f]
        elif a == "--mirrors":
            lists = [f for f in v.split(",") if f]
            bad = [f for f in lists if not MIRRORLIST.match(f)]
            if bad:
                raise Refused("not a mirror list: " + ", ".join(bad))
            p["mirrors"] = lists
        else:
            names = [n for n in v.split(",") if n]
            bad = [n for n in names if not NAME.match(n)]
            if bad:
                raise Refused("not a package name: " + ", ".join(bad))
            p[a[2:]] = names
    if feat:
        if feat[0].startswith("--"):
            raise Refused("--features: which ones")
        p["features"] = parse_features(feat)
    return p


# Pacman options that point it somewhere else: refused, or only allowed
# with their default value (yay and paru pass --config /etc/pacman.conf)
DEFAULTS = {"--config": ("/etc/pacman.conf",), "--dbpath": ("/var/lib/pacman", "/var/lib/pacman/"), "--root": ("/",)}
REFUSED = {"--cachedir", "--hookdir", "--gpgdir", "--logfile", "--sysroot"}
# Long options that take a value ("--x v" or "--x=v")
VALUED = {"--config", "--dbpath", "--root", "--cachedir", "--hookdir", "--gpgdir", "--logfile", "--sysroot", "--arch",
          "--color", "--assume-installed", "--ignore", "--ignoregroup", "--overwrite", "--print-format", "--ask"}
OPERATIONS = {"--database": "D", "--remove": "R", "--sync": "S", "--upgrade": "U",
              "--query": "Q", "--deptest": "T", "--files": "F", "--version": "V", "--help": "h"}
ALLOWED_OPS = {"D", "R", "S", "U"}


def check_pacman(argv):
    """The AUR helper's command, if it's a pacman we run: pacman (or
    /usr/bin/pacman) installing, upgrading, removing or marking packages,
    not pointed at another config, root, database, hooks, keys, cache or
    log. Returns the arguments after "pacman"."""
    if not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv):
        raise Refused("not a command")
    if argv[0] not in ("pacman", PACMAN):
        raise Refused(f"only pacman runs as root here, not {argv[0]}")
    args = argv[1:]
    if any("\0" in a for a in args):
        raise Refused("a NUL in an argument")
    ops = []
    i = 0
    while i < len(args):
        a = args[i]
        i += 1
        if a == "--":
            break  # the rest are package names or files
        if a.startswith("--"):
            name, eq, value = a.partition("=")
            # pacman takes abbreviations (--logf for --logfile): the risky
            # ones only written out in full
            if name not in VALUED and name not in OPERATIONS and \
                    any(o.startswith(name) and o != name for o in REFUSED | set(DEFAULTS)):
                raise Refused(f"write pacman {name} out in full")
            if name in VALUED and not eq:
                if i >= len(args):
                    raise Refused(f"{name} needs a value")
                value = args[i]
                i += 1
            if name in REFUSED:
                raise Refused(f"pacman {name} isn't allowed")
            if name in DEFAULTS and value not in DEFAULTS[name]:
                raise Refused(f"pacman {name} {value} isn't allowed (only {DEFAULTS[name][0]})")
            if name in OPERATIONS:
                ops.append(OPERATIONS[name])
        elif a.startswith("-") and len(a) > 1:
            for c in a[1:]:
                if c in "br":  # (they take a path: --dbpath, --root)
                    raise Refused(f"pacman -{c} isn't allowed")
                if c in "DRSUQTFV":
                    ops.append(c)
    if len(ops) != 1 or ops[0] not in ALLOWED_OPS:
        raise Refused("only pacman -S, -U, -R or -D runs as root here")
    return args


def _send(sock, obj):
    sock.sendall((json.dumps(obj) + "\n").encode())


def serve(sock, log=None, pacman=None, on_features=None):
    """The broker: one request per line from sysupdate.py ({"argv", "cwd"}),
    answered with {"out": text} lines and one {"exit": code}. Requests run
    one after another. Ends when the socket closes. {"features": true}:
    the features this update was started with (--features), once the AUR's
    packages are in: on_features(say) runs them, once."""
    pacman = pacman or PACMAN
    reader = sock.makefile("rb")
    for line in reader:
        try:
            req = json.loads(line)
            if isinstance(req, dict) and req.get("features") is True:
                if on_features is None:
                    raise Refused("this update has no features to set up")

                def say(text):
                    if log:
                        log.write(text + "\n")
                        log.flush()
                    _send(sock, {"out": text + "\n"})
                run_them, on_features = on_features, None
                code = 1 if run_them(say) else 0
                _send(sock, {"exit": code})
                continue
            args = check_pacman(req.get("argv"))
            cwd = req.get("cwd")
            if not isinstance(cwd, str) or not cwd.startswith("/") or not os.path.isdir(cwd):
                cwd = "/"
        except (ValueError, AttributeError, Refused) as e:
            msg = f"tessera-root: refused: {e}\n"
            if log:
                log.write(msg)
                log.flush()
            try:
                _send(sock, {"out": msg})
                _send(sock, {"exit": 1})
            except OSError:
                return
            continue
        if log:
            log.write("$ pacman " + " ".join(args) + "\n")
            log.flush()
        p = subprocess.Popen([pacman] + args, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, env=dict(os.environ, LANG="C", LC_ALL="C"))
        gone = False  # sysupdate.py went away: pacman still finishes
        while True:
            chunk = os.read(p.stdout.fileno(), 65536)
            if not chunk:
                break
            text = chunk.decode(errors="replace")
            if log:
                log.write(text)
                log.flush()
            if not gone:
                try:
                    _send(sock, {"out": text})
                except OSError:
                    gone = True
        code = p.wait()
        if gone:
            return
        try:
            _send(sock, {"exit": code})
        except OSError:
            return


def update(args, uid, out=sys.stdout):
    """Returns the exit code (see the top of this file). Whatever happened,
    the boot check last: will the next login work? (BOOT_CHECK)"""
    try:
        return _update(args, uid, out)
    finally:
        try:
            boot_check_said(boot_check_and_repair(PREFIX), lambda line: (out.write(line + "\n"), out.flush()))
        except Exception as e:  # (never the update's failure)
            with contextlib.suppress(Exception):
                out.write(f"@@boot-check problem the check itself failed: {e!r}\n")
                out.flush()


def _update(args, uid, out=sys.stdout):
    p = parse_update(args, package_dirs(uid))
    user_cache_packages_foreign(p["downgrade"], uid)
    # Nothing sysupdate.py does (exiting, closing its end) may stop a transaction
    for s in (signal.SIGPIPE, signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(s, signal.SIG_IGN)

    def say(line):
        out.write(line + "\n")
        out.flush()
    say("@@authenticated")
    if p["snapshot"]:
        say("@@step snapshot")
        if subprocess.run(SNAPSHOT[p["snapshot"]], stdout=out, stderr=subprocess.STDOUT).returncode != 0:
            return 20
    if p["setup"]:
        # Never a reason to stop the update: the old behaviour carries on
        say("@@step setup")
        try:
            done = setup(p["setup"], uid)
            say("Installed " + ", ".join(done) if done else "Tessera's system helper is up to date")
        except (OSError, Refused) as e:
            say(f"Tessera's system helper wasn't installed: {e}")
    if p["downgrade"]:
        say("@@step undo")
        if subprocess.run([PACMAN, "-U", "--noconfirm", "--"] + p["downgrade"], stdout=out, stderr=subprocess.STDOUT,
                          env=dict(os.environ, LANG="C", LC_ALL="C")).returncode != 0:
            return 24
    if p["system"] and p["mirrors"]:
        if not refresh_mirrors(p["mirrors"], say, out):
            say("The mirrors couldn't be re-ranked: going on with the ones there are")
    lock = None
    if p["lock"]:
        # (checked before anything changes: a lock that isn't one stops here)
        try:
            lock = check_lock(json.loads(read_as(uid, p["lock"], MAX_LOCK)))
        except ValueError:
            raise Refused("the package lock isn't JSON") from None
        # Held first: the system update below never moves them
        say("@@step hold")
        say(write_hold(lock_moves(lock)["hold"]))
    with tempfile.TemporaryDirectory(prefix="tessera-update-") as run_dir:
        code = subprocess.run(["/bin/sh", "-c", UPDATE_SCRIPT, "sh", p["snapshot"], ",".join(p["ignore"]),
                               ",".join(p["install"]), ",".join(p["remove"]), "1" if p["system"] else "0", HOLD_CONF, run_dir],
                              stdout=out, stderr=subprocess.STDOUT).returncode
        dropped = os.path.exists(os.path.join(run_dir, "dropped"))
    if code != 0:
        return code
    if lock is not None and dropped:
        # (moving them back would undo what the system update needed)
        say("The held packages stay where this system update put them; the next Tessera version moves them again")
    elif lock is not None:
        say("@@step lock")
        if not apply_lock(lock, say, out):
            return 25
    on_features = (lambda s: features(p["features"], uid, s)) if p["features"] else None
    say("@@root-done")
    if p["broker"]:
        try:
            sock = socket.socket(fileno=os.dup(0))
        except OSError:
            say("tessera-root: no channel for the AUR step")
            return 0
        if sock.type != socket.SOCK_STREAM:
            return 0
        serve(sock, out, on_features=on_features)
    elif on_features:
        on_features(say)
    return 0



# ── features (the installer's Features step, Settings' "Set up the rest") ─
# What root does for each of Tessera's features (features.py lists them):
# fixed jobs by name, nothing free-form. Each is done on its own: one that
# fails is said ("@@feature <id> failed <why>") and the rest go on.
FEATURE_IDS = ("fingerprint", "face", "bluetooth", "power", "quickshare", "login", "bootsplash")
BT_CONF = "/etc/bluetooth/main.conf"


def parse_features(args):
    """`features`' arguments: the ids (a comma list), then --face-camera
    <camera>, --fprintd-wait, --login-look <json>, --shell <folder>."""
    if not args or args[0].startswith("--"):
        raise Refused("features: which ones (" + ",".join(FEATURE_IDS) + ")")
    ids = [x for x in args[0].split(",") if x]
    bad = [x for x in ids if x not in FEATURE_IDS]
    if bad or not ids:
        raise Refused("no feature called " + ", ".join(bad or ["(none)"]))
    p = {"ids": list(dict.fromkeys(ids)), "camera": "", "fprintd_wait": False, "look": "", "shell": ""}
    i = 1
    while i < len(args):
        a = args[i]
        if a == "--fprintd-wait":
            p["fprintd_wait"] = True
            i += 1
            continue
        if a not in ("--face-camera", "--login-look", "--shell") or i + 1 >= len(args):
            raise Refused(f"unknown argument: {a}")
        v = args[i + 1]
        i += 2
        if a == "--face-camera":
            if not CAMERA_PATH.match(v):
                raise Refused(f"not a camera: {v}")
            p["camera"] = v
        elif a == "--login-look":
            p["look"] = v
        else:
            p["shell"] = check_folder(v)
    return p


def bt_with_experimental(text):
    """bluetoothd's main.conf with Experimental = true in [General] (BlueZ's
    battery levels of headphones and phones, and LE features Quick Share
    uses), everything else as it was."""
    out, section, done, general = [], "", False, False
    for line in text.splitlines():
        m = re.match(r"^\s*\[(.+)\]\s*$", line)
        if m:
            if section == "General" and not done:
                out.append("Experimental = true")
                done = True
            section = m.group(1).strip()
            general = general or section == "General"
        elif section == "General" and re.match(r"^\s*#?\s*Experimental\s*=", line):
            if not done:
                out.append("Experimental = true")
                done = True
            continue
        out.append(line)
    if not done:
        if section != "General":
            out += (["", "[General]"] if out else ["[General]"])
        out.append("Experimental = true")
    return "\n".join(out) + "\n"


def bluetooth_extras(prefix=None, run=subprocess.run):
    """Experimental on (main.conf kept once as .pre-tessera), and the
    pairing fix (it restarts Bluetooth, which reads both)."""
    prefix = PREFIX if prefix is None else prefix
    conf = _at(prefix, BT_CONF)
    text = read_text(conf)
    if text is not None:
        new = bt_with_experimental(text)
        if new != text:
            if not os.path.lexists(conf + ".pre-tessera"):
                copy_file(conf, conf + ".pre-tessera")
            write_root(conf, new.encode(), 0o644)
    elif os.path.isdir(os.path.dirname(conf)):
        write_root(conf, bt_with_experimental("").encode(), 0o644)
    if prefix == "/":
        r = run(FIXES["btpairing"], capture_output=True, text=True)
        if r.returncode != 0:
            raise Refused("Bluetooth didn't restart: " + ((r.stderr or r.stdout or "").strip().splitlines() or ["systemctl failed"])[-1])


def features(p, uid, say, run=subprocess.run):
    """Sets up each of p["ids"] (parse_features'); says "@@feature <id> ok"
    or "@@feature <id> failed <why>" for each. Returns how many failed."""
    failed = 0
    for id in p["ids"]:
        say(f"@@feature {id} start")
        try:
            if id == "fingerprint":
                if p["fprintd_wait"]:
                    # (a reader whose driver updates its firmware as fprintd
                    # first starts: fprintd gets the time it needs)
                    run(FIXES["fprintd"], capture_output=True, text=True)
                for where in ("polkit", "sudo", "login"):
                    for d in fingerprint(True, where):
                        say("changed " + d)
                for d in face_admin_refresh(run=run):
                    say("changed " + d)
            elif id == "face":
                if not p["camera"]:
                    raise Refused("which camera (--face-camera)")
                if face(["setup", p["camera"]], uid, run=run) != 0:
                    raise Refused("Howdy couldn't be set up")
            elif id == "bluetooth":
                bluetooth_extras(run=run)
            elif id == "power":
                if run(["systemctl", "enable", "--now", "power-profiles-daemon"], capture_output=True).returncode != 0:
                    raise Refused("power-profiles-daemon didn't start")
            elif id == "quickshare":
                for d in quickshare_trust(True, uid):
                    say("changed " + d)
                run(["systemctl", "enable", "--now", "avahi-daemon"], capture_output=True)
            elif id == "login":
                if not p["shell"] or not p["look"]:
                    raise Refused("the login screen needs --shell and --login-look")
                login_switch_on(p["shell"], p["look"], uid, run=run)
            elif id == "bootsplash":
                for d in bootsplash(BOOTSPLASH_IDS[0], run=run):
                    say(d)
            say(f"@@feature {id} ok")
        except (Refused, OSError, subprocess.SubprocessError, ValueError) as e:
            failed += 1
            why = " ".join(str(e).split()) or type(e).__name__
            say(f"@@feature {id} failed {why}")
    return failed


# ── wifi-helper ─────────────────────────────────────────────────────────
def wifi_helper():
    helper = os.path.join(LIB, "qs-wifi-helper.py")
    os.execv(sys.executable, [sys.executable, "-I", helper])


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    cmd, args = argv[0], argv[1:]
    try:
        # (install.sh, as you: a PAM file with the keyring's lines, on stdout;
        # never as root, where it would print any file)
        if cmd == "pam-keyring" and len(args) == 1 and os.geteuid() != 0:
            try:
                with open(args[0]) as f:
                    new = pam_with_keyring(f.read())
            except OSError:
                return 1
            if new is None:
                return 1
            # (read, never run: the password's path as without the lines)
            d = os.path.dirname(os.path.abspath(args[0]))
            why = password_path_problem(d, os.path.basename(args[0]), new, PAM_VENDOR)
            if why:
                print(f"tessera-root: {why}", file=sys.stderr)
                return 1
            sys.stdout.write(new)
            return 0
        if os.geteuid() != 0:
            raise Refused("this runs as root (through pkexec)")
        uid = caller_uid()
        if cmd == "wallpaper" and len(args) == 1:
            wallpaper(args[0], uid, os.path.join(login_share(), "wallpaper.jpg"))
            return 0
        if cmd == "login-look" and len(args) == 1:
            login_look(args[0], os.path.join(login_share(), "greeter.json"))
            return 0
        if cmd == "fix" and args == ["firewall"]:
            cmd, args = "sharing-ports", ["open"]  # (a shell from before sharing-ports: all of them, as before)
        if cmd == "sharing-ports" and (args[:1] == ["open"] or args == ["close"]):
            done, failed = sharing_ports((args[1:] or list(PORT_SETS)) if args[0] == "open" else [])
            for d in done:
                print(d)
            for d in failed:
                print(f"tessera-root: couldn't: {d}", file=sys.stderr)
            return 1 if failed else 0
        if cmd == "fix" and len(args) == 1:
            c = fix_command(args[0])
            os.execvp(c[0], c)
        if cmd == "wifi-helper" and not args:
            wifi_helper()
        if cmd == "update":
            return update(args, uid)
        if cmd == "fingerprint" and len(args) == 2 and args[0] in ("on", "off"):
            for d in fingerprint(args[0] == "on", args[1]):
                print("changed", d)
            if args[1] == "polkit":
                # (face admin's copy of polkit's PAM file follows it)
                for d in face_admin_refresh():
                    print("changed", d)
            return 0
        if cmd == "face" and args:
            return face(args, uid)
        if cmd == "unhold" and not args:
            for d in unhold():
                print("changed", d)
            return 0
        if cmd == "phonecam" and len(args) == 1 and args[0] in ("on", "off"):
            for d in phonecam(args[0] == "on"):
                print("changed", d)
            return 0
        if cmd == "quickshare-trust" and len(args) == 1 and args[0] in ("on", "off"):
            for d in quickshare_trust(args[0] == "on", uid):
                print("changed", d)
            return 0
        if cmd == "bootmenu" and len(args) == 1 and args[0] in ("on", "off"):
            for d in bootmenu(args[0] == "on"):
                print(d)
            return 0
        if cmd == "priority" and len(args) == 1 and args[0] in ("on", "off"):
            for d in priority(args[0] == "on"):
                print("changed", d)
            return 0
        if cmd == "bootsplash" and len(args) == 1:
            for d in bootsplash(args[0]):
                print(d)
            return 0
        if cmd == "login-switch" and len(args) == 3 and args[0] == "on":
            rec = login_switch_on(args[1], args[2], uid)
            # (with how its tessera-ui was vouched for: "signed", "built" (on
            # this computer), or "" and why not: the QML login screen then)
            ui = {"ui": LOGIN_UI_LAST["ui"], "uiWords": LOGIN_UI_HOW.get(LOGIN_UI_LAST["ui"], ""), "uiWhy": LOGIN_UI_LAST["why"]}
            print(json.dumps(dict(rec, **ui), sort_keys=True))
            return 0
        if cmd == "login-switch" and len(args) in (1, 2) and args[0] == "off":
            print(json.dumps(login_switch_off(args[1] if len(args) == 2 else None), sort_keys=True))
            return 0
        # (systemd's, as the computer starts: never through pkexec or sudo)
        if cmd in ("login-fallback", "login-check") and not args:
            if "PKEXEC_UID" in os.environ or "SUDO_UID" in os.environ:
                raise Refused(f"{cmd} is the system's own, as the computer starts")
            print((login_fallback if cmd == "login-fallback" else login_check)())
            return 0
        if cmd == "boot-check" and args in ([], ["--repair"]):
            rec = boot_check_and_repair(PREFIX, repair=args == ["--repair"])
            print(json.dumps(rec, sort_keys=True))
            return 0 if rec["ok"] else 1
        if cmd == "rescue":
            return rescue(args)
        if cmd == "login-rescue" and not args:
            if "PKEXEC_UID" in os.environ or "SUDO_UID" in os.environ:
                raise Refused(f"{cmd} is the system's own, as the computer starts: from a console, run rescue")
            print(login_rescue())
            return 0
        if cmd == "features" and args:
            p = parse_features(args)
            return 1 if features(p, uid, lambda t: print(t, flush=True)) else 0
        if cmd == "setup" and len(args) == 1:
            for d in setup(args[0], uid):
                print("installed", d)
            return 0
        raise Refused("usage: tessera-root wallpaper <path> | login-look <json> | fix <id> | wifi-helper | fingerprint on|off <where> | unhold | phonecam on|off | face … | bootmenu on|off | "
                      "bootsplash <id>|system|none | quickshare-trust on|off | priority on|off | sharing-ports open [<set>…]|close | "
                      "update … | boot-check [--repair] | rescue [undo|status] | features <ids> [--face-camera <camera>] [--fprintd-wait] [--login-look <json>] [--shell <folder>] | setup <folder> | login-switch on <shell folder> <look json> | login-switch off [<login manager>]")
    except Refused as e:
        print(f"tessera-root: {e}", file=sys.stderr)
        return REFUSED_EXIT
    except OSError as e:
        print(f"tessera-root: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
