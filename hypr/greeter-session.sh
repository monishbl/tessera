#!/bin/sh
# greetd's greeter command (/etc/greetd/config.toml, from ./install.sh): the
# login screen's compositor (greeter.lua), on the console the boot ends on.
# That console is cleared and its cursor hidden first, and the compositor's
# log goes to the journal (journalctl -t tessera-greeter), not onto the
# screen, so nothing but the splash, the login screen and your desktop shows.
# The login screen says how it ended (greeter-start.sh, into
# TESSERA_GREETER_RESULT: 0 once someone logged in). Ended any other way
# twice in a row (the compositor crashed, the login screen quit with an
# error): greetd's own text login (agreety) instead, in this same greetd
# session, so a password can always be typed.
#   TESSERA_GREETER_TRIES  how many tries before the text login (tests)
#   TESSERA_GREETER_LUA    the compositor's config (tests)
printf '\033[2J\033[H\033[?25l'
LUA="${TESSERA_GREETER_LUA:-/usr/share/tessera/greeter.lua}"
if command -v start-hyprland >/dev/null; then
    set -- start-hyprland -- --config "$LUA"
else
    set -- Hyprland --config "$LUA"
fi
say() { # what happened, into the journal (and stderr)
    if command -v systemd-cat >/dev/null; then echo "$1" | systemd-cat -t tessera-greeter -p warning; fi
    echo "$1" >&2
}
RESULT=$(mktemp "${XDG_RUNTIME_DIR:-/tmp}/tessera-greeter-result.XXXXXX" 2>/dev/null) || RESULT=""
export TESSERA_GREETER_RESULT="$RESULT"
tries=0
while [ "$tries" -lt "${TESSERA_GREETER_TRIES:-2}" ]; do
    [ -n "$RESULT" ] && : >"$RESULT"
    if command -v systemd-cat >/dev/null; then
        systemd-cat -t tessera-greeter "$@"
    else
        "$@" >/dev/null 2>&1
    fi
    code=$?
    said=""
    [ -n "$RESULT" ] && said=$(cat "$RESULT" 2>/dev/null)
    # (logged in; or a login screen from before it said, ending as it did then)
    if [ "$said" = 0 ] || { [ -z "$said" ] && [ "$code" = 0 ]; }; then
        [ -n "$RESULT" ] && rm -f "$RESULT"
        exit 0
    fi
    tries=$((tries + 1))
    say "greeter-session: the login screen ended without a login (compositor: $code, login screen: ${said:-nothing said}), try $tries"
done
[ -n "$RESULT" ] && rm -f "$RESULT"
# The text login: Hyprland as the login screen's sessions start it, else a shell
if command -v start-hyprland >/dev/null; then
    session=start-hyprland
elif command -v Hyprland >/dev/null; then
    session=Hyprland
else
    session="/bin/bash -l"
fi
if command -v agreety >/dev/null; then
    say "greeter-session: the login screen failed $tries times: greetd's text login (agreety) instead"
    printf '\033[2J\033[H\033[?25h'
    printf '%s\n\n' "Tessera's login screen couldn't start, so this is the text login. Type your user name and password; your desktop starts as usual. (What went wrong: journalctl -t tessera-greeter)"
    exec agreety --cmd "$session"
fi
say "greeter-session: the login screen failed and agreety isn't installed"
exit 1
