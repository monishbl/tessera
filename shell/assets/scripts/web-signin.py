#!/usr/bin/env python3
"""Microsoft 365's sign-in in tessera-web, Tessera's quick-look browser, for
GNOME Online Accounts (host/system accounts/cloud_accounts.rs,
connectMicrosoft; Rust only).

GOA's dialog (goa-signin ms_graph) opens Microsoft's sign-in page in the
default browser, and the browser following the redirect to goa-oauth2://...
hands the answer back to GOA (its goa-oauth2-handler). Here goa-signin runs
with one change: for it alone, web links open with this script. A
desktop-specific list (XDG_CURRENT_DESKTOP=tessera-signin, so
$XDG_CONFIG_HOME/tessera-signin-mimeapps.list, read by nobody else) names a
desktop file in the runtime folder, which is put first on XDG_DATA_DIRS.
That runs `web-signin.py open URL`: the page opens in
`tessera-web --signin URL --done goa-oauth2://`, the final address is read
from its JSON on stdout ({"event": "done", "url": ...}) and handed to GOA's
handler with xdg-open, as the browser would. Tessera still never sees a
password or a token: the address carries GOA's one-time code to GOA. No
tessera-web, or it fails to start: the default browser, as before.

Google is never signed in this way: Google refuses OAuth inside embedded web
views (403 disallowed_useragent), so goa-signin google keeps the default
browser. The user agent is never changed to get round that.

  web-signin.py goa GOA_SIGNIN PROVIDER TESSERA_WEB   run goa-signin so (its JSON line is printed as before)
  web-signin.py open URL                              the link handler

No third-party dependencies.
"""
import json
import os
import subprocess
import sys

DESKTOP = "tessera-signin"
HANDLER = "tessera-signin-web.desktop"
DONE = "goa-oauth2://"
TITLE = "Microsoft 365"


def runtime_folder(env):
    """Where the desktop file goes: the runtime folder's tessera-signin (0700)"""
    base = env.get("XDG_RUNTIME_DIR") or "/tmp"
    return os.path.join(base, "tessera-signin")


def config_home(env):
    v = env.get("XDG_CONFIG_HOME", "")
    return v if os.path.isabs(v) else os.path.join(env.get("HOME", ""), ".config")


def exec_quote(arg):
    """One argument of a desktop file's Exec, quoted as the spec says"""
    out = arg
    for c in ("\\", '"', "`", "$"):
        out = out.replace(c, "\\" + c)
    return '"' + out + '"'


def desktop_entry(script, python="python3"):
    return ("[Desktop Entry]\nType=Application\nName=Tessera sign-in\nNoDisplay=true\n"
            f"Exec={exec_quote(python)} -I {exec_quote(script)} open %u\n"
            "MimeType=x-scheme-handler/http;x-scheme-handler/https;\n")


def mimeapps():
    return (f"[Default Applications]\nx-scheme-handler/http={HANDLER}\nx-scheme-handler/https={HANDLER}\n"
            f"[Added Associations]\nx-scheme-handler/http={HANDLER};\nx-scheme-handler/https={HANDLER};\n")


def write(path, text):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(text)
    os.chmod(tmp, 0o600)
    os.replace(tmp, path)


def prepare(env, script, web):
    """The files, and goa-signin's environment: web links to this script"""
    rt = runtime_folder(env)
    apps = os.path.join(rt, "applications")
    os.makedirs(apps, mode=0o700, exist_ok=True)
    os.chmod(rt, 0o700)
    write(os.path.join(apps, HANDLER), desktop_entry(script))
    cfg = config_home(env)
    os.makedirs(cfg, exist_ok=True)
    write(os.path.join(cfg, DESKTOP + "-mimeapps.list"), mimeapps())
    out = dict(env)
    out["TESSERA_SIGNIN_DESKTOP"] = env.get("XDG_CURRENT_DESKTOP", "")
    out["TESSERA_SIGNIN_DATA"] = rt
    out["TESSERA_WEB"] = web
    out["XDG_CURRENT_DESKTOP"] = DESKTOP
    out["XDG_DATA_DIRS"] = rt + ":" + (env.get("XDG_DATA_DIRS") or "/usr/local/share:/usr/share")
    return out


def restored(env):
    """The environment as it was before goa-signin's change (what tessera-web
    and xdg-open get: a web link must reach the real browser, not this again)"""
    out = dict(env)
    if "TESSERA_SIGNIN_DESKTOP" in out:
        desk = out.pop("TESSERA_SIGNIN_DESKTOP")
        if desk:
            out["XDG_CURRENT_DESKTOP"] = desk
        else:
            out.pop("XDG_CURRENT_DESKTOP", None)
    rt = out.pop("TESSERA_SIGNIN_DATA", "")
    if rt and "XDG_DATA_DIRS" in out:
        dirs = [d for d in out["XDG_DATA_DIRS"].split(":") if d and d != rt]
        out["XDG_DATA_DIRS"] = ":".join(dirs) or "/usr/local/share:/usr/share"
    return out


def done_url(stdout):
    """The final address from tessera-web's JSON lines: GOA's redirect only"""
    for line in reversed(stdout.splitlines()):
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        if isinstance(ev, dict) and ev.get("event") == "done":
            url = ev.get("url")
            if isinstance(url, str) and url.startswith(DONE):
                return url
            return None
    return None


def closed(stdout):
    """tessera-web said it was closed without finishing"""
    for line in stdout.splitlines():
        try:
            if json.loads(line).get("event") == "closed":
                return True
        except (ValueError, AttributeError):
            continue
    return False


def open_url(url, env, run=subprocess.run):
    """The link handler: Microsoft's page in tessera-web, its answer to GOA;
    anything else, or no tessera-web, in the default browser"""
    env = restored(env)
    web = env.get("TESSERA_WEB", "")
    if url.startswith("https://") and web and os.path.isfile(web):
        try:
            p = run([web, "--signin", url, "--done", DONE, "--title", TITLE], env=env,
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        except OSError:
            p = None
        if p is not None:
            out = (p.stdout or b"").decode("utf-8", "replace")
            final = done_url(out)
            if final:
                # (GOA's own handler takes it, as from a browser)
                run(["xdg-open", final], env=env, stdin=subprocess.DEVNULL)
                return 0
            if p.returncode == 0 and closed(out):
                # (closed without signing in: GOA's dialog still waits; Cancel there)
                return 0
    run(["xdg-open", url], env=env, stdin=subprocess.DEVNULL)
    return 0


def main(argv):
    if len(argv) >= 2 and argv[0] == "open":
        return open_url(argv[1], os.environ)
    if len(argv) >= 4 and argv[0] == "goa":
        exe, provider, web = argv[1], argv[2], argv[3]
        env = prepare(os.environ, os.path.abspath(__file__), web) if web else dict(os.environ)
        os.execvpe(exe, [exe, provider], env)
    print(__doc__.strip().splitlines()[0], file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
