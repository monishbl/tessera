#!/usr/bin/python3 -I
"""Tessera's settings file (config.json), changed from a script the way the
shell changes it: under the same lock, so a change the shell makes at the
same moment is never lost, and a script's change never undoes the shell's.

The lock is the one tessera-ui's core/src/cfgfile.rs takes: an flock on the
settings folder itself (no extra file in it), held for a moment and never
waited on for more than two seconds. Inside it the file is read again, the
change applied and the whole file written to a temporary name of its own in
the folder the file really lives in (a dotfiles link stays a link), with
the file's permissions, then renamed over it. The shell sees the new file
and reloads it.

Works whether the shell is running or not. Scripts load it by path (they
run with python3 -I, so not by import):

    spec = importlib.util.spec_from_file_location("settings_file", os.path.join(HERE, "settings_file.py"))

    settings_file.edit(change)      change(dict) -> False when nothing changed
    settings_file.read()            the settings, {} when missing or unreadable
    with settings_file.Lock(path):  the lock alone (a read and a write of your own)
    settings_file.write_whole(path, data)

From a shell script:

    python3 -I settings_file.py set appearance.look glass   (the value as JSON when it parses)

Other files shared with the shell (health.jsonl) use lock_file(path): an
flock on a lock file beside it, taken the same way.
"""
import contextlib
import fcntl
import json
import os
import sys
import time

WAIT = 2.0  # s: the longest a lock is waited on (cfgfile.rs: 200 x 10 ms)


def config_dir():
    return os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "tessera")


def config_path():
    return os.path.join(config_dir(), "config.json")


class _Flock:
    """An flock on an open fd; taken without blocking, tried for WAIT
    seconds, then gone on without it (a stuck holder costs a moment, never
    the script)"""

    def __init__(self, fd):
        self.fd = fd
        self.held = False
        if fd is None:
            return
        end = time.monotonic() + WAIT
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                self.held = True
                return
            except OSError:
                if time.monotonic() >= end:
                    return
                time.sleep(0.01)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        if self.fd is None:
            return
        try:
            if self.held:
                fcntl.flock(self.fd, fcntl.LOCK_UN)
        finally:
            os.close(self.fd)
            self.fd = None


