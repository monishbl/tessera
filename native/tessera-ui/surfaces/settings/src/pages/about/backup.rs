//! Back up and restore (Rust only; About links here): your setup in one
//! file. shell/assets/scripts/backup.py does the work, run off the UI
//! thread (its children hear signals: util::child_signals in pre_exec).
//!
//! Back up: what to include, each part with its count and size (Tessera
//! settings, Hyprland edits, wallpapers, the login screen's look, the apps
//! you installed with where they came from), then a folder from the file
//! chooser. Nothing secret goes in (backup.py's `never`).
//! Restore: a backup from the file chooser, what's in it with switches,
//! what would change (restore-plan, as import's "what will change"), Apply
//! with Undo (the copy of what it replaced). A settings file of the older
//! kind (.tessera) restores as About's did (settings-backup.py).
//! Apps: grouped by where they came from, those not here picked; Arch's and
//! the AUR's go to Software Update's job (one password, the AUR helper:
//! about.backup.packages), the rest are installed as you, one by one, with
//! each one's result. A manager that's missing ("needs fnm") can be
//! installed first, with the system's packages.

use super::super::super::model::{btn, Action, Ctx, Page, Row};
use super::about::{answers as legacy_answers, config_dir, describe, helper as legacy_helper};
use super::{feed, s, tilde};
use serde_json::{json, Value};

fn script() -> String {
    crate::util::asset("shell/assets/scripts/backup.py").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// backup.py, as you, with signals it can hear (blocking: a thread's)
fn command(args: &[String]) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new("python3");
    c.arg("-I").arg(script()).args(args).arg("--config").arg(config_dir()).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    unsafe {
        c.pre_exec(|| {
            tessera_core::util::child_signals();
            Ok(())
        });
    }
    c
}

const NO_ANSWER: &str = "The backup helper didn't answer. Check that Python 3 is installed, then try again.";

/// The helper's last line, its JSON answer
pub fn answer_of(out: &str) -> Value {
    out.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v.get("ok").is_some())).unwrap_or_else(|| json!({"ok": false, "error": NO_ANSWER}))
}

fn run(args: Vec<String>) -> Value {
    match command(&args).output() {
        Ok(o) => answer_of(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => json!({"ok": false, "error": NO_ANSWER}),
    }
}

/// The same, telling each `{"progress": …}` line as it comes
fn run_telling(args: Vec<String>, say: &dyn Fn(Value)) -> Value {
    use std::io::BufRead;
    let Ok(mut child) = command(&args).stdout(std::process::Stdio::piped()).spawn() else { return json!({"ok": false, "error": NO_ANSWER}) };
    let mut last = String::new();
    if let Some(out) = child.stdout.take() {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            match serde_json::from_str::<Value>(&line) {
                Ok(v) if v.get("progress").is_some() => say(v["progress"].clone()),
                _ => last = line,
            }
        }
    }
    let _ = child.wait();
    answer_of(&last)
}

/// The parts, in order: (id, name)
pub const PARTS: &[(&str, &str)] = &[("settings", "Tessera settings"), ("hypr", "Hyprland edits"), ("wallpapers", "Wallpapers"), ("login", "Login screen look"), ("apps", "Apps I installed")];

/// Sources in the order backup.py lists them, with their names
pub const SOURCES: &[(&str, &str)] = &[
    ("pacman", "Arch repositories"),
    ("aur", "AUR"),
    ("flatpak", "Flatpak"),
    ("fnm", "fnm (Node.js versions)"),
    ("nvm", "nvm (Node.js versions)"),
    ("rustup", "rustup (Rust toolchains)"),
    ("cargo", "cargo install"),
    ("pipx", "pipx"),
    ("uv", "uv tools"),
    ("npm", "npm (global)"),
    ("bun", "bun (global)"),
    ("deno", "deno"),
    ("appimage", "AppImages"),
    ("local", "~/.local/bin"),
];

/// A pacman or AUR package's name as the root helper takes one
/// (^[a-z0-9@._+-]+$, never an option or a dot first)
pub fn is_package(n: &str) -> bool {
    !n.is_empty() && !n.starts_with(['-', '.']) && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@._+-".contains(c))
}

/// A settings file of the older kind (About's, settings-backup.py)
pub fn is_legacy(path: &str) -> bool {
    path.ends_with(".tessera") || path.ends_with(&format!(".{}", tessera_core::old_names::BACKUP_EXT))
}

/// A part's switch: chosen, or (untouched) on when it holds something
fn part_on(cx: &Ctx, key: &str, count: i64) -> bool {
    cx.local.get(key).and_then(Value::as_bool).unwrap_or(count > 0)
}

