#!/usr/bin/env python3
"""Writes Tessera's default keymap and gestures (hypr/tessera/keymap.json and
gestures.json). The defaults follow end4 (illogical-impulse) wherever both
have the same action; Tessera-only actions sit on keys end4 leaves free.

Each bind: id (stable, used by the user's changes), keys ("$MOD" is Super, or
Alt in the sandbox), group and label (Settings and the cheatsheet), action,
and options. Actions:
  {"shell": name}   a Tessera action (Hyprland global "tessera:<name>")
  {"app": role}     the default app for a role (Settings → Default apps)
  {"exec": cmd}     a command
  {"dsp": lua}      a Hyprland dispatcher, as Lua (e.g. hl.dsp.window.close())
  {"zoom": step}    screen zoom in or out
  {"space": op, "n": N} / {"space": op, "step": ±1}
                    space N (or the next/previous one) on the display you're
                    on; op is focus, send (the window, staying), move (the
                    window, following), new (a new space at N, the later ones
                    one along) or newmove (the same with the window). Spaces
                    are dynamic: 1..n with no gaps, past the last a new one
                    (keybinds.lua's TESSERA_SPACE; tessera-ui's
                    host/desktop/src/wm/dynspace.rs plans them)
Run again after editing: tools/gen-keymap.py
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
binds = []


def b(id, keys, group, label, action, **opts):
    e = {"id": id, "keys": keys, "group": group, "label": label, "action": action}
    if opts:
        e["opts"] = opts
    binds.append(e)


def shell(n):
    return {"shell": n}


def dsp(l):
    return {"dsp": l}


def space(op, n=None, step=None):
    return {"space": op, "n": n} if n is not None else {"space": op, "step": step}


# ── Shell ──────────────────────────────────────────────────────────────────
# A lone Super tap: Overview, or what Settings chose (the shell decides;
# skipped when another key was pressed)
b("spotlight-tap", "$MOD + $MODKEY", "Shell", "Tap Super (Overview, or as set in Settings)", shell("superTap"))
b("spotlight", "$MOD + Space", "Shell", "Search", shell("spotlight"))
b("overview", "$MOD + Tab", "Shell", "Overview", shell("overview"))
b("switcher", "ALT + Tab", "Shell", "Switch windows", shell("switcher"))
b("switcher-back", "ALT + SHIFT + Tab", "Shell", "Switch windows backwards", shell("switcherBack"))
# Finishes a quick Alt+Tab tap that ends before the switcher has the keyboard
# (a release bind that lets apps still see Alt; does nothing otherwise)
b("switcher-commit", "ALT + Alt_L", "Shell", "Switch windows (finish when Alt is let go)", shell("switcherCommit"), release=True, non_consuming=True, transparent=True, ignore_mods=True)
b("mission-control", "$MOD + SHIFT + Tab", "Shell", "All Windows", shell("missionControl"))
# (Super+Shift+Space is circle to search, below: the user's choice)
b("launchpad", "$MOD + CTRL + Space", "Shell", "App Drawer", shell("launchpad"))
b("clipboard", "$MOD + V", "Shell", "Clipboard history", shell("clipboard"))
b("shareSelection", "$MOD + CTRL + S", "Shell", "Share the selection", shell("shareSelection"))
b("replyLatest", "$MOD + CTRL + N", "Shell", "Reply to the newest message", shell("replyLatest"))
b("emoji", "$MOD + Period", "Shell", "Emoji and symbols", shell("emoji"))
b("sidebar", "$MOD + A", "Shell", "AI and tools sidebar", shell("sidebar"))
b("sidebar-b", "$MOD + B", "Shell", "AI and tools sidebar (alternative)", shell("sidebar"))
b("sidebar-o", "$MOD + O", "Shell", "AI and tools sidebar (alternative)", shell("sidebar"))
b("notifications", "$MOD + N", "Shell", "Notification Center", shell("notificationCenter"))
b("control-center", "$MOD + ALT + C", "Shell", "Quick Settings", shell("controlCenter"))
b("settings", "$MOD + I", "Shell", "Settings", shell("settings"))
b("settings-comma", "$MOD + Comma", "Shell", "Settings (second key)", shell("settings"))
b("cheatsheet", "$MOD + Slash", "Shell", "Keyboard shortcuts", shell("cheatsheet"))
b("keyboard", "$MOD + K", "Shell", "On-screen keyboard", shell("keyboard"))
b("overlay", "$MOD + G", "Shell", "Overlay widgets", shell("overlay"))
b("wallpaper", "CTRL + $MOD + T", "Shell", "Change wallpaper", shell("wallpaper"))
b("dark-mode", "CTRL + $MOD + SHIFT + D", "Shell", "Light or dark appearance", shell("darkMode"))
b("quick-note", "$MOD + ALT + N", "Shell", "Quick note", shell("quickNote"))
b("session", "CTRL + ALT + Delete", "Shell", "Power options", shell("session"))
b("reset", "$MOD + Escape", "Shell", "Close everything (if something is stuck)", shell("reset"))
b("repair", "CTRL + $MOD + R", "Shell", "Restart the shell (apps keep running)", {"exec": "setsid -f \"$TESSERA_SHELL/assets/scripts/repair-shell.sh\""})
b("repair-escape", "$MOD + CTRL + Escape", "Shell", "Restart the shell (alternative)", {"exec": "setsid -f \"$TESSERA_SHELL/assets/scripts/repair-shell.sh\""})

# ── Screen ─────────────────────────────────────────────────────────────────
b("snip", "$MOD + SHIFT + S", "Screen", "Screenshot of an area", shell("screenshotRegion"))
b("lens", "$MOD + SHIFT + A", "Screen", "Search an area with Google Lens", shell("lens"))
# (held down, Dictation listens until the key is let go: keybinds.lua binds the release too)
b("dictate", "$MOD + H", "Shell", "Dictation (tap to start and stop, or hold while you speak)", {"shell": "dictate", "release": "dictateRelease"})
b("captions", "$MOD + CTRL + L", "Shell", "Live captions on or off", shell("captions"))
b("shelf", "$MOD + ALT + D", "Shell", "Drop shelf", shell("shelf"))
b("ocr", "$MOD + SHIFT + X", "Screen", "Copy text from the screen", shell("ocr"))
b("translate", "$MOD + SHIFT + T", "Screen", "Translate text on the screen", shell("screenTranslate"))
b("ask-screen", "$MOD + ALT + A", "Screen", "Ask AI about part of the screen", shell("askScreen"))
b("circle-search", "$MOD + SHIFT + Space", "Screen", "Circle to search", shell("circleSearch"))
b("color-picker", "$MOD + SHIFT + C", "Screen", "Pick a colour", {"exec": "hyprpicker -a -r"})
# (capture keys are not bound on the lock screen: anyone at a locked laptop
# could fill ~/Pictures or start a recording that keeps running after unlock)
b("record", "$MOD + SHIFT + R", "Screen", "Record an area (again to stop)", shell("record"))
b("record-alt", "$MOD + ALT + R", "Screen", "Record an area (alternative)", shell("record"))
b("screenshot-print", "Print", "Screen", "Screenshot of the screen", shell("screenshotScreen"))
b("screenshot-ctrl-print", "CTRL + Print", "Screen", "Screenshot of the screen (alternative)", shell("screenshotScreen"))
b("screen-tools", "$MOD + Print", "Screen", "Screenshot and recording tools", shell("screenTools"))
b("zoom-in", "$MOD + Equal", "Screen", "Zoom in", {"zoom": 0.3}, repeating=True)
b("zoom-out", "$MOD + Minus", "Screen", "Zoom out", {"zoom": -0.3}, repeating=True)
b("zoom-in-pad", "$MOD + code:86", "Screen", "Zoom in (keypad)", {"zoom": 0.3}, repeating=True)
b("zoom-out-pad", "$MOD + code:82", "Screen", "Zoom out (keypad)", {"zoom": -0.3}, repeating=True)

# ── Media ──────────────────────────────────────────────────────────────────
b("media-play", "$MOD + SHIFT + P", "Media", "Play or pause", {"exec": "playerctl play-pause"}, locked=True)
b("media-next", "$MOD + SHIFT + N", "Media", "Next track", {"exec": "playerctl next"}, locked=True)
b("media-prev", "$MOD + SHIFT + B", "Media", "Previous track", {"exec": "playerctl previous"}, locked=True)
b("mute", "$MOD + SHIFT + M", "Media", "Mute sound", shell("volumeMute"), locked=True)
b("mic-mute", "$MOD + ALT + M", "Media", "Mute microphone", shell("micMute"), locked=True)
b("vol-up", "XF86AudioRaiseVolume", "Media", "Volume up", shell("volumeUp"), locked=True, repeating=True)
b("vol-down", "XF86AudioLowerVolume", "Media", "Volume down", shell("volumeDown"), locked=True, repeating=True)
b("vol-mute", "XF86AudioMute", "Media", "Mute (key)", shell("volumeMute"), locked=True)
b("mic-key", "XF86AudioMicMute", "Media", "Mute microphone (key)", shell("micMute"), locked=True)
b("bright-up", "XF86MonBrightnessUp", "Media", "Brightness up", shell("brightnessUp"), locked=True, repeating=True)
b("bright-down", "XF86MonBrightnessDown", "Media", "Brightness down", shell("brightnessDown"), locked=True, repeating=True)
# The same on F-keys, for keyboards without media keys
b("vol-up-f12", "$MOD + F12", "Media", "Volume up (without media keys)", shell("volumeUp"), locked=True, repeating=True)
b("vol-down-f11", "$MOD + F11", "Media", "Volume down (without media keys)", shell("volumeDown"), locked=True, repeating=True)
b("bright-up-f2", "$MOD + F2", "Media", "Brightness up (without media keys)", shell("brightnessUp"), locked=True, repeating=True)
b("bright-down-f1", "$MOD + F1", "Media", "Brightness down (without media keys)", shell("brightnessDown"), locked=True, repeating=True)
b("play-key", "XF86AudioPlay", "Media", "Play or pause (key)", {"exec": "playerctl play-pause"}, locked=True)
b("pause-key", "XF86AudioPause", "Media", "Pause (key)", {"exec": "playerctl play-pause"}, locked=True)
b("next-key", "XF86AudioNext", "Media", "Next track (key)", {"exec": "playerctl next"}, locked=True)
b("prev-key", "XF86AudioPrev", "Media", "Previous track (key)", {"exec": "playerctl previous"}, locked=True)

# ── Windows ────────────────────────────────────────────────────────────────
b("close", "$MOD + Q", "Window", "Close", dsp("hl.dsp.window.close()"))
b("close-altf4", "ALT + F4", "Window", "Close (alternative)", dsp("hl.dsp.window.close()"))
b("kill", "$MOD + SHIFT + ALT + Q", "Window", "Force quit", {"exec": "hyprctl kill"})
b("fullscreen", "$MOD + F", "Window", "Full screen", dsp('hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })'))
b("maximize", "$MOD + D", "Window", "Maximise (zoom)", dsp('hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" })'))
b("fullscreen-spoof", "$MOD + ALT + F", "Window", "Full screen inside its tile", dsp('hl.dsp.window.fullscreen_state({ internal = 0, client = 3, action = "toggle" })'))
b("float", "$MOD + ALT + Space", "Window", "Float or tile", dsp('hl.dsp.window.float({ action = "toggle" })'))
b("pin", "$MOD + P", "Window", "Keep on all workspaces", dsp("hl.dsp.window.pin()"))
b("pip", "$MOD + ALT + P", "Window", "Picture in Picture", shell("pip"))
b("tabs", "$MOD + ALT + T", "Window", "Merge into tabs / move tab out", shell("tabs"))
b("tab-next", "$MOD + bracketright", "Window", "Next tab", dsp("hl.dsp.group.next()"))
b("tab-prev", "$MOD + bracketleft", "Window", "Previous tab", dsp("hl.dsp.group.prev()"))
b("split-less", "$MOD + Semicolon", "Window", "Shrink split", dsp('hl.dsp.layout("splitratio -0.1")'), repeating=True)
b("split-more", "$MOD + Apostrophe", "Window", "Grow split", dsp('hl.dsp.layout("splitratio +0.1")'), repeating=True)
b("drag", "$MOD + mouse:272", "Window", "Move with the mouse", dsp("hl.dsp.window.drag()"), mouse=True)
b("drag-middle", "$MOD + mouse:274", "Window", "Move with the mouse (middle button)", dsp("hl.dsp.window.drag()"), mouse=True)
b("resize", "$MOD + mouse:273", "Window", "Resize with the mouse", dsp("hl.dsp.window.resize()"), mouse=True)
for key, d in (("Left", "l"), ("Right", "r"), ("Up", "u"), ("Down", "d")):
    b("focus-" + key.lower(), "$MOD + " + key, "Window", "Focus " + key.lower(), dsp('hl.dsp.focus({ direction = "%s" })' % d))
    b("move-" + key.lower(), "$MOD + SHIFT + " + key, "Window", "Move " + key.lower(), dsp('hl.dsp.window.move({ direction = "%s" })' % d))
for name, label in (("left", "Tile left half"), ("right", "Tile right half")):
    b("snap-" + name, "$MOD + CTRL + ALT + " + name.capitalize(), "Window", label, shell("snap-" + name))
b("snap-top", "$MOD + CTRL + Up", "Window", "Tile top half", shell("snap-top"))
b("snap-bottom", "$MOD + CTRL + Down", "Window", "Tile bottom half", shell("snap-bottom"))
b("snap-fill", "$MOD + CTRL + F", "Window", "Fill the screen", shell("snap-fill"))
b("snap-center", "$MOD + CTRL + C", "Window", "Centre", shell("snap-center"))
b("snap-restore", "$MOD + CTRL + BackSpace", "Window", "Back to the previous size", shell("snap-restore"))

# ── Workspaces ─────────────────────────────────────────────────────────────
for i in range(1, 11):
    k = str(i % 10)
    b("ws-%d" % i, "$MOD + " + k, "Workspace", "Go to space %d" % i, space("focus", n=i))
    b("ws-send-%d" % i, "$MOD + ALT + " + k, "Workspace", "Send window to space %d" % i, space("send", n=i))
    # (Super+Shift+N until 27 Nov: keybinds.lua and keymap.rs read a saved
    # entry still saying that as the default, OLD_DEFAULTS)
    b("ws-move-%d" % i, "$MOD + CTRL + ALT + " + k, "Workspace", "Move window to space %d (and follow)" % i, space("move", n=i))
    b("ws-new-%d" % i, "$MOD + SHIFT + " + k, "Workspace", "New space at %d" % i, space("new", n=i))
    b("ws-new-move-%d" % i, "$MOD + SHIFT + ALT + " + k, "Workspace", "New space at %d with this window" % i, space("newmove", n=i))
b("ws-prev", "CTRL + $MOD + Left", "Workspace", "Previous space", space("focus", step=-1))
b("ws-next", "CTRL + $MOD + Right", "Workspace", "Next space", space("focus", step=1))
b("ws-send-prev", "$MOD + SHIFT + Page_Up", "Workspace", "Move window to the previous space", space("move", step=-1))
b("ws-send-next", "$MOD + SHIFT + Page_Down", "Workspace", "Move window to the next space", space("move", step=1))
b("ws-scroll-down", "$MOD + mouse_down", "Workspace", "Next space (scroll)", space("focus", step=1))
b("ws-scroll-up", "$MOD + mouse_up", "Workspace", "Previous space (scroll)", space("focus", step=-1))
b("scratchpad", "$MOD + S", "Workspace", "Show or hide the scratchpad", dsp('hl.dsp.workspace.toggle_special("special")'))
b("scratchpad-mouse", "$MOD + mouse:275", "Workspace", "Show or hide the scratchpad (mouse)", dsp('hl.dsp.workspace.toggle_special("special")'))
b("scratchpad-send", "$MOD + ALT + S", "Workspace", "Send window to the scratchpad", dsp('hl.dsp.window.move({ workspace = "special:special", follow = false })'))

# ── Session ────────────────────────────────────────────────────────────────
b("lock", "$MOD + L", "Session", "Lock", shell("lock"))
b("lock-mac", "$MOD + CTRL + Q", "Session", "Lock (second key)", shell("lock"))
b("sleep", "$MOD + SHIFT + L", "Session", "Sleep", {"exec": "systemctl suspend || loginctl suspend"}, locked=True)
b("poweroff", "CTRL + SHIFT + ALT + $MOD + Delete", "Session", "Shut down", {"exec": "systemctl poweroff || loginctl poweroff"})

# ── Apps ───────────────────────────────────────────────────────────────────
b("terminal", "$MOD + Return", "Apps", "Terminal", {"app": "terminal"})
b("terminal-t", "$MOD + T", "Apps", "Terminal (alternative)", {"app": "terminal"})
b("terminal-ctrl", "CTRL + ALT + T", "Apps", "Terminal (alternative)", {"app": "terminal"})
b("files", "$MOD + E", "Apps", "Files", {"app": "files"})
b("browser", "$MOD + W", "Apps", "Browser", {"app": "browser"})
b("code", "$MOD + C", "Apps", "Code editor", {"app": "code"})
b("text", "$MOD + X", "Apps", "Text editor", {"app": "text"})
b("office", "CTRL + $MOD + SHIFT + ALT + W", "Apps", "Office", {"app": "office"})
b("mixer", "CTRL + $MOD + V", "Apps", "Volume mixer", {"app": "mixer"})
b("tasks", "CTRL + SHIFT + Escape", "Apps", "Task manager", {"app": "tasks"})

# ── Gestures ───────────────────────────────────────────────────────────────
# end4's set, plus Tessera's on finger counts end4 leaves free.
# action: a Hyprland gesture action ("workspace", "move", "fullscreen", ...)
# or {"shell": name}
gestures = [
    {"id": "move", "fingers": 3, "direction": "swipe", "label": "Move the window under the fingers", "action": "move"},
    {"id": "fullscreen", "fingers": 3, "direction": "pinch", "label": "Full screen", "action": "fullscreen"},
    {"id": "workspaces", "fingers": 4, "direction": "horizontal", "label": "Switch workspace", "action": "workspace"},
    {"id": "overview-up", "fingers": 4, "direction": "up", "label": "Overview", "action": {"shell": "overview"}},
    {"id": "overview-down", "fingers": 4, "direction": "down", "label": "Overview", "action": {"shell": "overview"}},
    {"id": "launchpad", "fingers": 4, "direction": "pinchin", "label": "App Drawer", "action": {"shell": "launchpad"}},
    {"id": "show-desktop", "fingers": 4, "direction": "pinchout", "label": "Show desktop", "action": {"shell": "showDesktop"}},
    # A long press (fingers resting still): the tessera-glass plugin sees it
    # (hold.cpp), gestures.lua runs it; Tessera actions only
    {"id": "circle-search", "fingers": 3, "direction": "hold", "label": "Circle to search", "action": {"shell": "circleSearch"}},
]

ids = [x["id"] for x in binds]
assert len(ids) == len(set(ids)), "duplicate bind ids"
# (a combination is its parts in any order: "$MOD + CTRL + S" and
# "CTRL + $MOD + S" are one, and Hyprland runs both binds; audit #63)
def combo(k):
    return tuple(sorted(p.strip().upper() for p in k.split("+")))


keys = [combo(x["keys"]) for x in binds]
assert len(keys) == len(set(keys)), "two binds on one key: " + str({k for k in keys if keys.count(k) > 1})
# ── docs/FEATURES.md's tables: from the binds above, never by hand (audit
# #59: they said Super+C was Quick Settings, Super+Shift+3 a screenshot…).
# tests/test_keymap.py checks the file says this.
DOC_KEYS = [
    (["spotlight-tap"], "Overview of spaces, with search (Super tapped alone; Settings can change it)"),
    (["spotlight"], "Search"),
    (["launchpad"], "App Drawer"),
    (["clipboard"], "Clipboard history"),
    (["overview", "mission-control"], "Overview / All Windows"),
    (["sidebar"], "AI and tools sidebar"),
    (["control-center", "notifications"], "Quick Settings / Notification Center"),
    (["settings-comma"], "Settings"),
    (["cheatsheet"], "All keyboard shortcuts"),
    (["screenshot-print", "snip", "screen-tools"], "Screenshot of the screen / of an area / screenshot and recording tools"),
    (["reset"], "Close everything (if a panel is stuck)"),
    (["repair-escape"], "Repair the shell: duplicated bars or a frozen shell. Restarts only the shell; apps and windows keep running (also in the system menu)"),
    (["ocr", "translate"], "Copy text from the screen / translate text on the screen"),
    (["ask-screen"], "Ask AI about part of the screen"),
    (["lens"], "Search an area with Google Lens (sends it to Google)"),
    (["circle-search"], "Circle to search"),
    (["record"], "Start or stop recording an area"),
    (["overlay", "keyboard"], "Overlay widgets / on-screen keyboard"),
    (["lock", "lock-mac"], "Lock"),
    (["close", "fullscreen", "maximize"], "Close, full screen, zoom window"),
    (["snap-left", "snap-right"], "Tile to the left / right half"),
    (["snap-fill", "snap-center", "snap-restore"], "Fill, centre, back to the previous size"),
    (["pip"], "Picture in Picture"),
    (["tabs", "tab-prev", "tab-next"], "Merge into tabs (or move a tab out), previous / next tab"),
    (["ws-1", "ws-9"], "Spaces 1 to 9 (the display you're on); past the last, a new one", " … "),
    (["ws-new-1", "ws-new-9"], "A new space at 1 to 9 (the ones after it move along)", " … "),
    (["ws-move-1", "ws-move-9"], "Move the window to space 1 to 9 and follow it", " … "),
    (["ws-prev", "ws-next"], "Previous / next space"),
    (["ws-scroll-down"], "Next or previous space (scroll)"),
]
DOC_GESTURES_AFTER = [
    ("Three-finger drag (Settings → Trackpad)", "Move the window under your fingers; the 3-finger gestures move to 4 fingers"),
    ("Touchscreen: pull down from the menu bar", "Quick Settings (right third) or Notification Center"),
    ("Touchscreen: swipe up from the bottom edge", "Show a hidden dock"),
]
NAMES = {"$MOD": "Super", "$MODKEY": "(tap)", "CTRL": "Ctrl", "SHIFT": "Shift", "ALT": "Alt", "Comma": ",", "Period": ".",
         "Slash": "/", "BackSpace": "Backspace", "bracketleft": "[", "bracketright": "]", "Return": "Enter",
         "mouse_down": "scroll", "mouse_up": "scroll", "Escape": "Escape"}
ORDER = ["Super", "Ctrl", "Alt", "Shift"]


def key_text(keys):
    """'$MOD + SHIFT + S' as the docs write it: 'Super+Shift+S'"""
    parts = [NAMES.get(p.strip(), p.strip()) for p in keys.split("+")]
    mods = sorted((p for p in parts if p in ORDER), key=ORDER.index)
    rest = [p for p in parts if p not in ORDER]
    if rest == ["(tap)"]:
        return "Super (tap)"
    return "+".join(mods + rest)


def doc_tables(binds, gestures):
    by_id = {b["id"]: b for b in binds}
    out = ["| Keys | Action |", "|---|---|"]
    for row in DOC_KEYS:
        ids, label, sep = row if len(row) == 3 else (*row, " / ")
        out.append("| " + sep.join(key_text(by_id[i]["keys"]) for i in ids) + " | " + label + " |")
    out += ["", "| Gesture | Action |", "|---|---|"]
    how = {"swipe": "swipe", "pinch": "pinch", "horizontal": "left or right", "up": "up", "down": "down",
           "pinchin": "pinch", "pinchout": "spread", "hold": "long press"}
    for g in gestures:
        note = " (needs Tessera's glass plugin)" if g["direction"] == "hold" else ""
        out.append("| %d fingers, %s | %s%s |" % (g["fingers"], how.get(g["direction"], g["direction"]), g["label"], note))
    out += ["| %s | %s |" % r for r in DOC_GESTURES_AFTER]
    return "\n".join(out) + "\n"


DOC = os.path.join(ROOT, "docs", "FEATURES.md")
START, END = "<!-- keymap: written by tools/gen-keymap.py -->\n", "<!-- keymap end -->\n"
with open(DOC) as f:
    doc = f.read()
if START in doc and END in doc:
    head, rest = doc.split(START, 1)
    doc = head + START + doc_tables(binds, gestures) + END + rest.split(END, 1)[1]
    with open(DOC, "w") as f:
        f.write(doc)
    print("wrote", DOC)

for name, data in (("keymap.json", binds), ("gestures.json", gestures)):
    path = os.path.join(ROOT, "hypr", "tessera", name)
    with open(path, "w") as f:
        json.dump(data, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("wrote", path, len(data))
