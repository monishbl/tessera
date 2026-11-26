//! Battery (UPower's display device) and Wi-Fi (NetworkManager) over the
//! system bus with zbus, read-only. Each runs on a thread that blocks on the
//! service's PropertiesChanged signals and sends a fresh reading.
//!
//! A sandbox's battery (TESSERA_FAKE_BATTERY=<file>, the dev sandbox only):
//! the file says "<percent> <charging|discharging|full>", read every second
//! in place of UPower, so battery saver and the low-battery alerts can be
//! tried and measured on a machine without one (docs/battery-saver.md).

use crate::Msg;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::time::Duration;
use zbus::blocking::{proxy::Builder, Connection, MessageIterator, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;
use zbus::MatchRule;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub present: bool,
    pub percent: f64,
    pub charging: bool,
    /// UPower's state: 1 charging, 2 discharging, 4 fully charged, 5 pending charge
    pub state: u32,
    /// seconds, 0 unknown
    pub time_to_empty: i64,
    pub time_to_full: i64,
    /// on mains (an adapter online; without one listed, UPower's state:
    /// charging, full or pending charge); #437
    pub plugged: bool,
    /// the charge limit below 100 % (charge_control_end_threshold), if set
    pub limit: Option<u8>,
}

/// What the battery is doing, worded the same in the bar, the battery menu
/// and Control Center (#437)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Full,
    /// plugged in, not charging: held at its charge limit, or the charger
    /// waits (UPower's pending charge)
    Held,
    OnBattery,
}

impl Battery {
    pub fn charge(&self) -> Charge {
        if self.state == 1 {
            return Charge::Charging;
        }
        if !self.plugged {
            return Charge::OnBattery;
        }
        // (at a limit below 100 % UPower may call it fully charged: it's held)
        let below_limit_full = self.limit.is_some_and(|l| l < 100) && self.percent < 99.0;
        if !below_limit_full && (self.state == 4 || self.percent >= 99.0) {
            return Charge::Full;
        }
        Charge::Held
    }

