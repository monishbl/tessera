//! Health (Rust only): how Tessera has been doing, from its own record
//! (tessera_core::health: a sample each 5 minutes, and the crashes, restarts
//! and fallbacks as they happened). At the top one line says how it's
//! going; then memory and processor over 24 hours or 7 days, the slow
//! moments, and the recent events in plain words, each with the one thing
//! worth doing about it (save its crash report, use the faster GPU).
//! Read off the UI thread when the page opens; nothing is sent anywhere.
//!
//! The one Health page (22 Nov): About → System health's service checks
//! and their fixes are a part of it (about/health.rs: `services`; the id
//! about/health comes here, map::RUST_ALIASES), and so is the Rust shell's
//! stability over a week and a month (days since the last crash, crashes,
//! fallbacks to the QML shell), for deciding when that fallback can go.
//! Failed updates are events too (health_event.py writes them); the
//! shell's alerts for them, for crashes and for fallbacks are host-shell's
//! (probes.rs, core health::due_alerts).

use super::super::super::model::{btn, Ctx, Opt, Page, Row};
use serde_json::{json, Value};
use tessera_core::health::{self, Sample};

/// The record, read off the UI thread: its samples (with the bucket still
/// being counted, and the memory now), its events, the crash reports kept
fn load(live: Option<Sample>) -> Value {
    let (mut samples, events) = health::read_all();
    let (rss, pss) = health::memory();
    let recording = live.is_some();
    if let Some(mut l) = live {
        (l.rss, l.pss) = (rss, pss);
        samples.push(l);
    }
    let reports: Vec<String> = tessera_core::crash::list().iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    json!({
        "now": tessera_core::util::now_ms() / 1000,
        "mem": if pss > 0 { pss } else { rss },
        "recording": recording,
        "samples": samples.iter().map(|s| json!([s.t, s.d, s.mem(), s.cpu_ms, s.s16, s.s33, s.worst, s.crashes, s.frames, s.missed, s.pressure])).collect::<Vec<_>>(),
        "events": events,
        "reports": reports,
    })
}

fn samples_of(v: &Value) -> Vec<Sample> {
    let n = |a: &Value, i: usize| a[i].as_u64().unwrap_or(0);
    v["samples"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| Sample { t: s[0].as_i64().unwrap_or(0), d: s[1].as_i64().unwrap_or(300).max(1), pss: n(s, 2), cpu_ms: n(s, 3), s16: n(s, 4), s33: n(s, 5), worst: n(s, 6), crashes: n(s, 7), frames: n(s, 8), missed: n(s, 9), pressure: n(s, 10), ..Default::default() })
                .collect()
        })
        .unwrap_or_default()
}

/// "142 MB" from KiB
pub fn mb(kib: u64) -> String {
    let m = kib as f64 / 1024.0;
    if m >= 1024.0 {
        format!("{:.1} GB", m / 1024.0)
    } else {
        format!("{} MB", m.round())
    }
}

/// "0.8%", "12%"
pub fn pct(p: f64) -> String {
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{}%", p.round())
    }
}

