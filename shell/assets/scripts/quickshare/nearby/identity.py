"""This computer's Quick Share identity, kept for a day.

Phones list a device by what it advertises: its endpoint id (4 characters,
also in the mDNS name), the 16 bytes after the first byte of its endpoint
info, and its Bluetooth device token. Until 17 Nov all three were new each
time the helper started, and it starts each time visibility goes from No
one to Everyone: the phone then listed the computer once per start ("five
Monish-Work"). Now they are made the first time and kept in
$XDG_STATE_HOME/tessera/quickshare/identity.json (only you can read it);
a lost or damaged file is made again.

Since 27 Nov they're renewed once a day (#1009): kept for ever, they let
anyone with a scanner recognise the computer from place to place while it's
visible to Everyone (Android renews its own). A phone in the same day still
sees one device.
"""
import json
import os
import string
import time

from . import endpoint

ALPHABET = string.ascii_uppercase + string.digits
MAX_AGE = 24 * 3600  # s an identity is kept


def path():
    state = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
    return os.path.join(state, "tessera", "quickshare", "identity.json")


def _valid(d):
    try:
        return (isinstance(d, dict) and isinstance(d.get("endpoint_id"), str) and len(d["endpoint_id"]) == 4
                and all(c in ALPHABET for c in d["endpoint_id"])
                and len(bytes.fromhex(d.get("salt", ""))) == 16 and len(bytes.fromhex(d.get("token", ""))) == 2)
    except (TypeError, ValueError):
        return False


def new(now=None):
    return {"endpoint_id": endpoint.new_endpoint_id(), "salt": os.urandom(16).hex(), "token": os.urandom(2).hex(),
            "made": int(time.time() if now is None else now)}


def seconds_left(d, now=None):
    """How long an identity is kept still (0: renew it)."""
    now = time.time() if now is None else now
    made = d.get("made") if isinstance(d, dict) else None
    if not isinstance(made, (int, float)) or isinstance(made, bool) or made > now + 60:
        return 0  # (none, or from the future: a clock that was wrong)
    return max(0, made + MAX_AGE - now)


def _save(file, d):
    try:
        os.makedirs(os.path.dirname(file), mode=0o700, exist_ok=True)
        tmp = file + ".tmp"
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(d, f)
        os.replace(tmp, file)
        return True
    except OSError:
        return False


def load(file=None, now=None):
    """(identity dict, "kept" | "new" | "renewed"): the one on disk while
    it's less than a day old, else a new one saved there (an unwritable state
    folder still gives one, for this run only). One from before renewal had
    a date is dated now, and kept for a day."""
    file = file or path()
    now = time.time() if now is None else now
    how = "new"
    try:
        with open(file, encoding="utf-8") as f:
            d = json.load(f)
        if _valid(d):
            if "made" not in d:
                d["made"] = int(now)
                _save(file, d)
                return d, "kept"
            if seconds_left(d, now) > 0:
                return d, "kept"
            how = "renewed"
    except (OSError, ValueError):
        pass
    return renew(file, now, how)


def renew(file=None, now=None, how="renewed"):
    """A new identity, saved: (dict, how)."""
    d = new(now)
    if not _save(file or path(), d):
        return d, how + " (not saved)"
    return d, how
