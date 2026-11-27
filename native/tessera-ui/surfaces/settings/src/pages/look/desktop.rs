//! Desktop & Widgets (pages/desktop.qml): Edit Widgets, the files app, the
//! weather's place and units, hot corners, and the drop shelf.

use super::super::super::model::{Ctx, Page, Row};
use super::feed;
use serde_json::Value;

/// Hot corners' actions: (value, label)
pub const CORNER_ACTIONS: [(&str, &str); 9] = [
    ("none", "None"),
    ("overview", "Overview"),
    ("missionControl", "All Windows"),
    ("desktop", "Show desktop"),
    ("notificationCenter", "Notification Center"),
    ("launchpad", "App Drawer"),
    ("lock", "Lock screen"),
    ("standby", "Start Bedside Mode"),
    ("quickNote", "Quick note"),
];

/// The file managers Files.qml knows, in its order
pub const FILE_MANAGERS: &[(&str, &str)] = &[
    ("nautilus", "Files (Nautilus)"),
    ("dolphin", "Dolphin"),
    ("thunar", "Thunar"),
    ("nemo", "Nemo"),
    ("caja", "Caja"),
    ("pcmanfm-qt", "PCManFM-Qt"),
    ("pcmanfm", "PCManFM"),
];

/// desktop.screen's choices (#501): "primary" first, then each connected
/// display by its friendly name ("Model · DP-1"), and one chosen that isn't
/// connected now (it comes back when plugged in; primary meanwhile).
/// screens: (connector, model); gives (values, labels, the chosen index)
pub fn screen_choices(screens: &[(String, String)], chosen: &str) -> (Vec<String>, Vec<String>, i32) {
    let mut values = vec!["primary".to_string()];
    let mut labels = vec!["Main display".to_string()];
    for (name, model) in screens {
        values.push(name.clone());
        labels.push(if model.is_empty() || model.eq_ignore_ascii_case(name) { name.clone() } else { format!("{model}  \u{b7}  {name}") });
    }
    if !chosen.is_empty() && chosen != "primary" && !values.iter().any(|v| v == chosen) {
        values.push(chosen.to_string());
        labels.push(format!("{chosen} (not connected)"));
    }
    let at = if chosen.is_empty() { 0 } else { values.iter().position(|v| v == chosen).map_or(0, |i| i as i32) };
    (values, labels, at)
}

/// "3 widgets", "1 widget"
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The Files app row's words: what it does, and that a change restarts the
/// desktop's portal at once (its open/save picker follows the Files app),
/// which ends a screen share or recording running through it (#331)
pub const FILES_APP_DETAIL: &str = "Opens folders, Downloads and the Trash. Changing it restarts the desktop's file picker at once, which ends any screen share or recording in progress";

