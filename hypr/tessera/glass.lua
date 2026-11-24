-- Liquid Glass: loads the tessera-glass plugin, if it's been built
-- (tools/build-plugin.sh), once Hyprland has started (loading while the config
-- is parsed is too early: there's no renderer yet). If it's missing or built
-- for another Hyprland version, nothing breaks: the shell uses Frosted glass.
local data = (os.getenv("XDG_DATA_HOME") ~= nil and os.getenv("XDG_DATA_HOME") ~= "") and os.getenv("XDG_DATA_HOME") or (os.getenv("HOME") .. "/.local/share")
-- The first that's there: TESSERA_GLASS_PLUGIN (a path, or a list: the
-- repo's own build in the tty session), then the installed one.
local candidates = {}
if type(TESSERA_GLASS_PLUGIN) == "table" then
    for _, p in ipairs(TESSERA_GLASS_PLUGIN) do table.insert(candidates, p) end
elseif TESSERA_GLASS_PLUGIN then
    table.insert(candidates, TESSERA_GLASS_PLUGIN)
end
table.insert(candidates, data .. "/tessera/tessera-glass.so")
-- A start that crashed with the plugin loaded (a Hyprland whose functions
-- changed under the plugin's hooks) leaves the mark: the next start skips
-- this same build and says so, so the login never loops into a crash;
-- a rebuild (another size) is tried again, and tools/build-plugin.sh clears
-- the mark (audit #183). Cleared 30 s after a load that held.
local mark = data .. "/tessera/tessera-glass.loading"
for _, plugin in ipairs(candidates) do
    local f = io.open(plugin, "r")
    if f then
        local size = tostring(f:seek("end"))
        f:close()
        local m = io.open(mark, "r")
        local crashed = m and m:read("*l") == size
        if m then m:close() end
        if crashed then
            hl.on("hyprland.start", function()
                hl.exec_cmd("notify-send -a Tessera -u critical 'Liquid Glass is off' " ..
                    "'Hyprland stopped the last time the glass plugin loaded, so it stays off: glass is Frosted. " ..
                    "Settings → Appearance rebuilds it for this Hyprland.'")
            end)
        else
            -- (the mark is written at the start only: this file runs again on
            -- every config reload, where nothing is loaded)
            hl.on("hyprland.start", function()
                hl.exec_cmd("sh -c \"echo " .. size .. " > '" .. mark .. "'; hyprctl plugin load '" .. plugin ..
                    "'; sleep 30; rm -f '" .. mark .. "'\"")
            end)
        end
        break
    end
end
