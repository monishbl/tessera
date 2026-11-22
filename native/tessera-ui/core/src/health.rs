//! The health record: what Settings → Health shows, so problems are seen,
//! not remembered. Local only: nothing here is ever sent.
//!
//! `$XDG_STATE_HOME/tessera/health.jsonl`, one JSON object a line:
//!
//! - **Samples**, one per 5 minutes on the wall clock (`BUCKET_SECS`):
//!   `{"t":<bucket start>,"d":<seconds covered>,"rss":<KiB>,"pss":<KiB>,"cpu":<ms>}`
//!   and, only when not zero, `s16`/`s33` (loop turns that held the UI
//!   thread over 16 / 33 ms), `w` (the worst, ms), `fr`/`mf` (animation
//!   frames on screen / missed, when wp_presentation was asked: the wake
//!   log or the slow-GPU watch) and `c` (parts that panicked and were made
//!   again), `mp` (the share of the bucket, in %, that the computer was
//!   short of memory: some task waited for it, /proc/pressure/memory's
//!   `some total`; so Health can tell Tessera's own slow moments from those
//!   while a big build or many browser tabs filled the memory). Memory is
//!   read from /proc/self/smaps_rollup, CPU time from getrusage (the whole
//!   process, its threads too).
//! - **Events**, when they happen (rare): `{"t":…,"ev":"start","why":…}`
//!   (why: normal, crash, stopped, watchdog, retry; and the GPU),
//!   `stop` (an orderly end), `crash` (a part's panic, or the loop's:
//!   `fatal`, with the report's file name), `off` (a part turned off after
//!   3 crashes), `lock` (the screen locked again after a restart),
//!   `gpu-slow`; and from the scripts: `watchdog` (tty-shell.sh: it
//!   restarted a shell that stopped answering), `fallback`
//!   (launch-shell.sh: the QML shell started instead), `update`
//!   (shell/assets/scripts/health_event.py: sysupdate.py, tessera-update.py,
//!   native-install.py and update-run.sh, `{"what": system|tessera|native,
//!   "ok": bool, "why", "step"}`; appended with one O_APPEND write).
//!
//! Cheap by design: the loop turns are timed with two clock reads (the
//! poll's return, a calloop source with no file of its own: `Woke`, and the
//! turn's end), no timer of its own (a bucket is closed by the first turn
//! after its end, whatever woke the loop), the memory read and the write go
//! to the work pool. The file is a ring: past `CAP_BYTES` it keeps its
//! newest `KEEP_BYTES` (a week of samples is ~110 KB): samples no older
//! than `KEEP_DAYS`, events (rare, short) no older than `EVENT_DAYS`, so
//! the stability summary can count a month.

use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A sample's length (s)
pub const BUCKET_SECS: i64 = 300;
/// The file's largest size…
pub const CAP_BYTES: u64 = 200 * 1024;
/// …and what's kept when it grows past it
pub const KEEP_BYTES: usize = 150 * 1024;
/// No sample older is kept (days)
pub const KEEP_DAYS: i64 = 8;
/// No event older is kept (days): the stability summary counts 30
pub const EVENT_DAYS: i64 = 31;
/// What the events may take of `KEEP_BYTES`
pub const EVENT_BYTES: usize = 32 * 1024;
/// A loop turn longer than this is a stall (ms): a 60 Hz frame
pub const STALL_MS: f64 = 16.0;
/// …and this, a long one: two frames
pub const LONG_STALL_MS: f64 = 33.0;
/// A bucket whose memory pressure (`mp`, %) is this or more: the computer
/// was short of memory then
pub const PRESSURE_PCT: u64 = 5;

/// Where the record is
pub fn path() -> PathBuf {
    crate::util::xdg("XDG_STATE_HOME", ".local/state").join("tessera/health.jsonl")
}

/// What one bucket counted so far (the UI thread's)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counts {
    pub s16: u32,
    pub s33: u32,
    pub worst_ms: f64,
    pub frames: u32,
    pub missed: u32,
    pub crashes: u32,
    /// memory pressure over the bucket (%), filled in as it's written
    pub pressure: u32,
}

impl Counts {
    /// One loop turn took `ms`
    pub fn turn(&mut self, ms: f64) {
        if ms > STALL_MS {
            self.s16 += 1;
            if ms > LONG_STALL_MS {
                self.s33 += 1;
            }
            self.worst_ms = self.worst_ms.max(ms);
        }
    }
}

