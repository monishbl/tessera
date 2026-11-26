#!/usr/bin/env python3
"""Dictation (services/Dictation.qml): speech to text on this computer, with
whisper.cpp (the whisper-cpp package) and one of its models. Nothing leaves
the computer.

  dictate.py status                JSON: {"binary": path | null, "dir",
                                   "models": {name: bool (downloaded)}}
  dictate.py download MODEL        fetches a model: JSON lines {"done", "total"}
                                   (bytes), ending with {"ok", "error"}
  dictate.py remove MODEL          deletes a downloaded model: {"ok", "error"}
  dictate.py transcribe WAV MODEL LANG
                                   the words in a recording (16 kHz mono WAV):
                                   {"ok", "text", "error"}; LANG is a code
                                   ("en") or "auto". The recording is deleted.
  dictate.py stream                the live words while you speak: one process
                                   for the whole dictation, fed requests on
                                   stdin, one JSON line each:
                                   {"id", "wav", "model", "lang"}; answers each
                                   with {"id", "ok", "text"} (the words so far:
                                   a draft, the final transcription is the
                                   `transcribe` above). Each recording is
                                   deleted; it ends when stdin closes.

Models live in $XDG_DATA_HOME/tessera/whisper (ggml-MODEL.bin), downloaded
from whisper.cpp's own Hugging Face repository.
"""
import json
import os
import re
import signal
import shutil
import subprocess
import sys
import urllib.request


def app_dir(base):
    """Tessera's folder under base."""
    return os.path.join(base, "tessera")

# name: (label, size in MB)
MODELS = {
    "tiny": ("Tiny", 75),
    "base": ("Base", 142),
    "small": ("Small", 466),
    "medium": ("Medium", 1500),
    "large-v3-turbo": ("Large (turbo)", 1620),
}
URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin"
# whisper.cpp's command-line tool, by the names packages give it
BINARIES = ["whisper-cli", "whisper-cpp", "whisper.cpp"]
MAGIC = b"lmgg"  # a ggml model file's first bytes


def model_dir():
    base = os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share")
    return os.path.join(app_dir(base), "whisper")


def model_path(name):
    return os.path.join(model_dir(), f"ggml-{name}.bin")


def binary():
    for b in BINARIES:
        p = shutil.which(b)
        if p:
            return p
    return None


def status():
    return {"binary": binary(), "dir": model_dir(),
            "models": {m: os.path.isfile(model_path(m)) for m in MODELS}}


def emit(obj):
    print(json.dumps(obj), flush=True)


class Incomplete(Exception):
    pass


def download(name, opener=urllib.request.urlopen):
    if name not in MODELS:
        emit({"ok": False, "error": "unknown model"})
        return 1
    os.makedirs(model_dir(), exist_ok=True)
    dest = model_path(name)
    part = dest + ".part"
    try:
        req = urllib.request.Request(URL.format(name), headers={"User-Agent": "Tessera"})
        with opener(req, timeout=30) as r, open(part, "wb") as f:
            total = int(r.headers.get("Content-Length") or 0)
            done, shown = 0, -1
            first = b""
            while True:
                chunk = r.read(1 << 20)
                if not chunk:
                    break
                if not first:
                    first = chunk[:4]
                f.write(chunk)
                done += len(chunk)
                pct = done * 100 // total if total else done >> 24
                if pct != shown:
                    shown = pct
                    emit({"done": done, "total": total})
        if first != MAGIC or (total and done != total):
            raise Incomplete()
        os.replace(part, dest)
    except Exception as e:  # noqa: BLE001
        emit({"ok": False, "error": "The download didn't finish. Try again." if isinstance(e, Incomplete)
              else "Couldn't download the model. Check your internet connection, then try again."})
        return 1
    finally:
        # (also when cancelled: Stopped below; a part can be 1.6 GB)
        try:
            os.unlink(part)
        except OSError:
            pass
    emit({"ok": True, "error": ""})
    return 0


def remove(name):
    if name not in MODELS:
        return {"ok": False, "error": "unknown model"}
    try:
        os.unlink(model_path(name))
    except FileNotFoundError:
        pass
    except OSError as e:
        return {"ok": False, "error": str(e)}
    return {"ok": True, "error": ""}


# Whisper's notes about sounds, not words: "[BLANK_AUDIO]", "(music)",
# "[ Silence ]", "*laughs*"
NOISE = re.compile(r"\[[^\]]*\]|\([^)]*\)|\*[^*]*\*")


def clean(out):
    """The words in whisper-cli's output (one segment per line)."""
    parts = []
    for line in out.splitlines():
        line = NOISE.sub("", line).strip()
        if line:
            parts.append(line)
    text = " ".join(parts)
    return re.sub(r"\s+", " ", text).strip()


