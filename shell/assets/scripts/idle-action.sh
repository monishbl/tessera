#!/usr/bin/env bash
# One idle step from hypridle (start-idle.sh writes the listeners from
# Settings → Battery & Power): the step only happens on the power source it
# was set for, so the charger's and the battery's timers can differ while
# hypridle has one list.
#   idle-action.sh <ac|battery|any> <dim|undim|lock|off|on|sleep>
#   idle-action.sh before-sleep | after-sleep     (lock; screen on after the wake)
# The lock command and the grace come from the files start-idle.sh writes
# next to its hypridle config ($TESSERA_IDLE_DIR).
set -uo pipefail
dir=${TESSERA_IDLE_DIR:-}

# On the charger? A computer with no mains supply at all (a desktop) counts
# as on the charger.
on_ac() {
    local seen=0 f
    for f in /sys/class/power_supply/*; do
        [[ $(cat "$f/type" 2>/dev/null) == Mains ]] || continue
        seen=1
        [[ $(cat "$f/online" 2>/dev/null) == 1 ]] && return 0
    done
    [[ $seen == 0 ]]
}
# The lock, through the shell. No shell answering (crashed, restarting
# after an update, hung) must never mean sleeping unlocked (audit #60):
# the mark a shell started again while locked finds is set first (so
# keep-shell starts one at once, and it starts locked), the shell is asked
# again for up to 8 s, then hyprlock locks if it's installed.
lock() {
    [[ -n $dir && -r $dir/lock-command ]] || return 0
    local cmd sig mark i
    cmd=$(cat "$dir/lock-command")
    sh -c "$cmd" 2>/dev/null && return 0
    sig=${HYPRLAND_INSTANCE_SIGNATURE:-}
    mark="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/tessera-locked-$sig"
    [[ -n $sig ]] && : >"$mark" 2>/dev/null
    for i in $(seq 16); do
        sleep 0.5
        sh -c "$cmd" 2>/dev/null && return 0
    done
    if command -v hyprlock >/dev/null && ! pgrep -x hyprlock >/dev/null; then
        hyprlock --immediate >/dev/null 2>&1 &
        sleep 1
        return 0
    fi
    echo "idle-action.sh: no shell answered the lock, and hyprlock isn't installed" >&2
    return 1
}

case "${1:-}" in
    before-sleep)
        # Always locked before it sleeps (audit #97: a grace that waited for
        # the wake showed the desktop unlocked until the lock came). Kept
        # for a hypridle.conf an older shell wrote.
        lock
        exit 0 ;;
    after-sleep)
        hyprctl dispatch 'hl.dsp.dpms({ action = "enable" })' >/dev/null 2>&1
        exit 0 ;;
esac

source=${1:-any} action=${2:-}
case "$source" in
    ac) on_ac || exit 0 ;;
    battery) on_ac && exit 0 ;;
esac
# What dim set, when it dimmed (undim puts the brightness back only then: a
# screen already at a tenth or less was left alone, and brightnessctl's saved
# level is still an older dim's)
dimmed="${dir:+$dir/dimmed}"
dimmed="${dimmed:-${XDG_RUNTIME_DIR:-/tmp}/tessera-idle-dimmed}"
case "$action" in
    dim)
        # Down to a tenth (never brighter), remembered so undim puts it back
        if command -v brightnessctl >/dev/null; then
            cur=$(brightnessctl get 2>/dev/null) max=$(brightnessctl max 2>/dev/null)
            if [[ $cur =~ ^[0-9]+$ && $max =~ ^[0-9]+$ ]] && (( cur > max / 10 )) && brightnessctl -q -s set "$((max / 10))"; then
                echo "$((max / 10))" >"$dimmed" 2>/dev/null
            fi
        fi ;;
    undim)
        # (only after a dim of ours, and not when you changed the brightness
        # meanwhile: yours stays)
        if [[ -f $dimmed ]]; then
            was=$(cat "$dimmed" 2>/dev/null)
            rm -f "$dimmed"
            if command -v brightnessctl >/dev/null && [[ "$(brightnessctl get 2>/dev/null)" == "$was" ]]; then
                brightnessctl -q -r
            fi
        fi ;;
    lock) lock ;;
    off) hyprctl dispatch 'hl.dsp.dpms({ action = "disable" })' >/dev/null 2>&1 ;;
    on) hyprctl dispatch 'hl.dsp.dpms({ action = "enable" })' >/dev/null 2>&1 ;;
    sleep) systemctl suspend ;;
esac
exit 0