    /// "Charging", "Fully charged", "Plugged in, not charging", "On battery"
    pub fn charge_words(&self) -> &'static str {
        match self.charge() {
            Charge::Charging => "Charging",
            Charge::Full => "Fully charged",
            Charge::Held => "Plugged in, not charging",
            Charge::OnBattery => "On battery",
        }
    }

    /// "Held at 80%" when it's held at (or near) its charge limit, else ""
    pub fn held_at(&self) -> String {
        match self.limit {
            Some(l) if self.charge() == Charge::Held && self.percent + 5.0 >= l as f64 => format!("Held at {l}%"),
            _ => String::new(),
        }
    }

    /// "Plugged in, not charging · Held at 80%" (or charge_words alone)
    pub fn charge_line(&self) -> String {
        let held = self.held_at();
        if held.is_empty() { self.charge_words().to_string() } else { format!("{} · {held}", self.charge_words()) }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wifi {
    pub present: bool, // a Wi-Fi device exists
    pub enabled: bool,
    pub ssid: String,  // "" not connected
    pub strength: u8,  // 0..100
}

/// What the Wi-Fi pages say when NetworkManager isn't running and no other
/// manager is found, yet this computer is online (its Wi-Fi link up, or a
/// route out): something runs the network all the same, so "No network
/// service is running" would be false beside the connection (audit #156/#173)
pub const OUTSIDE_TITLE: &str = "The network is managed outside Tessera";
pub const OUTSIDE_REASON: &str = "This computer is online, but not through NetworkManager, which Tessera's Wi-Fi menus need. Choose networks with the tool that set up this connection.";
pub const OUTSIDE_SHORT: &str = "Managed outside Tessera";

/// What the watcher hears (from the two readers below)
enum Heard {
    /// a property changed
    Changed,
    /// the service left the bus (stopped, crashed)
    Lost,
}

/// How long a burst of PropertiesChanged is let settle before one reading
/// (a Wi-Fi scan: dozens a second, each a full re-read before)
const SETTLE: Duration = Duration::from_millis(150);

fn watch<T: Send + Clone + PartialEq + Default + 'static>(
    tx: Sender<Msg>,
    sender: &'static str,
    read: fn(&Connection) -> zbus::Result<T>,
    wrap: fn(T) -> Msg,
) {
    // (a computer without the service, UPower or NetworkManager not
    // installed: asked again less and less often, up to once a minute)
    let mut wait = 5;
    let mut first = true;
    std::thread::spawn(move || loop {
        let mut up = false;
        let mut run = || -> zbus::Result<()> {
            let conn = Connection::system()?;
            // (a computer without it, NetworkManager under iwd or ConnMan, a
            // desktop without UPower: sleep until it appears, not a new bus
            // connection every few seconds, forever)
            wait_for_owner(&conn, sender, first)?;
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender(sender)?
                .interface("org.freedesktop.DBus.Properties")?
                .member("PropertiesChanged")?
                .build();
            let signals = MessageIterator::for_match_rule(rule, &conn, Some(16))?;
            // (the service leaving: its last reading mustn't stay on screen)
            let gone_rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")?
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg(0, sender)?
                .build();
            let owners = MessageIterator::for_match_rule(gone_rule, &conn, Some(8))?;
            // (read on threads of their own, so a burst can be let settle;
            // each ends at its next message once this run is over)
            let (htx, heard) = std::sync::mpsc::channel::<Heard>();
            let h1 = htx.clone();
            std::thread::spawn(move || {
                for _ in signals {
                    if h1.send(Heard::Changed).is_err() {
                        return;
                    }
                }
            });
            std::thread::spawn(move || {
                for m in owners.flatten() {
                    let lost = m.body().deserialize::<(String, String, String)>().is_ok_and(|(_, _, new)| new.is_empty());
                    if lost && htx.send(Heard::Lost).is_err() {
                        return;
                    }
                }
            });
            // (only what changed: NetworkManager and UPower signal often,
            // for properties these readings don't hold)
            let mut last = read(&conn)?;
            up = true;
            tx.send(wrap(last.clone())).map_err(|_| zbus::Error::Failure("ui gone".into()))?;
            let lost = || -> zbus::Result<()> {
                // (not there: no reading, until it's back)
                tx.send(wrap(T::default())).map_err(|_| zbus::Error::Failure("ui gone".into()))?;
                Ok(())
            };
            while let Ok(h) = heard.recv() {
                if let Heard::Lost = h {
                    return lost();
                }
                // (the rest of the burst)
                loop {
                    match heard.recv_timeout(SETTLE) {
                        Ok(Heard::Changed) => {}
                        Ok(Heard::Lost) => return lost(),
                        Err(_) => break,
                    }
                }
                let now = read(&conn)?;
                if now != last {
                    tx.send(wrap(now.clone())).map_err(|_| zbus::Error::Failure("ui gone".into()))?;
                    last = now;
                }
            }
            Ok(())
        };
        if let Err(zbus::Error::Failure(_)) = run() {
            return;
        }
        first = false;
        wait = if up { 5 } else { (wait * 2).min(60) };
        std::thread::sleep(Duration::from_secs(wait));
    });
}

/// Return once `name` has an owner on the bus: at once when it has, else
/// when NameOwnerChanged says it got one (the system bus never activates
/// NetworkManager or UPower for a property read here)
fn wait_for_owner(conn: &Connection, name: &'static str, activate: bool) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, name)?
        .build();
    // (the rule first, so an owner that appears between the two isn't missed)
    let mut changes = MessageIterator::for_match_rule(rule, conn, Some(8))?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn)?;
    let has = |n: &str| zbus::names::BusName::try_from(n).ok().map(|b| dbus.name_has_owner(b).unwrap_or(false)).unwrap_or(false);
    if has(name) {
        return Ok(());
    }
    // (activatable over D-Bus: the first read starts it, as before; a
    // masked one that never starts is then waited for like a missing one)
    if activate && dbus.list_activatable_names().is_ok_and(|l| l.iter().any(|n| n.as_str() == name)) {
        return Ok(());
    }
    while let Some(m) = changes.next() {
        let m = m?;
        let body = m.body();
        if let Ok((_, _, new)) = body.deserialize::<(String, String, String)>() {
            if !new.is_empty() {
                return Ok(());
            }
        }
    }
    Ok(())
}

// Plain property reads: no cache (each reading is fresh, and a cached proxy
// would add its own match rules)
fn proxy<'a>(conn: &Connection, path: &'a str, iface: &'a str, dest: &'a str) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn).destination(dest)?.path(path)?.interface(iface)?.cache_properties(CacheProperties::No).build()
}

