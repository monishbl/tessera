#!/bin/bash
# Installs the release builder on the build machine (server1) from this
# checkout: ~/bin/tessera-release and its systemd user timer. Run it there:
#   tools/release-builder/install.sh             install or update, timer on
#   tools/release-builder/install.sh --no-timer  the script only
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
install -Dm755 "$here/tessera-release" "$HOME/bin/tessera-release"
# (the test before a set is offered: a sandbox arena, tools/release-builder/pretest.sh)
install -Dm755 "$here/pretest.sh" "$HOME/bin/tessera-pretest"
[[ "${1:-}" == --no-timer ]] && exit 0
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
install -Dm644 "$here/tessera-release.service" "$units/tessera-release.service"
install -Dm644 "$here/tessera-release.timer" "$units/tessera-release.timer"
systemctl --user daemon-reload
systemctl --user enable --now tessera-release.timer
# (a user timer runs only while the user's manager does: without lingering,
# not after a reboot or once the last ssh session ends; audit #187)
if [[ "$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)" != yes ]]; then
    if loginctl enable-linger "$USER" 2>/dev/null; then
        echo "Lingering is on for $USER, so the timer runs without a login."
    else
        echo "Turn lingering on, or the timer stops when you log out: sudo loginctl enable-linger $USER" >&2
    fi
fi
systemctl --user list-timers tessera-release.timer --no-pager
