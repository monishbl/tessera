//! logind inhibitors held by this process itself: the file descriptor logind
//! hands back, kept open for as long as it's held. It closes when the
//! process ends, however it ends (a crash, a kill), so an inhibitor can never
//! outlive the shell. (16 Nov: systemd-inhibit helpers ignored SIGTERM while
//! their child ran, outlived crashed shells, and a closed lid didn't sleep a
//! laptop for 7 hours.)

use zbus::zvariant::OwnedFd;

const DEST: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const IFACE: &str = "org.freedesktop.login1.Manager";

/// Hold a "block" inhibitor for `what` ("handle-lid-switch", "idle",
/// "sleep"…); drop the fd to let go. None: no logind (or it refused).
pub fn hold(what: &str, why: &str) -> Option<OwnedFd> {
    take(what, why, "block")
}

/// Hold a "delay" inhibitor for `what` ("sleep", "shutdown"): logind waits
/// (up to its InhibitDelayMaxSec, 5 s by default) after PrepareForSleep
/// until it's dropped, so a part can tidy up first. Drop it once done, and
/// take it again after the wake.
pub fn delay(what: &str, why: &str) -> Option<OwnedFd> {
    take(what, why, "delay")
}

/// `hold` on the pool: `done` gets the answer there (send it to the part
/// by message). For a caller on the UI thread, which a system-bus round
/// trip right after a wake could stall (audit #48)
pub fn hold_then(what: &str, why: &str, done: impl FnOnce(Option<OwnedFd>) + Send + 'static) {
    let (what, why) = (what.to_string(), why.to_string());
    crate::work::spawn(move || done(hold(&what, &why)));
}

/// `lid_closed` on the pool, the same way
pub fn lid_closed_then(done: impl FnOnce(Option<bool>) + Send + 'static) {
    crate::work::spawn(move || done(lid_closed()));
}

static CONN: std::sync::Mutex<Option<zbus::blocking::Connection>> = std::sync::Mutex::new(None);

/// One system-bus connection for these calls, made once (each call made
/// its own before: a connect, auth and Hello every time, and a zbus
/// executor thread started and dropped), made again after it dies.
/// Shared with other one-off system-bus calls (host/system's energy mode,
/// audit #135); call it off the UI thread
pub fn system_bus() -> Option<zbus::blocking::Connection> {
    system()
}

fn system() -> Option<zbus::blocking::Connection> {
    let mut c = CONN.lock().unwrap_or_else(|e| e.into_inner());
    if c.is_none() {
        *c = zbus::blocking::Connection::system().ok();
    }
    c.clone()
}

/// A call's answer; an I/O error (the socket closed) drops the connection
fn answer<T>(r: zbus::Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            if matches!(e, zbus::Error::InputOutput(_)) {
                *CONN.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            None
        }
    }
}

fn take(what: &str, why: &str, mode: &str) -> Option<OwnedFd> {
    let conn = system()?;
    let reply = answer(conn.call_method(Some(DEST), PATH, Some(IFACE), "Inhibit", &(what, "Tessera", why, mode)))?;
    reply.body().deserialize::<OwnedFd>().ok()
}

/// One of logind's inhibitors (ListInhibitors): `what` is colon-separated
/// ("shutdown:sleep"), `who` the app's own name for itself, `why` its
/// reason, `mode` block | block-weak | delay
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inhibitor {
    pub what: String,
    pub who: String,
    pub why: String,
    pub mode: String,
    pub uid: u32,
    pub pid: u32,
}

/// Every inhibitor logind holds now (None: no logind). Only a read: it
/// changes nothing. Call it off the UI thread (#305: the power menu asks
/// before Shut Down, Restart and Log Out)
pub fn list() -> Option<Vec<Inhibitor>> {
    let conn = system()?;
    let reply = answer(conn.call_method(Some(DEST), PATH, Some(IFACE), "ListInhibitors", &()))?;
    let v: Vec<(String, String, String, String, u32, u32)> = reply.body().deserialize().ok()?;
    Some(v.into_iter().map(|(what, who, why, mode, uid, pid)| Inhibitor { what, who, why, mode, uid, pid }).collect())
}

