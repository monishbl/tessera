#!/usr/bin/env python3
"""Puts the computer to sleep when the battery is about to run out.

Settings → Battery & Power → Critical battery. The shell warns at 10% and
5% (with Sleep now); this guard is what makes sure the computer sleeps at
3% on battery even if the shell has crashed or isn't running, and even with
Caffeine on (that only holds off idle sleep, not this). Sleep, never
hibernate.

It runs as a systemd user service (tessera-battery-guard.service, installed by
battery-guard-install.sh) and only reacts: UPower tells it when the battery
changes (PropertiesChanged on its display device), so it costs nothing in
between. It asks logind to suspend once per level: after a wake on a
still-empty battery it sleeps again as soon as the battery drops another 1%,
and plugging in resets it.

  battery-guard.py [--sleep-at 3]
  battery-guard.py --check <percent> <state>    (the decision only, for tests)
"""
import argparse
import subprocess
import sys

UPOWER = "org.freedesktop.UPower"
DISPLAY = "/org/freedesktop/UPower/devices/DisplayDevice"
DEVICE = "org.freedesktop.UPower.Device"
DISCHARGING = 2  # UPower's state for "on battery, draining"
EMPTY = 5


def should_sleep(percent, state, sleep_at):
    """On battery (draining or empty) and at or under the level."""
    return state in (DISCHARGING, EMPTY) and 0 <= percent <= sleep_at


def decide(percent, state, sleep_at, slept_at):
    """(sleep now?, the level to remember). slept_at is the level it last
    slept at in this discharge (None: not yet). After a wake on a
    still-empty battery it acts again once the level drops a further 1%."""
    if state not in (DISCHARGING, EMPTY):
        return False, None  # plugged in: ready for the next time
    if not should_sleep(percent, state, sleep_at):
        return False, slept_at
    if slept_at is None or percent <= slept_at - 1:
        return True, slept_at
    return False, slept_at


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sleep-at", type=float, default=3)
    ap.add_argument("--check", nargs=2, type=float, metavar=("PERCENT", "STATE"))
    args = ap.parse_args()
    if args.check:
        print("sleep" if should_sleep(args.check[0], int(args.check[1]), args.sleep_at) else "wait")
        return 0

    try:
        import gi
        gi.require_version("Gio", "2.0")
        from gi.repository import Gio, GLib
    except (ImportError, ValueError, AttributeError) as e:
        print(f"battery-guard needs python-gobject ({e})", file=sys.stderr)
        return 1

    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM)
    device = Gio.DBusProxy.new_sync(bus, Gio.DBusProxyFlags.NONE, None, UPOWER, DISPLAY, DEVICE)
    done = {"slept_at": None}

    def check(*_):
        percent = device.get_cached_property("Percentage")
        state = device.get_cached_property("State")
        present = device.get_cached_property("IsPresent")
        if percent is None or state is None or (present is not None and not present.unpack()):
            return
        p, s = percent.unpack(), state.unpack()
        now, done["slept_at"] = decide(p, s, args.sleep_at, done["slept_at"])
        if now:
            # (the shell's own "Battery empty" says the same: power_policy.rs
            # battery_alert; while this guard runs, it's the only one that does).
            # Never waited for: a frozen notification server, or no
            # notify-send at all, must not stop the sleep (audit #96)
            try:
                subprocess.Popen(["notify-send", "-u", "critical", "-a", "Tessera", "-i", "battery-caution",
                                  # (the battery's one live entry: x-tessera-key, live.rs)
                                  "-h", "string:x-tessera-key:battery", "-h", "int:x-tessera-severity:3", "Battery empty",
                                  f"{p:.0f}% left · sleeping now to keep your work"],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            except (OSError, subprocess.SubprocessError) as e:
                print(f"couldn't say it: {e}", file=sys.stderr)
            try:
                login = Gio.DBusProxy.new_sync(bus, Gio.DBusProxyFlags.NONE, None, "org.freedesktop.login1",
                                               "/org/freedesktop/login1", "org.freedesktop.login1.Manager")
                login.call_sync("Suspend", GLib.Variant("(b)", (False,)), Gio.DBusCallFlags.NONE, 10000, None)
                done["slept_at"] = p  # (only once it slept: a failed try is tried again)
            except GLib.Error as e:
                print(f"couldn't sleep: {e.message}", file=sys.stderr)

    device.connect("g-properties-changed", check)
    check()
    GLib.MainLoop().run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
