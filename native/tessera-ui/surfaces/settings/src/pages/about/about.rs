//! About (pages/about.qml): this build (version.rs: Version, with Copy), this computer, the subpages (System health,
//! now a part of System → Health; Login Items, Report a Bug; Back up and
//! restore, backup.rs), the welcome screen again. The helpers here run
//! assets/scripts/settings-backup.py as the QML page runs it (a settings
//! file, .tessera: the settings, shortcuts, Focus modes and wallpaper,
//! without keys, passwords, accounts or this computer's displays): Back up
//! and restore still restores one (shows what's in the file first, then
//! Undo puts back what was there).

use super::super::super::model::{Action, Ctx, Page, Row};
use super::{feed, s, sysinfo};
use serde_json::{json, Value};

pub fn script() -> String {
    crate::util::asset("shell/assets/scripts/settings-backup.py").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}
pub fn config_dir() -> String {
    crate::theme::config_path().parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The helper's answer: one JSON object (blocking)
pub fn helper(args: Vec<String>) -> Value {
    let out = std::process::Command::new("python3").arg("-I").arg(script()).args(&args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output();
    out.ok()
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
        .unwrap_or_else(|| json!({"ok": false, "error": "The backup helper didn't answer. Check that Python 3 is installed, then try again."}))
}

/// "2026-11-05T15:04:00" as "5 Nov 2026, 3:04 PM"
pub fn created(iso: &str) -> String {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let num = |a: usize, b: usize| iso.get(a..b).and_then(|x| x.parse::<u32>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi)) = (num(0, 4), num(5, 7), num(8, 10), num(11, 13), num(14, 16)) else { return String::new() };
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{d} {} {y}, {h12}:{mi:02} {}", MON.get(mo.saturating_sub(1) as usize).unwrap_or(&""), if h < 12 { "AM" } else { "PM" })
}

/// What a settings file holds, in a sentence (about.qml's describe)
pub fn describe(i: &Value) -> String {
    let mode = s(&i["mode"]);
    let mut parts = vec![format!(
        "{} look{}",
        if i["look"] == "material" { "Material" } else { "Glass" },
        if mode.is_empty() {
            String::new()
        } else {
            format!(", {}", match mode.as_str() {
                "light" => "light",
                "dark" => "dark",
                "auto" => "light and dark by the sun",
                m => m,
            })
        }
    )];
    parts.push(format!("wallpaper: {}", s(&i["wallpaper"])));
    let n = i["shortcuts"].as_i64().unwrap_or(0);
    if n > 0 {
        parts.push(format!("{n} {}", if n == 1 { "shortcut change" } else { "shortcut changes" }));
    }
    let focus = super::strings(&i["focusModes"]);
    if !focus.is_empty() {
        parts.push(format!("Focus: {}", focus.join(", ")));
    }
    let c = created(&s(&i["created"]));
    format!("{}{}", parts.join(" · "), if c.is_empty() { ".".to_string() } else { format!(". Saved {c}.") })
}

/// The look restored: switched to, as Settings → Appearance does
pub fn follow(cx: &Ctx, look: &str) {
    let now = if cx.material { "material" } else { "glass" };
    if !look.is_empty() && look != now {
        if let Some(sw) = crate::util::asset("shell/assets/scripts/switch-look.sh") {
            Ctx::detach(vec!["setsid".into(), "-f".into(), sw.to_string_lossy().into_owned(), look.to_string()]);
        }
    }
}

/// The helper's answers that came in since the page was last built
pub fn answers(cx: &mut Ctx) {
    if let Some(out) = cx.local.remove("about.backup.export") {
        let ok = out["ok"] == true;
        cx.local_set("about.backup.status", if ok { String::new() } else { s(&out["error"]) });
        cx.local_set("about.backup.savedTo", if ok { s(&out["file"]) } else { String::new() });
    }
    if let Some(out) = cx.local.remove("about.backup.inspect") {
        let ok = out["ok"] == true;
        cx.local_set("about.backup.status", if ok { String::new() } else { s(&out["error"]) });
        let file = cx.local_s("about.backup.choosing");
        cx.local_set("about.backup.pending", if ok { json!({"file": file, "info": out}) } else { Value::Null });
    }
    if let Some(out) = cx.local.remove("about.backup.restore") {
        if out["ok"] != true {
            cx.local_set("about.backup.status", s(&out["error"]));
        } else {
            let skipped = super::strings(&out["skipped"]);
            cx.local_set("about.backup.status", if skipped.is_empty() { String::new() } else { format!("Left out {}.", skipped.join(" and ")) });
            cx.ask("about", "focusReload", Value::Null);
            let undo_file = s(&out["undo"]);
            let undo: Action = super::action(move |cx, _| {
                let f = undo_file.clone();
                cx.spawn("about.backup.undo", move || helper(vec!["restore".into(), f, "--config".into(), config_dir(), "--no-undo".into()]));
            });
            cx.toast("Settings restored", Some(undo));
            follow(cx, &s(&out["look"]));
        }
    }
    if let Some(back) = cx.local.remove("about.backup.undo") {
        let ok = back["ok"] == true;
        cx.local_set("about.backup.status", if ok { String::new() } else { s(&back["error"]) });
        if ok {
            cx.ask("about", "focusReload", Value::Null);
            cx.toast("Your earlier settings are back", None);
            follow(cx, &s(&back["look"]));
        }
    }
}

pub fn page(cx: &mut Ctx, p: &mut Page) {
    let f = feed(cx);
    let info = cx.once("about.sysinfo", sysinfo::read);
    let name_ph = {
        let n = s(&f["welcomeName"]);
        let n = if n.is_empty() { s(&info["name"]) } else { n };
        let first = n.split_whitespace().next().unwrap_or("").to_string();
        if first.is_empty() { "Your name".to_string() } else { first }
    };
    let name_set = cx.on(|cx, a| {
        if a.i == 1 {
            let t = a.s.trim().to_string();
            if t != cx.s("today.name") {
                cx.set("today.name", t);
            }
        }
    });
    let today_name = cx.s("today.name");
    // (the build that's running, as `tessera-ui --version` says it: version.rs)
    let version = super::version::row(cx, "Version", "");
    p.group("")
        .row(version)
        .row(Row::new("System").shows(if info.is_null() { "Linux".to_string() } else { s(&info["os"]) }))
        .row(Row::new("Computer name").shows(s(&info["host"])))
        .row(Row::new("Your name").detail("Today greets you by it. Empty: your account's name").field(today_name, name_ph, name_set).width(220.0))
        .row(Row::new("Processor").shows(s(&info["cpu"])))
        .row(Row::new("Graphics").shows(s(&info["gpu"])))
        .row(Row::new("Memory").shows(s(&info["memory"])));

    let checks = f["healthChecks"].as_i64().unwrap_or(0);
    let attention = f["healthAttention"].as_i64().unwrap_or(0);
    let health = cx.link("health", "");
    let login = cx.link("about/loginitems", "");
    let bug = cx.link("about/bugreport", "");
    p.group("")
        .row(Row::link("Health", "health")
            .icon("heartbeat")
            .detail("Whether the services Tessera uses are working, and how to fix them")
            .value(if checks == 0 { String::new() } else if attention == 0 { "All good".into() } else { format!("{attention} to check") })
            .id(health))
        .row(Row::link("Login items", "about/loginitems").icon("rocket-launch").detail("The apps that start when you log in").id(login))
        .row(Row::link("Report a bug", "about/bugreport").icon("bug").detail("Tell Tessera what went wrong, with what helps fix it").id(bug));

    let welcome = cx.on(|cx, _| cx.ask("about", "welcome", Value::Null));
    p.group("")
        .footer("Settings live in ~/.config/tessera/config.json and apply as you change them.")
        .row(Row::new("Welcome screen").detail("Pick the look, light or dark, and a wallpaper again").button("Show", welcome));

    // ── Back up and restore: its own page (backup.rs) ──
    let backup = cx.link("about/backup", "");
    p.group("").row(
        Row::link("Back up and restore", "about/backup")
            .icon("archive")
            .detail("Your settings, Hyprland edits, wallpapers and the apps you installed, in one file; nothing secret")
            .id(backup),
    );

    // ── More options ──
    p.more(cx);
    {
        let cfg = crate::theme::config_path().to_string_lossy().into_owned();
        let open = cx.on(move |cx, _| cx.ask("about", "openFile", json!([cfg])));
        p.group("").row(Row::new("Configuration file").button("Open", open));
    }
    // (only where the release publisher runs: the repository owner's
    // machine. How server1's builds and this machine's signing are doing)
    let pipe = &f["pipeline"];
    if !pipe.is_null() {
        let (title, detail, tone) = pipeline_words(pipe);
        p.group("Release pipeline")
            .footer("Builds happen on the build machine; signing and publishing happen here. The log is ~/.cache/tessera-publish.log.")
            .row(Row::new(title).detail(detail).tone(i32::from(tone)));
    }
    p.end_more();
}

/// The release pipeline's health in a line: (title, detail, tone)
pub fn pipeline_words(st: &Value) -> (String, String, u8) {
    let h = &st["health"];
    let b = &h["builder"];
    let waiting = h["waiting"].as_array().cloned().unwrap_or_default();
    let last = st["published"].as_object().and_then(|m| m.values().filter_map(|v| v["at"].as_str()).max().map(str::to_string)).unwrap_or_default();
    let last = if last.is_empty() { "nothing published yet".to_string() } else { format!("last published {}", last.get(..16).unwrap_or(&last).replace('T', " ")) };
    if b["state"] == "failed" {
        return ("The last build failed".into(), format!("{} · {last}", s(&b["error"]).chars().take(160).collect::<String>()), 2);
    }
    if st["state"] == "failed" {
        return ("The last publish failed".into(), format!("{} · {last}", s(&st["error"]).chars().take(160).collect::<String>()), 2);
    }
    if !waiting.is_empty() {
        return (format!("{} waiting to be signed", if waiting.len() == 1 { "1 set".to_string() } else { format!("{} sets", waiting.len()) }), last, 0);
    }
    if b["state"] == "building" {
        return ("Building on server1".into(), format!("{} · {last}", s(&b["step"])), 0);
    }
    if !s(&st["reach"]).is_empty() {
        return ("The build machine is out of reach".into(), format!("Nothing can be published until it answers · {last}"), 1);
    }
    ("Everything is published".into(), last, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipeline_in_a_line() {
        let ok = json!({"state": "idle", "health": {"builder": {"state": "idle"}, "waiting": []}, "published": {"tessera-ui-beta": {"at": "2026-11-19T07:39:59+0530"}}});
        assert_eq!(pipeline_words(&ok), ("Everything is published".to_string(), "last published 2026-11-19 07:39".to_string(), 0));
        let failed = json!({"health": {"builder": {"state": "failed", "error": "the x86-64-v3 build failed its test before publishing: it did not answer IPC within 30 s"}, "waiting": []}});
        assert_eq!(pipeline_words(&failed).0, "The last build failed");
        assert_eq!(pipeline_words(&failed).2, 2);
        let waiting = json!({"health": {"builder": {"state": "ready"}, "waiting": [{"set": "tessera-ui-beta/abc"}]}});
        assert_eq!(pipeline_words(&waiting).0, "1 set waiting to be signed");
        let away = json!({"reach": "ssh: connect to host server1: No route to host", "health": {"builder": {}, "waiting": []}});
        assert_eq!(pipeline_words(&away).0, "The build machine is out of reach");
    }

    #[test]
    fn a_settings_file_described() {
        let i = json!({"look": "material", "mode": "auto", "wallpaper": "Tessera's own", "shortcuts": 1, "focusModes": ["Work"], "created": "2026-11-05T15:04:00"});
        assert_eq!(describe(&i), "Material look, light and dark by the sun · wallpaper: Tessera's own · 1 shortcut change · Focus: Work. Saved 5 Nov 2026, 3:04 PM.");
        let i = json!({"look": "glass", "mode": "", "wallpaper": "a.jpg", "shortcuts": 0, "focusModes": [], "created": ""});
        assert_eq!(describe(&i), "Glass look · wallpaper: a.jpg.");
        assert_eq!(created("2026-01-02T00:09:00"), "2 Jan 2026, 12:09 AM");
    }
}
