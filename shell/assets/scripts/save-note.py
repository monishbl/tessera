#!/usr/bin/env python3
"""Publish a complete note without replacing another note on a name collision."""
import os
from pathlib import Path
import sys
import tempfile


def save(path, content):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.note-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as out:
            out.write(content)
            out.flush()
            os.fsync(out.fileno())
        try:
            os.link(temporary, path)  # atomic publication; fails if destination exists
        except FileExistsError:
            raise
        except OSError:
            # (no hard links: exFAT/FAT sticks, FUSE mounts such as the
            # Google Drive and OneDrive folders) created only if it isn't
            # there, then written: never replaces another note
            exclusive_write(path, content)
    finally:
        os.unlink(temporary)


def exclusive_write(path, content):
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        with os.fdopen(fd, 'w') as out:
            out.write(content)
            out.flush()
            os.fsync(out.fileno())
    except BaseException:
        os.unlink(path)  # (half a note is no note)
        raise


if __name__ == '__main__':
    save(sys.argv[1], sys.stdin.read())
