#!/bin/sh
# Ends the session only on a second press within 3 s: a stray Super+Shift+Escape
# or Ctrl+Alt+Backspace used to log out at once. Hyprland's own notice, not
# the shell's, so it still works when the shell is frozen.
# (the user's own runtime folder, never /tmp: there another user could plant
# the stamp; read as a number only, never fed to the shell's arithmetic as is)
stamp="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/tessera-exit-armed"
now=$(date +%s)
armed=0
[ -f "$stamp" ] && read -r armed <"$stamp" 2>/dev/null
case $armed in '' | *[!0-9]*) armed=0 ;; esac
if [ "$armed" -gt 0 ] && [ $((now - armed)) -le 3 ]; then
    rm -f "$stamp"
    exec hyprctl dispatch "hl.dsp.exit()"
fi
echo "$now" >"$stamp"
hyprctl notify 1 3000 "rgb(ff9f0a)" "Press again to log out"