/// "1 restart", "2 restarts"
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// When, in words: "Today 14:02", "Yesterday 09:10", "Mon 16 Nov 14:02"
pub fn when(t: i64, now: i64) -> String {
    let day = tessera_core::util::local_day;
    let hm = tessera_core::util::strftime("%H:%M", t);
    match day(now) - day(t) {
        0 => format!("Today {hm}"),
        1 => format!("Yesterday {hm}"),
        _ => tessera_core::util::strftime("%a %e %b %H:%M", t).split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// "14:02", "yesterday 09:10", "Mon 16 Nov"
pub fn since(t: i64, now: i64) -> String {
    let day = tessera_core::util::local_day;
    match day(now) - day(t) {
        0 => tessera_core::util::strftime("%H:%M", t),
        1 => format!("yesterday {}", tessera_core::util::strftime("%H:%M", t)),
        _ => tessera_core::util::strftime("%a %e %b", t).split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// A part's name in a sentence ("the dock")
fn part_name(part: &str) -> String {
    let n = tessera_core::contain::display_name(part);
    match n.strip_prefix("The ") {
        Some(rest) => format!("the {rest}"),
        None => n,
    }
}

fn cap(t: &str) -> String {
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

use tessera_core::health::is_restart;

/// An update kind's name in a sentence ("The system update")
fn update_name(what: &str) -> &'static str {
    match what {
        "system" => "The system update",
        "tessera" => "Tessera's update",
        _ => "Installing Tessera's programs",
    }
}

/// The update kinds whose last try failed today
fn failed_updates_today(events: &[Value], now: i64) -> Vec<&'static str> {
    let day = tessera_core::util::local_day;
    tessera_core::health::UPDATE_KINDS
        .iter()
        .filter(|(what, _)| tessera_core::health::failing_run(events, what).last().is_some_and(|t| day(*t) == day(now)))
        .map(|(what, _)| *what)
        .collect()
}

/// The line at the top: what, more, whether all is well, and what to do
/// about it (numbered steps, a page to open)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Top {
    pub label: String,
    pub detail: String,
    pub ok: bool,
    pub steps: Vec<String>,
    /// (page, part to show)
    pub open: Option<(&'static str, &'static str)>,
    /// the detail already says when this run started
    pub says_start: bool,
}

/// Report a Bug: where a crash, a part turned off or a fallback goes next
const BUG: (&str, &str) = ("about/bugreport", "");

fn steps(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

/// Slow moments while the computer was short of memory: what to do
const SHORT_OF_MEMORY: &[&str] = &[
    "Close apps you aren't using, browser tabs above all",
    "Run big builds while you're away, or with fewer jobs at once so they use less memory",
    "If it happens with little else open, Report a Bug and say what you were doing",
];
/// Slow moments otherwise
const SLOW: &[&str] = &[
    "If it's while windows move or open over the glass, try Frosted or Solid in Appearance → Glass",
    "If the computer runs hot, turn on Cool-down in Battery & Power",
    "If it keeps happening, Report a Bug and say what you were doing",
];

/// The last start (this run's), from the record
pub fn run_start(events: &[Value]) -> Option<i64> {
    events.iter().rev().find(|e| e["ev"] == "start").and_then(|e| e["t"].as_i64())
}

pub fn status(events: &[Value], samples: &[Sample], now: i64) -> Top {
    let day = tessera_core::util::local_day;
    let today = |e: &&Value| e["t"].as_i64().is_some_and(|t| day(t) == day(now));
    let week = |e: &&Value| e["t"].as_i64().is_some_and(|t| now - t < 7 * 86400);
    let restarts = events.iter().filter(today).filter(|e| is_restart(e)).count();
    let fallbacks = events.iter().filter(today).filter(|e| e["ev"] == "fallback").count();
    let parts = events.iter().filter(today).filter(|e| e["ev"] == "crash" && e["fatal"] != true).count();
    let offs: Vec<String> = events.iter().filter(today).filter(|e| e["ev"] == "off").filter_map(|e| e["part"].as_str()).map(part_name).collect();
    let bad = |label: String, detail: String, st: &[&str], open: Option<(&'static str, &'static str)>| Top { label, detail, ok: false, steps: steps(st), open, says_start: false };
    if restarts > 0 {
        let kinds: Vec<&str> = events.iter().filter(today).filter(|e| is_restart(e)).filter_map(|e| e["why"].as_str()).collect();
        let why = if kinds.iter().all(|k| *k == "watchdog") {
            "It stopped responding and was started again."
        } else if kinds.iter().all(|k| *k == "crash") {
            "It stopped after an error and was started again."
        } else {
            "It stopped and was started again."
        };
        let st: &[&str] = if kinds.iter().any(|k| *k == "stopped") {
            &["If the computer was short of memory then (a big build, many browser tabs), close what you don't need", "Save the crash report from an event below, if it has one", "Send it with Report a Bug"]
        } else {
            &["Save the crash report from an event below, if it has one", "Send it with Report a Bug"]
        };
        return bad(format!("{} today", count(restarts, "restart", "restarts")), format!("{why} The events below say when and why."), st, Some(BUG));
    }
    if fallbacks > 0 {
        return bad(
            "Switched to the basic shell today".into(),
            "Tessera stopped twice in a row, so the basic shell stood in until it was started again.".into(),
            &["Nothing to do if it's back: Tessera tries again by itself", "If it keeps switching, Report a Bug"],
            Some(BUG),
        );
    }
    if let Some(first) = offs.first() {
        return bad(
            format!("{} was turned off today", cap(first)),
            "It stopped 3 times in 2 minutes. It comes back when you log in again.".into(),
            &["Log out and in again to bring it back", "Save its crash report from an event below and send it with Report a Bug"],
            Some(BUG),
        );
    }
    if parts > 0 {
        return bad(
            format!("{} recovered today", count(parts, "part", "parts")),
            "A part of Tessera stopped after an error and was started again. Everything else kept working.".into(),
            &["Nothing to do if it doesn't happen again", "If it does, save the crash report from an event below and send it with Report a Bug"],
            Some(BUG),
        );
    }
    if let Some(what) = failed_updates_today(events, now).first() {
        return bad(
            format!("{} didn't finish today", update_name(what)),
            "The events below say why. Try it again from Software Update.".into(),
            &["Open Software Update and try again", "If it stops again, the event below says why"],
            Some(("update", "")),
        );
    }
    let day_ago = now - 86400;
    let recent: Vec<&Sample> = samples.iter().filter(|s| s.t >= day_ago).collect();
    let long: u64 = recent.iter().map(|s| s.s33).sum();
    let worst = recent.iter().map(|s| s.worst).max().unwrap_or(0);
    if long >= 20 {
        // This run's and the earlier ones' (a sample is this run's when it
        // ends after the start: the bucket the start fell in is), and those
        // while the computer was short of memory
        let up = run_start(events);
        let this_run: u64 = recent.iter().filter(|s| up.map_or(true, |u| s.t + s.d > u)).map(|s| s.s33).sum();
        let before = long - this_run;
        let squeezed: u64 = recent.iter().filter(|s| s.pressure >= tessera_core::health::PRESSURE_PCT).map(|s| s.s33).sum();
        let mut detail = format!("Tessera paused for over two frames {long} times in the last 24 hours (the longest {worst} ms).");
        let says_start = up.is_some() && before > 0;
        if let Some(u) = up.filter(|_| before > 0) {
            detail.push_str(&format!(" {this_run} since it started ({}), {before} before that.", since(u, now)));
        }
        let short = squeezed > 0 && squeezed * 2 >= long;
        if short {
            detail.push_str(" Most happened while the computer was short of memory (for example a big build or many browser tabs).");
        }
        let label = if up.is_some() && this_run * 10 < long { "Some slow moments, before this start" } else { "Some slow moments today" };
        return Top { says_start, ..bad(label.into(), detail, if short { SHORT_OF_MEMORY } else { SLOW }, Some(BUG)) };
    }
    let earlier = events.iter().filter(week).filter(|e| is_restart(e) || e["ev"] == "fallback" || e["ev"] == "off").count();
    let more = if earlier == 0 { "No crashes or restarts in the last 7 days.".to_string() } else { format!("No crashes or restarts today; {} in the last 7 days.", count(earlier, "problem", "problems")) };
    Top { label: "Running smoothly".into(), detail: more, ok: true, ..Default::default() }
}

/// The top line with the service checks (About's System health) counted
/// in: never "Running smoothly" while any of them needs attention. When the
/// record's own line is already a warning (restarts, slow moments) it stays,
/// and the services are added to its explanation.
pub fn with_services(line: (String, String, bool), attention: &[String]) -> (String, String, bool) {
    if attention.is_empty() {
        return line;
    }
    let (label, detail, ok) = line;
    let n = count(attention.len(), "thing needs attention", "things need attention");
    let names = attention.join(", ");
    if ok {
        (n, format!("{names}. System health, below, says how to fix {}.", if attention.len() == 1 { "it" } else { "them" }), false)
    } else {
        (label, format!("{detail} Also: {n} ({names})."), false)
    }
}

/// The names of the service checks that need attention (warn or fail)
pub fn attention(checks: &Value) -> Vec<String> {
    checks
        .as_array()
        .map(|a| a.iter().filter(|c| c["status"] == "fail" || c["status"] == "warn").filter_map(|c| c["name"].as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// The link to System health, saying what its checks found
fn services_row(needs: &[String], act: String) -> Row {
    let label = if needs.is_empty() { "System health".to_string() } else { count(needs.len(), "thing needs attention", "things need attention") };
    let detail = if needs.is_empty() { "Whether the services Tessera uses are working".to_string() } else { format!("{}: open System health to fix", needs.join(", ")) };
    Row::link(label, "about/health").icon(if needs.is_empty() { "heartbeat" } else { "warning-circle" }).detail(detail).id(act)
}

/// What one event was, for people: (title, explanation, what to offer)
pub enum Offer {
    None,
    /// Save report (the crash report's file name)
    Report(String),
    /// Use the other GPU (else Cool-down, where Graphics processor is)
    OtherGpu,
    /// Open a page: (page, part to show)
    Open(&'static str, &'static str),
}

/// An explanation with what to do by hand after it, numbered
fn with_steps(text: &str, st: &[&str]) -> String {
    format!("{text}\n{}", super::super::about::health::numbered(&steps(st)))
}

pub fn explain(e: &Value) -> Option<(String, String, Offer)> {
    // (a crash: its report, else Report a Bug)
    let report = || e["report"].as_str().filter(|r| !r.is_empty()).map(|r| Offer::Report(r.to_string())).unwrap_or(Offer::Open(BUG.0, BUG.1));
    let part = e["part"].as_str().unwrap_or("");
    let bug: &[&str] = &["If it happens again, Report a Bug and say what you were doing"];
    Some(match e["ev"].as_str()? {
        "start" => match e["why"].as_str()? {
            "crash" => ("Restarted after a crash".into(), with_steps("Tessera stopped after an error and was started again. The crash report has the details.", &["Save its crash report (Save report), if it was kept", "Send it with Report a Bug"]), report()),
            "stopped" => (
                "Restarted after it stopped".into(),
                with_steps(
                    "Tessera ended without saying why (the computer may have run out of memory) and was started again.",
                    &["If the computer was short of memory then (a big build, many browser tabs), close what you don't need", "If it happens with memory to spare, Report a Bug"],
                ),
                Offer::Open(BUG.0, BUG.1),
            ),
            "watchdog" => ("Restarted because it stopped responding".into(), with_steps("Tessera didn't answer for over a minute, so it was started again.", bug), Offer::Open(BUG.0, BUG.1)),
            "retry" => ("Back from the basic shell".into(), "Tessera started again after the basic shell had stood in for it.".into(), Offer::None),
            _ => return None,
        },
        "crash" if e["fatal"] != true => (format!("{} stopped and recovered", cap(&part_name(part))), "An error was caught and this part was started again. Everything else kept working.".into(), report()),
        "off" => (
            format!("{} was turned off", cap(&part_name(part))),
            with_steps("It stopped 3 times in 2 minutes, so it stays off until you log in again.", &["Log out and in again to bring it back", "If it stops again, Report a Bug"]),
            Offer::Open(BUG.0, BUG.1),
        ),
        "fallback" => {
            let why = e["why"].as_str().unwrap_or("").trim();
            let text = if why.is_empty() { "Tessera stopped twice in a row, so the basic shell stood in.".to_string() } else { format!("{why}. The basic shell stood in until Tessera was started again.") };
            ("Switched to the basic shell".into(), with_steps(&text, &["Tessera tries again by itself at its next start", "If it keeps switching, Report a Bug"]), Offer::Open(BUG.0, BUG.1))
        }
        "lock" => ("Screen locked again after a restart".into(), "Tessera ended while your screen was locked; it locked again as it started, so nothing was left open.".into(), Offer::None),
        "gpu-slow" => {
            let share = e["share"].as_f64().unwrap_or(0.0) * 100.0;
            let on = e["on"].as_str().unwrap_or("low-power");
            (format!("Animations were slow on the {on} graphics"), format!("{}% of animation frames were missed. The other GPU is faster but uses more power.", share.round()), Offer::OtherGpu)
        }
        // (an update's end: only a failure is listed)
        "update" if e["ok"] != true => {
            let why = e["why"].as_str().unwrap_or("").trim();
            let step = match e["step"].as_str().unwrap_or("") {
                "stage" => " while it was being made ready",
                "finish" | "restart" => " while it was being installed",
                _ => "",
            };
            (format!("{} didn't finish", update_name(e["what"].as_str().unwrap_or(""))), if why.is_empty() { format!("It stopped{step}. Try again from Software Update.") } else { format!("It stopped{step}: {why}") }, Offer::Open("update", ""))
        }
        "gpu-fallback" => {
            let (from, to) = (e["from"].as_str().unwrap_or("chosen"), e["to"].as_str().unwrap_or("desktop's"));
            (
                format!("Couldn't draw on {from}: back on {to}"),
                with_steps(&format!("Tessera couldn't start on the {from} GPU with this desktop, so Graphics processor was set to Same as the desktop."), &["Leave it on Same as the desktop", "To try the other GPU again after an update: Cool-down → Graphics processor"]),
                Offer::Open("power/cooldown", "Performance"),
            )
        }
        _ => return None,
    })
}

/// The root helper's policy file: world-readable, installed with its polkit
/// rules (whose folder isn't), so its note says which rules are there
const POLICY: &str = "/usr/share/polkit-1/actions/org.tessera.root.policy";
/// The note the policy carries since the fingerprint reader's restart rule
const READER_RULE_NOTE: &str = "tessera-fprintd-restart";

/// The one-time setup is still to run for the lock to restart a stuck
/// fingerprint reader by itself: fprintd is here, and the installed rules
/// are older than that rule (or missing)
pub fn reader_setup_pending(has_fprintd: bool, policy: Option<&str>) -> bool {
    has_fprintd && !policy.is_some_and(|p| p.contains(READER_RULE_NOTE))
}

fn has_fprintd() -> bool {
    ["/usr/lib/systemd/system/fprintd.service", "/etc/systemd/system/fprintd.service", "/usr/share/dbus-1/system-services/net.reactivated.Fprint.service"].iter().any(|p| std::path::Path::new(p).exists())
}

/// pkexec <root helper> setup <the shell's folder>: the installed helper,
/// else this version's own script (the password, once)
fn run_setup() -> Value {
    let Some(shell) = tessera_core::util::asset("shell") else {
        return json!({"ok": false, "err": "Tessera's shell folder wasn't found. Reinstall Tessera, then try again."});
    };
    let shell = shell.to_string_lossy().into_owned();
    let installed = "/usr/lib/tessera/tessera-root";
    let mut cmd = std::process::Command::new("pkexec");
    if std::path::Path::new(installed).exists() {
        cmd.args([installed, "setup", &shell]);
    } else {
        cmd.args(["python3", "-I", &format!("{shell}/assets/scripts/tessera-root.py"), "setup", &shell]);
    }
    match cmd.stdin(std::process::Stdio::null()).output() {
        Ok(o) if o.status.success() => json!({"ok": true}),
        // (126: the password dialog was closed)
        Ok(o) if o.status.code() == Some(126) => json!({"ok": false, "err": ""}),
        Ok(o) => {
            let said = String::from_utf8_lossy(&o.stderr).trim().trim_start_matches("tessera-root: ").to_string();
            json!({"ok": false, "err": if said.is_empty() { "The setup didn't finish. Try again.".to_string() } else { format!("{said}. Try again.") }})
        }
        Err(_) => json!({"ok": false, "err": "Setup needs polkit (pkexec), which isn't installed. Install polkit, then try again."}),
    }
}

/// "Fingerprint after sleep" while the one-time setup is still to run
fn setup_steps(cx: &mut Ctx, p: &mut Page) {
    let policy = std::fs::read_to_string(POLICY).ok();
    let done = cx.local.get("health.setup").cloned();
    if !reader_setup_pending(has_fprintd(), policy.as_deref()) || done.as_ref().is_some_and(|d| d["ok"] == true) {
        return;
    }
    let busy = cx.local_b("health.setup.busy");
    let act = cx.on(|cx, _| cx.spawn("health.setup", run_setup));
    let err = done.as_ref().and_then(|d| d["err"].as_str()).filter(|e| !e.is_empty()).map(str::to_string);
    let detail = err.unwrap_or_else(|| "After a sleep the fingerprint reader can stop answering until it's restarted. Run this once so the lock restarts it by itself; it asks for an administrator's password.".into());
    p.group("Setup").row(Row::new("Restart the fingerprint reader by itself").detail(detail).icon("fingerprint").buttons(vec![btn(if busy { "Setting up…" } else { "Run Setup" }).icon("fingerprint").primary().on(busy)], act));
}

/// A scale's top for `max` (a round number above it)
pub fn nice_top(max: f64, floor: f64) -> f64 {
    let m = max.max(floor);
    let p = 10f64.powf(m.log10().floor());
    for k in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if k * p >= m {
            return k * p;
        }
    }
    10.0 * p
}

/// A bar chart row (settings-system.slint's "chart"): one bar a slot
fn chart(title: &str, now: String, top: String, bars: Vec<Opt>, low: &str, summary: String, first: bool, last: bool) -> Row {
    let mut r = Row::custom("system", "chart").data(bars);
    r.label = title.into();
    r.value = now;
    r.text = top;
    r.low = low.into();
    r.high = "Now".into();
    r.detail = summary;
    r.from = if first { 1.0 } else { 0.0 };
    r.to = if last { 1.0 } else { 0.0 };
    r
}

/// A bar: its height (0..1), what hovering it says, and its style (0 the
/// accent, 1 a warning, 3 no record)
fn bar(frac: f64, says: String, style: i32) -> Opt {
    Opt::new("").num(frac.clamp(0.0, 1.0) as f32).detail(says).style(style)
}

/// "N in 7 days · M in 30 days"
fn week_month(w: usize, m: usize) -> String {
    format!("{w} in 7 days · {m} in 30 days")
}

/// The Rust shell's stability, for deciding when the basic (QML) shell can
/// go: (label, value, detail) rows, and the group's footer
pub fn stability_rows(s: &health::Stability, now: i64) -> (Vec<(String, String, String)>, String) {
    let mut rows = Vec::new();
    let start = s.since.map(|t| since(t, now)).unwrap_or_default();
    rows.push(match (s.days_since_crash(now), s.last_crash) {
        (Some(d), Some(t)) => ("Days since the last crash".to_string(), d.to_string(), format!("The last one: {}", when(t, now))),
        _ => ("Days since the last crash".to_string(), "No crash recorded".to_string(), if start.is_empty() { String::new() } else { format!("Nothing since the record starts, {start}") }),
    });
    rows.push(("Crashes".into(), week_month(s.crashes_7, s.crashes_30), "Restarts that weren't asked for: an error, the computer out of memory, or Tessera not answering".into()));
    rows.push(("Switched to the basic shell".into(), week_month(s.fallbacks_7, s.fallbacks_30), "Tessera stopped twice in a row, so the basic (QML) shell stood in".into()));
    let days = s.days_recorded(now);
    let covers = if s.since.is_none() {
        " There's no record yet.".to_string()
    } else if days < 30 {
        format!(" The record covers {} so far.", count(days.max(1) as usize, "day", "days"))
    } else {
        String::new()
    };
    (rows, format!("How the Rust shell has held up, to decide when the basic shell that stands in for it can go.{covers}"))
}

/// A row with "Open <page>": the page where what's wrong is dealt with
fn open_button(cx: &mut Ctx, row: Row, (page, target): (&'static str, &'static str)) -> Row {
    let Some(name) = super::super::about::health::page_name(page) else { return row };
    let act = cx.on(move |cx, _| cx.go(page, target));
    row.buttons(vec![btn(format!("Open {name}"))], act)
}

/// A missed share of animation frames this high is worth acting on (%)
const MISSED_PCT: f64 = 10.0;

pub fn page(cx: &mut Ctx, p: &mut Page) {
    p.title = String::new();
    let live = health::live();
    let v = cx.once("health.data", move || load(live));
    let footer = "Kept on this computer only: a week of measurements and a month of events (about 200 KB). Nothing is sent.";
    // (the services Tessera uses: About → System health once, now here)
    let services = super::super::about::health::services;
    if v.is_null() {
        p.group("").row(Row::new("Reading Tessera's record…").spinner());
        services(cx, p);
        return;
    }
    let now = v["now"].as_i64().unwrap_or(0);
    let samples = samples_of(&v);
    let events: Vec<Value> = v["events"].as_array().cloned().unwrap_or_default();
    // (the service checks, as About's System health lists them: the bridge
    // runs them when this page opens too)
    let feed = cx.feed;
    let needs = attention(&feed["about"]["health"]["checks"]);
    let services_link = cx.link("about/health", "");
    if samples.is_empty() && events.is_empty() {
        let (label, detail) = if v["recording"] == true {
            ("No history yet", "Tessera notes its memory, processor time and slow moments every 5 minutes, and any crash or restart as it happens. Check back in a few minutes.")
        } else {
            ("Not recorded here", "Tessera keeps this record when it runs as the whole shell. Started beside the basic shell, it doesn't.")
        };
        let (label, detail, ok) = with_services((label.to_string(), detail.to_string(), true), &needs);
        p.group("").footer(footer).row(Row::new(label).detail(detail).icon(if ok { "heartbeat" } else { "warning-circle" })).row(services_row(&needs, services_link.clone()));
        setup_steps(cx, p);
        services(cx, p);
        return;
    }

    // ── How it's going ──
    let mut top = status(&events, &samples, now);
    // (never "running smoothly" while services need attention: audit #172)
    let (label, detail, ok) = with_services((top.label.clone(), top.detail.clone(), top.ok), &needs);
    top.label = label;
    top.detail = detail;
    top.ok = ok;
    let mut detail = match run_start(&events) {
        Some(t) if v["recording"] == true && !top.says_start => format!("{} Running since {}.", top.detail, since(t, now)),
        _ => top.detail.clone(),
    };
    if !top.steps.is_empty() {
        detail = format!("{detail}\n{}", super::super::about::health::numbered(&top.steps));
    }
    let row = Row::new(top.label.clone()).detail(detail).icon(if top.ok { "check-circle" } else { "warning-circle" });
    p.group("")
        .row(match top.open {
            Some(open) => open_button(cx, row, open),
            None => row,
        })
        .row(services_row(&needs, services_link));
    setup_steps(cx, p);

    // ── The services ──
    services(cx, p);

    // ── Rust shell stability ──
    let (rows, stab_footer) = stability_rows(&health::stability(&events, now), now);
    let g = p.group("Rust shell stability");
    g.footer(stab_footer);
    for (label, value, detail) in rows {
        g.row(Row::new(label).detail(detail).shows(value));
    }

    // ── Memory and processor ──
    let week = cx.local_s("health.range") == "week";
    let (secs, n, low) = if week { (6 * 3600, 28usize, "7 days ago") } else { (3600, 24usize, "24 hours ago") };
    let from = (now / secs) * secs - (n as i64 - 1) * secs;
    let slots = health::slots(&samples, from, secs, n);
    let label_of = |t: i64| if week { tessera_core::util::strftime("%a %H:%M", t) } else { tessera_core::util::strftime("%H:%M", t) };
    let range = cx.on(|cx, a| cx.local_set("health.range", if a.i == 1 { "week" } else { "day" }));
    let mem_top = nice_top(slots.iter().map(|s| s.mem_max as f64).fold(0.0, f64::max), 1024.0);
    let mem_bars: Vec<Opt> = slots
        .iter()
        .map(|s| if s.n == 0 { bar(0.0, format!("{} · no record", label_of(s.start)), 3) } else { bar(s.mem_avg as f64 / mem_top, format!("{} · {}", label_of(s.start), mb(s.mem_avg)), 0) })
        .collect();
    let recorded: Vec<&health::Slot> = slots.iter().filter(|s| s.n > 0).collect();
    let mem_lo = recorded.iter().map(|s| s.mem_avg).filter(|m| *m > 0).min().unwrap_or(0);
    let mem_hi = recorded.iter().map(|s| s.mem_max).max().unwrap_or(0);
    let mem_now = v["mem"].as_u64().unwrap_or(0);
    let cpu_top = nice_top(slots.iter().map(|s| s.cpu_pct).fold(0.0, f64::max), 1.0);
    let cpu_bars: Vec<Opt> = slots
        .iter()
        .map(|s| if s.n == 0 { bar(0.0, format!("{} · no record", label_of(s.start)), 3) } else { bar(s.cpu_pct / cpu_top, format!("{} · {}", label_of(s.start), pct(s.cpu_pct)), 0) })
        .collect();
    let (cpu_ms, cpu_s): (u64, i64) = samples.iter().filter(|s| s.t >= from).fold((0, 0), |a, s| (a.0 + s.cpu_ms, a.1 + s.d));
    let cpu_avg = if cpu_s > 0 { cpu_ms as f64 / (cpu_s as f64 * 10.0) } else { 0.0 };
    let span = if week { "the last 7 days" } else { "the last 24 hours" };
    p.group("Memory and processor")
        .footer("Memory is Tessera's fair share of what it uses (shared parts split with other apps). Processor is the share of one core, averaged.")
        .row(Row::new("Show").segmented(&["24 hours", "7 days"], if week { 1 } else { 0 }, range))
        .row(chart("Memory", if mem_now > 0 { format!("{} now", mb(mem_now)) } else { String::new() }, mb(mem_top as u64), mem_bars, low, format!("Memory over {span}: {} to {}", mb(mem_lo), mb(mem_hi)), false, false))
        .row(chart("Processor", format!("{} average", pct(cpu_avg)), pct(cpu_top), cpu_bars, low, format!("Processor over {span}: {} on average", pct(cpu_avg)), false, true));

    // ── Slow moments ──
    let stall_top = nice_top(slots.iter().map(|s| s.stalls_per_hour).fold(0.0, f64::max), 5.0);
    let stall_bars: Vec<Opt> = slots
        .iter()
        .map(|s| {
            if s.n == 0 {
                bar(0.0, format!("{} · no record", label_of(s.start)), 3)
            } else {
                let says = if s.stalls == 0 { format!("{} · none", label_of(s.start)) } else { format!("{} · {}, the longest {} ms", label_of(s.start), count(s.stalls as usize, "slow moment", "slow moments"), s.worst) };
                bar(s.stalls_per_hour / stall_top, says, if s.long_stalls > 0 { 1 } else { 0 })
            }
        })
        .collect();
    let total: u64 = slots.iter().map(|s| s.stalls).sum();
    let worst = slots.iter().map(|s| s.worst).max().unwrap_or(0);
    let (frames, missed): (u64, u64) = samples.iter().filter(|s| s.t >= from).fold((0, 0), |a, s| (a.0 + s.frames, a.1 + s.missed));
    let g = p.group("Slow moments");
    g.footer("A slow moment is when Tessera was busy for longer than a frame (16 ms), which can show as a stutter; over 33 ms (in orange) you'd likely notice it.");
    g.row(chart(
        "Per hour",
        if total == 0 { "None".into() } else { format!("{}, the longest {worst} ms", count(total as usize, "in all", "in all")) },
        format!("{}", stall_top.round()),
        stall_bars,
        low,
        format!("Slow moments over {span}: {total}, the longest {worst} ms"),
        true,
        frames + missed == 0,
    ));
    if frames + missed > 0 {
        let share = missed as f64 / (frames + missed) as f64 * 100.0;
        let row = Row::new("Animation frames missed");
        if share >= MISSED_PCT {
            // (many missed: the faster GPU, or a lighter glass)
            let detail = format!(
                "{} measured while the timing log or the graphics check was on.\n{}",
                pct(share),
                super::super::about::health::numbered(&steps(&["Choose the faster GPU in Cool-down → Graphics processor (it uses more power)", "Or try Frosted or Solid in Appearance → Glass"]))
            );
            g.row(open_button(cx, row.detail(detail), ("power/cooldown", "Performance")));
        } else {
            g.row(row.detail("Measured while the timing log or the graphics check was on").shows(pct(share)));
        }
    }

    // ── Recent events ──
    let reports: Vec<String> = v["reports"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut shown: Vec<(String, String, Offer)> = events
        .iter()
        .rev()
        .filter(|e| e["t"].as_i64().is_some_and(|t| now - t < 7 * 86400))
        .filter_map(|e| explain(e).map(|(t, d, o)| (t, format!("{} · {d}", when(e["t"].as_i64().unwrap_or(0), now)), o)))
        .take(20)
        .collect();
    let g = p.group("Recent events");
    g.footer(footer);
    if shown.is_empty() {
        g.row(Row::new("No crashes or restarts in the last 7 days").tone(1));
    }
    let other_gpu = tessera_core::gpu::faster_family();
    for (title, detail, offer) in shown.drain(..) {
        let row = Row::new(title).detail(detail);
        let row = match offer {
            Offer::Report(name) => {
                let kept = reports.contains(&name);
                let n = name.clone();
                let act = cx.on(move |cx, _| match tessera_core::crash::save_one(&n) {
                    Ok(path) => cx.toast(format!("Saved to Downloads as {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), None),
                    Err(e) => cx.toast(format!("Couldn't save the report ({e}). Check that your Downloads folder exists and has space, then try again."), None),
                });
                row.buttons(vec![btn(if kept { "Save report" } else { "Report deleted" }).icon("download-simple").off(!kept)], act)
            }
            Offer::OtherGpu if other_gpu.is_some() => {
                let act = cx.on(|cx, _| {
                    if !tessera_core::gpu::use_faster() {
                        cx.toast("Couldn't switch. Choose the GPU in Battery & Power → Cool-down and performance.", None);
                    }
                });
                row.buttons(vec![btn("Use the other GPU").icon("arrow-clockwise")], act)
            }
            // (one GPU here, or it isn't known which is faster: where it's chosen)
            Offer::OtherGpu => open_button(cx, row, ("power/cooldown", "Performance")),
            Offer::Open(page, target) => open_button(cx, row, (page, target)),
            Offer::None => row,
        };
        g.row(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Vec<Value> {
        health::parse_lines(s)
    }

    fn st(events: &[Value], samples: &[Sample], now: i64) -> (String, String, bool) {
        let t = status(events, samples, now);
        (t.label, t.detail, t.ok)
    }

    #[test]
    fn slow_moments_say_this_run_from_earlier_and_memory_pressure() {
        let now = tessera_core::util::now_ms() / 1000;
        let up = now - 3600;
        let start = ev(&format!("{{\"t\":{},\"ev\":\"start\",\"why\":\"normal\"}}\n{{\"t\":{up},\"ev\":\"start\",\"why\":\"normal\"}}", now - 20000));
        // (367 before this start, all while memory was short; 3 since)
        let mut s: Vec<Sample> = (0..10).map(|i| Sample { t: up - 10000 + i * 300, d: 300, s33: 36, worst: 470, pressure: 40, ..Default::default() }).collect();
        s.push(Sample { t: up - 300, d: 300, s33: 7, pressure: 30, ..Default::default() });
        s.push(Sample { t: up + 600, d: 300, s33: 3, ..Default::default() });
        let top = status(&start, &s, now);
        assert_eq!(top.label, "Some slow moments, before this start");
        assert!(top.detail.contains("370 times") && top.detail.contains("3 since it started") && top.detail.contains("367 before that"), "{}", top.detail);
        assert!(top.detail.contains("short of memory"), "{}", top.detail);
        assert!(top.says_start && !top.ok);
        assert_eq!(top.steps[0], SHORT_OF_MEMORY[0]);
        assert_eq!(top.open, Some(BUG));
        // (the bucket the start fell in is this run's)
        let straddle = vec![Sample { t: up - 100, d: 400, s33: 25, ..Default::default() }];
        let top = status(&start, &straddle, now);
        assert!(!top.says_start && top.label == "Some slow moments today" && !top.detail.contains("short of memory"), "{}", top.detail);
        assert_eq!(top.steps[0], SLOW[0]);
    }

    #[test]
    fn every_warning_has_a_remedy() {
        let now = tessera_core::util::now_ms() / 1000;
        for line in [
            "{\"t\":T,\"ev\":\"start\",\"why\":\"crash\"}",
            "{\"t\":T,\"ev\":\"fallback\"}",
            "{\"t\":T,\"ev\":\"off\",\"part\":\"dock\"}",
            "{\"t\":T,\"ev\":\"crash\",\"part\":\"dock\"}",
            "{\"t\":T,\"ev\":\"update\",\"what\":\"system\",\"ok\":false}",
        ] {
            let e = ev(&line.replace('T', &now.to_string()));
            let top = status(&e, &[], now);
            assert!(!top.ok && !top.steps.is_empty() && top.open.is_some(), "{line}");
            // (the event itself: a button, a page, or steps)
            let (_, d, o) = explain(&e[0]).unwrap();
            assert!(!matches!(o, Offer::None) || d.contains("\n1. "), "{line}");
        }
        for line in ["{\"t\":1,\"ev\":\"start\",\"why\":\"stopped\"}", "{\"t\":1,\"ev\":\"start\",\"why\":\"watchdog\"}", "{\"t\":1,\"ev\":\"gpu-fallback\",\"from\":\"NVIDIA\"}"] {
            let (_, d, o) = explain(&ev(line)[0]).unwrap();
            assert!(matches!(o, Offer::Open(..)) && d.contains("\n1. "), "{line}");
        }
        // (a crash without its report: Report a Bug)
        assert!(matches!(explain(&ev("{\"t\":1,\"ev\":\"crash\",\"part\":\"dock\"}")[0]).unwrap().2, Offer::Open("about/bugreport", "")));
        let pages = [BUG.0, "update", "power/cooldown"];
        assert!(pages.iter().all(|p| super::super::super::about::health::page_name(p).is_some()));
    }

    #[test]
    fn the_top_line_says_how_it_is_going() {
        let now = tessera_core::util::now_ms() / 1000;
        let (l, d, ok) = st(&[], &[], now);
        assert!(ok && l == "Running smoothly" && d.contains("No crashes"), "{l} {d}");
        let two = ev(&format!("{{\"t\":{now},\"ev\":\"start\",\"why\":\"crash\"}}\n{{\"t\":{now},\"ev\":\"start\",\"why\":\"watchdog\"}}\n{{\"t\":{now},\"ev\":\"start\",\"why\":\"normal\"}}"));
        let (l, _, ok) = st(&two, &[], now);
        assert_eq!((l.as_str(), ok), ("2 restarts today", false));
        let part = ev(&format!("{{\"t\":{now},\"ev\":\"crash\",\"part\":\"dock\"}}"));
        assert_eq!(st(&part, &[], now).0, "1 part recovered today");
        let off = ev(&format!("{{\"t\":{now},\"ev\":\"off\",\"part\":\"dock\"}}"));
        assert_eq!(st(&off, &[], now).0, "The dock was turned off today");
        let slow = vec![Sample { t: now - 600, s33: 25, worst: 140, ..Default::default() }];
        assert_eq!(st(&[], &slow, now).0, "Some slow moments today");
        // (a crash last week, nothing today: smooth, and it says so)
        let old = ev(&format!("{{\"t\":{},\"ev\":\"start\",\"why\":\"crash\"}}", now - 3 * 86400));
        let (l, d, ok) = st(&old, &[], now);
        assert!(ok && l == "Running smoothly" && d.contains("1 problem in the last 7 days"), "{d}");
    }

    #[test]
    fn the_top_line_never_says_smoothly_while_services_need_attention() {
        let smooth = ("Running smoothly".to_string(), "No crashes or restarts in the last 7 days.".to_string(), true);
        assert_eq!(with_services(smooth.clone(), &[]), smooth);
        let (l, d, ok) = with_services(smooth.clone(), &["Glass plugin".into()]);
        assert_eq!((l.as_str(), ok), ("1 thing needs attention", false));
        assert!(d.starts_with("Glass plugin."), "{d}");
        let (l, _, _) = with_services(smooth, &["Avahi".into(), "Portal".into()]);
        assert_eq!(l, "2 things need attention");
        // (a stronger message of the record's own stays, the services added)
        let slow = ("Some slow moments today".to_string(), "Tessera paused.".to_string(), false);
        let (l, d, ok) = with_services(slow, &["Avahi".into()]);
        assert_eq!((l.as_str(), ok), ("Some slow moments today", false));
        assert!(d.contains("Also: 1 thing needs attention (Avahi)."), "{d}");
        let checks = json!([{"name": "Avahi", "status": "fail"}, {"name": "Glass plugin", "status": "ok"}, {"name": "Bluetooth", "status": "off"}, {"name": "Portal", "status": "warn"}]);
        assert_eq!(attention(&checks), vec!["Avahi".to_string(), "Portal".to_string()]);
        assert!(attention(&Value::Null).is_empty());
    }

    #[test]
    fn events_in_plain_words() {
        let e = ev("{\"t\":1,\"ev\":\"crash\",\"part\":\"dock\",\"report\":\"20261116-140233-dock.txt\"}\n{\"t\":2,\"ev\":\"start\",\"why\":\"normal\"}\n{\"t\":3,\"ev\":\"stop\"}\n{\"t\":4,\"ev\":\"gpu-slow\",\"share\":0.18,\"on\":\"Intel\",\"to\":\"NVIDIA\"}\n{\"t\":5,\"ev\":\"crash\",\"part\":\"shell\",\"fatal\":true}");
        let (t, _, o) = explain(&e[0]).unwrap();
        assert_eq!(t, "The dock stopped and recovered");
        assert!(matches!(o, Offer::Report(ref r) if r == "20261116-140233-dock.txt"));
        // (ordinary starts and stops, and the loop's own crash — its restart says it — aren't listed)
        assert!(explain(&e[1]).is_none() && explain(&e[2]).is_none() && explain(&e[4]).is_none());
        let (t, d, o) = explain(&e[3]).unwrap();
        assert_eq!(t, "Animations were slow on the Intel graphics");
        assert!(d.starts_with("18% of animation frames"));
        assert!(matches!(o, Offer::OtherGpu));
    }

    #[test]
    fn failed_updates_are_listed_and_said_at_the_top() {
        let now = tessera_core::util::now_ms() / 1000;
        let e = ev(&format!("{{\"t\":{},\"ev\":\"update\",\"what\":\"system\",\"ok\":false,\"why\":\"pacman: a conflict\",\"step\":\"system\"}}\n{{\"t\":{now},\"ev\":\"update\",\"what\":\"tessera\",\"ok\":true}}", now - 60));
        let (t, d, _) = explain(&e[0]).unwrap();
        assert_eq!(t, "The system update didn't finish");
        assert_eq!(d, "It stopped: pacman: a conflict");
        // (one that worked isn't listed)
        assert!(explain(&e[1]).is_none());
        let (l, _, ok) = st(&e, &[], now);
        assert_eq!((l.as_str(), ok), ("The system update didn't finish today", false));
        // (tried again and it worked: all well)
        let mut again = e.clone();
        again.push(json!({"t": now, "ev": "update", "what": "system", "ok": true}));
        assert!(st(&again, &[], now).2);
        let staged = ev(&format!("{{\"t\":{now},\"ev\":\"update\",\"what\":\"tessera\",\"ok\":false,\"step\":\"stage\"}}"));
        assert_eq!(explain(&staged[0]).unwrap().1, "It stopped while it was being made ready. Try again from Software Update.");
    }

    #[test]
    fn the_stability_summary() {
        let now = tessera_core::util::now_ms() / 1000;
        let e = ev(&format!(
            "{{\"t\":{},\"ev\":\"start\",\"why\":\"crash\"}}\n{{\"t\":{},\"ev\":\"start\",\"why\":\"stopped\"}}\n{{\"t\":{},\"ev\":\"fallback\",\"why\":\"x\"}}\n{{\"t\":{},\"ev\":\"start\",\"why\":\"normal\"}}",
            now - 20 * 86400,
            now - 3 * 86400,
            now - 3 * 86400 + 60,
            now - 100
        ));
        let s = health::stability(&e, now);
        let (rows, footer) = stability_rows(&s, now);
        assert_eq!(rows[0].0, "Days since the last crash");
        assert_eq!(rows[0].1, "3");
        assert!(rows[0].2.starts_with("The last one: "));
        assert_eq!(rows[1].1, "1 in 7 days · 2 in 30 days");
        assert_eq!(rows[2].1, "1 in 7 days · 1 in 30 days");
        assert!(footer.contains("The record covers 20 days so far."), "{footer}");
        // (no crash, a record a month long)
        let quiet = ev(&format!("{{\"t\":{},\"ev\":\"start\",\"why\":\"normal\"}}", now - 35 * 86400));
        let (rows, footer) = stability_rows(&health::stability(&quiet, now), now);
        assert_eq!((rows[0].1.as_str(), rows[1].1.as_str()), ("No crash recorded", "0 in 7 days · 0 in 30 days"));
        assert!(!footer.contains("covers"), "{footer}");
        let (_, footer) = stability_rows(&health::stability(&[], now), now);
        assert!(footer.ends_with("There's no record yet."));
    }

    #[test]
    fn the_reader_setup_step_shows_until_its_rule_is_installed() {
        let old = "<policyconfig><action id=\"org.tessera.root.fix\"/></policyconfig>";
        let new = include_str!("../../../../../../../shell/assets/polkit/org.tessera.root.policy");
        assert!(reader_setup_pending(true, None));
        assert!(reader_setup_pending(true, Some(old)));
        assert!(!reader_setup_pending(true, Some(new)), "the shipped policy carries the note");
        assert!(!reader_setup_pending(false, None), "no fprintd: nothing to set up");
    }

    #[test]
    fn numbers_read_well() {
        assert_eq!(mb(145_000), "142 MB");
        assert_eq!(mb(3 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(pct(0.84), "0.8%");
        assert_eq!(pct(12.4), "12%");
        assert_eq!(nice_top(0.0, 5.0), 5.0);
        assert_eq!(nice_top(7.0, 5.0), 10.0);
        assert_eq!(nice_top(130_000.0, 1024.0), 200_000.0);
        assert_eq!(nice_top(0.3, 1.0), 1.0);
        let now = tessera_core::util::now_ms() / 1000;
        assert!(when(now, now).starts_with("Today "));
    }
}