/// "settings,hypr": the parts switched on
pub fn chosen(on: &[(String, bool)]) -> String {
    on.iter().filter(|(_, b)| *b).map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(",")
}

/// What restoring would change, in lines: (title, detail)
pub fn plan_lines(plan: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let parts = &plan["parts"];
    let st = &parts["settings"];
    if st.is_object() {
        let n = st["count"].as_u64().unwrap_or(0);
        let changes = st["changes"].as_array().cloned().unwrap_or_default();
        if n == 0 && st["shortcuts"] != true && st["focus"] != true {
            out.push(("Settings".into(), "The same as now".into()));
        }
        for c in changes.iter().take(12) {
            out.push((s(&c["key"]), format!("{} → {}", s(&c["now"]), s(&c["then"]))));
        }
        if n > 12 {
            out.push((format!("{} more settings", n - 12), "Change too".into()));
        }
        if st["shortcuts"] == true {
            out.push(("Shortcuts and gestures".into(), "The backup's".into()));
        }
        if st["focus"] == true {
            out.push(("Focus modes".into(), "The backup's".into()));
        }
    }
    for f in parts["hypr"]["files"].as_array().cloned().unwrap_or_default() {
        let what = match f["action"].as_str().unwrap_or("") {
            "add" => "Added",
            "replace" => "Replaced (yours is kept for Undo)",
            "beside" => "Put beside Tessera's file, which stays",
            _ => continue,
        };
        out.push((s(&f["path"]), what.into()));
    }
    let wp = &parts["wallpapers"];
    if wp.is_object() {
        let n = wp["count"].as_u64().unwrap_or(0);
        out.push(("Wallpapers".into(), if n == 0 { "Already here".into() } else { format!("{n} {} into ~/Pictures/Wallpapers/Restored", if n == 1 { "picture goes" } else { "pictures go" }) }));
    }
    for c in parts["login"]["changes"].as_array().cloned().unwrap_or_default() {
        out.push((s(&c["key"]), format!("{} → {}", s(&c["now"]), s(&c["then"]))));
    }
    for k in plan["skipped"].as_array().cloned().unwrap_or_default() {
        out.push(("Left out".into(), s(&k)));
    }
    out
}

/// The apps, by where they came from, in SOURCES' order
pub fn by_source(apps: &[Value]) -> Vec<(&'static str, &'static str, Vec<Value>)> {
    SOURCES
        .iter()
        .filter_map(|(id, name)| {
            let list: Vec<Value> = apps.iter().filter(|a| a["group"] == *id).cloned().collect();
            (!list.is_empty()).then_some((*id, *name, list))
        })
        .collect()
}

/// An app is picked unless it's here already, can't be installed again,
/// or its manager is missing (then it waits for the manager)
pub fn default_pick(a: &Value) -> bool {
    a["installed"] != true && a["installable"] != false && s(&a["needs"]).is_empty()
}

/// Can it be picked at all
pub fn pickable(a: &Value) -> bool {
    a["installed"] != true && a["installable"] != false
}

/// The packages Software Update installs: (Arch's, the AUR's), each name
/// checked, the managers asked for first among them ("aur:nvm" → the AUR)
pub fn system_packages(picked: &[Value], managers: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut repo, mut aur) = (Vec::new(), Vec::new());
    let add = |list: &mut Vec<String>, n: &str| {
        if is_package(n) && !list.iter().any(|x| x == n) {
            list.push(n.to_string());
        }
    };
    for m in managers {
        match m.strip_prefix("aur:") {
            Some(n) => add(&mut aur, n),
            None => add(&mut repo, m),
        }
    }
    for a in picked {
        match a["group"].as_str() {
            Some("pacman") => add(&mut repo, a["name"].as_str().unwrap_or("")),
            Some("aur") => add(&mut aur, a["name"].as_str().unwrap_or("")),
            _ => {}
        }
    }
    (repo, aur)
}

/// An app's result in a word
pub fn result_word(r: &Value) -> String {
    match r["state"].as_str().unwrap_or("") {
        "ok" => "Installed".into(),
        "running" => "Installing…".into(),
        "failed" => "Didn't install".into(),
        "skipped" => {
            let why = s(&r["why"]);
            if why.is_empty() { "Skipped".into() } else { format!("Skipped: {why}") }
        }
        "system" => "With Software Update".into(),
        _ => String::new(),
    }
}

fn kib(n: u64) -> String {
    if n < 1024 * 1024 {
        format!("{} KB", (n / 1024).max(1))
    } else {
        format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
    }
}