thread_local! {
    /// recording (the shell's loop, standalone: `start`)
    static ON: Cell<bool> = const { Cell::new(false) };
    /// when the loop's poll last returned (Woke)
    static WOKE: Cell<Option<Instant>> = const { Cell::new(None) };
    static NOW: RefCell<Counts> = RefCell::new(Counts::default());
    /// the bucket's wall-clock start (s) and when it ends (the loop's clock)
    static BUCKET: Cell<(i64, Option<Instant>)> = const { Cell::new((0, None)) };
    /// the process's CPU time (µs) at the bucket's start
    static CPU_AT: Cell<i64> = const { Cell::new(0) };
    /// the memory pressure's `some total` (µs) at the bucket's start
    static PSI_AT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Recording on this thread (the shell's loop): the start event (its reason
/// worked out from the record's end, off this thread) and the first bucket
pub fn start() {
    ON.with(|o| o.set(true));
    ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    begin_bucket(crate::util::now_ms() / 1000);
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default();
    let gpu = gpu_now();
    crate::work::spawn(move || {
        let path = path();
        let tail = read_tail(&path, 16 * 1024);
        let (why, report) = start_reason(&parse_lines(&tail), &sig);
        let mut e = json!({"t": crate::util::now_ms() / 1000, "ev": "start", "why": why, "s": sig});
        if !gpu.is_empty() {
            e["gpu"] = json!(gpu);
        }
        if let Some(r) = report {
            e["report"] = json!(r);
        }
        append(&path, &e.to_string());
    });
}

/// Recording, for events from any thread
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Is it recording (on this thread)?
pub fn on() -> bool {
    ON.with(Cell::get)
}

/// What the shell draws on, in a few words ("Intel", or the renderer's name)
fn gpu_now() -> String {
    if let Some(f) = crate::gpu::drawing_family() {
        return crate::gpu::family_label(f).to_string();
    }
    let r = crate::egl::renderer();
    r.chars().take(60).collect()
}

fn begin_bucket(now_secs: i64) {
    let start = now_secs - now_secs.rem_euclid(BUCKET_SECS);
    let left = (start + BUCKET_SECS - now_secs).max(1) as u64;
    BUCKET.with(|b| b.set((start, Some(Instant::now() + Duration::from_secs(left)))));
    CPU_AT.with(|c| c.set(cpu_us()));
    PSI_AT.with(|c| c.set(memory_stall_us()));
}

/// The time some task waited for memory since boot (µs; None: no PSI here)
pub fn memory_stall_us() -> Option<u64> {
    psi_some_total(&std::fs::read_to_string("/proc/pressure/memory").ok()?)
}

/// `some … total=N` of a /proc/pressure file
pub fn psi_some_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("some "))?;
    line.split_whitespace().find_map(|w| w.strip_prefix("total=")).and_then(|n| n.parse().ok())
}

/// The share of `secs` (%, rounded) spent waiting for memory, from two
/// `some total`s (0 when either is missing)
pub fn pressure_pct(at: Option<u64>, now: Option<u64>, secs: i64) -> u32 {
    match (at, now) {
        (Some(a), Some(n)) if n >= a && secs > 0 => ((n - a) as f64 / (secs as f64 * 10_000.0)).round().min(100.0) as u32,
        _ => 0,
    }
}

/// The process's processor time so far (µs, every thread)
fn cpu_us() -> i64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return 0;
    }
    let us = |t: libc::timeval| t.tv_sec * 1_000_000 + t.tv_usec;
    us(ru.ru_utime) + us(ru.ru_stime)
}

/// The loop's poll returned (Woke's before_handle_events)
#[inline]
pub fn woke() {
    if on() {
        WOKE.with(|w| w.set(Some(Instant::now())));
    }
}

/// The loop's turn ended: counted when it held the thread over a frame;
/// the bucket closed when its time is up
#[inline]
pub fn turn_end() {
    let Some(w) = WOKE.with(Cell::take) else { return };
    let now = Instant::now();
    let ms = now.duration_since(w).as_secs_f64() * 1000.0;
    if ms > STALL_MS {
        NOW.with(|c| c.borrow_mut().turn(ms));
    }
    if BUCKET.with(|b| b.get().1.is_some_and(|end| now >= end)) {
        close_bucket(false);
    }
}

/// wp_presentation: animation frames on screen, and how many were missed
pub fn frames(shown: u32, missed: u32) {
    if on() {
        NOW.with(|c| {
            let mut c = c.borrow_mut();
            c.frames += shown;
            c.missed += missed;
        });
    }
}

/// A part panicked (and is made again: contain.rs)
pub fn part_crashed() {
    if on() {
        NOW.with(|c| c.borrow_mut().crashes += 1);
    }
}

/// The bucket written (its memory read off this thread; `now` at the end:
/// on this one), the next begun
fn close_bucket(now: bool) {
    let (start, _) = BUCKET.with(Cell::get);
    let counts = NOW.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let cpu_ms = (cpu_us() - CPU_AT.with(Cell::get)).max(0) / 1000;
    let psi_at = PSI_AT.with(Cell::get);
    let end = crate::util::now_ms() / 1000;
    begin_bucket(end);
    let job = move || {
        let mut counts = counts;
        counts.pressure = pressure_pct(psi_at, memory_stall_us(), (end - start).max(1));
        let (rss, pss) = memory();
        append(&path(), &sample_line(start, (end - start).max(1), rss, pss, cpu_ms, &counts));
    };
    if now {
        job();
    } else {
        crate::work::spawn(job);
    }
}

