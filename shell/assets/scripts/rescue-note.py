#!/usr/bin/env python3
"""The login rescue's note, once, at the login after it (launch-shell.sh
starts this on its own): what broke, what was rolled back, and Restore,
which puts it back once it would no longer stop the login screen
(pkexec tessera-root rescue undo: the password, and the helper checks
everything with the real Linux-PAM first). Settings → Health keeps showing
it (Login screen) until it's restored.

The note is the root helper's (/var/lib/tessera/rescue/note.json, written
by tessera-login-rescue.service, by a rescue from a console, or by the boot
check after an update); this only reads it. Shown once per note: its time
is kept in ~/.local/state/tessera/rescue-note-seen.

  rescue-note.py           wait for the notifications (up to a minute), show it
  rescue-note.py text      what it would say, as plain text
"""
import json
import os
import shutil
import subprocess
import sys
import time

SYSTEM = os.environ.get("TESSERA_SYSTEM_ROOT", "/")
NOTE = os.path.join(SYSTEM, "var/lib/tessera/rescue/note.json")
HELPER = "/usr/lib/tessera/tessera-root"
STATE = os.path.join(os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state"), "tessera")
SEEN = os.path.join(STATE, "rescue-note-seen")


def read_note(path=NOTE):
    try:
        with open(path) as f:
            n = json.load(f)
    except (OSError, ValueError):
        return None
    return n if isinstance(n, dict) and isinstance(n.get("time"), (int, float)) else None


def words(note):
    """(title, body) of the notification"""
    why = [w for w in note.get("why") or [] if isinstance(w, str)]
    did = [d for d in note.get("did") or [] if isinstance(d, str)]
    title = "The login rescue stepped in"
    body = (("What broke: " + "; ".join(why[:2]) + ". ") if why else "") + \
           (("What was rolled back: " + "; ".join(did[:3]) + ". ") if did else "") + \
           "Your password works as before. Restore puts it back once it would no longer stop the login screen " \
           "(Settings → Health → Login screen shows this too)."
    return title, body


def seen_time():
    try:
        with open(SEEN) as f:
            return float(f.read().strip() or 0)
    except (OSError, ValueError):
        return 0.0


def mark_seen(t):
    os.makedirs(STATE, exist_ok=True)
    with open(SEEN, "w") as f:
        f.write(f"{t}\n")


def wait_for_notifications(run=subprocess.run, timeout=60):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            if run(["busctl", "--user", "status", "org.freedesktop.Notifications"], capture_output=True, timeout=5).returncode == 0:
                return True
        except (OSError, subprocess.SubprocessError):
            return False
        time.sleep(2)
    return False


def show(note, run=subprocess.run):
    """The notification, with Restore; returns what was pressed ("" for none)"""
    title, body = words(note)
    cmd = ["notify-send", "-u", "critical", "-a", "Tessera", "-i", "dialog-warning", "-A", "restore=Restore", "-w", title, body]
    try:
        p = run(cmd, capture_output=True, text=True, timeout=24 * 3600)
    except (OSError, subprocess.SubprocessError):
        return ""
    if p.returncode != 0:
        # (an older notify-send, without actions: the words alone)
        with_no_actions = [c for c in cmd if c not in ("-A", "restore=Restore", "-w")]
        try:
            run(with_no_actions, capture_output=True, timeout=10)
        except (OSError, subprocess.SubprocessError):
            pass
        return ""
    return (p.stdout or "").strip()


def restore(run=subprocess.run):
    """pkexec tessera-root rescue undo; returns (ok, what it said)"""
    if not os.access(HELPER, os.X_OK) or not shutil.which("pkexec"):
        return False, "Tessera's root helper isn't installed"
    try:
        p = run(["pkexec", HELPER, "rescue", "undo"], capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        return False, str(e)
    said = ((p.stdout or "") + (p.stderr or "")).strip()
    return p.returncode == 0, said


def main(argv, run=subprocess.run):
    note = read_note()
    if argv[:1] == ["text"]:
        if note:
            print("\n".join(words(note)))
        return 0
    if not note or note["time"] <= seen_time():
        return 0
    if not wait_for_notifications(run):
        return 0
    mark_seen(note["time"])
    if show(note, run) == "restore":
        ok, said = restore(run)
        last = said.splitlines()[-1] if said else ""
        run(["notify-send", "-a", "Tessera", "Login settings put back" if ok else "Not put back yet",
             last or ("Done" if ok else "It would still stop the login screen")], capture_output=True, timeout=10)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
