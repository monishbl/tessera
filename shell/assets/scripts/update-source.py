#!/usr/bin/env python3
"""Locate the source checkout for both copied and linked shell installations,
or, for a copy that runs from a bundle (tools/tessera-bundle.py), its
current version: $XDG_DATA_HOME/tessera/current (the link, so the answer
stays right when an update swaps it)."""
import os
from pathlib import Path
import sys


def resolve(shell):
    # (the Material look runs from a folder of links in the cache: its
    # shell.qml leads back to the checkout, material/shell.qml)
    candidates = [os.environ.get('TESSERA_UPDATE_REPO', ''), str(Path(shell).resolve().parent),
                  str((Path(shell) / 'shell.qml').resolve().parent.parent)]
    bases = [os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')), str(Path.home() / '.local/share')]
    for base in bases:
        try:
            candidates.append((Path(base) / 'tessera/source').read_text().strip())
        except OSError:
            pass
    for candidate in candidates:
        if not candidate or not (Path(candidate) / 'tools/tessera-update.py').is_file():
            continue
        if (Path(candidate) / '.git').exists():
            return str(Path(candidate).resolve())
        if (Path(candidate) / 'bundle.json').is_file():
            real = Path(candidate).resolve()
            for base in bases:
                current = Path(base) / 'tessera/current'
                if current.is_symlink() and current.resolve() == real:
                    return str(current)
            return str(real)
    return ''


if __name__ == '__main__':
    print(resolve(sys.argv[1]))
