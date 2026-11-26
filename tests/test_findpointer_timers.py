"""Shake to find the pointer: turned off, Hyprland no longer wakes every
200 ms for it (audit #656); turned on again (the shell's hyprctl eval), it
comes back. Plain Lua with a stub `hl` that records the timers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

SCRIPT = r'''
local root = arg[1]
TESSERA_USER_DIR = '/nonexistent-tessera-test'
local timers = {}
hl = {
    get_active_monitor = function() return { name = 'X' } end,
    get_cursor_pos = function() return { x = 0, y = 0 } end,
    on = function() end,
    timer = function(fn, o)
        local t = { fn = fn, timeout = o.timeout, on = true }
        function t:set_enabled(v) self.on = v end
        function t:is_enabled() return self.on end
        table.insert(timers, t)
        return t
    end,
}
dofile(root .. '/hypr/tessera/findpointer.lua')
local by = {}
for _, t in ipairs(timers) do by[t.timeout] = t end
local slow, watch = by[200], by[2000]
assert(slow and watch, 'timers')
assert(slow.on and not watch.on, 'on by default: the 200 ms look')
TESSERA_FIND_POINTER = false
slow.fn()
assert(not slow.on and watch.on, 'off: only the 2 s look')
watch.fn()
assert(not slow.on and watch.on)
TESSERA_FIND_POINTER = true
watch.fn()
assert(slow.on and not watch.on, 'on again')
'''


@unittest.skipUnless(shutil.which("lua"), "requires Lua")
class FindPointerTimers(unittest.TestCase):
    def test_off_means_no_fast_wakeups(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "t.lua"
            p.write_text(SCRIPT)
            r = subprocess.run(["lua", str(p), str(ROOT)], capture_output=True, text=True, timeout=30)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main()
