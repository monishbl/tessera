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

Models live in $XDG_DATA_HOME/tessera/whisper (ggml-MODEL.bin), downloaded
from whisper.cpp's own Hugging Face repository at a pinned revision and
kept only when their size and SHA-256 match (SUMS).
"""
import hashlib
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
# Each model's exact size and SHA-256 at the pinned revision below (Hugging
# Face's own LFS ids): a changed or damaged file is refused, never kept and
# handed to whisper.cpp's parser (#991). A new revision means new sums here.
REVISION = "5359861c739e955e79d9a303bcbc70fb988958b1"
SUMS = {
    "tiny": (77691713, "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
    "base": (147951465, "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
    "small": (487601967, "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
    "medium": (1533763059, "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
    "large-v3-turbo": (1624555275, "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
}
URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/" + REVISION + "/ggml-{}.bin"
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


class Mismatch(Exception):
    """Whole, but not the file whose checksum is pinned."""


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
            sha = hashlib.sha256()
            while True:
                chunk = r.read(1 << 20)
                if not chunk:
                    break
                if not first:
                    first = chunk[:4]
                f.write(chunk)
                sha.update(chunk)
                done += len(chunk)
                pct = done * 100 // total if total else done >> 24
                if pct != shown:
                    shown = pct
                    emit({"done": done, "total": total})
        size, want = SUMS[name]
        if (total and done != total) or (first == MAGIC and done < size):
            raise Incomplete()
        if first != MAGIC or done != size or sha.hexdigest() != want:
            raise Mismatch()
        os.replace(part, dest)
    except Exception as e:  # noqa: BLE001
        emit({"ok": False, "error": "The download didn't finish. Try again." if isinstance(e, Incomplete)
              else "The downloaded model wasn't the expected file, so it wasn't kept. Try again later."
              if isinstance(e, Mismatch)
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


def transcribe(wav, name, lang, run=subprocess.run):
    try:
        exe = binary()
        if not exe:
            return {"ok": False, "text": "", "error": "Dictation needs whisper-cpp. Install it in Settings → Dictation."}
        if name not in MODELS or not os.path.isfile(model_path(name)):
            return {"ok": False, "text": "", "error": "Download a speech model in Settings → Dictation."}
        threads = str(max(1, min(8, (os.cpu_count() or 4) - 1)))
        cmd = [exe, "-m", model_path(name), "-f", wav, "-l", lang or "auto", "-t", threads, "-nt", "-np"]
        try:
            r = run(cmd, capture_output=True, text=True, timeout=180)
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
    if len(argv) == 4 and argv[0] == "transcribe":
        r = transcribe(argv[1], argv[2], argv[3])
        print(json.dumps(r))
        return 0 if r["ok"] else 1
    print(json.dumps({"ok": False, "error": "usage: dictate.py status | download MODEL | remove MODEL | transcribe WAV MODEL LANG"}))
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Stopped:
        sys.exit(143)
