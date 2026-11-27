//! System health (pages/about-health.qml), now the services part of the
//! one Health page (system/health.rs; the old id about/health goes there):
//! each service Tessera uses, checked by the shell when the page opens
//! (services/Health.qml; the bridge asks it), with how to fix what's
//! broken. A fix that runs as administrator is confirmed first: the row
//! shows exactly what will run, and only the second press runs it (the
//! shell runs it, never this).

use super::super::super::model::{btn, Ctx, Opt, Page, Row};
use super::{b, feed, s, strings};
use serde_json::{json, Value};

/// A status as a word (never colour alone)
pub fn word(status: &str) -> &str {
    match status {
        "ok" => "OK",
        "warn" => "Check",
        "fail" => "Not working",
        "off" => "Off",
        s => s,
    }
}

/// A status's colour for the dot: 0 success, 1 warning, 2 danger, 3 tertiary
pub fn tone(status: &str) -> i32 {
    match status {
        "ok" => 0,
        "warn" => 1,
        "fail" => 2,
        _ => 3,
    }
}

fn cap(t: &str) -> String {
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The services' line: (label, detail, any failing). `checks` as the feed
/// has them, `checking` while the shell asks them
pub fn summary(checks: &[Value], checking: bool) -> (String, String, bool) {
    let attention: Vec<&Value> = checks.iter().filter(|c| c["status"] == "fail" || c["status"] == "warn").collect();
    let label = if checking && checks.is_empty() {
        "Checking…".to_string()
    } else if attention.is_empty() {
        "All good".into()
    } else if attention.len() == 1 {
        "1 thing needs attention".into()
    } else {
        format!("{} things need attention", attention.len())
    };
    let detail = if checks.is_empty() {
        String::new()
    } else if attention.is_empty() {
        "Every service Tessera uses is answering.".into()
    } else {
        attention.iter().map(|c| s(&c["name"])).collect::<Vec<_>>().join(", ")
    };
    (label, detail, attention.iter().any(|c| c["status"] == "fail"))
}

/// The services part of Settings → Health (it was About → System health,
/// whose id now goes there: map::RUST_ALIASES): their line with Copy
/// report and Check again, what else is running, each group's checks with
/// their fixes
pub fn services(cx: &mut Ctx, p: &mut Page) {
    let f = feed(cx);
    let h = &f["health"];
    let checks: Vec<Value> = h["checks"].as_array().cloned().unwrap_or_default();
    let checking = b(&h["checking"]);
    let err = s(&h["error"]);
    let at = s(&h["checkedAt"]);
    let (label, detail, failing) = summary(&checks, checking);
    let top = cx.on(|cx, a| cx.ask("about", if a.i == 0 { "health.copy" } else { "health.check" }, Value::Null));
    p.group("Services")
        .footer(if !err.is_empty() {
            err
        } else if !at.is_empty() {
            format!("Checked at {at}. Nothing is checked in the background; this page asks each service when it opens.")
        } else {
            String::new()
        })
        .row(Row::new(label).tone(if failing { 2 } else { 0 }).detail(detail).buttons(
            vec![btn("Copy report").icon("copy").off(checks.is_empty()), btn(if checking { "Checking…" } else { "Check again" }).icon("arrow-clockwise").on(checking)],
            top,
        ));

    // Things also running that do Tessera's job twice (services/Conflicts.qml)
    let found: Vec<Value> = f["conflicts"].as_array().cloned().unwrap_or_default();
    let mut rows = Vec::new();
    for c in &found {
        let muted = b(&c["muted"]);
        let id = s(&c["id"]);
        let t = cx.on(move |cx, _| cx.ask("about", if muted { "conflicts.unmute" } else { "conflicts.mute" }, json!([id])));
        rows.push(Row::new(cap(&s(&c["name"]))).detail(format!("{}.\nTo stop it: {}", cap(&s(&c["what"])), s(&c["fix"]))).button(if muted { "Show alerts" } else { "Don't show again" }, t));
    }
    let g = p.group("Also running");
    g.footer("These do jobs Tessera already does, so things can happen twice or not show. Stop the ones you don't need; Tessera doesn't stop them for you.");
    for r in rows {
        g.row(r);
    }

    // Each group's checks
    for group in strings(&h["groups"]) {
        let mut rows = Vec::new();
        for c in checks.iter().filter(|c| s(&c["group"]) == group) {
            rows.push(check_row(cx, c, "health"));
        }
        let g = p.group(group);
        for r in rows {
            g.row(r);
        }
    }
}

/// The pages a remedy can open that aren't a sidebar page of their own:
/// their names in "Open …"
const SUB_PAGES: &[(&str, &str)] = &[("appearance/glass", "Glass"), ("power/cooldown", "Cool-down"), ("about/bugreport", "Report a bug"), ("notifications/island", "Island")];

/// A page's name for its "Open …" button (None: no such page)
pub fn page_name(page: &str) -> Option<&'static str> {
    if let Some((_, n)) = SUB_PAGES.iter().find(|(id, _)| *id == page) {
        return Some(n);
    }
    super::super::super::map::section(page).map(|s| s.name)
}

