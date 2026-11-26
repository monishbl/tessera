//! Bluetooth (BlueZ) over the system bus, read-only for now
//! (services::bluez::Bluetooth): the first adapter and every known device,
//! connected or not, with its battery (org.bluez.Battery1) when it reports
//! one. A thread reads BlueZ's objects (ObjectManager) and reads again
//! after each burst of its signals (properties, devices added or removed).
//! (Turning it on or off and connecting are the Control Center's, still in
//! QML: Bluetooth acts on the real hardware, even from the sandbox.)

use crate::Msg;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::MatchRule;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub address: String,
    pub name: String,
    /// BlueZ's icon name (audio-headset, input-mouse…)
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    /// 0..100, when it says
    pub battery: Option<u8>,
    /// signal strength while it's seen in a scan (dBm), for the order
    pub rssi: Option<i16>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bluetooth {
    /// an adapter exists
    pub present: bool,
    pub powered: bool,
    pub devices: Vec<Device>,
}

type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

fn s(p: &HashMap<String, OwnedValue>, k: &str) -> String {
    match p.get(k).map(|v| &**v) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}
fn b(p: &HashMap<String, OwnedValue>, k: &str) -> bool {
    matches!(p.get(k).map(|v| &**v), Some(Value::Bool(true)))
}

/// A device's own name, or "" when it has none: BlueZ fills Alias with the
/// address ("41-AC-C9-CB-23-B0") for a device that never said a name, and
/// that isn't one (the lists hide nameless nearby devices and show a paired
/// one by its address)
fn real_name(alias: &str, name: &str, address: &str) -> String {
    let dashed = address.replace(':', "-");
    let is_address = |n: &str| n.is_empty() || n.eq_ignore_ascii_case(address) || n.eq_ignore_ascii_case(&dashed);
    if !is_address(alias) {
        alias.to_string()
    } else if !is_address(name) {
        name.to_string()
    } else {
        String::new()
    }
}

/// BlueZ's objects as the bar needs them (devices by name)
pub fn from_objects(objs: &Objects) -> Bluetooth {
    let mut out = Bluetooth::default();
    let mut adapters: Vec<(&str, bool)> = objs
        .iter()
        .filter_map(|(path, ifs)| ifs.get("org.bluez.Adapter1").map(|a| (path.as_str(), b(a, "Powered"))))
        .collect();
    adapters.sort();
    if let Some((_, powered)) = adapters.first() {
        out.present = true;
        out.powered = *powered;
    }
    for (path, ifs) in objs {
        let Some(d) = ifs.get("org.bluez.Device1") else { continue };
        let battery = ifs.get("org.bluez.Battery1").and_then(|bat| match bat.get("Percentage").map(|v| &**v) {
            Some(Value::U8(p)) => Some(*p),
            _ => None,
        });
        let address = s(d, "Address");
        let name = real_name(&s(d, "Alias"), &s(d, "Name"), &address);
        let rssi = match d.get("RSSI").map(|v| &**v) {
            Some(Value::I16(r)) => Some(*r),
            _ => None,
        };
        out.devices.push(Device { path: path.to_string(), address, name, icon: s(d, "Icon"), paired: b(d, "Paired"), connected: b(d, "Connected"), battery, rssi });
    }
    // connected, then paired, then nearby ones with a name (strongest signal
    // first), and nameless ones last: a scan in a busy place finds dozens of
    // unnamed beacons, and sorted by name their addresses (digits) came first
    out.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.paired.cmp(&a.paired))
            .then_with(|| a.name.is_empty().cmp(&b.name.is_empty()))
            .then_with(|| if a.paired { std::cmp::Ordering::Equal } else { b.rssi.unwrap_or(i16::MIN).cmp(&a.rssi.unwrap_or(i16::MIN)) })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.address.cmp(&b.address))
    });
    out
}