/// The answers that came in since the page was last built
fn take_answers(cx: &mut Ctx) {
    if let Some(out) = cx.local.remove("about.bk.created") {
        let ok = out["ok"] == true;
        cx.local_set("about.bk.status", if ok { String::new() } else { s(&out["error"]) });
        cx.local_set("about.bk.savedTo", if ok { s(&out["file"]) } else { String::new() });
        if ok {
            cx.toast(format!("Backed up ({})", kib(out["bytes"].as_u64().unwrap_or(0))), None);
        }
    }
    if let Some(out) = cx.local.remove("about.bk.inspected") {
        if out["ok"] == true {
            cx.local_set("about.bk.info", out);
            cx.local_set("about.bk.rstatus", "");
        } else {
            cx.local_set("about.bk.rstatus", s(&out["error"]));
            cx.local_set("about.bk.file", "");
        }
    }
    if let Some(out) = cx.local.remove("about.bk.restored") {
        if out["ok"] != true {
            cx.local_set("about.bk.rstatus", s(&out["error"]));
        } else {
            let skipped = super::strings(&out["skipped"]);
            cx.local_set("about.bk.rstatus", if skipped.is_empty() { String::new() } else { format!("Left out {}.", skipped.join(" and ")) });
            cx.local.remove("about.bk.plan");
            cx.ask("about", "focusReload", Value::Null);
            let undo_file = s(&out["undo"]);
            cx.local_set("about.bk.undo", undo_file.clone());
            let undo: Action = super::action(move |cx, _| {
                let f = undo_file.clone();
                cx.spawn("about.bk.undone", move || run(vec!["undo".into(), f]));
            });
            cx.toast("Restored", if s(&out["undo"]).is_empty() { None } else { Some(undo) });
            super::about::follow(cx, &s(&out["look"]));
        }
    }
    if let Some(back) = cx.local.remove("about.bk.undone") {
        let ok = back["ok"] == true;
        cx.local_set("about.bk.rstatus", if ok { String::new() } else { s(&back["error"]) });
        if ok {
            cx.local_set("about.bk.undo", "");
            cx.local.remove("about.bk.plan");
            cx.ask("about", "focusReload", Value::Null);
            cx.toast("What was there before is back", None);
            super::about::follow(cx, &s(&back["look"]));
        }
    }
    if let Some(out) = cx.local.remove("about.bk.status.answer") {
        if out["ok"] == true {
            let mut info = cx.local.get("about.bk.info").cloned().unwrap_or(Value::Null);
            if info.is_object() {
                info["apps"] = out["apps"].clone();
                cx.local_set("about.bk.info", info);
            }
        }
    }
    if let Some(out) = cx.local.remove("about.bk.installed") {
        cx.local.remove("about.bk.progress");
        if out["ok"] == true {
            let mut all = cx.local.get("about.bk.results").cloned().unwrap_or(json!({}));
            if let Some(m) = out["results"].as_object() {
                for (k, v) in m {
                    all[k] = v.clone();
                }
            }
            cx.local_set("about.bk.results", all);
            let (n, bad) = (out["installed"].as_u64().unwrap_or(0), out["failed"].as_u64().unwrap_or(0));
            cx.toast(if bad == 0 { format!("{n} installed") } else { format!("{n} installed, {bad} didn't") }, None);
        } else {
            cx.local_set("about.bk.astatus", s(&out["error"]));
        }
        refresh_status(cx);
    }
    // (one app's step: its row says it)
    if let Some(p) = cx.local.get("about.bk.progress").cloned() {
        let id = s(&p["id"]);
        if !id.is_empty() {
            let mut all = cx.local.get("about.bk.results").cloned().unwrap_or(json!({}));
            if all[&id] != p {
                all[&id] = p.clone();
                cx.local_set("about.bk.results", all);
            }
        }
    }
}

/// Which of the backup's apps are here now (after an install)
fn refresh_status(cx: &mut Ctx) {
    let file = cx.local_s("about.bk.file");
    if !file.is_empty() {
        cx.spawn("about.bk.status.answer", move || run(vec!["apps-status".into(), file]));
    }
}

pub fn page(cx: &mut Ctx, p: &mut Page) {
    p.title = "Back up and restore".into();
    take_answers(cx);
    legacy_answers(cx);
    back_up(cx, p);
    restore(cx, p);
    apps(cx, p);
}

// ── Back up ─────────────────────────────────────────────────────────────

