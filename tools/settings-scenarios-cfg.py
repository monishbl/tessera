import json, os, sys
src, dst, p, look = sys.argv[1:]
c = json.load(open(src))
c.setdefault("rust", {})["standalone"] = p == "rust"
c["rust"]["surfaces"] = []
# (a fresh Rust install's file has no "appearance" section yet)
c.setdefault("appearance", {})["look"] = look
for k in ("ai", "google", "cloud", "phone"):   # (accounts and keys stay out of the arena)
    c.pop(k, None)
# (nothing that names you, holds a key or is tied to your hardware either,
# as site/build.sh: it can show in a recording)
for sec, key in (("sharing", "name"), ("today", "name"), ("weather", "location"), ("emoji", "klipyKey"),
                 ("notes", "folder"), ("desktop", "photosFolder")):
    if isinstance(c.get(sec), dict) and key in c[sec]:
        c[sec][key] = ""
for sec, key in (("lock", "faceDevice"), ("lock", "faceCount")):
    if isinstance(c.get(sec), dict):
        c[sec].pop(key, None)
fd = os.open(dst, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.fchmod(fd, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump(c, f, indent=4)