/// What to do by hand, numbered ("1. …\n2. …")
pub fn numbered(steps: &[String]) -> String {
    steps.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect::<Vec<_>>().join("\n")
}

/// The remedies a check offers on page `here`: the steps (numbered, or the
/// old one-line "To fix"), and the page to open (none on that page itself)
pub fn remedy(c: &Value, here: &str) -> (String, Option<&'static str>) {
    let status = s(&c["status"]);
    if status != "fail" && status != "warn" {
        return (String::new(), None);
    }
    let steps = strings(&c["steps"]);
    let fix = s(&c["fix"]);
    let page = s(&c["page"]);
    let text = if !steps.is_empty() {
        numbered(&steps)
    } else if !fix.is_empty() {
        format!("To fix: {fix}")
    } else {
        String::new()
    };
    let open = if page.is_empty() || page == here || here.starts_with(&format!("{page}/")) { None } else { page_name(&page) };
    (text, open)
}

/// A check's row: its status, what's wrong, and its remedies: Fix (a root
/// fix is confirmed first: the row shows exactly what will run), Open <the
/// page where its setting lives> (not on that page: `here`), and the steps
/// by hand
/// The fix button's words: the check's own (`fixLabel`, "Restore the login
/// screen") or Fix; while confirming, what pressing it again does; while
/// it runs, Fixing…
fn fix_label(own: &str, running: bool, asking: bool, password: bool) -> String {
    if running {
        "Fixing…".into()
    } else if asking {
        (if password { "Run with password" } else { "Run fix" }).to_string()
    } else if own.is_empty() {
        "Fix".into()
    } else {
        own.into()
    }
}

pub fn check_row(cx: &mut Ctx, c: &Value, here: &str) -> Row {
    let confirming = cx.local_s("about.health.confirming");
    let fixing = s(&feed(cx)["health"]["fixing"]);
    let id = s(&c["id"]);
    let status = s(&c["status"]);
    let asking = confirming == id;
    let pw = b(&c["fixPassword"]);
    let (how, open) = remedy(c, here);
    let detail = format!(
        "{}{}",
        s(&c["detail"]),
        if asking {
            format!("{}{}", if pw { "\nRuns as administrator, after your password: " } else { "\nRuns as administrator: " }, s(&c["fixCommand"]))
        } else if !how.is_empty() {
            format!("\n{how}")
        } else {
            String::new()
        }
    );
    let mut opts: Vec<Opt> = Vec::new();
    let mut roles: Vec<&'static str> = Vec::new();
    if asking {
        opts.push(btn("Cancel"));
        roles.push("cancel");
    }
    if let Some(name) = open.filter(|_| !asking) {
        opts.push(btn(format!("Open {name}")));
        roles.push("open");
    }
    if b(&c["canFix"]) {
        let label = fix_label(&s(&c["fixLabel"]), fixing == id, asking, pw);
        let mut o = btn(label).on(fixing == id).off(!fixing.is_empty());
        if asking {
            o = o.primary();
        }
        opts.push(o);
        roles.push("fix");
    }
    let root = b(&c["fixRoot"]);
    let (page, target) = (s(&c["page"]), s(&c["target"]));
    let idc = id.clone();
    let act = cx.on(move |cx, a| match roles.get(a.i as usize) {
        Some(&"cancel") => cx.local_set("about.health.confirming", ""),
        Some(&"open") => cx.go(&page, &target),
        Some(&"fix") => {
            if root && cx.local_s("about.health.confirming") != idc {
                cx.local_set("about.health.confirming", idc.clone());
                return;
            }
            cx.local_set("about.health.confirming", "");
            cx.ask("about", "health.fix", json!([idc]));
        }
        _ => {}
    });
    let mut r = Row::new(s(&c["name"])).detail(detail).ctl("about", "health", act);
    r.opts = opts;
    r.value = word(&status).to_string();
    r.index = tone(&status);
    r
}

/// The checks `ids` that are failing or need a look, from the same health
/// check Health shows (the bridge asks it again when a page showing them
/// opens and the last look is old)
pub fn broken(cx: &Ctx, ids: &[&str]) -> Vec<Value> {
    let checks = feed(cx)["health"]["checks"].as_array().cloned().unwrap_or_default();
    checks.into_iter().filter(|c| ids.contains(&c["id"].as_str().unwrap_or("")) && (c["status"] == "fail" || c["status"] == "warn")).collect()
}