/// Does this inhibitor hold up a shutdown, restart or logout (#305)?
/// (`me_pid`, `me_uid`: this shell's own process and user.) A "shutdown"
/// one in block mode, anyone's (a disc or firmware write, a package tool),
/// or in delay mode when it's one of your own apps; a "sleep" one in block
/// mode from your own apps. Never Tessera's own (the lock's sleep delay,
/// Caffeine), and never a system service's delay: it only waits a few
/// seconds, and nothing of yours is lost
pub fn holds_up(i: &Inhibitor, me_pid: u32, me_uid: u32) -> bool {
    if i.pid == me_pid || i.who == "Tessera" {
        return false;
    }
    let what: Vec<&str> = i.what.split(':').map(str::trim).collect();
    // (block and block-weak; delay only waits)
    let block = i.mode.starts_with("block");
    let mine = i.uid == me_uid;
    (what.contains(&"shutdown") && (block || mine)) || (what.contains(&"sleep") && block && mine)
}

/// This process's user
pub fn my_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX)
}

/// Is anything holding up a shutdown, restart or logout now? (false when
/// logind doesn't answer.) Blocking: off the UI thread
pub fn held_up() -> bool {
    let me = (std::process::id(), my_uid());
    list().is_some_and(|l| l.iter().any(|i| holds_up(i, me.0, me.1)))
}

/// logind's LidClosed now (None: no lid, or no logind)
pub fn lid_closed() -> Option<bool> {
    let conn = system()?;
    let reply = answer(conn.call_method(Some(DEST), PATH, Some("org.freedesktop.DBus.Properties"), "Get", &(IFACE, "LidClosed")))?;
    let v: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_inhibitors_hold_up_a_shutdown() {
        let inh = |what: &str, who: &str, mode: &str, uid: u32, pid: u32| Inhibitor { what: what.into(), who: who.into(), why: String::new(), mode: mode.into(), uid, pid };
        let me = (100, 1000);
        // a block on shutdown: anyone's
        assert!(holds_up(&inh("shutdown", "Firefox", "block", 1000, 5), me.0, me.1));
        assert!(holds_up(&inh("sleep:shutdown", "fwupd", "block", 0, 6), me.0, me.1));
        assert!(holds_up(&inh("shutdown", "x", "block-weak", 0, 6), me.0, me.1));
        // a delay: only your own apps'
        assert!(holds_up(&inh("shutdown", "Writer", "delay", 1000, 7), me.0, me.1));
        assert!(!holds_up(&inh("shutdown", "Unattended Upgrades", "delay", 0, 8), me.0, me.1));
        // sleep: only a block by your own apps
        assert!(holds_up(&inh("sleep", "Backup", "block", 1000, 9), me.0, me.1));
        assert!(!holds_up(&inh("sleep", "NetworkManager", "delay", 0, 10), me.0, me.1));
        assert!(!holds_up(&inh("sleep", "Backup", "delay", 1000, 9), me.0, me.1));
        assert!(!holds_up(&inh("sleep", "udisks", "block", 0, 9), me.0, me.1));
        // not ones that don't touch power, and never Tessera's own
        assert!(!holds_up(&inh("idle", "mpv", "block", 1000, 11), me.0, me.1));
        assert!(!holds_up(&inh("handle-lid-switch", "x", "block", 1000, 11), me.0, me.1));
        assert!(!holds_up(&inh("shutdown", "Tessera", "block", 1000, 12), me.0, me.1));
        assert!(!holds_up(&inh("shutdown", "helper", "block", 1000, 100), me.0, me.1));
    }

    #[test]
    fn a_dead_system_connection_is_made_again() {
        assert_eq!(answer(Ok(3u8)), Some(3));
        assert_eq!(answer::<u8>(Err(zbus::Error::Failure("refused".into()))), None);
        // (an I/O error: the kept connection goes, the next call makes one)
        assert_eq!(answer::<u8>(Err(zbus::Error::InputOutput(std::sync::Arc::new(std::io::Error::other("closed"))))), None);
        assert!(CONN.lock().unwrap().is_none());
    }
}