impl Bluetooth {
    pub fn any_connected(&self) -> bool {
        self.devices.iter().any(|d| d.connected)
    }
    /// The menu bar item's icon (MenuBar.qml)
    pub fn icon(&self) -> &'static str {
        if !self.powered {
            "bluetooth-slash"
        } else if self.any_connected() {
            "bluetooth-connected"
        } else {
            "bluetooth"
        }
    }
    /// Its hover tip: off, nothing connected, or who is (with batteries)
    pub fn summary(&self) -> String {
        if !self.powered {
            return "Bluetooth off".into();
        }
        let on: Vec<String> = self.devices.iter().filter(|d| d.connected).map(|d| d.name.clone() + &d.battery.map(|p| format!(" · {p}%")).unwrap_or_default()).collect();
        if on.is_empty() {
            "Bluetooth on · nothing connected".into()
        } else {
            on.join(", ")
        }
    }
}

fn read(conn: &Connection) -> zbus::Result<Bluetooth> {
    let m = conn.call_method(Some("org.bluez"), "/", Some("org.freedesktop.DBus.ObjectManager"), "GetManagedObjects", &())?;
    let objs: Objects = m.body().deserialize()?;
    Ok(from_objects(&objs))
}

pub fn spawn(tx: Sender<Msg>) {
    std::thread::spawn(move || loop {
        let run = || -> zbus::Result<()> {
            let conn = Connection::system()?;
            let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender("org.bluez")?.build();
            let signals = MessageIterator::for_match_rule(rule, &conn, Some(64))?;
            // (signals pass through a one-slot channel: a burst, a scan's RSSI
            // changes many times a second, is one re-read, not one each, which
            // fell minutes behind; audit #192)
            let (sig_tx, sig_rx) = std::sync::mpsc::sync_channel::<()>(1);
            std::thread::spawn(move || {
                for m in signals {
                    if m.is_err() {
                        continue;
                    }
                    if let Err(std::sync::mpsc::TrySendError::Disconnected(_)) = sig_tx.try_send(()) {
                        return;
                    }
                }
            });
            let mut last: Option<Bluetooth> = None;
            loop {
                // (no BlueZ at all: an empty reading, and wait for it)
                let now = read(&conn).unwrap_or_default();
                if last.as_ref() != Some(&now) {
                    tx.send(Msg::Data(Box::new(now.clone()))).map_err(|_| zbus::Error::Failure("ui gone".into()))?;
                    last = Some(now);
                }
                if sig_rx.recv().is_err() {
                    return Ok(());
                }
                // coalesce a burst (a device connecting changes several properties)
                while sig_rx.recv_timeout(Duration::from_millis(80)).is_ok() {}
            }
        };
        if let Err(zbus::Error::Failure(_)) = run() {
            return;
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(list: &[(&str, Value<'static>)]) -> HashMap<String, OwnedValue> {
        list.iter().map(|(k, v)| (k.to_string(), OwnedValue::try_from(v.clone()).unwrap())).collect()
    }

    fn objects() -> Objects {
        let mut o: Objects = HashMap::new();
        let path = |p: &str| OwnedObjectPath::try_from(p.to_string()).unwrap();
        o.insert(path("/org/bluez/hci0"), [("org.bluez.Adapter1".to_string(), props(&[("Powered", Value::from(true))]))].into());
        o.insert(
            path("/org/bluez/hci0/dev_AA"),
            [
                ("org.bluez.Device1".to_string(), props(&[("Alias", Value::from("Maya's earbuds")), ("Address", Value::from("AA")), ("Connected", Value::from(true)), ("Icon", Value::from("audio-headset"))])),
                ("org.bluez.Battery1".to_string(), props(&[("Percentage", Value::from(80u8))])),
            ]
            .into(),
        );
        o.insert(path("/org/bluez/hci0/dev_BB"), [("org.bluez.Device1".to_string(), props(&[("Name", Value::from("Keyboard")), ("Address", Value::from("BB")), ("Paired", Value::from(true))]))].into());
        o
    }

    #[test]
    fn an_address_is_not_a_name() {
        assert_eq!(real_name("41-AC-C9-CB-23-B0", "", "41:AC:C9:CB:23:B0"), "");
        assert_eq!(real_name("41:ac:c9:cb:23:b0", "", "41:AC:C9:CB:23:B0"), "");
        assert_eq!(real_name("41-AC-C9-CB-23-B0", "Pixel Buds", "41:AC:C9:CB:23:B0"), "Pixel Buds");
        assert_eq!(real_name("Kitchen speaker", "JBL Go", "41:AC:C9:CB:23:B0"), "Kitchen speaker");
        assert_eq!(real_name("", "", "41:AC:C9:CB:23:B0"), "");
    }

    #[test]
    fn named_and_near_come_before_nameless() {
        let mut o: Objects = HashMap::new();
        let path = |p: &str| OwnedObjectPath::try_from(p.to_string()).unwrap();
        o.insert(path("/org/bluez/hci0"), [("org.bluez.Adapter1".to_string(), props(&[("Powered", Value::from(true))]))].into());
        let dev = |o: &mut Objects, id: &str, alias: &str, name: &str, rssi: i16, paired: bool| {
            let addr = id.replace('_', ":");
            let mut p = vec![("Alias", Value::from(alias.to_string())), ("Address", Value::from(addr)), ("RSSI", Value::from(rssi)), ("Paired", Value::from(paired))];
            if !name.is_empty() {
                p.push(("Name", Value::from(name.to_string())));
            }
            o.insert(path(&format!("/org/bluez/hci0/dev_{id}")), [("org.bluez.Device1".to_string(), props(&p))].into());
        };
        dev(&mut o, "00_3E_73_17_EF_88", "00-3E-73-17-EF-88", "", -40, false);
        dev(&mut o, "41_AC_C9_CB_23_B0", "41-AC-C9-CB-23-B0", "", -50, false);
        dev(&mut o, "AA_00_00_00_00_01", "Zed's speaker", "Zed's speaker", -80, false);
        dev(&mut o, "AA_00_00_00_00_02", "Buds", "Buds", -60, false);
        dev(&mut o, "AA_00_00_00_00_03", "Mouse", "Mouse", -90, true);
        let bt = from_objects(&o);
        let names: Vec<&str> = bt.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Mouse", "Buds", "Zed's speaker", "", ""]);
        assert_eq!(bt.devices[3].address, "00:3E:73:17:EF:88");
    }

    #[test]
    fn devices_from_bluez_objects() {
        let bt = from_objects(&objects());
        assert!(bt.present && bt.powered);
        assert_eq!(bt.devices.len(), 2);
        // connected ones first
        assert_eq!(bt.devices[0].name, "Maya's earbuds");
        assert_eq!(bt.devices[0].battery, Some(80));
        assert_eq!(bt.devices[1].name, "Keyboard");
        assert!(bt.devices[1].paired && !bt.devices[1].connected);
        assert_eq!(bt.icon(), "bluetooth-connected");
        assert_eq!(bt.summary(), "Maya's earbuds · 80%");
    }

    #[test]
    fn off_and_idle_say_so() {
        let mut bt = from_objects(&objects());
        bt.devices.iter_mut().for_each(|d| d.connected = false);
        assert_eq!(bt.icon(), "bluetooth");
        assert_eq!(bt.summary(), "Bluetooth on · nothing connected");
        bt.powered = false;
        assert_eq!((bt.icon(), bt.summary().as_str()), ("bluetooth-slash", "Bluetooth off"));
        assert!(!from_objects(&HashMap::new()).present);
    }
}

/// `bluetoothctl <args>`: its exit code (None: it couldn't start, or was
/// given up after `secs`) and what it printed. Never with bluetoothctl's own
/// `--timeout`: with it, bluetoothctl waits out the whole time and exits 0
/// whatever happened (BlueZ 5.88: `--timeout 3 info <absent>` prints "not
/// available", waits 3 s, exits 0), so every pairing took 30 s and a failed
/// one looked like any other. Without it, it ends on BlueZ's answer, 1 on a failure
pub(crate) fn ctl_full(args: &[&str], secs: u64) -> (Option<i32>, String) {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new("bluetoothctl").args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
        return (None, String::new());
    };
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    let mut code = None;
    let mut ended = false;
    while std::time::Instant::now() < end {
        if let Ok(Some(st)) = child.try_wait() {
            code = st.code();
            ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !ended {
        let _ = child.kill();
    }
    let _ = child.wait();
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut out);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut out);
    }
    (code, out)
}

/// `bluetoothctl <args>`'s output, given up after `secs`
fn ctl(args: &[&str], secs: u64) -> String {
    ctl_full(args, secs).1
}

/// Whether `bluetoothctl show` says the adapter is pairable
pub fn pairable_in(show: &str) -> bool {
    show.lines().any(|l| l.trim() == "Pairable: yes")
}

/// Whether a `bluetoothctl pair` run paired (or it already was)
pub fn paired_in(out: &str) -> bool {
    out.contains("Pairing successful") || out.contains("AlreadyExists")
}

/// Whether `bluetoothctl info <device>` says the pairing was kept: Bonded
/// (newer BlueZ), or Paired where BlueZ is older and has no Bonded
pub fn kept_in(info: &str) -> bool {
    let has = |l: &str| info.lines().any(|x| x.trim() == l);
    if info.lines().any(|x| x.trim().starts_with("Bonded:")) {
        has("Bonded: yes")
    } else {
        has("Paired: yes")
    }
}

/// What a `bluetoothctl <verb> <device>` run came to (`code`: its exit code,
/// None when it was given up on): Ok, or why not, in words that say what to do
pub fn outcome(verb: &str, code: Option<i32>, out: &str) -> Result<(), String> {
    let low = out.to_lowercase();
    let already = (verb == "pair" && out.contains("AlreadyExists")) || (verb == "connect" && out.contains("AlreadyConnected")) || (verb == "disconnect" && out.contains("NotConnected"));
    let failed = low.contains("failed") || low.contains("not available") || low.contains("org.bluez.error");
    if already || (code == Some(0) && !failed) {
        return Ok(());
    }
    let doing = match verb {
        "pair" => "pair with it",
        "connect" => "connect to it",
        "disconnect" => "disconnect it",
        "remove" => "forget it",
        "trust" | "untrust" => "change whether it connects by itself",
        "cancel-pairing" => "stop the pairing",
        _ => "do that",
    };
    Err(if low.contains("not available") {
        "It isn't in range, or it's turned off. Turn it on (to pair: put it in pairing mode), then try again.".to_string()
    } else if low.contains("authentication") {
        "The device turned the pairing down. Put it in pairing mode (or forget this computer on it), then try again.".to_string()
    } else if low.contains("profile-unavailable") {
        "It has nothing this computer can connect to. For headphones and speakers, check that PipeWire's Bluetooth support is installed.".to_string()
    } else if low.contains("blocked") || low.contains("notready") || low.contains("not ready") {
        "Bluetooth is off or blocked. Turn it on, then try again.".to_string()
    } else if low.contains("inprogress") || low.contains("in progress") || low.contains("busy") {
        "It's still busy with the last try. Wait a moment, then try again.".to_string()
    } else if code.is_none() || low.contains("timeout") || low.contains("noreply") || low.contains("attemptfailed") {
        format!("It didn't answer. Check that it's on and close by, then try to {doing} again.")
    } else {
        format!("Couldn't {doing}. Try again.")
    })
}

/// `bluetoothctl <verb> <device>` (connect, disconnect, remove, trust,
/// untrust, cancel-pairing; "pair" is `pair`), its answer read: blocking,
/// call on a thread
pub fn act(verb: &str, addr: &str) -> Result<(), String> {
    // (no discovery while it runs: busy())
    let _busy = busy_guard();
    if verb == "pair" {
        return pair(addr);
    }
    let (code, out) = ctl_full(&[verb, addr], if verb == "connect" { 30 } else { 15 });
    outcome(verb, code, &out)
}

/// Pair a device so it stays paired, then trust and connect it (blocking:
/// call on a thread). While the adapter isn't Pairable, BlueZ pairs without
/// a bond: the keys aren't kept, the link drops at once and the device shows
/// unpaired again (22 Nov: earbuds "paired" and fell off every time). So the
/// adapter is pairable for the pairing, and set back after; and the bond is
/// read back from BlueZ, not taken from "Pairing successful"
pub fn pair(addr: &str) -> Result<(), String> {
    let _busy = busy_guard();
    let was = pairable_in(&ctl(&["show"], 5));
    if !was {
        ctl(&["pairable", "on"], 5);
    }
    let (code, out) = ctl_full(&["pair", addr], 40);
    let mut result = outcome("pair", code, &out);
    if result.is_ok() && !kept_in(&ctl(&["info", addr], 5)) {
        result = Err("It paired, but the pairing wasn't kept. Turn Bluetooth off and on, then pair again.".to_string());
    }
    if result.is_ok() {
        ctl(&["trust", addr], 5);
        let (code, out) = ctl_full(&["connect", addr], 30);
        result = outcome("connect", code, &out).map_err(|e| format!("Paired. {e}"));
    }
    if !was {
        ctl(&["pairable", "off"], 5);
    }
    result
}

// ── Busy: no discovery while a device pairs or connects ─────────────────────

/// Devices pairing, connecting or answering a pairing prompt right now
/// (act, pair, and the pairing agent's open requests: surfaces/polkit/btagent.rs)
static BUSY: AtomicUsize = AtomicUsize::new(0);

/// The discovery runs started with `scan` (by who started them), stopped
/// while busy: an inquiry on the radio while it pairs or connects slows the
/// link down or breaks it (22 Nov: Control Center's page scanned every 8 s,
/// Settings' for an hour, whatever was pairing)
static SCANS: Mutex<Vec<(&'static str, std::process::Child)>> = Mutex::new(Vec::new());

/// Held while something pairs or connects: `busy()` says so, and discovery
/// stops at once (`scan` starts none until the last one is dropped)
pub struct Busy(());

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Something pairs or connects from now until the guard is dropped
pub fn busy_guard() -> Busy {
    BUSY.fetch_add(1, Ordering::SeqCst);
    stop_scans();
    Busy(())
}

/// A device is pairing or connecting (no rescans now)
pub fn busy() -> bool {
    BUSY.load(Ordering::SeqCst) > 0
}

/// Whether a rescan may start: the adapter is on and nothing is busy
pub fn may_scan(powered: bool, busy: bool) -> bool {
    powered && !busy
}

fn scans() -> std::sync::MutexGuard<'static, Vec<(&'static str, std::process::Child)>> {
    SCANS.lock().unwrap_or_else(|e| e.into_inner())
}

/// `bluetoothctl --timeout <secs> scan on` for `who` (its last one replaced),
/// unless a device is pairing or connecting: false then (ask again later)
pub fn scan(who: &'static str, secs: u64) -> bool {
    use std::process::{Command, Stdio};
    let mut list = scans();
    // (read under the lock: a guard taken meanwhile stops this one too)
    if busy() {
        return false;
    }
    // (the ones that ended by themselves reaped; `who`'s last one replaced)
    list.retain_mut(|(w, c)| {
        if *w == who {
            let _ = c.kill();
            let _ = c.wait();
            return false;
        }
        !matches!(c.try_wait(), Ok(Some(_)))
    });
    let secs = secs.to_string();
    let mut cmd = Command::new("bluetoothctl");
    cmd.args(["--timeout", secs.as_str(), "scan", "on"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // (a shell that dies or is replaced mustn't leave the radio discovering)
    crate::util::dies_with_us(&mut cmd);
    match cmd.spawn() {
        Ok(c) => {
            list.push((who, c));
            true
        }
        Err(_) => false,
    }
}

/// Whether `who`'s discovery run is still going
pub fn scanning(who: &'static str) -> bool {
    scans().iter_mut().any(|(w, c)| *w == who && matches!(c.try_wait(), Ok(None)))
}

/// Stop `who`'s discovery run (its bluetoothctl leaves the bus: BlueZ ends
/// the discovery it asked for)
pub fn stop_scan(who: &'static str) {
    scans().retain_mut(|(w, c)| {
        if *w == who {
            let _ = c.kill();
            let _ = c.wait();
            return false;
        }
        true
    });
}

/// Stop every discovery run (something starts pairing or connecting)
pub fn stop_scans() {
    for (_, mut c) in scans().drain(..) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Why the last action on each device failed: shown on its row in place of
/// its status until the next action on it, or until it connects or
/// disconnects by itself
#[derive(Clone, Debug, Default)]
pub struct Failures(HashMap<String, (String, bool)>);

impl Failures {
    /// An action on it starts: its last failure goes
    pub fn clear(&mut self, addr: &str) {
        self.0.remove(addr);
    }
    /// An action on it ended (`bt`: the reading now)
    pub fn note(&mut self, addr: &str, result: Result<(), String>, bt: Option<&Bluetooth>) {
        match result {
            Ok(()) => {
                self.0.remove(addr);
            }
            Err(e) => {
                let connected = bt.and_then(|b| b.devices.iter().find(|d| d.address == addr)).is_some_and(|d| d.connected);
                self.0.insert(addr.to_string(), (e, connected));
            }
        }
    }
    /// The reason to show on its row, if any
    pub fn of(&self, d: &Device) -> Option<&str> {
        self.0.get(&d.address).filter(|(_, c)| *c == d.connected).map(|(e, _)| e.as_str())
    }
}

#[cfg(test)]
mod pair_tests {
    use super::*;

    #[test]
    fn reads_pairable_and_a_pairing() {
        assert!(pairable_in("Controller 00:11 (public)\n\tPowered: yes\n\tPairable: yes\n"));
        assert!(!pairable_in("\tPairable: no\n"));
        assert!(paired_in("[CHG] Device X Paired: yes\nPairing successful\n"));
        assert!(paired_in("Failed to pair: org.bluez.Error.AlreadyExists\n"));
        assert!(!paired_in("Failed to pair: org.bluez.Error.AuthenticationFailed\n"));
    }

    #[test]
    fn the_bond_is_read_back() {
        // (bluetoothctl info, BlueZ 5.88)
        assert!(kept_in("Device AA\n\tPaired: yes\n\tBonded: yes\n\tBREDR.Bonded: yes\n"));
        assert!(!kept_in("Device AA\n\tPaired: yes\n\tBonded: no\n\tBREDR.Bonded: yes\n"));
        assert!(!kept_in("Device AA\n\tPaired: no\n\tBonded: no\n"));
        // (older BlueZ: no Bonded)
        assert!(kept_in("Device AA\n\tPaired: yes\n"));
        assert!(!kept_in("Device AA not available\n"));
    }

    #[test]
    fn answers_are_read_not_assumed() {
        assert_eq!(outcome("connect", Some(0), "Attempting to connect to AA\n[CHG] Device AA Connected: yes\nConnection successful\n"), Ok(()));
        assert_eq!(outcome("disconnect", Some(0), "Attempting to disconnect from AA\nSuccessful disconnected\n"), Ok(()));
        assert_eq!(outcome("remove", Some(0), "Device has been removed\n"), Ok(()));
        assert_eq!(outcome("connect", Some(1), "Failed to connect: org.bluez.Error.AlreadyConnected\n"), Ok(()));
        // (exit 0 with a failure printed is still a failure)
        assert!(outcome("connect", Some(0), "Device AA not available\n").unwrap_err().contains("isn't in range"));
        assert!(outcome("connect", Some(1), "Failed to connect: org.bluez.Error.Failed br-connection-profile-unavailable\n").unwrap_err().contains("PipeWire"));
        assert!(outcome("connect", Some(1), "Failed to connect: org.bluez.Error.Failed br-connection-page-timeout\n").unwrap_err().contains("didn't answer"));
        assert!(outcome("pair", Some(1), "Failed to pair: org.bluez.Error.AuthenticationRejected\n").unwrap_err().contains("turned the pairing down"));
        assert!(outcome("connect", None, "").unwrap_err().contains("connect to it again"));
        assert_eq!(outcome("remove", Some(1), "Failed to remove device: org.bluez.Error.Failed\n"), Err("Couldn't forget it. Try again.".to_string()));
    }

    #[test]
    fn no_rescans_while_busy() {
        assert!(may_scan(true, false));
        assert!(!may_scan(true, true));
        assert!(!may_scan(false, false));
        // (the guard counts: two at once, busy until both are dropped)
        let before = busy();
        let a = busy_guard();
        let b = busy_guard();
        assert!(busy());
        drop(a);
        assert!(busy());
        drop(b);
        assert_eq!(busy(), before);
    }

    #[test]
    fn a_failure_shows_until_the_device_changes() {
        let mut bt = Bluetooth { present: true, powered: true, devices: vec![Device { address: "AA".into(), paired: true, ..Default::default() }] };
        let mut f = Failures::default();
        f.note("AA", Err("It didn't answer.".to_string()), Some(&bt));
        assert_eq!(f.of(&bt.devices[0]), Some("It didn't answer."));
        bt.devices[0].connected = true;
        assert_eq!(f.of(&bt.devices[0]), None);
        f.note("AA", Ok(()), Some(&bt));
        bt.devices[0].connected = false;
        assert_eq!(f.of(&bt.devices[0]), None);
    }
}
