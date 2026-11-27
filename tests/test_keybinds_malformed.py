"""A wrong-typed entry in ~/.config/tessera/keybinds.json is skipped and the
rest of the Hyprland config still loads (the shell is started after the
keybinds; audit #314). Plain Lua with a stub `hl`; no compositor."""
import json
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
TESSERA_RUNTIME_DIR = userDir -- (the failed-binds file lands here, not in the session's)
BINDS, GESTURES = {}, {}
local any = setmetatable({}, { __index = function(t, k) return function(...) return { kind = k, args = { ... } } end end })
hl = setmetatable({
    dsp = setmetatable({ window = any }, getmetatable(any)),
    bind = function(keys, action, opts)
        if keys:find('Nosuchkey') then error('invalid key') end
        table.insert(BINDS, keys)
    end,
    gesture = function(g) table.insert(GESTURES, g.direction) end,
}, { __index = function() return function() end end })
'''


@unittest.skipUnless(shutil.which("lua"), "requires Lua")
class Malformed(unittest.TestCase):
    def run_lua(self, user, tail):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "keybinds.json").write_text(json.dumps(user) if not isinstance(user, str) else user)
            p = Path(d) / "t.lua"
            p.write_text(STUB + tail)
            r = subprocess.run(["lua", str(p), str(ROOT), d], capture_output=True, text=True, timeout=30)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            return r

    def test_bad_entries_are_skipped_and_the_rest_bound(self):
        tail = r'''
dofile(root .. '/hypr/tessera/keybinds.lua')
dofile(root .. '/hypr/tessera/gestures.lua')
assert(#BINDS > 100, 'default binds missing: ' .. #BINDS)
assert(#GESTURES >= 5, 'default gestures missing: ' .. #GESTURES)
'''
        for user in (
            {"keybinds": {"lock": {"keys": 5}, "close": True, "terminal": {"keys": "$MOD + T", "disabled": 3}},
             "shortcuts": [{"id": "a", "keys": "$MOD + F9", "action": {"exec": 1}},
                           {"id": "b", "keys": "$MOD + F8", "action": {"dsp": 2}},
                           {"id": "c", "keys": True, "action": {"exec": "true"}},
                           {"id": "d", "keys": "$MOD + F7", "label": {}, "action": {"exec": "true"}}, 7],
             "gestures": {"changes": {"move": 5, "fullscreen": {"fingers": "x"}}, "custom": [3, {"action": {}}]}},
            {"keybinds": 3, "shortcuts": "x", "gestures": "y"},
            {"gestures": {"changes": 1, "custom": "z"}},
            "[1, 2]",
            "not json",
        ):
            self.run_lua(user, tail)

    def test_refused_binds_are_written_for_settings(self):
        # (audit #904: a key Hyprland can't bind is named in the runtime
        # file Settings reads; the file is written again, empty, once fixed)
        tail = r'''
dofile(root .. '/hypr/tessera/keybinds.lua')
local f = assert(io.open(userDir .. '/tessera-keybinds-failed.json'))
local t = f:read('*a')
f:close()
assert(t == EXPECT, t)
'''
        self.run_lua({"shortcuts": [{"id": "typo", "keys": "$MOD + Nosuchkey", "action": {"exec": "true"}},
                                    {"id": "fine", "keys": "$MOD + F9", "action": {"exec": "true"}}]},
                     'EXPECT = \'{"failed": ["typo"]}\\n\'\n' + tail)
        self.run_lua({}, 'EXPECT = \'{"failed": []}\\n\'\n' + tail)

    def test_hyprland_lua_loads_keybinds_then_starts_the_shell(self):
        # (the real hyprland.lua with dofile recorded: keybinds and gestures
        # by their real paths, each protected, and execs.lua after them)
        tail = r'''
local seen = {}
local real = dofile
dofile = function(p)
    table.insert(seen, p)
    if p:match('/keybinds%.lua$') then error('boom') end
    return nil
end
local home = os.getenv('HOME')
real(root .. '/hypr/hyprland.lua')
local cfg = (os.getenv('XDG_CONFIG_HOME') ~= nil and os.getenv('XDG_CONFIG_HOME') ~= '') and os.getenv('XDG_CONFIG_HOME') or (home .. '/.config')
local want = { cfg .. '/hypr/tessera/keybinds.lua', cfg .. '/hypr/tessera/gestures.lua', cfg .. '/hypr/tessera/execs.lua' }
local at = {}
for i, p in ipairs(seen) do at[p] = i end
for _, w in ipairs(want) do assert(at[w], 'not loaded: ' .. w) end
assert(at[want[1]] < at[want[3]] and at[want[2]] < at[want[3]])
'''
        self.run_lua({}, tail)


if __name__ == "__main__":
    unittest.main()