fn back_up(cx: &mut Ctx, p: &mut Page) {
    let inv = cx.once("about.bk.inventory", || run(vec!["inventory".into(), "--json".into()]));
    let busy = cx.local_b("about.bk.created.busy");
    let status = cx.local_s("about.bk.status");
    let saved = cx.local_s("about.bk.savedTo");
    let footer = if !status.is_empty() {
        status
    } else {
        "One file you keep. Nothing secret goes in it: no passwords, keyrings, ssh or gpg keys, tokens, browser profiles or documents.".to_string()
    };
    if inv.is_null() {
        p.group("Back up").footer(footer).row(Row::new("Looking at what there is to back up…").spinner());
        return;
    }
    if inv["ok"] != true {
        p.group("Back up").footer(footer).row(Row::new("Couldn't look at what there is").detail(s(&inv["error"])).tone(2));
        return;
    }
    let parts = inv["parts"].as_array().cloned().unwrap_or_default();
    let mut on: Vec<(String, bool)> = Vec::new();
    let mut rows = Vec::new();
    for (id, name) in PARTS {
        let Some(part) = parts.iter().find(|x| x["id"] == *id) else { continue };
        let count = part["count"].as_i64().unwrap_or(0);
        let key = format!("about.bk.inc.{id}");
        let v = part_on(cx, &key, count);
        on.push((id.to_string(), v));
        let k = key.clone();
        let act = cx.on(move |cx, _| {
            let now = cx.local.get(&k).and_then(Value::as_bool).unwrap_or(count > 0);
            cx.local_set(&k, !now);
        });
        rows.push(Row::new(*name).detail(s(&part["summary"])).switch(v, act));
    }
    let include = chosen(&on);
    let go = cx.on(move |cx, a| {
        if a.i == 1 {
            let (dir, inc) = (a.s.clone(), include.clone());
            cx.local_set("about.bk.status", "Backing up…");
            cx.local_set("about.bk.savedTo", "");
            cx.spawn("about.bk.created", move || run(vec!["create".into(), dir, "--include".into(), inc]));
        } else {
            cx.choose_file(&a.id, true, "Choose where to save the backup", "~/Documents", "");
        }
    });
    let show = {
        let folder = saved.rsplit_once('/').map_or("/".to_string(), |x| if x.0.is_empty() { "/".to_string() } else { x.0.to_string() });
        cx.on(move |cx, _| cx.ask("about", "openFile", json!([folder])))
    };
    let nothing = on.iter().all(|(_, b)| !b);
    let g = p.group("Back up");
    g.footer(footer);
    for r in rows {
        g.row(r);
    }
    g.row(Row::new("Back up").detail(format!("Saves “Tessera backup {}” in a folder you choose", crate::util::now("%Y-%m-%d"))).buttons(vec![btn(if busy { "Backing Up…" } else { "Back Up…" }).primary().on(busy).off(busy || nothing)], go));
    g.row(Row::new("Saved").visible(!saved.is_empty()).detail(tilde(&saved)).button("Show in Files", show));
}

// ── Restore ─────────────────────────────────────────────────────────────

