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
        let name = [s(d, "Alias"), s(d, "Name"), s(d, "Address")].into_iter().find(|n| !n.is_empty()).unwrap_or_default();
        out.devices.push(Device { path: path.to_string(), address: s(d, "Address"), name, icon: s(d, "Icon"), paired: b(d, "Paired"), connected: b(d, "Connected"), battery });
    }
    out.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
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
            let mut signals = MessageIterator::for_match_rule(rule, &conn, Some(64))?;
            let mut last: Option<Bluetooth> = None;
            loop {
                // (no BlueZ at all: an empty reading, and wait for it)
                let now = read(&conn).unwrap_or_default();
                if last.as_ref() != Some(&now) {
                    tx.send(Msg::Data(Box::new(now.clone()))).map_err(|_| zbus::Error::Failure("ui gone".into()))?;
                    last = Some(now);
                }
                if signals.next().is_none() {
                    return Ok(());
                }
                // coalesce a burst (a device connecting changes several properties)
                std::thread::sleep(Duration::from_millis(80));
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
fn ctl_full(args: &[&str], secs: u64) -> (Option<i32>, String) {
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