pub fn desktop(cx: &mut Ctx, p: &mut Page) {
    let widgets = cx.list("desktop.widgets").len();
    let edit = cx.on(|cx, _| cx.ask("look", "editWidgets", Value::Null));
    // Files app: Automatic (the first installed), or one of them (the shell says which are)
    let mut managers: Vec<(String, String)> = match feed(cx)["files"].as_array() {
        Some(a) => a.iter().map(|m| (m["bin"].as_str().unwrap_or("").to_string(), m["name"].as_str().unwrap_or("").to_string())).collect(),
        // (the standalone shell doesn't say: Files.qml's list, those on the PATH)
        None => FILE_MANAGERS.iter().filter(|(bin, _)| crate::pages::accounts::defaultapps::on_path(bin)).map(|(b, n)| (b.to_string(), n.to_string())).collect(),
    };
    let fm = cx.s("desktop.fileManager");
    // (one chosen that isn't installed here still shows, by its name, as the
    // QML's Dropdown shows a value it has no option for: not "Choose")
    let auto = managers.first().map(|m| format!(" ({})", m.1)).unwrap_or_default();
    let none_installed = managers.is_empty();
    if !fm.is_empty() && !managers.iter().any(|m| m.0 == fm) {
        managers.push((fm.clone(), fm.clone()));
    }
    let mut opts = vec![format!("Automatic{auto}")];
    opts.extend(managers.iter().map(|m| m.1.clone()));
    let index = if fm.is_empty() { 0 } else { managers.iter().position(|m| m.0 == fm).map_or(-1, |i| i as i32 + 1) };
    let bins: Vec<String> = managers.iter().map(|m| m.0.clone()).collect();
    let fp = cx.on(move |cx, a| {
        let v = if a.i <= 0 { String::new() } else { bins.get(a.i as usize - 1).cloned().unwrap_or_default() };
        cx.set("desktop.fileManager", v.clone());
        // (the open/save picker apps ask the desktop for follows it:
        // shell/assets/scripts/file-picker.py; told the choice, as the
        // config may be written a moment later)
        if let Some(script) = crate::util::asset("shell/assets/scripts/file-picker.py") {
            std::thread::spawn(move || {
                let mut c = std::process::Command::new("python3");
                c.arg("-I").arg(script).arg("apply").arg("--app").arg(&v).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                unsafe {
                    std::os::unix::process::CommandExt::pre_exec(&mut c, || {
                        tessera_core::util::child_signals();
                        Ok(())
                    });
                }
                let _ = c.status();
            });
        }
    });
    // Which display shows the widgets and files (desktop.screen, #501)
    let screens: Vec<(String, String)> = match feed(cx)["screens"].as_array() {
        Some(a) if !a.is_empty() => a.iter().map(|s| (s["name"].as_str().unwrap_or("").to_string(), s["model"].as_str().unwrap_or("").to_string())).collect(),
        _ => cx.screens.iter().map(|n| (n.clone(), String::new())).collect(),
    };
    let chosen = cx.s("desktop.screen");
    let (svalues, slabels, sat) = screen_choices(&screens, &chosen);
    let show_screen = screens.len() > 1 || svalues.len() > 2;
    let sp = cx.on(move |cx, a| {
        if let Some(v) = svalues.get(a.i.max(0) as usize) {
            cx.set("desktop.screen", v.clone());
        }
    });
    p.group("Widgets")
        .footer("Drag a widget to move it; it snaps to the grid. Right-click a widget to change its size, set what it shows (Widget options) or remove it, or right-click the desktop.")
        .row(Row::new("Edit widgets").detail(format!("{} on the desktop", count(widgets, "widget", "widgets"))).button("Edit…", edit).primary())
        .row(Row::new("Show on")
            .visible(show_screen)
            .detail("The display with the widgets and the Desktop folder's files; if it's unplugged, the main display")
            .choice(&slabels, sat, sp)
            .width(260.0))
        .row(Row::new("Files app")
            .detail(if none_installed { format!("No file manager installed (e.g. {})", tessera_core::util::install_hint("nautilus", "", "")) } else { FILES_APP_DETAIL.to_string() })
            .choice(&opts, index, fp)
            .width(220.0));

    // Weather
    cx.want("weather");
    let w = match cx.shell.get::<crate::services::weather::Weather>() {
        Some(s) if !feed(cx)["weather"].is_object() => serde_json::json!({"ready": s.ready, "place": s.place, "temp": s.temp, "condition": s.condition, "error": s.error}),
        _ => feed(cx)["weather"].clone(),
    };
    let footer = if w["ready"].as_bool().unwrap_or(false) {
        format!("Showing {}: {}°, {}.", w["place"].as_str().unwrap_or(""), w["temp"].as_i64().unwrap_or(0), w["condition"].as_str().unwrap_or("").to_lowercase())
    } else {
        w["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("Loading…").to_string()
    };
    let loc = cx.on(|cx, a| {
        if a.i == 1 {
            let t = a.s.trim().to_string();
            if t != cx.s("weather.location") {
                cx.set("weather.location", t);
            }
        }
    });
    let units = cx.s("weather.units");
    let up = cx.on(|cx, a| cx.set("weather.units", if a.i == 1 { "imperial" } else { "metric" }));
    p.group("Weather")
        .footer(footer)
        .row(Row::new("Location").detail("Leave empty to detect it from your network.").field(cx.s("weather.location"), "City, e.g. Bengaluru", loc).width(220.0))
        .row(Row::new("Units").segmented(&["Celsius", "Fahrenheit"], if units == "imperial" { 1 } else { 0 }, up).width(200.0));

    // Hot corners
    let labels: Vec<String> = CORNER_ACTIONS.iter().map(|a| a.1.to_string()).collect();
    let mut rows = Vec::new();
    for (key, label) in [("topLeft", "Top left"), ("topRight", "Top right"), ("bottomLeft", "Bottom left"), ("bottomRight", "Bottom right")] {
        let path = format!("hotCorners.{key}");
        let cur = cx.s(&path);
        let i = CORNER_ACTIONS.iter().position(|a| a.0 == cur).map_or(-1, |i| i as i32);
        let values: Vec<&str> = CORNER_ACTIONS.iter().map(|a| a.0).collect();
        let pick = cx.pick(&path, &values);
        rows.push(Row::new(label).choice(&labels, i, pick).width(220.0));
    }
    let g = p.group("Hot corners");
    g.footer("Move the pointer into a corner and hold it there. Corners do nothing while a full-screen window is in front.");
    for r in rows {
        g.row(r);
    }

    // ── More options: the drop shelf ──
    p.more(cx);
    {
        let shake = cx.b("shelf.shake");
        let st = cx.toggle("shelf.shake");
        let edge = cx.b("shelf.edge");
        let et = cx.toggle("shelf.edge");
        let bar = cx.b("shelf.menubar");
        let bt = cx.toggle("shelf.menubar");
        let n = feed(cx)["shelfCount"].as_u64().unwrap_or(0) as usize;
        let opened = feed(cx)["shelfOpened"].as_bool().unwrap_or(false);
        let sh = cx.on(|cx, _| cx.ask("look", "shelf", Value::Null));
        p.group("Drop shelf")
            .footer("A place to park files while you get to where they're going: drop them on the shelf, then drag them out again, one at a time or all together. Super+Alt+D opens it too.")
            .row(Row::new("Shake while dragging").detail("Shake the pointer quickly from side to side with files held").switch(shake, st))
            .row(Row::new("Drag to the side of the screen").detail("Hold the files at the left or right edge for a moment").switch(edge, et))
            .row(Row::new("Menu bar icon").detail("While the shelf holds something, and while you drag: drop files on it").switch(bar, bt))
            .row(Row::new("Open the shelf")
                .detail(if n > 0 { format!("{} on it", count(n, "item", "items")) } else { "Empty".into() })
                .button(if opened { "Hide Shelf" } else { "Show Shelf" }, sh));
    }
    p.end_more();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_the_files_app_warns_about_screen_shares() {
        // (#331: the portal restarts at once)
        assert!(FILES_APP_DETAIL.contains("screen share") && FILES_APP_DETAIL.contains("at once"));
    }

    #[test]
    fn counts_and_corners() {
        assert_eq!(count(1, "widget", "widgets"), "1 widget");
        assert_eq!(count(4, "widget", "widgets"), "4 widgets");
        assert_eq!(CORNER_ACTIONS.len(), 9);
    }

    #[test]
    fn the_desktops_display_is_picked_by_friendly_name() {
        let screens = vec![("eDP-1".to_string(), "BOE 0x0BCA".to_string()), ("DP-1".to_string(), "DELL U2720Q".to_string())];
        let (values, labels, at) = screen_choices(&screens, "primary");
        assert_eq!(values, ["primary", "eDP-1", "DP-1"]);
        assert_eq!(labels, ["Main display", "BOE 0x0BCA  \u{b7}  eDP-1", "DELL U2720Q  \u{b7}  DP-1"]);
        assert_eq!(at, 0);
        // (unset reads as primary)
        assert_eq!(screen_choices(&screens, "").2, 0);
        assert_eq!(screen_choices(&screens, "DP-1").2, 2);
        // chosen and unplugged: still shown, said so
        let (values, labels, at) = screen_choices(&screens[..1], "DP-1");
        assert_eq!(values, ["primary", "eDP-1", "DP-1"]);
        assert_eq!(labels[2], "DP-1 (not connected)");
        assert_eq!(at, 2);
        // no model: the connector alone
        assert_eq!(screen_choices(&[("HDMI-A-1".to_string(), String::new())], "primary").1[1], "HDMI-A-1");
    }
}