/// A feature page's part of Health: its checks `ids` while one is failing
/// or needs a look, each with the same remedies as on Health; nothing while
/// all is well. `here` is the page (it isn't offered as a link to itself)
pub fn problems(cx: &mut Ctx, p: &mut Page, ids: &[&str], here: &str) {
    let bad = broken(cx, ids);
    if bad.is_empty() {
        return;
    }
    let rows: Vec<Row> = bad.iter().map(|c| check_row(cx, c, here)).collect();
    let g = p.group("");
    g.footer("From Health, which asks each service Tessera uses.");
    for r in rows {
        g.row(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_in_words_and_colours() {
        assert_eq!((word("fail"), tone("fail")), ("Not working", 2));
        assert_eq!((word("ok"), tone("ok")), ("OK", 0));
        assert_eq!((word("off"), tone("off")), ("Off", 3));
        assert_eq!(cap("pipewire-pulse"), "Pipewire-pulse");
    }

    /// The page ids health-check.py names (page="x", page="x" if … else "y")
    fn script_pages() -> Vec<String> {
        let py = include_str!("../../../../../../../shell/assets/scripts/health-check.py");
        let mut out = Vec::new();
        for part in py.split("page=\"").skip(1) {
            let Some((id, rest)) = part.split_once('"') else { continue };
            if !id.is_empty() {
                out.push(id.to_string());
            }
            if rest.starts_with(" if ") {
                if let Some(other) = rest.split_once("else \"").and_then(|(_, r)| r.split_once('"')) {
                    out.push(other.0.to_string());
                }
            }
        }
        out
    }

    #[test]
    fn every_page_a_remedy_opens_is_a_page_with_a_name() {
        let pages = script_pages();
        assert!(pages.len() >= 15, "{pages:?}");
        for id in &pages {
            assert!(page_name(id).is_some(), "health-check.py names {id}, which has no name here");
            // (a section, or one of the sub-pages a group builds)
            assert!(crate::map::section(id).is_some() || SUB_PAGES.iter().any(|(s, _)| s == id), "{id}");
        }
        assert_eq!(page_name("appearance/glass"), Some("Glass"));
        assert_eq!(page_name("sound"), Some("Sound"));
        assert_eq!(page_name("nowhere"), None);
    }

    #[test]
    fn a_fix_can_say_what_it_does() {
        assert_eq!(fix_label("", false, false, false), "Fix");
        assert_eq!(fix_label("Restore the login screen", false, false, true), "Restore the login screen");
        // (confirming first: the password is asked; running: Fixing…)
        assert_eq!(fix_label("Restore the login screen", false, true, true), "Run with password");
        assert_eq!(fix_label("", false, true, false), "Run fix");
        assert_eq!(fix_label("Restore the login screen", true, false, true), "Fixing…");
        // (health-check.py names the button for the login rescue's Restore, audit #805)
        let src = include_str!("../../../../../../../shell/assets/scripts/health-check.py");
        // (and the approved-build fix's, #802)
        let labels = src.lines().find(|l| l.starts_with("FIX_LABELS = {")).expect("FIX_LABELS");
        assert!(labels.contains("\"loginrestore\": \"Restore the login screen\""), "{labels}");
        assert!(labels.contains("\"loginui\": \"Use it at the login screen\""), "{labels}");
    }

    #[test]
    fn remedies_are_steps_and_a_page() {
        let c = json!({"status": "warn", "fix": "x", "steps": ["Log out and in again", "Then check"], "page": "appearance/glass"});
        assert_eq!(remedy(&c, "health"), ("1. Log out and in again\n2. Then check".into(), Some("Glass")));
        // (on that page itself: no link to it)
        assert_eq!(remedy(&c, "appearance/glass").1, None);
        assert_eq!(remedy(&json!({"status": "fail", "fix": "Restart it", "page": "sound"}), "sound/x").1, None);
        // (no steps: the one line)
        assert_eq!(remedy(&json!({"status": "fail", "fix": "Restart it"}), "health"), ("To fix: Restart it".into(), None));
        // (working: nothing)
        assert_eq!(remedy(&json!({"status": "ok", "fix": "x", "page": "sound", "steps": ["a"]}), "health"), (String::new(), None));
    }

    #[test]
    fn the_services_line() {
        assert_eq!(summary(&[], true).0, "Checking…");
        let ok = vec![json!({"name": "Sound", "status": "ok"}), json!({"name": "Wi-Fi", "status": "off"})];
        assert_eq!(summary(&ok, false), ("All good".into(), "Every service Tessera uses is answering.".into(), false));
        let bad = vec![json!({"name": "Sound", "status": "fail"}), json!({"name": "Bluetooth", "status": "warn"}), json!({"name": "Wi-Fi", "status": "ok"})];
        assert_eq!(summary(&bad, false), ("2 things need attention".into(), "Sound, Bluetooth".into(), true));
        let warn = vec![json!({"name": "Bluetooth", "status": "warn"})];
        assert_eq!(summary(&warn, false), ("1 thing needs attention".into(), "Bluetooth".into(), false));
    }
}
