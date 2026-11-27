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
# as on the charger. A laptop charged over USB-C may show only USB supplies
# and a battery: then a USB supply online, or a battery not discharging,
# counts as the charger.
on_ac() {
    local root=${TESSERA_POWER_SUPPLY_DIR:-/sys/class/power_supply}
    local seen=0 usb=0 battery=0 discharging=0 f type
    for f in "$root"/*; do
        type=$(cat "$f/type" 2>/dev/null)
        case "$type" in
            Mains)
                seen=1
                [[ $(cat "$f/online" 2>/dev/null) == 1 ]] && return 0 ;;
            USB*)
                [[ $(cat "$f/online" 2>/dev/null) == 1 ]] && usb=1 ;;
            Battery)
                [[ $(cat "$f/scope" 2>/dev/null) == Device ]] && continue  # (a mouse's or headset's)
                battery=1
                [[ $(cat "$f/status" 2>/dev/null) == Discharging ]] && discharging=1 ;;
        esac
    done
    [[ $seen == 1 ]] && return 1
    [[ $usb == 1 ]] && return 0
    [[ $battery == 0 || $discharging == 0 ]]
}
# The lock, through the shell. No shell answering (crashed, restarting
# after an update, hung) must never mean sleeping unlocked (audit #60):
# the mark a shell started again while locked finds is set first (so
# keep-shell starts one at once, and it starts locked), the shell is asked
# again for up to 8 s, then hyprlock locks if it's installed.
# Exit 3 from the lock command: the shell answered but has no such command
# (a typo or a renamed verb in it; shell-ipc.sh has said so in a
# notification). Asking again can't help, and the shell isn't restarting:
# no mark, no retries, straight to hyprlock (audit #563: it used to look
# like success, and the screen never locked).
lock() {
    [[ -n $dir && -r $dir/lock-command ]] || return 0
    local cmd sig mark i rc
    cmd=$(cat "$dir/lock-command")
    sh -c "$cmd" 2>/dev/null && return 0
    rc=$?
    if [[ $rc != 3 ]]; then
        sig=${HYPRLAND_INSTANCE_SIGNATURE:-}
        mark="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/tessera-locked-$sig"
        [[ -n $sig ]] && : >"$mark" 2>/dev/null
        for i in $(seq 16); do
            sleep 0.5
            sh -c "$cmd" 2>/dev/null && return 0
            [[ $? == 3 ]] && break
        done
    fi
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
# (never a fixed name under /tmp: no private runtime folder, no note, and
# undim then leaves the brightness alone)
dimmed="${dir:+$dir/dimmed}"
if [[ -z $dimmed ]]; then
    rt=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
    [[ -d $rt && -O $rt && ! -L $rt ]] && dimmed="$rt/tessera-idle-dimmed"
fi
case "$action" in
    dim)
        # Down to a tenth (never brighter), remembered so undim puts it back
        if [[ -n $dimmed ]] && command -v brightnessctl >/dev/null; then
            cur=$(brightnessctl get 2>/dev/null) max=$(brightnessctl max 2>/dev/null)
            if [[ $cur =~ ^[0-9]+$ && $max =~ ^[0-9]+$ ]] && (( cur > max / 10 )) && brightnessctl -q -s set "$((max / 10))"; then
                [[ -n $dimmed ]] && echo "$((max / 10))" >"$dimmed" 2>/dev/null
            fi
        fi ;;
    undim)
        # (only after a dim of ours, and not when you changed the brightness
        # meanwhile: yours stays)
        if [[ -n $dimmed && -f $dimmed ]]; then
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
