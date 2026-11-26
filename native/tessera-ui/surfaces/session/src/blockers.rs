//! What holds up Shut Down, Restart and Log Out (#305): logind's
//! inhibitors, read just before the action, as rows of app name, icon and
//! reason ("Firefox: Downloading files"), and the words of the dialog that
//! shows them with Cancel and "Shut Down Anyway".
//!
//! Which count: tessera_core::inhibit::holds_up (shared with the shell's
//! System.power, which asks the same before a Shut Down from Search or a
//! script).

use std::path::PathBuf;
use tessera_core::inhibit::{holds_up, Inhibitor};

/// One app holding the action up
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blocker {
    pub name: String,
    pub why: String,
    /// its icon's file, when one was found
    pub icon: Option<PathBuf>,
}

/// What the dialog lists at most; the rest are counted in one row
pub const MAX_ROWS: usize = 5;

/// The power actions that look for blockers first
pub fn asks(action: &str) -> bool {
    matches!(action, "shutdown" | "restart" | "logout")
}

/// The name shown: its desktop entry's, else what it calls itself (the
/// last part of a reverse-DNS id: org.gnome.Nautilus → Nautilus), else its
/// process name, else "An app"
pub fn name_for(who: &str, entry: Option<&str>, comm: &str) -> String {
    if let Some(e) = entry.map(str::trim).filter(|e| !e.is_empty()) {
        return e.to_string();
    }
    let who = who.trim();
    if !who.is_empty() {
        let dns = !who.contains(char::is_whitespace) && who.matches('.').count() >= 2;
        let last = who.rsplit('.').next().unwrap_or(who);
        return if dns && !last.is_empty() { last.to_string() } else { who.to_string() };
    }
    let comm = comm.trim();
    if comm.is_empty() { "An app".into() } else { comm.to_string() }
}

/// The reason shown: its first line, at most 120 characters; "Busy" when
/// it gave none
pub fn reason(why: &str) -> String {
    let first = why.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if first.is_empty() {
        return "Busy".into();
    }
    if first.chars().count() > 120 {
        first.chars().take(119).collect::<String>() + "…"
    } else {
        first.to_string()
    }
}

/// "Firefox: Downloading files" (a screen reader's row, a notification)
pub fn line(b: &Blocker) -> String {
    format!("{}: {}", b.name, b.why)
}

/// The rows for these inhibitors, each app and reason once. `resolve`
/// finds an inhibitor's desktop-entry name, process name and icon
pub fn gather(list: &[Inhibitor], me_pid: u32, me_uid: u32, resolve: impl Fn(&Inhibitor) -> (Option<String>, String, Option<PathBuf>)) -> Vec<Blocker> {
    let mut out: Vec<Blocker> = Vec::new();
    for i in list.iter().filter(|i| holds_up(i, me_pid, me_uid)) {
        let (entry, comm, icon) = resolve(i);
        let b = Blocker { name: name_for(&i.who, entry.as_deref(), &comm), why: reason(&i.why), icon };
        if !out.iter().any(|o| o.name == b.name && o.why == b.why) {
            out.push(b);
        }
    }
    out
}

/// At most MAX_ROWS rows; past that, one row counts the rest
pub fn shown(list: &[Blocker]) -> Vec<Blocker> {
    if list.len() <= MAX_ROWS {
        return list.to_vec();
    }
    let mut v = list[..MAX_ROWS - 1].to_vec();
    let rest = list.len() - (MAX_ROWS - 1);
    v.push(Blocker { name: format!("{rest} more apps"), why: "Also busy".into(), icon: None });
    v
}

fn verb(action: &str) -> &'static str {
    match action {
        "restart" => "restarting",
        "logout" => "logging out",
        _ => "shutting down",
    }
}

/// The dialog's heading: "Firefox is keeping the computer from shutting
/// down", "2 apps are keeping you from logging out"
pub fn title(action: &str, list: &[Blocker]) -> String {
    let (who, is) = match list {
        [one] => (one.name.clone(), "is"),
        l => (format!("{} apps", l.len()), "are"),
    };
    let whom = if action == "logout" { "you" } else { "the computer" };
    format!("{who} {is} keeping {whom} from {}", verb(action))
}

/// The line under it: "Shutting down anyway could lose its work."
pub fn subtitle(action: &str, n: usize) -> String {
    let v = verb(action);
    let mut c = v.chars();
    let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    format!("{cap} anyway could lose {} work.", if n == 1 { "its" } else { "their" })
}

