#!/usr/bin/env python3
"""Decode missing cliphist previews once and publish complete, stable paths."""
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile


def prune(directory, keep):
    """Removes previews of entries no longer listed: the folder is in RAM
    (XDG_RUNTIME_DIR), and every image copied added one for good."""
    for path in directory.iterdir():
        if path.is_file() and str(path) not in keep and not path.name.startswith("tmp"):
            try:
                path.unlink()
            except OSError:
                pass


def own_folder(directory):
    """A real folder (not a link to one) that is yours: else another user
    could point the name at one of your folders, and prune would empty it."""
    try:
        st = os.lstat(directory)
    except OSError:
        return False
    return stat.S_ISDIR(st.st_mode) and st.st_uid == os.getuid()


def previews(db, directory, pairs):
    pairs = list(pairs)
    directory = Path(directory)
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not own_folder(directory):
        return
    os.chmod(directory, 0o700)
    prune(directory, {str(Path(t)) for _, t in pairs})
    for identifier, target in pairs:
        path = Path(target)
        if path.parent != directory or not identifier.isdigit():
            continue
        if not path.exists() or not path.stat().st_size:
            fd, tmp = tempfile.mkstemp(dir=directory)
            try:
                with os.fdopen(fd, 'wb') as output:
                    result = subprocess.run(['cliphist', '-db-path', db, 'decode', identifier],
                                            stdout=output, stderr=subprocess.DEVNULL, timeout=20)
                if result.returncode or not os.path.getsize(tmp):
                    continue
                os.replace(tmp, path)
            except (OSError, subprocess.TimeoutExpired):
                continue
            finally:
                if os.path.exists(tmp):
                    os.unlink(tmp)
        print(str(path), flush=True)


if __name__ == '__main__':
    previews(sys.argv[1], sys.argv[2], zip(sys.argv[3::2], sys.argv[4::2]))