fn restore(cx: &mut Ctx, p: &mut Page) {
    let file = cx.local_s("about.bk.file");
    let info = cx.local.get("about.bk.info").cloned().unwrap_or(Value::Null);
    let reading = cx.local_b("about.bk.inspected.busy");
    let restoring = cx.local_b("about.bk.restored.busy") || cx.local_b("about.bk.undone.busy");
    let status = cx.local_s("about.bk.rstatus");
    let undo_file = cx.local_s("about.bk.undo");
    let open = cx.on(|cx, a| {
        if a.i == 1 {
            let f = a.s.clone();
            cx.local.remove("about.bk.info");
            cx.local.remove("about.bk.plan");
            cx.local.remove("about.bk.results");
            cx.local_set("about.bk.file", f.clone());
            cx.local_set("about.backup.pending", Value::Null);
            if is_legacy(&f) {
                // (a settings file of the older kind: About's restore)
                cx.local_set("about.backup.choosing", f.clone());
                cx.local_set("about.backup.status", "Reading…");
                cx.spawn("about.backup.inspect", move || legacy_helper(vec!["inspect".into(), f]));
            } else {
                cx.local_set("about.bk.rstatus", "Reading…");
                cx.spawn("about.bk.inspected", move || run(vec!["inspect".into(), f]));
            }
        } else {
            cx.choose_file(&a.id, false, "Choose a Tessera backup", "~/Documents", &format!("Tessera backups|*.tar.zst *.tar.gz *.tessera *.{}", tessera_core::old_names::BACKUP_EXT));
        }
    });
    let legacy_status = cx.local_s("about.backup.status");
    let footer = if !status.is_empty() {
        status
    } else if !legacy_status.is_empty() && is_legacy(&file) {
        legacy_status
    } else {
        "Your passwords, accounts and this computer's displays stay as they are. Shows what will change first, and Undo puts back what was there.".into()
    };
    let g = p.group("Restore");
    g.footer(footer);
    g.row(Row::new("Restore from a backup").detail("A backup made here, or a settings file (.tessera)").buttons(vec![btn(if reading { "Reading…" } else { "Choose…" }).on(reading).off(reading || restoring)], open));
    if !undo_file.is_empty() {
        let f = undo_file.clone();
        let undo = cx.on(move |cx, _| {
            let f = f.clone();
            cx.spawn("about.bk.undone", move || run(vec!["undo".into(), f]));
        });
        g.row(Row::new("Undo the restore").detail(format!("What was there before is kept in {}", tilde(&undo_file))).buttons(vec![btn("Undo").icon("arrow-counter-clockwise").off(restoring)], undo));
    }
    // (an older settings file: as About restored one)
    let pending = cx.local.get("about.backup.pending").cloned().unwrap_or(Value::Null);
    if !pending.is_null() {
        let lf = s(&pending["file"]);
        let decide = cx.on(move |cx, a| {
            cx.local_set("about.backup.pending", Value::Null);
            if a.i == 1 {
                let f = lf.clone();
                cx.local_set("about.backup.status", "Restoring…");
                cx.spawn("about.backup.restore", move || legacy_helper(vec!["restore".into(), f, "--config".into(), config_dir()]));
            }
        });
        let busy = cx.local_b("about.backup.restore.busy");
        p.group("").row(Row::new("Restore these settings?").detail(describe(&pending["info"])).buttons(vec![btn("Cancel"), btn("Restore Settings").primary().off(busy)], decide));
    }
    if !info.is_object() || file.is_empty() || is_legacy(&file) {
        return;
    }

    // What's in it, each part switched on or off
    let parts: Vec<Value> = info["parts"].as_array().cloned().unwrap_or_default().into_iter().filter(|x| x["id"] != "apps").collect();
    let mut on: Vec<(String, bool)> = Vec::new();
    let mut rows = Vec::new();
    for part in &parts {
        let id = s(&part["id"]);
        let key = format!("about.bk.rinc.{id}");
        let v = cx.local.get(&key).and_then(Value::as_bool).unwrap_or(true);
        on.push((id.clone(), v));
        let k = key.clone();
        let act = cx.on(move |cx, _| {
            let now = cx.local.get(&k).and_then(Value::as_bool).unwrap_or(true);
            cx.local_set(&k, !now);
        });
        rows.push(Row::new(s(&part["name"])).detail(s(&part["summary"])).switch(v, act));
    }
    let include = chosen(&on);
    let created = super::about::created(&s(&info["created"]));
    let g = p.group(if created.is_empty() { "In this backup".to_string() } else { format!("In this backup, from {created}") });
    g.footer(tilde(&file));
    if rows.is_empty() {
        g.row(Row::text("Only apps: they're below."));
    }
    for r in rows {
        g.row(r);
    }
    if include.is_empty() {
        return;
    }

    // What would change (restore-plan for the parts switched on)
    let plan = cx.local.get("about.bk.plan").cloned().unwrap_or(Value::Null);
    // (the plan for this backup and these parts: one for a backup chosen
    // before is asked for again)
    if plan["for"].as_str() != Some(include.as_str()) || plan["file"].as_str() != Some(file.as_str()) || plan.is_null() {
        if !cx.local_b("about.bk.plan.busy") {
            let (f, inc) = (file.clone(), include.clone());
            cx.spawn("about.bk.plan", move || {
                let mut v = run(vec!["restore-plan".into(), f.clone(), "--include".into(), inc.clone()]);
                v["for"] = json!(inc);
                v["file"] = json!(f);
                v
            });
        }
        p.group("What will change").row(Row::new("Looking at what would change…").spinner());
        return;
    }
    let g = p.group("What will change");
    if plan["ok"] != true {
        g.row(Row::new("Couldn't read the backup").detail(s(&plan["error"])).tone(2));
        return;
    }
    let lines = plan_lines(&plan);
    let nothing = plan["writes"].as_u64() == Some(0);
    g.footer(if nothing { "Restoring these changes nothing: they're the same as now.".to_string() } else { "Applying keeps a copy of what it replaces, for Undo.".into() });
    for (title, detail) in lines {
        g.row(Row::new(title).detail(detail));
    }
    let (f, inc) = (file.clone(), include.clone());
    let apply = cx.on(move |cx, a| {
        if a.i == 0 {
            cx.local_set("about.bk.file", "");
            cx.local.remove("about.bk.info");
            cx.local.remove("about.bk.plan");
            return;
        }
        let (f, inc) = (f.clone(), inc.clone());
        cx.local_set("about.bk.rstatus", "Restoring…");
        cx.spawn("about.bk.restored", move || run(vec!["restore".into(), f, "--include".into(), inc]));
    });
    g.row(Row::new("Restore these").buttons(vec![btn("Cancel"), btn(if restoring { "Restoring…" } else { "Apply" }).primary().on(restoring).off(restoring || nothing)], apply));
}