fn battery(conn: &Connection) -> zbus::Result<Battery> {
    let p = proxy(conn, "/org/freedesktop/UPower/devices/DisplayDevice", "org.freedesktop.UPower.Device", "org.freedesktop.UPower")?;
    let kind: u32 = p.get_property("Type")?;
    let state: u32 = p.get_property("State")?;
    // (the adapters and the charge limit, read from the kernel: UPower's
    // display device says neither; #437)
    let m = crate::power_supply::mains(&crate::power_supply::read(std::path::Path::new(crate::power_supply::CLASS)));
    Ok(Battery {
        plugged: plugged_in(m.plugged, state),
        limit: m.limit,
        present: kind == 2 && p.get_property::<bool>("IsPresent")?,
        percent: p.get_property("Percentage")?,
        // charging, fully charged, or pending charge: on the charger at a
        // charge limit (80%), which read as unplugged; audit #182
        charging: state == 1 || state == 4 || state == 5,
        state,
        time_to_empty: p.get_property("TimeToEmpty").unwrap_or(0),
        time_to_full: p.get_property("TimeToFull").unwrap_or(0),
    })
}

const NM: &str = "org.freedesktop.NetworkManager";

/// One Wi-Fi device's reading
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Radio {
    pub connected: bool,
    pub strength: u8,
    pub ssid: String,
}

/// Several Wi-Fi devices (a built-in card and a USB stick): the connected
/// one, the strongest of those; else the first. None: no Wi-Fi device.
pub fn pick_radio(radios: &[Radio]) -> Option<&Radio> {
    radios.iter().filter(|r| r.connected).max_by_key(|r| r.strength).or(radios.first())
}

fn radio(conn: &Connection, d: &OwnedObjectPath) -> zbus::Result<Option<Radio>> {
    let dev = proxy(conn, d.as_str(), "org.freedesktop.NetworkManager.Device", NM)?;
    if dev.get_property::<u32>("DeviceType")? != 2 {
        return Ok(None); // not Wi-Fi
    }
    let w = proxy(conn, d.as_str(), "org.freedesktop.NetworkManager.Device.Wireless", NM)?;
    let ap: OwnedObjectPath = w.get_property("ActiveAccessPoint")?;
    let mut r = Radio::default();
    if ap.as_str() != "/" {
        // (the access point can go between two reads: still a device)
        if let Ok(ap) = proxy(conn, ap.as_str(), "org.freedesktop.NetworkManager.AccessPoint", NM) {
            if let (Ok(strength), Ok(ssid)) = (ap.get_property::<u8>("Strength"), ap.get_property::<Vec<u8>>("Ssid")) {
                r = Radio { connected: true, strength, ssid: String::from_utf8_lossy(&ssid).into_owned() };
            }
        }
    }
    Ok(Some(r))
}

fn wifi(conn: &Connection) -> zbus::Result<Wifi> {
    let nm = proxy(conn, "/org/freedesktop/NetworkManager", "org.freedesktop.NetworkManager", NM)?;
    let mut out = Wifi { enabled: nm.get_property("WirelessEnabled")?, ..Default::default() };
    let devices: Vec<OwnedObjectPath> = nm.call("GetDevices", &())?;
    // (a device that goes while it's read, a USB stick pulled out, is
    // skipped: the others still count, and its removal brings a new reading)
    let radios: Vec<Radio> = devices.iter().filter_map(|d| radio(conn, d).ok().flatten()).collect();
    if let Some(r) = pick_radio(&radios) {
        out.present = true;
        out.strength = r.strength;
        out.ssid = r.ssid.clone();
    }
    Ok(out)
}

/// On mains: an adapter's word when one says, else UPower's state
/// (charging, fully charged, pending charge); charging is always plugged
pub fn plugged_in(adapter: Option<bool>, state: u32) -> bool {
    state == 1 || adapter.unwrap_or(matches!(state, 4 | 5))
}

/// The fake battery file's reading: "15 discharging", "80 charging", "100
/// full", "80 held" (plugged in at an 80 % limit)
pub fn fake_battery(text: &str) -> Option<Battery> {
    let mut f = text.split_whitespace();
    let percent: f64 = f.next()?.parse().ok().filter(|p: &f64| (0.0..=100.0).contains(p))?;
    let word = f.next().unwrap_or("discharging");
    let state = match word {
        "charging" => 1,
        "full" => 4,
        "held" => 5,
        _ => 2,
    };
    let limit = (word == "held").then(|| percent.round() as u8).filter(|l| *l < 100);
    Some(Battery { present: true, percent, charging: state != 2, state, time_to_empty: 0, time_to_full: 0, plugged: state != 2, limit })
}

