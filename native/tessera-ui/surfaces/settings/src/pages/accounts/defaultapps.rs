//! Default Apps (pages/defaultapps.qml): what the shortcuts and the shell
//! open. Each list offers the ones installed (Tools.has: the shell says,
//! feed.accounts.tools; else this process looks on the PATH).

use super::super::super::model::{Ctx, Page, Row};
use super::feed;
use serde_json::Value;

/// Whether a program is on the PATH
pub fn on_path(name: &str) -> bool {
    // (looked up once per program: the page is built again after every change)
    thread_local! {
        static FOUND: std::cell::RefCell<std::collections::HashMap<String, bool>> = Default::default();
    }
    FOUND.with(|m| *m.borrow_mut().entry(name.to_string()).or_insert_with(|| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))))
}

/// The choices a list offers: Automatic, then the installed ones (a
/// command's first and last words are programs: "kitty -e btop" needs both
/// kitty and btop)
pub fn choices(auto: &str, all: &[&str], has: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), auto.to_string())];
    for c in all {
        let first = c.split(' ').next().unwrap_or(c);
        let last = c.split(' ').next_back().unwrap_or(c);
        if has(first) && has(last) {
            out.push((c.to_string(), c.to_string()));
        }
    }
    out
}

fn row(cx: &mut Ctx, label: &str, keys: &str, key: &str, auto: &str, all: &[&str]) -> Row {
    let f = feed(cx);
    let told: Option<Vec<String>> = f["tools"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
    let list = choices(auto, all, |p| match &told {
        Some(t) => t.iter().any(|x| x == p),
        None => on_path(p),
    });
    let path = format!("defaultApps.{key}");
    let cur = cx.s(&path);
    let index = list.iter().position(|(v, _)| *v == cur).map_or(-1, |i| i as i32);
    let labels: Vec<String> = list.iter().map(|c| c.1.clone()).collect();
    let pick = cx.pick_values(&path, list.into_iter().map(|c| Value::from(c.0)).collect());
    // (a value no longer installed shows as itself, as Dropdown's label does)
    let mut r = Row::new(label).detail(keys).choice(&labels, index, pick).width(220.0);
    if index < 0 {
        r = r.placeholder(cur);
    }
    r
}

/// Tessera's quick-look browser (tessera-web): its history and website
/// data, cleared (tessera_core::settings::web_dirs; the bar's menu does the
/// same). Where links open and Wi-Fi sign-in pages: "Open links from Tessera
/// in" above and Settings → Network.
fn web(cx: &mut Ctx, p: &mut Page) {
    let clear = cx.on(|cx, _| {
        if !cx.confirmed("webClear", "Choose Clear again to remove the quick-look browser's history, cookies and website data") {
            return;
        }
        let all = tessera_core::settings::web_dirs().iter().all(|d| !d.exists() || std::fs::remove_dir_all(d).is_ok());
        cx.toast(if all { "History and website data cleared" } else { "Some website data couldn't be cleared" }, None);
    });
    let g = p.group("Quick-look browser");
    g.footer("Its history stays on this computer and is never sent anywhere.");
    g.row(Row::new("Clear history and website data").button("Clear", clear).danger());
}

/// "Open links from Tessera in" (`apps.webLinks`): Tessera's quick-look
/// browser, tessera-web (the default), or your browser (tessera_core::weblink)
pub fn web_links_index(v: &Value) -> i32 {
    if v.as_str() == Some("browser") {
        1
    } else {
        0
    }
}

fn web_links_row(cx: &mut Ctx) -> Row {
    let cur = web_links_index(&cx.get("apps.webLinks"));
    let pick = cx.pick("apps.webLinks", &["tessera", "browser"]);
    let labels = vec!["Tessera's quick-look browser".to_string(), "Your browser".to_string()];
    Row::new("Open links from Tessera in").detail("Links in notifications, AI answers, Search and menus").choice(&labels, cur, pick).width(220.0)
}

pub fn page(cx: &mut Ctx, p: &mut Page) {
    let rows = vec![
        row(cx, "Terminal", "Super+Return", "terminal", "Automatic", &["kitty", "foot", "alacritty", "wezterm", "ghostty", "konsole", "gnome-terminal", "kgx", "ptyxis", "xfce4-terminal", "xterm"]),
        row(cx, "Browser", "Super+W", "browser", "Automatic (system default)", &["zen-browser", "firefox", "google-chrome-stable", "chromium", "brave"]),
        web_links_row(cx),
        row(cx, "Code editor", "Super+C", "code", "Automatic", &["code", "codium", "zed", "cursor", "windsurf"]),
        row(cx, "Text editor", "Super+X", "text", "Automatic", &["gnome-text-editor", "kate", "mousepad"]),
    ];
    // (no heading: the page is Default Apps already)
    let g = p.group("");
    g.footer("Used by keyboard shortcuts and the shell. Automatic picks the first one installed. The Files app is set in Desktop & Widgets.");
    for r in rows {
        g.row(r);
    }
    web(cx, p);
    p.more(cx);
    let rows = vec![
        row(cx, "Volume mixer", "Ctrl+Super+V", "mixer", "Automatic", &["pavucontrol", "pavucontrol-qt"]),
        row(cx, "Task manager", "Ctrl+Shift+Escape", "tasks", "Automatic", &["gnome-system-monitor", "plasma-systemmonitor", "kitty -e btop", "kitty -e htop"]),
    ];
    let g = p.group("Default apps");
    for r in rows {
        g.row(r);
    }
    p.end_more();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_installed_ones_are_offered() {
        let c = choices("Automatic", &["kitty", "foot", "kitty -e btop"], |p| p == "kitty" || p == "btop");
        assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["", "kitty", "kitty -e btop"]);
        // (btop without kitty: not offered)
        let c = choices("Automatic", &["foot", "kitty -e btop"], |p| p == "foot" || p == "btop");
        assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["", "foot"]);
        assert_eq!(c[0].1, "Automatic");
    }

    #[test]
    fn links_open_in_tessera_unless_you_chose_your_browser() {
        assert_eq!(web_links_index(&Value::Null), 0);
        assert_eq!(web_links_index(&Value::from("tessera")), 0);
        assert_eq!(web_links_index(&Value::from("browser")), 1);
    }
}