def threads_for(partial):
    """whisper-cli's threads: all but one core (at most 8) for the words you
    asked for; half that for a live draft, which mustn't slow the computer
    (or the final words) down."""
    full = max(1, min(8, (os.cpu_count() or 4) - 1))
    return max(1, full // 2) if partial else full


def transcribe(wav, name, lang, run=subprocess.run, partial=False):
    try:
        exe = binary()
        if not exe:
            return {"ok": False, "text": "", "error": "Dictation needs whisper-cpp. Install it in Settings → Dictation."}
        if name not in MODELS or not os.path.isfile(model_path(name)):
            return {"ok": False, "text": "", "error": "Download a speech model in Settings → Dictation."}
        threads = str(threads_for(partial))
        cmd = [exe, "-m", model_path(name), "-f", wav, "-l", lang or "auto", "-t", threads, "-nt", "-np"]
        try:
            # (a draft that takes this long is out of date anyway)
            r = run(cmd, capture_output=True, text=True, timeout=20 if partial else 180)
        except subprocess.TimeoutExpired:
            return {"ok": False, "text": "", "error": "That took too long. Try a smaller model."}
        if r.returncode != 0:
            last = (r.stderr or "").strip().splitlines()[-1:] or ["whisper-cli failed"]
            return {"ok": False, "text": "", "error": last[0]}
        return {"ok": True, "text": clean(r.stdout), "error": ""}
    finally:
        try:
            os.unlink(wav)  # (the recording isn't kept)
        except OSError:
            pass


# A draft's recording: only ever the shell's own, in its runtime folder
PARTIAL_NAME = re.compile(r"^dictation-partial-[0-9]+\.wav$")


def stream_one(line, run=subprocess.run):
    """One live-words request (a JSON line) and its answer (a dict, or None
    for a line that isn't a request)."""
    try:
        req = json.loads(line)
    except ValueError:
        return None
    if not isinstance(req, dict):
        return None
    rid = req.get("id", 0)
    wav = req.get("wav")
    if not isinstance(wav, str) or not PARTIAL_NAME.match(os.path.basename(wav)):
        return {"id": rid, "ok": False, "text": ""}
    name, lang = str(req.get("model") or ""), str(req.get("lang") or "auto")
    r = transcribe(wav, name, lang, run=run, partial=True)
    return {"id": rid, "ok": r["ok"], "text": r["text"]}


def stream(inp=None, out=None, run=subprocess.run):
    """Answers requests one at a time until stdin closes. The shell sends
    the next only after this one's answer, so they never pile up; if several
    wait anyway, only the newest is worth the work (the others are older
    drafts of the same words) and theirs are answered as skipped."""
    inp = inp or sys.stdin
    out = out or sys.stdout
    pending = []
    for line in inp:
        line = line.strip()
        if not line:
            continue
        pending.append(line)
        # (whatever else is already waiting, without blocking for more)
        pending.extend(more_waiting(inp))
        *older, newest = pending
        pending = []
        for o in older:
            skip = stream_one_skip(o)
            if skip:
                out.write(json.dumps(skip) + "\n")
        r = stream_one(newest, run=run)
        if r is not None:
            out.write(json.dumps(r) + "\n")
        out.flush()
    return 0


def more_waiting(inp):
    """Lines already waiting on a pipe (none for anything else)."""
    lines = []
    try:
        import select
        fd = inp.fileno()
    except (AttributeError, OSError, ValueError):
        return lines
    while True:
        try:
            ready, _, _ = select.select([fd], [], [], 0)
        except (OSError, ValueError):
            break
        if not ready:
            break
        line = inp.readline()
        if not line:
            break
        if line.strip():
            lines.append(line.strip())
    return lines


def stream_one_skip(line):
    """An older request passed over: its recording deleted, answered as skipped."""
    try:
        req = json.loads(line)
    except ValueError:
        return None
    if not isinstance(req, dict):
        return None
    wav = req.get("wav")
    if isinstance(wav, str) and PARTIAL_NAME.match(os.path.basename(wav)):
        try:
            os.unlink(wav)
        except OSError:
            pass
    return {"id": req.get("id", 0), "ok": False, "text": "", "skipped": True}


class Stopped(BaseException):
    """SIGTERM (Cancel): raised so the `finally`s run, and subprocess.run
    ends whisper-cli instead of leaving it working."""


def stopped(*_):
    raise Stopped()


def main(argv):
    signal.signal(signal.SIGTERM, stopped)
    if argv[:1] == ["status"]:
        print(json.dumps(status()))
        return 0
    if len(argv) == 2 and argv[0] == "download":
        return download(argv[1])
    if len(argv) == 2 and argv[0] == "remove":
        r = remove(argv[1])
        print(json.dumps(r))
        return 0 if r["ok"] else 1
    if argv == ["stream"]:
        return stream()
    if len(argv) == 4 and argv[0] == "transcribe":
        r = transcribe(argv[1], argv[2], argv[3])
        print(json.dumps(r))
        return 0 if r["ok"] else 1
    print(json.dumps({"ok": False, "error": "usage: dictate.py status | download MODEL | remove MODEL | transcribe WAV MODEL LANG | stream"}))
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Stopped:
        sys.exit(143)
