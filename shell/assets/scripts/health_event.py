#!/usr/bin/python3 -I
"""One event added to Tessera's health record, the file Settings → Health
reads ($XDG_STATE_HOME/tessera/health.jsonl; tessera-ui's
core/src/health.rs keeps and trims it). The same line format: one JSON
object a line, its time first ("t", seconds), its kind ("ev"), then its
fields. Appended with one write to a file opened O_APPEND, so lines from
the shell and from scripts never mix. Local only: nothing is ever sent.

    health_event.py update <system|tessera|native> ok|failed [why] [step]

As a module (sysupdate.py, tools/tessera-update.py, tools/native-install.py
load it by path): record(kind, **fields), update(what, ok, why, step).
Never raises: a record that can't be written is skipped.
"""
import json
import os
import sys
import time

WHATS = ("system", "tessera", "native")
MAX_TEXT = 300  # a field's longest text (the record stays small)


def path():
    base = os.environ.get("XDG_STATE_HOME") or os.path.join(os.path.expanduser("~"), ".local", "state")
    return os.path.join(base, "tessera", "health.jsonl")


def line(kind, fields, now=None):
    """The event's line, as core/src/health.rs writes and reads one"""
    out = {"t": int(time.time() if now is None else now), "ev": str(kind)}
    for k, v in fields.items():
        if k in ("t", "ev") or v is None:
            continue
        if isinstance(v, str):
            v = " ".join(v.split())[:MAX_TEXT]
        out[k] = v
    return json.dumps(out, separators=(",", ":"), ensure_ascii=False) + "\n"


def record(kind, now=None, where=None, **fields):
    """Appends the event; True when it was written"""
    p = where or path()
    data = line(kind, fields, now).encode()
    try:
        os.makedirs(os.path.dirname(p), mode=0o700, exist_ok=True)
        fd = os.open(p, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_CLOEXEC, 0o600)
        try:
            # (one write: O_APPEND puts it at the end as a whole)
            n = os.write(fd, data)
            while n < len(data):
                n += os.write(fd, data[n:])
        finally:
            os.close(fd)
        return True
    except OSError:
        return False


def update(what, ok, why="", step="", now=None, where=None):
    """An update's end: worked (ok) or failed, and why"""
    if what not in WHATS:
        return False
    fields = {"what": what, "ok": bool(ok)}
    if not ok and why:
        fields["why"] = str(why)
    if step:
        fields["step"] = str(step)
    return record("update", now=now, where=where, **fields)


def main(argv):
    if len(argv) >= 3 and argv[0] == "update" and argv[1] in WHATS and argv[2] in ("ok", "failed"):
        update(argv[1], argv[2] == "ok", argv[3] if len(argv) > 3 else "", argv[4] if len(argv) > 4 else "")
        return 0
    print(__doc__.strip(), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
