#!/usr/bin/env python3
"""Merge the shell's UI state journals into state.json (common/UiState.qml).

Each change the shell makes is first written to its own small journal file
under pending/, so a crash or a quit can't lose it. This folds them into the
shared state.json, under a lock (two shells can share the file: the dev
sandbox and a tty session), field by field, keeping the newest revision of
each, so neither shell overwrites the other's unrelated changes.

A state.json that can't be read as an object is set aside as
state.json.corrupt-<time> (the newest KEEP_CORRUPT kept) and the merge starts
from empty: one bad file must not stop the shell from loading and saving its
state ever again. One that exists but can't be opened is left alone (nothing
is merged, the journals wait). A journal that can't be read, and isn't one
being written right now, goes to pending/bad/ (the newest KEEP_BAD kept):
never erased, and not read again at every merge for good.
"""
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile
import time

KEEP_CORRUPT = 3
KEEP_BAD = 10
SETTLED = 60  # s: a journal older than this isn't one still being written


def keep_newest(paths, n):
    for old in sorted(paths, key=lambda q: q.stat().st_mtime, reverse=True)[n:]:
        old.unlink(missing_ok=True)


def merge(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(directory, 0o700)
    pending = directory / 'pending'
    pending.mkdir(exist_ok=True, mode=0o700)
    os.chmod(pending, 0o700)
    with open(directory / '.state.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = directory / 'state.json'
        try:
            state = json.loads(path.read_text())
        except FileNotFoundError:
            state = {}
        except (ValueError, UnicodeDecodeError):
            state = None
        except OSError as e:
            # (there, but not readable: merging would replace it with what
            # the journals hold alone)
            print(f'state-merge: state.json can\'t be read ({e.strerror or e}); nothing merged', file=sys.stderr)
            return 1
        if not isinstance(state, dict):
            os.replace(path, path.with_name('state.json.corrupt-' + time.strftime('%Y%m%d-%H%M%S')))
            keep_newest(directory.glob('state.json.corrupt-*'), KEEP_CORRUPT)
            state = {}
        if not isinstance(state.get('_tessera_revisions'), dict):
            state['_tessera_revisions'] = {}
        revisions = state.setdefault('_tessera_revisions', {})
        consumed = []
        bad = []
        for journal in pending.glob('*.json'):
            try:
                update = json.loads(journal.read_text())
                # (all of a journal or none of it: a bad entry after good
                # ones left those applied and the journal kept)
                changes = {}
                for key, change in update.items():
                    if key.startswith('_'):
                        continue
                    if change['revision'] > revisions.get(key, [0, '', 0]):
                        changes[key] = (change['revision'], change['value'])
            except (ValueError, KeyError, TypeError, AttributeError, OSError):
                # Never erase a malformed/unreadable recovery journal: set
                # aside once it has settled (not one being written now)
                try:
                    if time.time() - journal.stat().st_mtime > SETTLED:
                        bad.append(journal)
                except OSError:
                    pass
                continue
            for key, (revision, value) in changes.items():
                state[key] = value
                revisions[key] = revision
            consumed.append(journal)
        fd, temp = tempfile.mkstemp(dir=directory)
        try:
            with os.fdopen(fd, 'w') as out:
                json.dump(state, out, ensure_ascii=False)
                out.flush()
                os.fsync(out.fileno())
            os.replace(temp, path)
        finally:
            if os.path.exists(temp): os.unlink(temp)
        for journal in consumed:
            journal.unlink(missing_ok=True)
        if bad:
            aside = pending / 'bad'
            aside.mkdir(exist_ok=True, mode=0o700)
            for journal in bad:
                os.replace(journal, aside / journal.name)
            print(f'state-merge: {len(bad)} unreadable journal(s) set aside in {aside}', file=sys.stderr)
            keep_newest(aside.glob('*.json'), KEEP_BAD)
    return 0


if __name__ == '__main__':
    sys.exit(merge(sys.argv[1]))