class Lock(_Flock):
    """The settings folder's lock, as the shell's own writes take it"""

    def __init__(self, path=None):
        d = os.path.dirname(path or config_path()) or "."
        fd = None
        with contextlib.suppress(OSError):
            os.makedirs(d, exist_ok=True)
        with contextlib.suppress(OSError):
            fd = os.open(d, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        super().__init__(fd)


def lock_file(path):
    """The lock of another shared file: `.<name>.lock` beside it (the shell
    takes the same one: core/src/health.rs for health.jsonl)"""
    d, name = os.path.split(path)
    fd = None
    with contextlib.suppress(OSError):
        os.makedirs(d or ".", mode=0o700, exist_ok=True)
    with contextlib.suppress(OSError):
        fd = os.open(os.path.join(d, "." + name + ".lock"), os.O_WRONLY | os.O_CREAT | os.O_CLOEXEC, 0o600)
    return _Flock(fd)


def text(c):
    """The file's text, as the shell writes it (four spaces, a newline at
    the end; cfgfile.rs config_text)"""
    return json.dumps(c, indent=4, ensure_ascii=False) + "\n"


_seq = 0


def write_whole(path, data, mode=None):
    """PATH becomes DATA (str or bytes) in one rename: written to the file a
    link points at, under a temporary name of its own, with the old file's
    permissions (else 0644 under the umask), on disk before the rename"""
    global _seq
    if isinstance(data, str):
        data = data.encode()
    real = os.path.realpath(path)
    d = os.path.dirname(real) or "."
    os.makedirs(d, exist_ok=True)
    if mode is None:
        try:
            mode = os.stat(real).st_mode & 0o7777
        except OSError:
            umask = os.umask(0)
            os.umask(umask)
            mode = 0o644 & ~umask
    _seq += 1
    tmp = os.path.join(d, ".%s.tmp-%d-%d" % (os.path.basename(real), os.getpid(), _seq))
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        with os.fdopen(fd, "wb") as f:
            os.fchmod(f.fileno(), mode)
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, real)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise
    with contextlib.suppress(OSError):
        dfd = os.open(d, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(dfd)
        finally:
            os.close(dfd)


def read(path=None):
    """The settings as they are now: {} when missing, unreadable or not an object"""
    try:
        with open(path or config_path(), encoding="utf-8") as f:
            c = json.load(f)
    except (OSError, ValueError):
        return {}
    return c if isinstance(c, dict) else {}


def edit(change, path=None, missing=None):
    """config.json changed by change(dict) under the lock and written whole.
    change returns False when it changed nothing (nothing is written).
    A missing file starts from `missing` (default the shell's first start:
    {"welcome": {"done": false}}); a file that isn't valid settings is left
    alone (it's the shell's to set aside). True when it was written."""
    p = path or config_path()
    with Lock(p):
        try:
            with open(p, encoding="utf-8") as f:
                c = json.load(f)
        except FileNotFoundError:
            c = json.loads(json.dumps(missing)) if missing is not None else {"welcome": {"done": False}}
        except (OSError, ValueError):
            return False
        if not isinstance(c, dict):
            return False
        if change(c) is False:
            return False
        write_whole(p, text(c))
    return True


_GONE = object()


def rebase(base, planned, now):
    """What a script planned from `base` (the settings it read earlier),
    laid over the settings as they are `now`: the keys it changed (set,
    changed or taken out) take its values, every other key keeps now's (a
    change the shell made in between survives)"""
    if not (isinstance(base, dict) and isinstance(planned, dict) and isinstance(now, dict)):
        return planned
    out = dict(now)
    for k in set(base) | set(planned):
        b, p = base.get(k, _GONE), planned.get(k, _GONE)
        if b == p:
            continue
        if p is _GONE:
            out.pop(k, None)
        elif isinstance(b, dict) and isinstance(p, dict) and isinstance(now.get(k), dict):
            out[k] = rebase(b, p, now[k])
        else:
            out[k] = p
    return out


def replace(base, planned, path=None):
    """config.json becomes `planned` (made from `base`, read earlier), under
    the lock: when the file changed since `base` was read, only what the
    script changed is laid over it (rebase). True when it was written."""
    p = path or config_path()
    with Lock(p):
        try:
            with open(p, encoding="utf-8") as f:
                now = json.load(f)
        except FileNotFoundError:
            now = base
        except (OSError, ValueError):
            now = base
        out = planned if now == base else rebase(base, planned, now)
        write_whole(p, text(out))
    return True


def keep_newest(folder, prefix, n):
    """Only the newest n files in folder whose names start with prefix kept
    (by time changed; the rest removed); the removed ones' paths"""
    try:
        names = [x for x in os.listdir(folder) if x.startswith(prefix)]
    except OSError:
        return []
    paths = []
    for x in names:
        p = os.path.join(folder, x)
        try:
            if os.path.isfile(p) and not os.path.islink(p):
                paths.append((os.stat(p).st_mtime, x, p))
        except OSError:
            continue
    gone = []
    for _, _, p in sorted(paths, reverse=True)[n:]:
        with contextlib.suppress(OSError):
            os.unlink(p)
            gone.append(p)
    return gone


def put(c, dotted, value):
    """A dotted path ("dock.iconSize") set in c, sections made on the way (a
    section that isn't an object becomes one); False when it was already so"""
    keys = dotted.split(".")
    at = c
    for k in keys[:-1]:
        if not isinstance(at.get(k), dict):
            at[k] = {}
        at = at[k]
    if keys[-1] in at and at[keys[-1]] == value:
        return False
    at[keys[-1]] = value
    return True


def get(c, dotted, default=None):
    at = c
    for k in dotted.split("."):
        if not isinstance(at, dict) or k not in at:
            return default
        at = at[k]
    return at


def main(argv):
    if len(argv) >= 3 and argv[0] == "set" and len(argv) % 2 == 1:
        pairs = []
        for i in range(1, len(argv), 2):
            try:
                v = json.loads(argv[i + 1])
            except ValueError:
                v = argv[i + 1]
            pairs.append((argv[i], v))

        def change(c):
            return any([put(c, k, v) for k, v in pairs])
        try:
            edit(change)
        except OSError as e:
            print(f"settings_file: can't save {config_path()}: {e.strerror or e}", file=sys.stderr)
            return 1
        return 0
    print("usage: settings_file.py set KEY VALUE [KEY VALUE ...]", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
