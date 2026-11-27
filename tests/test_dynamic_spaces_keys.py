"""Dynamic spaces' keys (27 Nov): keybinds.lua binds Super+N (go), Super+Alt+N
(send), Super+Alt+Ctrl+N (move and follow), Super+Shift+N (new space at N)
and Super+Shift+Alt+N (new space at N with the window); with tessera-ui's
dynamic spaces (TESSERA_SPACES_DYNAMIC) each goes to it as the global
shortcut tessera:space-<op>-<n>, otherwise the fixed numbering does it as
before. A saved change still saying the old default (Super+Shift+N for
ws-move-N) is read as no change; your own keys stay. Plain Lua with a stub
`hl`; no compositor. (The planning itself: dynspace.rs's Rust tests.)"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

STUB = r'''
local root, userDir = arg[1], arg[2]
TESSERA_MOD = 'SUPER'
TESSERA_HYPR_DIR = root .. '/hypr/tessera'
TESSERA_USER_DIR = userDir
TESSERA_RUNTIME_DIR = userDir
BOUND, DONE = {}, {}
local function rec(kind) return function(t) return { kind = kind, t = t } end end
hl = setmetatable({
    dsp = setmetatable({
        global = function(name) return { kind = 'global', name = name } end,
        focus = rec('focus'),
        exec_cmd = rec('exec'),
        window = setmetatable({ move = rec('move') }, { __index = function(_, k) return rec(k) end }),
        workspace = setmetatable({}, { __index = function(_, k) return rec(k) end }),
    }, { __index = function(_, k) return rec(k) end }),
    bind = function(keys, action, opts)
        if opts and opts.description and not BOUND[opts.description] then
            BOUND[opts.description] = { keys = keys, action = action }
        end
    end,
    dispatch = function(d) table.insert(DONE, d) end,
    exec_cmd = function(c) table.insert(DONE, { kind = 'exec', t = c }) end,
    get_active_monitor = function() return { name = 'eDP-1', description = '' } end,
    get_active_workspace = function() return { id = ACTIVE or 1 } end,
}, { __index = function() return function() end end })
function press(label)
    local b = assert(BOUND['Workspace: ' .. label], 'not bound: ' .. label)
    DONE = {}
    if type(b.action) == 'function' then b.action() else table.insert(DONE, b.action) end
    return DONE[1]
end
function keys(label)
    return assert(BOUND['Workspace: ' .. label], 'not bound: ' .. label).keys
end
dofile(root .. '/hypr/tessera/keybinds.lua')
'''


@unittest.skipUnless(shutil.which("lua"), "requires Lua")
class DynamicSpaceKeys(unittest.TestCase):
    def run_lua(self, tail, user=None):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "keybinds.json").write_text(json.dumps(user or {}))
            p = Path(d) / "t.lua"
            p.write_text(STUB + tail)
            env = dict(os.environ, TESSERA_STANDALONE="0")
            r = subprocess.run(["lua", str(p), str(ROOT), d], capture_output=True, text=True, timeout=30, env=env)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            return r

    def test_the_keys(self):
        self.run_lua(r'''
assert(keys('Go to space 3') == 'SUPER + 3', keys('Go to space 3'))
assert(keys('Send window to space 3') == 'SUPER + ALT + 3')
assert(keys('Move window to space 3 (and follow)') == 'SUPER + CTRL + ALT + 3', keys('Move window to space 3 (and follow)'))
assert(keys('New space at 3') == 'SUPER + SHIFT + 3')
assert(keys('New space at 3 with this window') == 'SUPER + SHIFT + ALT + 3')
assert(keys('New space at 10') == 'SUPER + SHIFT + 0')
''')

    def test_dynamic_spaces_hand_the_keys_to_tessera_ui(self):
        self.run_lua(r'''
TESSERA_SPACES_DYNAMIC = true
local function g(label) local d = press(label); assert(d and d.kind == 'global', label); return d.name end
assert(g('Go to space 3') == 'tessera:space-focus-3')
assert(g('Go to space 10') == 'tessera:space-focus-10')
assert(g('Send window to space 2') == 'tessera:space-send-2')
assert(g('Move window to space 4 (and follow)') == 'tessera:space-move-4')
assert(g('New space at 1') == 'tessera:space-new-1')
assert(g('New space at 5 with this window') == 'tessera:space-newmove-5')
assert(g('Next space') == 'tessera:space-focus-next')
assert(g('Previous space') == 'tessera:space-focus-prev')
assert(g('Next space (scroll)') == 'tessera:space-focus-next')
assert(g('Move window to the next space') == 'tessera:space-move-next')
-- hyprctl eval: the same function
DONE = {}
TESSERA_SPACE('newmove', 2)
assert(DONE[1].name == 'tessera:space-newmove-2')
-- past the keys' names with no tessera-ui binary: the fixed numbering
DONE = {}
TESSERA_SPACE('focus', 12)
assert(DONE[1].kind == 'focus' and DONE[1].t.workspace == 12)
-- an unknown op does nothing
DONE = {}
TESSERA_SPACE('fly', 2)
assert(#DONE == 0)
''')

    def test_fixed_numbering_as_before(self):
        self.run_lua(r'''
TESSERA_SPACES = { ['eDP-1'] = 100 }
local d = press('Go to space 3')
assert(d.kind == 'focus' and d.t.workspace == 103)
d = press('Move window to space 2 (and follow)')
assert(d.kind == 'move' and d.t.workspace == 102 and d.t.follow == true)
d = press('Send window to space 2')
assert(d.kind == 'move' and d.t.follow == false)
-- a new space at N is just space N
d = press('New space at 4')
assert(d.kind == 'focus' and d.t.workspace == 104)
d = press('New space at 4 with this window')
assert(d.kind == 'move' and d.t.workspace == 104 and d.t.follow == true)
ACTIVE = 102
d = press('Next space (scroll)')
assert(d.kind == 'focus' and d.t.workspace == 103)
''')

    def test_a_saved_old_default_is_no_change(self):
        # (Super+Shift+3 saved for ws-move-3: the old default, so the new
        # keys apply and Super+Shift+3 is "New space at 3"; your own keys stay)
        self.run_lua(r'''
assert(keys('Move window to space 3 (and follow)') == 'SUPER + CTRL + ALT + 3', keys('Move window to space 3 (and follow)'))
assert(keys('New space at 3') == 'SUPER + SHIFT + 3')
assert(keys('Move window to space 4 (and follow)') == 'SUPER + F9')
assert(BOUND['Workspace: Move window to space 5 (and follow)'] == nil, 'disabled stays disabled')
''', {"keybinds": {"ws-move-3": {"keys": "SHIFT + $MOD + 3"}, "ws-move-4": {"keys": "$MOD + F9"},
                   "ws-move-5": {"keys": "$MOD + SHIFT + 5", "disabled": True}}})

    def test_no_two_defaults_share_keys(self):
        binds = json.loads((ROOT / "hypr/tessera/keymap.json").read_text())

        def norm(k):
            return "+".join(sorted(p.strip().upper() for p in k.split("+")))
        seen = {}
        for b in binds:
            k = norm(b["keys"])
            self.assertNotIn(k, seen, f'{b["id"]} and {seen.get(k)} share {b["keys"]}')
            seen[k] = b["id"]
        ids = {b["id"] for b in binds}
        for i in range(1, 11):
            for p in ("ws-%d", "ws-send-%d", "ws-move-%d", "ws-new-%d", "ws-new-move-%d"):
                self.assertIn(p % i, ids)


if __name__ == "__main__":
    unittest.main()