// ── Apps ────────────────────────────────────────────────────────────────

fn app_key(id: &str) -> String {
    format!("about.bk.app.{id}")
}

fn picked(cx: &Ctx, a: &Value) -> bool {
    pickable(a) && cx.local.get(&app_key(&s(&a["id"]))).and_then(Value::as_bool).unwrap_or_else(|| default_pick(a))
}

fn apps(cx: &mut Ctx, p: &mut Page) {
    let file = cx.local_s("about.bk.file");
    let info = cx.local.get("about.bk.info").cloned().unwrap_or(Value::Null);
    let Some(apps) = info["apps"].as_array().cloned() else { return };
    if file.is_empty() || apps.is_empty() {
        return;
    }
    let results = cx.local.get("about.bk.results").cloned().unwrap_or(json!({}));
    let installing = cx.local_b("about.bk.installed.busy");
    let job = &feed(cx)["up"]["job"];
    let job_running = job["state"] == "running";
    // (Software Update's job for the system's part ended: look again)
    if cx.local_b("about.bk.sysAsked") && !job_running && !job.is_null() {
        cx.local_set("about.bk.sysAsked", false);
        refresh_status(cx);
    }

    let groups = by_source(&apps);
    let mut managers: Vec<String> = Vec::new();
    for (gid, name, list) in &groups {
        let open_key = format!("about.bk.open.{gid}");
        let open = cx.local.get(&open_key).and_then(Value::as_bool).unwrap_or(list.len() <= 12);
        let here = list.iter().filter(|a| a["installed"] == true).count();
        let chosen_n = list.iter().filter(|a| picked(cx, a)).count();
        let needs = list.iter().map(|a| s(&a["needs"])).find(|n| !n.is_empty()).unwrap_or_default();
        let package = list.iter().map(|a| s(&a["needsPackage"])).find(|n| !n.is_empty()).unwrap_or_default();
        let g = p.group(*name);
        // A missing manager: offered first (installed with the system's packages)
        if !needs.is_empty() {
            let mk = format!("about.bk.mgr.{gid}");
            let want = cx.local.get(&mk).and_then(Value::as_bool).unwrap_or(false);
            if want && !package.is_empty() {
                managers.push(package.clone());
            }
            let act = cx.on(move |cx, _| {
                let now = cx.local.get(&mk).and_then(Value::as_bool).unwrap_or(false);
                cx.local_set(&mk, !now);
            });
            let detail = if package.is_empty() {
                format!("{needs} isn't installed here. Install it, then come back.")
            } else {
                format!("{needs} isn't installed here: these need it. Install it first ({}), with the system's packages; then pick them.", package.trim_start_matches("aur:"))
            };
            g.row(Row::new(format!("Needs {needs}")).detail(detail).icon("warning-circle").switch(want, act).enabled(!package.is_empty()));
        }
        // The group's line: how many, what's here, Show and Select
        let ids: Vec<String> = list.iter().filter(|a| pickable(a) && s(&a["needs"]).is_empty()).map(|a| s(&a["id"])).collect();
        let ok = open_key.clone();
        let all_on = chosen_n == ids.len() && !ids.is_empty();
        let act = cx.on(move |cx, a| match a.i {
            0 => {
                let now = cx.local.get(&ok).and_then(Value::as_bool).unwrap_or(false);
                cx.local_set(&ok, !now);
            }
            _ => {
                for id in &ids {
                    cx.local_set(&app_key(id), !all_on);
                }
            }
        });
        g.row(Row::new(format!("{} {}", list.len(), if list.len() == 1 { "app" } else { "apps" }))
            .detail(format!("{here} here already · {chosen_n} chosen"))
            .buttons(vec![btn(if open { "Hide" } else { "Show" }), btn(if all_on { "Choose None" } else { "Choose All" }).off(!list.iter().any(|a| pickable(a) && s(&a["needs"]).is_empty()))], act));
        if !open {
            continue;
        }
        for a in list {
            let id = s(&a["id"]);
            let r = &results[&id];
            let word = result_word(r);
            let version = s(&a["version"]);
            let mut detail = vec![];
            if !version.is_empty() && a["group"] != "fnm" && a["group"] != "nvm" {
                detail.push(version);
            }
            detail.push(s(&a["source"]));
            let cmd = s(&a["command"]);
            if !cmd.is_empty() {
                detail.push(cmd);
            }
            if r["state"] == "failed" && !s(&r["why"]).is_empty() {
                detail.push(s(&r["why"]));
            }
            let label = if a["group"] == "fnm" || a["group"] == "nvm" { format!("Node.js {}", s(&a["version"])) } else { s(&a["name"]) };
            let row = Row::new(label).detail(detail.join(" · "));
            let row = if a["installed"] == true {
                row.shows("Installed")
            } else if !word.is_empty() && r["state"] != "running" {
                row.shows(word)
            } else if r["state"] == "running" {
                row.spinner()
            } else if a["installable"] == false {
                row.shows("Origin unknown")
            } else if !s(&a["needs"]).is_empty() {
                row.shows(format!("Needs {}", s(&a["needs"])))
            } else {
                let k = app_key(&id);
                let def = default_pick(a);
                let act = cx.on(move |cx, _| {
                    let now = cx.local.get(&k).and_then(Value::as_bool).unwrap_or(def);
                    cx.local_set(&k, !now);
                });
                row.switch(picked(cx, a), act)
            };
            g.row(row);
        }
    }

    // Install what's chosen: the system's part with Software Update, the rest as you
    let chosen_apps: Vec<Value> = apps.iter().filter(|a| picked(cx, a) && results[&s(&a["id"])]["state"] != "ok").cloned().collect();
    let (repo, aur) = system_packages(&chosen_apps, &managers);
    let user: Vec<String> = chosen_apps.iter().filter(|a| a["system"] != true).map(|a| s(&a["id"])).collect();
    let progress = cx.local.get("about.bk.progress").cloned().unwrap_or(Value::Null);
    let status = cx.local_s("about.bk.astatus");
    let sys_n = repo.len() + aur.len();
    let g = p.group("");
    g.footer(if !status.is_empty() {
        status
    } else {
        "Arch's and the AUR's go through Software Update (one password; the AUR's with your AUR helper). The others install as you, one at a time.".into()
    });
    if job_running {
        g.row(Row::new(format!("Software Update: {}", s(&job["text"]))).detail(format!("{}%", job["percent"].as_i64().unwrap_or(0))).spinner());
    }
    if installing {
        let (done, total) = (progress["done"].as_u64().unwrap_or(0), progress["total"].as_u64().unwrap_or(user.len() as u64));
        g.row(Row::new(format!("Installing {} of {total}", (done + 1).min(total.max(1)))).detail(s(&progress["id"])).spinner());
    }
    let words = match (sys_n, user.len()) {
        (0, 0) => "Nothing chosen".to_string(),
        (a, 0) => format!("{a} with Software Update"),
        (0, b) => format!("{b} as you"),
        (a, b) => format!("{a} with Software Update, {b} as you"),
    };
    let f = file.clone();
    let act = cx.on(move |cx, _| {
        if !repo.is_empty() || !aur.is_empty() {
            cx.ask("about", "backup.packages", json!([repo, aur]));
            cx.local_set("about.bk.sysAsked", true);
        }
        if !user.is_empty() {
            let args = vec!["apps-install".to_string(), f.clone(), "--ids".into(), user.join(",")];
            cx.local_set("about.bk.astatus", "");
            cx.spawn_progress("about.bk.installed", "about.bk.progress", move |say| run_telling(args, say));
        }
    });
    g.row(Row::new("Install chosen apps").detail(words).buttons(vec![btn(if installing { "Installing…" } else { "Install" }).primary().on(installing).off(installing || job_running || sys_n + chosen_apps.iter().filter(|a| a["system"] != true).count() == 0)], act));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_are_checked() {
        for good in ["firefox", "lib32-glibc", "python-pipx", "gtk+3", "c++utilities", "nvm", "yay-bin", "foo@1.2_x"] {
            assert!(is_package(good), "{good}");
        }
        for bad in ["", "-Syu", ".hidden", "Firefox", "a b", "a;rm", "a/b", "ä"] {
            assert!(!is_package(bad), "{bad}");
        }
    }

    #[test]
    fn older_settings_files_restore_the_old_way() {
        assert!(is_legacy("/home/x/Documents/Tessera settings 2026-11-01.tessera"));
        assert!(!is_legacy("/home/x/Tessera backup 2026-11-22.tar.zst"));
        assert_eq!(chosen(&[("settings".into(), true), ("hypr".into(), false), ("apps".into(), true)]), "settings,apps");
    }

    #[test]
    fn the_plan_in_lines() {
        let plan = json!({"ok": true, "parts": {
            "settings": {"count": 14, "changes": (0..14).map(|i| json!({"key": format!("a.k{i}"), "now": "1", "then": "2"})).collect::<Vec<_>>(), "shortcuts": true, "focus": false},
            "hypr": {"files": [{"path": "~/.config/hypr/custom.lua", "action": "replace"}, {"path": "~/.config/hypr/x.lua", "action": "same"}, {"path": "~/.config/hypr/tessera/look.lua.from-backup", "action": "beside"}]},
            "wallpapers": {"count": 2}, "login": {"changes": [{"key": "lock.loginStyle", "now": "look", "then": "plain"}]}},
            "skipped": ["the live wallpaper (its video isn't on this computer)"]});
        let l = plan_lines(&plan);
        assert_eq!(l[0], ("a.k0".to_string(), "1 → 2".to_string()));
        assert!(l.iter().any(|x| x.0 == "2 more settings"));
        assert!(l.iter().any(|x| x.0 == "Shortcuts and gestures"));
        assert!(!l.iter().any(|x| x.0 == "Focus modes"));
        assert!(l.iter().any(|x| x.0 == "~/.config/hypr/custom.lua" && x.1.starts_with("Replaced")));
        assert!(!l.iter().any(|x| x.0 == "~/.config/hypr/x.lua"), "the same file isn't a change");
        assert!(l.iter().any(|x| x.1 == "Put beside Tessera's file, which stays"));
        assert!(l.iter().any(|x| x.0 == "Wallpapers" && x.1 == "2 pictures go into ~/Pictures/Wallpapers/Restored"));
        assert!(l.iter().any(|x| x.0 == "lock.loginStyle" && x.1 == "look → plain"));
        assert!(l.iter().any(|x| x.0 == "Left out"));
        let same = json!({"parts": {"settings": {"count": 0, "changes": []}}});
        assert_eq!(plan_lines(&same), vec![("Settings".to_string(), "The same as now".to_string())]);
    }

    #[test]
    fn apps_grouped_picked_and_sent_to_the_right_installer() {
        let apps = vec![
            json!({"id": "pacman:firefox", "group": "pacman", "name": "firefox", "installed": false, "installable": true, "system": true, "needs": ""}),
            json!({"id": "pacman:git", "group": "pacman", "name": "git", "installed": true, "installable": true, "system": true, "needs": ""}),
            json!({"id": "aur:yay-bin", "group": "aur", "name": "yay-bin", "installed": false, "installable": true, "system": true, "needs": ""}),
            json!({"id": "fnm:v20.11.1", "group": "fnm", "name": "node", "version": "v20.11.1", "installed": false, "installable": true, "system": false, "needs": "fnm", "needsPackage": "fnm"}),
            json!({"id": "local:foo", "group": "local", "name": "foo", "installed": false, "installable": false, "system": false, "needs": ""}),
            json!({"id": "pacman:-Syu", "group": "pacman", "name": "-Syu", "installed": false, "installable": true, "system": true, "needs": ""}),
        ];
        let g = by_source(&apps);
        assert_eq!(g.iter().map(|x| x.0).collect::<Vec<_>>(), ["pacman", "aur", "fnm", "local"]);
        assert!(default_pick(&apps[0]) && !default_pick(&apps[1]) && !default_pick(&apps[3]) && !default_pick(&apps[4]));
        assert!(pickable(&apps[3]) && !pickable(&apps[4]));
        let picked: Vec<Value> = apps.iter().filter(|a| default_pick(a)).cloned().collect();
        // (fnm asked for first, from Arch; nvm's from the AUR; a name that's an option never)
        let (repo, aur) = system_packages(&picked, &["fnm".into(), "aur:nvm".into(), "fnm".into()]);
        assert_eq!(repo, ["fnm", "firefox"]);
        assert_eq!(aur, ["nvm", "yay-bin"]);
        assert_eq!(result_word(&json!({"state": "skipped", "why": "needs fnm"})), "Skipped: needs fnm");
        assert_eq!(result_word(&json!({"state": "ok"})), "Installed");
        assert_eq!(result_word(&json!({"state": "system"})), "With Software Update");
    }

    #[test]
    fn the_helpers_answer_is_its_last_json_line() {
        let out = "{\"progress\": {\"id\": \"a\"}}\nnoise\n{\"ok\": true, \"results\": {}}\n";
        assert_eq!(answer_of(out)["ok"], true);
        assert_eq!(answer_of("")["ok"], false);
        assert_eq!(kib(2048), "2 KB");
    }
}