/// The button that goes ahead: "Shut Down Anyway" (Material: "Shut down anyway")
pub fn anyway(action: &str, material: bool) -> String {
    crate::label(
        match action {
            "restart" => "Restart Anyway",
            "logout" => "Log Out Anyway",
            _ => "Shut Down Anyway",
        },
        material,
    )
}

/// The process name of a pid ("" when it's gone)
fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// An inhibitor's desktop-entry name, process name and icon
fn resolve(i: &Inhibitor) -> (Option<String>, String, Option<PathBuf>) {
    use tessera_core::util::desktop_field;
    let comm = comm(i.pid);
    let keys: Vec<&str> = [i.who.as_str(), comm.as_str()].into_iter().filter(|k| !k.trim().is_empty()).collect();
    let entry = keys.iter().find_map(|k| desktop_field(k, "Name"));
    let icon = keys.iter().find_map(|k| tessera_core::icon_theme::for_class(k));
    (entry, comm, icon)
}

/// What holds the actions up now (None: logind didn't answer, so nothing
/// is known to). Blocking: call it off the UI thread
pub fn now() -> Option<Vec<Blocker>> {
    let list = tessera_core::inhibit::list()?;
    Some(gather(&list, std::process::id(), tessera_core::inhibit::my_uid(), resolve))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inh(what: &str, who: &str, why: &str, mode: &str, uid: u32, pid: u32) -> Inhibitor {
        Inhibitor { what: what.into(), who: who.into(), why: why.into(), mode: mode.into(), uid, pid }
    }

    #[test]
    fn names_and_reasons() {
        assert_eq!(name_for("firefox", Some("Firefox"), "firefox"), "Firefox");
        assert_eq!(name_for("org.gnome.Nautilus", None, "nautilus"), "Nautilus");
        assert_eq!(name_for("Unattended Upgrades Shutdown", None, "python3"), "Unattended Upgrades Shutdown");
        assert_eq!(name_for("fwupd", Some("  "), ""), "fwupd");
        assert_eq!(name_for("", None, "rsync"), "rsync");
        assert_eq!(name_for("", None, ""), "An app");
        assert_eq!(reason("Downloading files\nsecond line"), "Downloading files");
        assert_eq!(reason("  "), "Busy");
        assert_eq!(reason(&"x".repeat(200)).chars().count(), 120);
        assert_eq!(line(&Blocker { name: "Firefox".into(), why: "Downloading files".into(), icon: None }), "Firefox: Downloading files");
    }

    #[test]
    fn the_rows_each_once() {
        let l = vec![
            inh("shutdown", "Firefox", "Downloading files", "block", 1000, 5),
            inh("shutdown:sleep", "Firefox", "Downloading files", "block", 1000, 5),
            inh("idle", "mpv", "Playing", "block", 1000, 6),
            inh("shutdown", "", "", "block", 0, 7),
        ];
        let rows = gather(&l, 1, 1000, |i| (None, if i.pid == 7 { "pacman".into() } else { String::new() }, None));
        assert_eq!(rows.len(), 2);
        assert_eq!(line(&rows[0]), "Firefox: Downloading files");
        assert_eq!(line(&rows[1]), "pacman: Busy");
        // many: the last row counts the rest
        let many: Vec<Blocker> = (0..8).map(|n| Blocker { name: format!("App {n}"), why: "x".into(), icon: None }).collect();
        let s = shown(&many);
        assert_eq!(s.len(), MAX_ROWS);
        assert_eq!(s[MAX_ROWS - 1].name, "4 more apps");
        assert_eq!(shown(&many[..3]).len(), 3);
    }

    #[test]
    fn the_words_follow_the_action() {
        let one = vec![Blocker { name: "Firefox".into(), why: "Downloading files".into(), icon: None }];
        let two = vec![one[0].clone(), Blocker { name: "fwupd".into(), why: "Updating".into(), icon: None }];
        assert_eq!(title("shutdown", &one), "Firefox is keeping the computer from shutting down");
        assert_eq!(title("restart", &two), "2 apps are keeping the computer from restarting");
        assert_eq!(title("logout", &one), "Firefox is keeping you from logging out");
        assert_eq!(subtitle("shutdown", 1), "Shutting down anyway could lose its work.");
        assert_eq!(subtitle("logout", 2), "Logging out anyway could lose their work.");
        assert_eq!(anyway("shutdown", false), "Shut Down Anyway");
        assert_eq!(anyway("restart", false), "Restart Anyway");
        assert_eq!(anyway("logout", true), "Log out anyway");
        assert_eq!(anyway("shutdown", true), "Shut down anyway");
        assert!(asks("shutdown") && asks("restart") && asks("logout"));
        assert!(!asks("sleep") && !asks("lock"));
    }
}