/// The shell ends in order: the bucket so far and a `stop` (on this thread)
pub fn stop() {
    if !on() {
        return;
    }
    close_bucket(true);
    append(&path(), &json!({"t": crate::util::now_ms() / 1000, "ev": "stop"}).to_string());
    ON.with(|o| o.set(false));
    ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// One sample's line
pub fn sample_line(t: i64, d: i64, rss: u64, pss: u64, cpu_ms: i64, c: &Counts) -> String {
    let mut s = format!("{{\"t\":{t},\"d\":{d},\"rss\":{rss},\"pss\":{pss},\"cpu\":{cpu_ms}");
    for (k, v) in [("s16", c.s16), ("s33", c.s33), ("w", c.worst_ms.round() as u32), ("fr", c.frames), ("mf", c.missed), ("c", c.crashes), ("mp", c.pressure)] {
        if v > 0 {
            s.push_str(&format!(",\"{k}\":{v}"));
        }
    }
    s.push('}');
    s
}

/// This process's memory (KiB): resident, and its fair share of what's
/// shared (proportional)
pub fn memory() -> (u64, u64) {
    let t = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
    rollup(&t)
}

/// Rss and Pss of a smaps_rollup text (KiB)
pub fn rollup(text: &str) -> (u64, u64) {
    let field = |name: &str| {
        text.lines().find_map(|l| l.strip_prefix(name)).and_then(|r| r.trim().trim_end_matches("kB").trim().parse::<u64>().ok()).unwrap_or(0)
    };
    (field("Rss:"), field("Pss:"))
}

/// An event, written now (on the work pool: any thread may call it)
pub fn event(kind: &str, mut fields: Value) {
    if !ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if !fields.is_object() {
        fields = json!({});
    }
    fields["t"] = json!(crate::util::now_ms() / 1000);
    fields["ev"] = json!(kind);
    let line = fields.to_string();
    crate::work::spawn(move || append(&path(), &line));
}

/// The same, written before this returns (a fatal panic: the process ends next)
pub fn event_now(kind: &str, mut fields: Value) {
    fields["t"] = json!(crate::util::now_ms() / 1000);
    fields["ev"] = json!(kind);
    append(&path(), &fields.to_string());
}

static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A line added; past the cap the file is cut to its newest lines
pub fn append(path: &Path, line: &str) {
    let _g = WRITE.lock();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    let _ = f.write_all(format!("{line}\n").as_bytes());
    if f.metadata().is_ok_and(|m| m.len() > CAP_BYTES) {
        drop(f);
        trim(path, crate::util::now_ms() / 1000);
    }
}

/// Cut to the newest `KEEP_BYTES` of whole lines: samples none older than
/// `KEEP_DAYS`, events none older than `EVENT_DAYS`
fn trim(path: &Path, now_secs: i64) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let kept = ring_events(&text, KEEP_BYTES, now_secs - KEEP_DAYS * 86400, now_secs - EVENT_DAYS * 86400);
    let tmp = path.with_extension("jsonl.tmp");
    if std::fs::write(&tmp, kept).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// The newest whole lines of `text` within `keep` bytes, none older than `since`
pub fn ring(text: &str, keep: usize, since: i64) -> String {
    ring_events(text, keep, since, since)
}

/// The same, with the events kept longer: the newest of them, none older
/// than `events_since`, within `EVENT_BYTES` (or `keep`, if smaller), then
/// the newest samples, none older than `since`, in what's left. In the
/// file's order
pub fn ring_events(text: &str, keep: usize, since: i64, events_since: i64) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let is_event = |l: &str| l.contains("\"ev\":");
    let mut kept = vec![false; lines.len()];
    let mut size = 0;
    let budget = EVENT_BYTES.min(keep);
    for (i, l) in lines.iter().enumerate().rev() {
        if !is_event(l) || line_time(l).is_some_and(|t| t < events_since) {
            continue;
        }
        if size + l.len() + 1 > budget {
            break;
        }
        size += l.len() + 1;
        kept[i] = true;
    }
    for (i, l) in lines.iter().enumerate().rev() {
        if is_event(l) {
            continue;
        }
        if size + l.len() + 1 > keep {
            break;
        }
        // (an old line: those before it are older still)
        if line_time(l).is_some_and(|t| t < since) {
            break;
        }
        size += l.len() + 1;
        kept[i] = true;
    }
    let mut s = lines.iter().zip(&kept).filter(|(_, k)| **k).map(|(l, _)| *l).collect::<Vec<_>>().join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

fn line_time(l: &str) -> Option<i64> {
    let i = l.find("\"t\":")? + 4;
    let rest = &l[i..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The file's last `n` bytes, from its first whole line
fn read_tail(path: &Path, n: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map_or(0, |m| m.len());
    let from = len.saturating_sub(n);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    if from > 0 {
        text.split_once('\n').map(|x| x.1.to_string()).unwrap_or_default()
    } else {
        text
    }
}

/// The lines that are JSON objects
pub fn parse_lines(text: &str) -> Vec<Value> {
    text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(Value::is_object).collect()
}

/// Why this start: what the record says after the last start (`sig`: this
/// Hyprland session's). normal (the last run ended in order, or with its
/// session), crash (the loop panicked: its report), watchdog (it stopped
/// answering and was restarted), retry (the QML shell stood in since),
/// stopped (it ended without a word: killed, out of memory, a crash in a
/// library)
pub fn start_reason(lines: &[Value], sig: &str) -> (&'static str, Option<String>) {
    let Some(last) = lines.iter().rposition(|v| v["ev"] == "start") else { return ("normal", None) };
    let after = &lines[last + 1..];
    let has = |ev: &str| after.iter().any(|v| v["ev"] == ev);
    if has("watchdog") {
        return ("watchdog", None);
    }
    if has("fallback") {
        return ("retry", None);
    }
    if let Some(c) = after.iter().rev().find(|v| v["ev"] == "crash" && v["fatal"] == true) {
        return ("crash", c["report"].as_str().map(String::from));
    }
    if has("stop") {
        return ("normal", None);
    }
    // (a run of another session that never said stop: the computer was
    // turned off or the session ended under it, not a crash)
    let before = lines[last]["s"].as_str().unwrap_or("");
    if before != sig {
        return ("normal", None);
    }
    ("stopped", None)
}

// ── Reading it back (Settings → Health) ─────────────────────────────────

/// One sample read back
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub t: i64,
    pub d: i64,
    /// KiB
    pub rss: u64,
    pub pss: u64,
    pub cpu_ms: u64,
    pub s16: u64,
    pub s33: u64,
    pub worst: u64,
    pub frames: u64,
    pub missed: u64,
    pub crashes: u64,
    /// memory pressure (%): `mp`
    pub pressure: u64,
}

impl Sample {
    pub fn from(v: &Value) -> Option<Sample> {
        if !v["ev"].is_null() {
            return None;
        }
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        Some(Sample {
            t: v["t"].as_i64()?,
            d: v["d"].as_i64().unwrap_or(BUCKET_SECS).max(1),
            rss: n("rss"),
            pss: n("pss"),
            cpu_ms: n("cpu"),
            s16: n("s16"),
            s33: n("s33"),
            worst: n("w"),
            frames: n("fr"),
            missed: n("mf"),
            crashes: n("c"),
            pressure: n("mp"),
        })
    }
    /// Memory (KiB): the fair share when it was read, else resident
    pub fn mem(&self) -> u64 {
        if self.pss > 0 { self.pss } else { self.rss }
    }
}

/// The record read: its samples and its events, oldest first
pub fn read_all() -> (Vec<Sample>, Vec<Value>) {
    split(&parse_lines(&std::fs::read_to_string(path()).unwrap_or_default()))
}

pub fn split(lines: &[Value]) -> (Vec<Sample>, Vec<Value>) {
    let mut samples: Vec<Sample> = lines.iter().filter_map(Sample::from).collect();
    let mut events: Vec<Value> = lines.iter().filter(|v| v["ev"].is_string() && v["t"].is_i64()).cloned().collect();
    samples.sort_by_key(|s| s.t);
    events.sort_by_key(|v| v["t"].as_i64().unwrap_or(0));
    (samples, events)
}

/// The bucket still being counted (the UI thread's), as a sample with no
/// memory: Settings shows it with the memory it reads itself
pub fn live() -> Option<Sample> {
    if !on() {
        return None;
    }
    let (start, _) = BUCKET.with(Cell::get);
    let c = NOW.with(|c| c.borrow().clone());
    let now = crate::util::now_ms() / 1000;
    Some(Sample {
        t: start,
        d: (now - start).max(1),
        cpu_ms: ((cpu_us() - CPU_AT.with(Cell::get)).max(0) / 1000) as u64,
        s16: c.s16 as u64,
        s33: c.s33 as u64,
        worst: c.worst_ms.round() as u64,
        frames: c.frames as u64,
        missed: c.missed as u64,
        crashes: c.crashes as u64,
        pressure: pressure_pct(PSI_AT.with(Cell::get), memory_stall_us(), (now - start).max(1)) as u64,
        ..Default::default()
    })
}

// ── Stability and alerts (Settings → Health, the shell's watch) ─────────

/// A start that wasn't asked for: the shell before it ended badly (an
/// error, killed, out of memory, or it stopped answering)
pub fn is_restart(e: &Value) -> bool {
    e["ev"] == "start" && matches!(e["why"].as_str(), Some("crash" | "stopped" | "watchdog"))
}

/// The Rust shell's record over the last month, for deciding when the QML
/// fallback can go
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stability {
    /// the last crash (a restart that wasn't asked for), if the record has one
    pub last_crash: Option<i64>,
    pub crashes_7: usize,
    pub crashes_30: usize,
    pub fallbacks_7: usize,
    pub fallbacks_30: usize,
    /// the record's first event (it counts from there)
    pub since: Option<i64>,
}

impl Stability {
    /// Whole days since the last crash (None: none in the record)
    pub fn days_since_crash(&self, now: i64) -> Option<i64> {
        self.last_crash.map(|t| (now - t).max(0) / 86400)
    }
    /// Whole days the record covers
    pub fn days_recorded(&self, now: i64) -> i64 {
        self.since.map_or(0, |t| (now - t).max(0) / 86400)
    }
}

pub fn stability(events: &[Value], now: i64) -> Stability {
    let t = |e: &Value| e["t"].as_i64().unwrap_or(0);
    let within = |e: &Value, days: i64| t(e) <= now && now - t(e) < days * 86400;
    let crashes: Vec<&Value> = events.iter().filter(|e| is_restart(e)).collect();
    let fallbacks: Vec<&Value> = events.iter().filter(|e| e["ev"] == "fallback").collect();
    Stability {
        last_crash: crashes.iter().map(|e| t(e)).max(),
        crashes_7: crashes.iter().filter(|e| within(e, 7)).count(),
        crashes_30: crashes.iter().filter(|e| within(e, 30)).count(),
        fallbacks_7: fallbacks.iter().filter(|e| within(e, 7)).count(),
        fallbacks_30: fallbacks.iter().filter(|e| within(e, 30)).count(),
        since: events.iter().map(t).filter(|t| *t > 0).min(),
    }
}

/// The updates of one kind (system, tessera, native) that failed one after
/// another, up to the last: their times, oldest first (empty: the last one
/// worked, or there's none)
pub fn failing_run(events: &[Value], what: &str) -> Vec<i64> {
    let mut run = Vec::new();
    for e in events.iter().rev().filter(|e| e["ev"] == "update" && e["what"].as_str() == Some(what)) {
        if e["ok"] == true {
            break;
        }
        run.push(e["t"].as_i64().unwrap_or(0));
    }
    run.reverse();
    run
}

/// The update kinds, as the scripts write them, and their names
pub const UPDATE_KINDS: &[(&str, &str)] = &[("system", "System updates"), ("tessera", "Tessera's updates"), ("native", "Installing Tessera's programs")];

/// What each alert kind was last raised for (the incident's time), so the
/// same incident never alerts twice
pub fn alerts_path() -> PathBuf {
    crate::util::xdg("XDG_STATE_HOME", ".local/state").join("tessera/health-alerts.json")
}

/// The alerts due now: Tessera crashed 2+ times in 24 hours, it fell back
/// to the QML shell, an update failed twice in a row. Each is one live
/// entry (its key `health:<kind>`), raised once per incident: `seen` holds
/// the incident each kind last alerted for, and is updated. An incident:
/// two crashes newer than the last one alerted, within 24 hours; a
/// fallback newer than the last one alerted, within 24 hours; a run of
/// failed updates of one kind that started after the last one alerted, its
/// latest within 24 hours. Island events (`island.post`); their button
/// opens Health
pub fn due_alerts(events: &[Value], now: i64, seen: &mut Value) -> Vec<Value> {
    if !seen.is_object() {
        *seen = json!({});
    }
    let day_ago = now - 86400;
    let t = |e: &Value| e["t"].as_i64().unwrap_or(0);
    let mut out = Vec::new();
    let post = |kind: &str, title: String, subtitle: String| {
        json!({"kind": "custom", "key": format!("health:{kind}"), "icon": "warning-circle", "title": title, "subtitle": subtitle,
               "color": "#FF453A", "severity": 2, "failure": true, "action": "Open Health",
               "onAction": {"call": "panels.openSettings", "args": ["health"]}})
    };
    // Crashes
    let last = seen["crashes"].as_i64().unwrap_or(0);
    let fresh: Vec<i64> = events.iter().filter(|e| is_restart(e)).map(t).filter(|x| *x > day_ago.max(last) && *x <= now).collect();
    if fresh.len() >= 2 {
        let all = events.iter().filter(|e| is_restart(e)).map(t).filter(|x| *x > day_ago && *x <= now).count();
        out.push(post("crashes", format!("Tessera stopped {all} times today"), "It was started again each time. Health says when and why.".into()));
        seen["crashes"] = json!(fresh.iter().max().copied().unwrap_or(now));
    }
    // A fallback to the basic (QML) shell
    let last = seen["fallback"].as_i64().unwrap_or(0);
    if let Some(f) = events.iter().filter(|e| e["ev"] == "fallback" && t(e) > day_ago.max(last) && t(e) <= now).last() {
        let why = f["why"].as_str().unwrap_or("").trim().to_string();
        out.push(post("fallback", "Tessera switched to the basic shell".into(), if why.is_empty() { "It stopped twice in a row. Health has what happened.".to_string() } else { why.chars().take(120).collect() }));
        seen["fallback"] = json!(t(f));
    }
    // Updates that failed twice in a row
    for (what, name) in UPDATE_KINDS {
        let run = failing_run(events, what);
        let key = format!("update:{what}");
        let last = seen[&key].as_i64().unwrap_or(0);
        if run.len() >= 2 && run[0] > last && run.last().is_some_and(|x| *x > day_ago) {
            let why = events.iter().rev().find(|e| e["ev"] == "update" && e["what"].as_str() == Some(what)).and_then(|e| e["why"].as_str()).unwrap_or("").trim().to_string();
            out.push(post(&format!("update-{what}"), format!("{name} failed twice in a row"), if why.is_empty() { "Health lists each try.".to_string() } else { why.chars().take(120).collect() }));
            seen[&key] = json!(run[0]);
        }
    }
    out
}

/// due_alerts over the record on disk, the seen file read and written back
/// (blocking: call it on a thread)
pub fn check_alerts(now: i64) -> Vec<Value> {
    let (_, events) = read_all();
    let p = alerts_path();
    let mut seen: Value = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let before = seen.clone();
    let out = due_alerts(&events, now, &mut seen);
    if seen != before {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, seen.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
    out
}

/// One slot of a chart
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub start: i64,
    /// the samples in it
    pub n: u32,
    /// memory: the average and the highest (KiB)
    pub mem_avg: u64,
    pub mem_max: u64,
    /// processor: % of one core over the time recorded
    pub cpu_pct: f64,
    /// stalls over 16 ms, per hour recorded
    pub stalls_per_hour: f64,
    pub stalls: u64,
    pub long_stalls: u64,
    pub worst: u64,
}

/// The samples from `from` laid in `count` slots of `secs` each
pub fn slots(samples: &[Sample], from: i64, secs: i64, count: usize) -> Vec<Slot> {
    let mut out: Vec<Slot> = (0..count).map(|i| Slot { start: from + i as i64 * secs, ..Default::default() }).collect();
    let mut acc: Vec<(u64, u64, i64, u64)> = vec![(0, 0, 0, 0); count]; // mem sum, mem samples, seconds, cpu ms
    for s in samples {
        if s.t < from {
            continue;
        }
        let i = ((s.t - from) / secs) as usize;
        let Some(o) = out.get_mut(i) else { continue };
        o.n += 1;
        let m = s.mem();
        if m > 0 {
            acc[i].0 += m;
            acc[i].1 += 1;
            o.mem_max = o.mem_max.max(m);
        }
        acc[i].2 += s.d;
        acc[i].3 += s.cpu_ms;
        o.stalls += s.s16;
        o.long_stalls += s.s33;
        o.worst = o.worst.max(s.worst);
    }
    for (o, a) in out.iter_mut().zip(acc) {
        if a.1 > 0 {
            o.mem_avg = a.0 / a.1;
        }
        if a.2 > 0 {
            o.cpu_pct = a.3 as f64 / (a.2 as f64 * 1000.0) * 100.0;
            o.stalls_per_hour = o.stalls as f64 * 3600.0 / a.2 as f64;
        }
    }
    out
}

/// The loop's poll returning, timed for `turn_end`: a calloop source with no
/// file of its own, only the lifecycle call every dispatch makes
pub struct Woke;

impl calloop::EventSource for Woke {
    type Event = ();
    type Metadata = ();
    type Ret = ();
    type Error = std::io::Error;
    const NEEDS_EXTRA_LIFECYCLE_EVENTS: bool = true;

    fn process_events<F>(&mut self, _: calloop::Readiness, _: calloop::Token, _: F) -> Result<calloop::PostAction, Self::Error>
    where
        F: FnMut((), &mut ()),
    {
        Ok(calloop::PostAction::Continue)
    }
    fn register(&mut self, _: &mut calloop::Poll, _: &mut calloop::TokenFactory) -> calloop::Result<()> {
        Ok(())
    }
    fn reregister(&mut self, _: &mut calloop::Poll, _: &mut calloop::TokenFactory) -> calloop::Result<()> {
        Ok(())
    }
    fn unregister(&mut self, _: &mut calloop::Poll) -> calloop::Result<()> {
        Ok(())
    }
    fn before_handle_events(&mut self, _: calloop::EventIterator) {
        woke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_count_as_stalls_over_a_frame() {
        let mut c = Counts::default();
        for ms in [3.0, 15.9, 16.5, 20.0, 40.0, 120.4] {
            c.turn(ms);
        }
        assert_eq!((c.s16, c.s33), (4, 2));
        assert_eq!(c.worst_ms, 120.4);
    }

    #[test]
    fn samples_are_short_lines_that_read_back() {
        let quiet = sample_line(1_763_380_800, 300, 61234, 50123, 912, &Counts::default());
        assert_eq!(quiet, r#"{"t":1763380800,"d":300,"rss":61234,"pss":50123,"cpu":912}"#);
        let c = Counts { s16: 3, s33: 1, worst_ms: 41.6, frames: 120, missed: 4, crashes: 1, pressure: 37 };
        let busy = sample_line(1_763_381_100, 300, 1, 2, 3, &c);
        let s = Sample::from(&serde_json::from_str(&busy).unwrap()).unwrap();
        assert_eq!((s.s16, s.s33, s.worst, s.frames, s.missed, s.crashes, s.mem(), s.pressure), (3, 1, 42, 120, 4, 1, 2, 37));
        // (a week of quiet samples fits the cap with room)
        assert!(quiet.len() * 7 * 288 < KEEP_BYTES);
        // (an event isn't a sample)
        assert!(Sample::from(&json!({"t": 1, "ev": "start"})).is_none());
    }

    #[test]
    fn buckets_lie_on_the_wall_clock() {
        // (the first bucket of a start mid-way ends at the next 5-minute mark)
        let now: i64 = 1_763_380_800 + 137;
        assert_eq!(now - now.rem_euclid(BUCKET_SECS), 1_763_380_800);
        let samples = vec![
            Sample { t: 1000 * 300, d: 300, pss: 100, cpu_ms: 3000, s16: 2, ..Default::default() },
            Sample { t: 1001 * 300, d: 300, pss: 300, cpu_ms: 0, s16: 1, s33: 1, worst: 50, ..Default::default() },
            Sample { t: 1012 * 300, d: 300, rss: 900, ..Default::default() },
            // (before the first slot, after the last: left out)
            Sample { t: 999 * 300, d: 300, pss: 5, ..Default::default() },
            Sample { t: 1030 * 300, d: 300, pss: 5, ..Default::default() },
        ];
        let s = slots(&samples, 1000 * 300, 3600, 2);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].n, s[0].mem_avg, s[0].mem_max, s[0].stalls, s[0].long_stalls, s[0].worst), (2, 200, 300, 3, 1, 50));
        assert!((s[0].cpu_pct - 0.5).abs() < 1e-9, "{}", s[0].cpu_pct);
        assert!((s[0].stalls_per_hour - 18.0).abs() < 1e-9);
        assert_eq!((s[1].n, s[1].mem_avg, s[1].start), (1, 900, 1000 * 300 + 3600));
    }

    #[test]
    fn the_ring_keeps_the_newest_lines() {
        let text: String = (0..1000).map(|i| format!("{{\"t\":{},\"d\":300,\"rss\":1,\"pss\":1,\"cpu\":1}}\n", 10_000 + i)).collect();
        let kept = ring(&text, 4096, 0);
        assert!(kept.len() <= 4096 && kept.len() > 4000, "{}", kept.len());
        assert!(kept.ends_with("\"t\":10999,\"d\":300,\"rss\":1,\"pss\":1,\"cpu\":1}\n"));
        assert!(kept.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()));
        // (nothing older than the cut-off)
        let recent = ring(&text, 1 << 20, 10_990);
        assert_eq!(recent.lines().count(), 10);
        // (the file itself: past the cap, cut)
        let d = std::env::temp_dir().join(format!("tessera-health-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("health.jsonl");
        let now = crate::util::now_ms() / 1000;
        let line = format!("{{\"t\":{now},\"d\":300,\"rss\":61234,\"pss\":50123,\"cpu\":912}}");
        for _ in 0..(CAP_BYTES as usize / line.len() + 50) {
            append(&p, &line);
        }
        let size = std::fs::metadata(&p).unwrap().len();
        assert!(size <= CAP_BYTES, "{size}");
        assert!(size >= (KEEP_BYTES - line.len()) as u64 / 2, "{size}");
        assert!(std::fs::read_to_string(&p).unwrap().lines().all(|l| l == line));
        // (the tail read at start begins at a whole line)
        let tail = read_tail(&p, 1000);
        assert!(tail.lines().all(|l| l == line) && !tail.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn memory_pressure_over_a_bucket() {
        let t = "some avg10=0.00 avg60=1.20 avg300=0.40 total=123456789\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=4567\n";
        assert_eq!(psi_some_total(t), Some(123_456_789));
        assert_eq!(psi_some_total("full avg10=0 total=5\n"), None);
        // (60 s of 300 waiting: 20%)
        assert_eq!(pressure_pct(Some(1_000_000), Some(61_000_000), 300), 20);
        assert_eq!(pressure_pct(Some(0), Some(1_000), 300), 0);
        assert_eq!(pressure_pct(None, Some(5), 300), 0);
        // (a counter that went back, a reboot between: nothing claimed)
        assert_eq!(pressure_pct(Some(9), Some(5), 300), 0);
    }

    #[test]
    fn rollup_reads_rss_and_pss() {
        let t = "55d0-7ff [rollup]\nRss:              123456 kB\nPss:               98765 kB\nPss_Anon:          50000 kB\n";
        assert_eq!(rollup(t), (123456, 98765));
        assert_eq!(rollup(""), (0, 0));
    }

    #[test]
    fn why_it_started() {
        let l = |s: &str| parse_lines(s);
        assert_eq!(start_reason(&[], "a"), ("normal", None));
        // (ended in order)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"d\":300}\n{\"t\":3,\"ev\":\"stop\"}"), "a").0, "normal");
        // (no stop, same session: it stopped without a word)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"d\":300}"), "a").0, "stopped");
        // (no stop, another session: the computer went off under it)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}"), "b").0, "normal");
        // (the loop panicked: its report)
        let crash = l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"ev\":\"crash\",\"part\":\"dock\",\"report\":\"x-dock.txt\"}\n{\"t\":3,\"ev\":\"crash\",\"part\":\"shell\",\"fatal\":true,\"report\":\"y-shell.txt\"}");
        assert_eq!(start_reason(&crash, "a"), ("crash", Some("y-shell.txt".into())));
        // (a part's crash alone isn't the shell's end)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"ev\":\"crash\",\"part\":\"dock\"}\n{\"t\":3,\"ev\":\"stop\"}"), "a").0, "normal");
        // (the watchdog restarted it, even if it then ended in order)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"ev\":\"watchdog\"}\n{\"t\":3,\"ev\":\"stop\"}"), "a").0, "watchdog");
        // (the QML shell stood in)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"ev\":\"fallback\",\"why\":\"x\"}"), "a").0, "retry");
        // (only what came after the last start counts)
        assert_eq!(start_reason(&l("{\"t\":1,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":2,\"ev\":\"watchdog\"}\n{\"t\":3,\"ev\":\"start\",\"s\":\"a\"}\n{\"t\":4,\"ev\":\"stop\"}"), "a").0, "normal");
        // (a broken line is skipped)
        assert_eq!(parse_lines("{\"t\":1}\nnot json\n{\"t\":\n[1]\n").len(), 1);
    }


    #[test]
    fn events_are_kept_a_month_samples_a_week() {
        let now: i64 = 100 * 86400;
        let mut text = String::new();
        // (an event 20 days ago, one 40 days ago, then 10 days of samples)
        text.push_str(&format!("{{\"t\":{},\"ev\":\"start\",\"why\":\"crash\"}}\n", now - 40 * 86400));
        text.push_str(&format!("{{\"t\":{},\"ev\":\"fallback\",\"why\":\"x\"}}\n", now - 20 * 86400));
        for i in 0..(10 * 288) {
            text.push_str(&format!("{{\"t\":{},\"d\":300,\"rss\":1,\"pss\":1,\"cpu\":1}}\n", now - 10 * 86400 + i * 300));
        }
        let kept = ring_events(&text, KEEP_BYTES, now - KEEP_DAYS * 86400, now - EVENT_DAYS * 86400);
        let lines = parse_lines(&kept);
        assert!(lines.iter().any(|l| l["ev"] == "fallback"), "the 20-day-old event stays");
        assert!(!lines.iter().any(|l| l["why"] == "crash"), "the 40-day-old one goes");
        let samples: Vec<i64> = lines.iter().filter(|l| l["ev"].is_null()).map(|l| l["t"].as_i64().unwrap()).collect();
        assert!(samples.iter().all(|t| *t >= now - KEEP_DAYS * 86400) && !samples.is_empty());
        assert!(kept.len() <= KEEP_BYTES);
        // (in the file's order: the event first)
        assert_eq!(lines[0]["ev"], "fallback");
    }

    fn evs(list: &[(i64, &str)]) -> Vec<Value> {
        list.iter()
            .map(|(t, k)| match *k {
                "crash" => json!({"t": t, "ev": "start", "why": "crash"}),
                "watchdog" => json!({"t": t, "ev": "start", "why": "watchdog"}),
                "normal" => json!({"t": t, "ev": "start", "why": "normal"}),
                "fallback" => json!({"t": t, "ev": "fallback", "why": "The Rust shell crashed twice"}),
                "sys-ok" => json!({"t": t, "ev": "update", "what": "system", "ok": true}),
                "sys-fail" => json!({"t": t, "ev": "update", "what": "system", "ok": false, "why": "pacman failed"}),
                "tes-fail" => json!({"t": t, "ev": "update", "what": "tessera", "ok": false}),
                _ => json!({"t": t, "ev": k}),
            })
            .collect()
    }

    #[test]
    fn stability_counts_a_week_and_a_month() {
        let now: i64 = 1_000 * 86400;
        let e = evs(&[(now - 25 * 86400, "crash"), (now - 6 * 86400, "watchdog"), (now - 3 * 86400, "normal"), (now - 2 * 86400, "fallback"), (now - 40 * 86400, "fallback")]);
        let s = stability(&e, now);
        assert_eq!((s.crashes_7, s.crashes_30, s.fallbacks_7, s.fallbacks_30), (1, 2, 1, 1));
        assert_eq!(s.days_since_crash(now), Some(6));
        assert_eq!(s.days_recorded(now), 40);
        // (an ordinary start or a part's crash isn't the shell crashing)
        let quiet = stability(&evs(&[(now - 86400, "normal"), (now - 100, "crash-part")]), now);
        assert_eq!((quiet.last_crash, quiet.crashes_30), (None, 0));
        assert_eq!(quiet.days_since_crash(now), None);
    }

    #[test]
    fn alerts_come_once_per_incident() {
        let now: i64 = 1_000 * 86400;
        let mut seen = json!({});
        // (one crash: nothing; a second within 24 hours: one alert)
        assert!(due_alerts(&evs(&[(now - 3600, "crash")]), now, &mut seen).is_empty());
        let two = evs(&[(now - 3600, "crash"), (now - 60, "watchdog")]);
        let a = due_alerts(&two, now, &mut seen);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["key"], "health:crashes");
        assert_eq!(a[0]["onAction"]["args"][0], "health");
        assert!(a[0]["title"].as_str().unwrap().contains("2 times"));
        // (the same incident again: nothing)
        assert!(due_alerts(&two, now + 30, &mut seen).is_empty());
        // (a third crash alone: still the same incident; two more: a new one)
        let mut three = two.clone();
        three.extend(evs(&[(now + 100, "crash")]));
        assert!(due_alerts(&three, now + 200, &mut seen).is_empty());
        three.extend(evs(&[(now + 300, "crash")]));
        assert_eq!(due_alerts(&three, now + 400, &mut seen).len(), 1);
        // (two crashes a day apart: no alert)
        let mut fresh = json!({});
        assert!(due_alerts(&evs(&[(now - 90_000, "crash"), (now - 60, "crash")]), now, &mut fresh).is_empty());
    }

    #[test]
    fn fallbacks_and_failed_updates_alert_once() {
        let now: i64 = 1_000 * 86400;
        let mut seen = json!({});
        let e = evs(&[(now - 50, "fallback")]);
        let a = due_alerts(&e, now, &mut seen);
        assert_eq!((a.len(), a[0]["key"].as_str()), (1, Some("health:fallback")));
        assert!(due_alerts(&e, now, &mut seen).is_empty());
        // (an old fallback, from before the last day: no alert)
        assert!(due_alerts(&evs(&[(now - 2 * 86400, "fallback")]), now, &mut json!({})).is_empty());
        // Updates: one failure, nothing; a success between, nothing; twice in a row, once
        let mut seen = json!({});
        assert!(due_alerts(&evs(&[(now - 500, "sys-fail")]), now, &mut seen).is_empty());
        assert!(due_alerts(&evs(&[(now - 500, "sys-fail"), (now - 400, "sys-ok"), (now - 300, "sys-fail")]), now, &mut seen).is_empty());
        let twice = evs(&[(now - 500, "sys-fail"), (now - 400, "sys-ok"), (now - 300, "sys-fail"), (now - 200, "sys-fail")]);
        let a = due_alerts(&twice, now, &mut seen);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["key"], "health:update-system");
        assert_eq!(a[0]["subtitle"], "pacman failed");
        assert_eq!(failing_run(&twice, "system"), vec![now - 300, now - 200]);
        // (a third failure in the same run: the same incident)
        let mut more = twice.clone();
        more.extend(evs(&[(now - 100, "sys-fail")]));
        assert!(due_alerts(&more, now, &mut seen).is_empty());
        // (another kind's failures are their own run, interleaved or not)
        assert!(due_alerts(&evs(&[(now - 90, "tes-fail"), (now - 80, "sys-ok")]), now, &mut seen).is_empty());
        let t2 = evs(&[(now - 90, "tes-fail"), (now - 80, "sys-ok"), (now - 70, "tes-fail")]);
        assert_eq!(due_alerts(&t2, now, &mut seen)[0]["key"], "health:update-tessera");
    }

    #[test]
    fn events_and_samples_part() {
        let (s, e) = split(&parse_lines("{\"t\":5,\"ev\":\"stop\"}\n{\"t\":600,\"d\":300,\"pss\":1}\n{\"t\":300,\"d\":300,\"pss\":2}\n{\"t\":2,\"ev\":\"start\"}"));
        assert_eq!(s.iter().map(|s| s.t).collect::<Vec<_>>(), vec![300, 600]);
        assert_eq!(e.iter().map(|v| v["t"].as_i64().unwrap()).collect::<Vec<_>>(), vec![2, 5]);
    }
}