pub fn spawn(tx: Sender<Msg>) {
    if let Some(file) = crate::env::var("FAKE_BATTERY").filter(|_| crate::env::sandboxed()) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut last: Option<Battery> = None;
            loop {
                let b = std::fs::read_to_string(&file).ok().and_then(|t| fake_battery(&t));
                if b.is_some() && b != last {
                    last = b.clone();
                    if tx.send(Msg::Data(Box::new(b.unwrap()))).is_err() {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    } else {
        watch(tx.clone(), "org.freedesktop.UPower", battery, |b| Msg::Data(Box::new(b)));
    }
    watch(tx, "org.freedesktop.NetworkManager", wifi, |w| Msg::Data(Box::new(w)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sandbox_battery() {
        let b = fake_battery("15 discharging\n").unwrap();
        assert!(b.present && !b.charging && b.state == 2 && b.percent == 15.0);
        assert_eq!(fake_battery("80 charging").unwrap().state, 1);
        assert_eq!(fake_battery("100 full").unwrap().state, 4);
        assert_eq!(fake_battery("42").unwrap().state, 2);
        assert!(fake_battery("").is_none() && fake_battery("x").is_none() && fake_battery("150 full").is_none());
        let h = fake_battery("80 held").unwrap();
        assert_eq!((h.state, h.plugged, h.limit, h.charge()), (5, true, Some(80), Charge::Held));
    }

    #[test]
    fn plugged_in_at_the_charge_limit() {
        let b = Battery { present: true, percent: 80.0, charging: true, state: 5, time_to_empty: 0, time_to_full: 0, plugged: true, limit: Some(80) };
        // held at the limit: UPower's pending charge, or its "fully charged" below 100 %
        assert_eq!(b.charge(), Charge::Held);
        assert_eq!(Battery { state: 4, ..b }.charge(), Charge::Held);
        assert_eq!(b.charge_words(), "Plugged in, not charging");
        assert_eq!(b.held_at(), "Held at 80%");
        assert_eq!(b.charge_line(), "Plugged in, not charging · Held at 80%");
        // above the limit (set after it charged) it's still held there; far below, no "Held at"
        assert_eq!(Battery { percent: 84.0, ..b }.held_at(), "Held at 80%");
        assert_eq!(Battery { percent: 40.0, ..b }.charge_line(), "Plugged in, not charging");
        // no limit: plugged in and not charging; full at the top
        let n = Battery { limit: None, ..b };
        assert_eq!((n.charge(), n.held_at()), (Charge::Held, String::new()));
        assert_eq!(Battery { state: 4, percent: 100.0, ..n }.charge(), Charge::Full);
        assert_eq!(Battery { state: 5, percent: 100.0, ..n }.charge_words(), "Fully charged");
        // charging, and on battery (an adapter offline wins over a stale "full")
        assert_eq!(Battery { state: 1, ..b }.charge_words(), "Charging");
        assert_eq!(Battery { state: 2, plugged: false, ..b }.charge_words(), "On battery");
        assert_eq!(Battery { state: 4, plugged: plugged_in(Some(false), 4), ..b }.charge(), Charge::OnBattery);
        // plugged: the adapter's word, else UPower's state
        assert!(plugged_in(Some(true), 2) && plugged_in(None, 5) && plugged_in(None, 4) && plugged_in(Some(false), 1));
        assert!(!plugged_in(None, 2) && !plugged_in(Some(false), 5));
    }

    fn r(connected: bool, strength: u8, ssid: &str) -> Radio {
        Radio { connected, strength, ssid: ssid.into() }
    }

    #[test]
    fn the_connected_wifi_device_is_the_one_shown() {
        // Ethernet only: no Wi-Fi device
        assert_eq!(pick_radio(&[]), None);
        // one card, not connected
        assert_eq!(pick_radio(&[r(false, 0, "")]), Some(&r(false, 0, "")));
        // a built-in card idle and a USB stick connected: the stick
        assert_eq!(pick_radio(&[r(false, 0, ""), r(true, 40, "Cafe")]).unwrap().ssid, "Cafe");
        // both connected: the stronger
        assert_eq!(pick_radio(&[r(true, 30, "A"), r(true, 70, "B")]).unwrap().ssid, "B");
    }
}
