//! The lock's rules as plain functions (Lock.qml, LockSurface.qml,
//! StandBy.qml, LockGlass.qml, LockMaterial.qml): too many attempts
//! (pam_faillock), what the fingerprint reader said, Bedside Mode's moves
//! against burn-in and its night, the clock's and the date's text, the
//! notification shelf, the morph's ramps. The part (mod.rs) keeps the state.

use tessera_core::locale;

/// pam_faillock's settings (faillock.conf), its defaults when unread
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Faillock {
    pub deny: u32,
    /// seconds; 0: never unlocks by itself ("never")
    pub unlock: i64,
    pub interval: i64,
}

impl Default for Faillock {
    fn default() -> Self {
        Self { deny: 3, unlock: 600, interval: 900 }
    }
}

/// faillock.conf's `deny`, `unlock_time` and `fail_interval` (the last
/// setting of each wins, comments skipped)
pub fn faillock_conf(text: &str) -> Faillock {
    let mut f = Faillock::default();
    let get = |key: &str| -> Option<String> {
        let mut found = None;
        for l in text.lines() {
            let l = l.trim();
            if l.starts_with('#') {
                continue;
            }
            let Some(rest) = l.strip_prefix(key) else { continue };
            let Some(v) = rest.trim_start().strip_prefix('=') else { continue };
            let v: String = v.trim().chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            found = Some(v);
        }
        found
    };
    if let Some(d) = get("deny").and_then(|v| v.parse::<u32>().ok()).filter(|d| *d > 0) {
        f.deny = d;
    }
    match get("unlock_time").as_deref() {
        Some("never") => f.unlock = 0,
        Some(v) => {
            if let Ok(n) = v.parse::<i64>() {
                f.unlock = n;
            }
        }
        None => {}
    }
    if let Some(i) = get("fail_interval").and_then(|v| v.parse::<i64>().ok()).filter(|i| *i > 0) {
        f.interval = i;
    }
    f
}

/// What the error says while the account is locked by too many attempts
pub fn locked_text(seconds: i64) -> String {
    if seconds <= 0 {
        return "Too many attempts: this account stays locked until it's unlocked with faillock --reset from another session.".into();
    }
    let m = ((seconds as f64) / 60.0).ceil().max(1.0) as i64;
    format!("Too many attempts: try again in about {m} {}.", if m == 1 { "minute" } else { "minutes" })
}

/// Seconds until pam_faillock lets the account in again (None: not locked).
/// `tally`: `faillock --user` (None where it can't be read: then `fails` in
/// a row, from this lock, decide); `local`: a tally line's local time in ms
pub fn lockout(conf: &Faillock, tally: Option<&str>, fails: u32, now_ms: i64, local: impl Fn(i32, i32, i32, i32, i32, i32) -> i64) -> Option<i64> {
    let readable = tally.filter(|t| t.lines().any(|l| l.starts_with("When")));
    let Some(text) = readable else {
        return (fails >= conf.deny).then_some(conf.unlock);
    };
    // "2026-10-30 13:27:01 TTY /dev/pts/1 V"
    let mut times = Vec::new();
    for l in text.lines() {
        let l = l.trim_end();
        if !l.ends_with(" V") && !l.ends_with("\tV") {
            continue;
        }
        let b = l.as_bytes();
        if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b':' || b[16] != b':' {
            continue;
        }
        let n = |a: usize, z: usize| l[a..z].parse::<i32>().ok();
        if let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19)) {
            times.push(local(y, mo, d, h, mi, s));
        }
    }
    let recent: Vec<i64> = times.into_iter().filter(|t| now_ms - t <= conf.interval * 1000).collect();
    if (recent.len() as u32) < conf.deny {
        return None;
    }
    if conf.unlock == 0 {
        return Some(0);
    }
    let last = *recent.iter().max()?;
    let left = ((last + conf.unlock * 1000 - now_ms) as f64 / 1000.0).round() as i64;
    (left > 0).then_some(left)
}

/// What a fingerprint conversation's message means (Lock.qml's tests)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Finger {
    /// not this finger (or a failed read)
    Miss,
    /// the reader asks for a finger
    Ask,
    Other,
}

pub fn finger_message(m: &str) -> Finger {
    let l = m.to_lowercase();
    if ["fail", "no match", "not match", "not recogni"].iter().any(|w| l.contains(w)) {
        Finger::Miss
    } else if ["place", "swipe", "touch", "finger"].iter().any(|w| l.contains(w)) {
        Finger::Ask
    } else {
        Finger::Other
    }
}

/// The island round Glass's password field (Material's pill): which reader
/// it holds, in which phase, and its one hint line. kind "" = no reader to
/// show (only the field: the island has no glass of its own then);
/// "finger" or "face" = grown to hold that reader's big glyph. `finger` is
/// the reader's state, `face` the camera's ("" | waiting | reading | match
/// | miss); typing or a password being checked shrinks it back (a reader's
/// match stays on the way out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Island {
    pub kind: &'static str,
    pub phase: &'static str,
    pub hint: &'static str,
}

pub const ISLAND_NONE: Island = Island { kind: "", phase: "", hint: "" };

/// Keys reach the password field: not while the screen is locked with the
/// lid shut and no other display (the lid, or a bag, pressing keys; what's
/// typed is put back). Docked with the lid shut, an outside keyboard types.
pub fn typing_counts(lid_closed: bool, locked: bool, outputs: &[&str]) -> bool {
    !(lid_closed && locked) || outputs.iter().any(|o| !built_in(o))
}

/// The longest answer PAM takes (PAM_MAX_RESP_SIZE, 512 bytes)
pub const MAX_PASSWORD: usize = 512;

/// TEXT cut to MAX_PASSWORD bytes (on a character's edge); true when it was
/// longer. The cut-off part is overwritten first
pub fn cap_password(text: &mut String) -> bool {
    if text.len() <= MAX_PASSWORD {
        return false;
    }
    let mut at = MAX_PASSWORD;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let mut tail = text.split_off(at);
    wipe(&mut tail);
    true
}

/// A password's bytes overwritten before they go back to the allocator, then
/// emptied (String::clear leaves them in memory)
pub fn wipe(s: &mut String) {
    let mut b = std::mem::take(s).into_bytes();
    b.iter_mut().for_each(|x| *x = 0);
    std::hint::black_box(&b);
}

/// A key's text that types something: one character, not a control one
/// (Return, Escape, Backspace, Shift… are control characters in Slint's
/// Key) nor Slint's private-use codes (the arrows, F1…, Home, Insert…)
pub fn printable_key(t: &str) -> bool {
    let mut c = t.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) => !ch.is_control() && !('\u{E000}'..='\u{F8FF}').contains(&ch),
        _ => false,
    }
}

/// A laptop's own panel (eDP, LVDS, DSI)
pub fn built_in(output: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| output.starts_with(p))
}

pub fn island(finger: &str, face: &str, typing: bool, busy: bool, leaving: bool) -> Island {
    let known = |p: &str| -> Option<&'static str> { ["waiting", "reading", "match", "miss"].into_iter().find(|q| *q == p) };
    let (f, c) = (known(finger), known(face));
    let make = |kind: &'static str, phase: &'static str| Island {
        kind,
        phase,
        hint: match (kind, phase) {
            (_, "match") => "Unlocked",
            (_, "miss") => "Not recognised. Try again",
            ("face", _) => "Look at the camera",
            _ => "Touch the fingerprint reader",
        },
    };
    if f == Some("match") {
        return make("finger", "match");
    }
    if c == Some("match") {
        return make("face", "match");
    }
    if leaving || typing || busy {
        return ISLAND_NONE;
    }
    for phase in ["miss", "reading", "waiting"] {
        // (the camera first while it looks: it gives up after a few tries)
        let order = if phase == "waiting" { [("finger", f), ("face", c)] } else { [("face", c), ("finger", f)] };
        for (kind, p) in order {
            if p == Some(phase) {
                return make(kind, phase);
            }
        }
    }
    ISLAND_NONE
}

/// A new spot for Bedside Mode's content, up or down (-1 .. 1 of the room it
/// has), well away from the last one so no pixel holds a colour for long
pub fn next_drift(prev: f64, mut rnd: impl FnMut() -> f64) -> f64 {
    let mut y = 0.0;
    for _ in 0..8 {
        y = rnd() * 2.0 - 1.0;
        if (y - prev).abs() > 0.5 {
            break;
        }
    }
    y
}

/// How far content `cw`×`ch` can move on a `w`×`h` screen and stay 48 px in
pub fn room(w: f64, h: f64, cw: f64, ch: f64) -> (f64, f64) {
    (((w - cw) / 2.0 - 48.0).max(0.0), ((h - ch) / 2.0 - 48.0).max(0.0))
}

/// Bedside Mode's night (a dim red): after dark, between 22:00 and 6:00
pub fn night(day: bool, hour: u32) -> bool {
    !day && (hour >= 22 || hour < 6)
}

/// A tiny random number from the clock (0 .. 1): Bedside Mode's spots
pub fn random() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// The hour as the menu bar writes it (TimeFormat.hour): 24-hour "09", or
/// 12-hour "9"
pub fn hour(h: u32, h24: bool) -> String {
    if h24 {
        format!("{h:02}")
    } else {
        format!("{}", (h + 11) % 12 + 1)
    }
}

/// "9:41" (large clocks: no AM/PM)
pub fn clock(h: u32, m: u32, h24: bool) -> String {
    format!("{}:{m:02}", hour(h, h24))
}

/// TimeFormat.hm: "21:41", or "9:41 PM"
pub fn hm(h: u32, m: u32, h24: bool) -> String {
    if h24 {
        format!("{h:02}:{m:02}")
    } else {
        format!("{}:{m:02} {}", (h + 11) % 12 + 1, if h < 12 { "AM" } else { "PM" })
    }
}

// (names in the system's language: tessera_core::locale, LC_TIME; English
// without one)

/// Qt's "dddd d MMMM": "Thursday 5 November" (`wday` 0 = Sunday)
pub fn date_long(wday: u32, day: u32, month0: u32) -> String {
    format!("{} {day} {}", locale::day_name(wday % 7, false), locale::month_name(month0 % 12 + 1, false))
}

/// Qt's "d MMM": "5 Nov" (the dynamic clock with 24-hour time)
pub fn date_short(day: u32, month0: u32) -> String {
    format!("{day} {}", locale::month_name(month0 % 12 + 1, true))
}

/// Bedside Mode's calendar's header: "Nov 2026"
pub fn month_short(month0: u32, year: i32) -> String {
    format!("{} {year}", locale::month_name(month0 % 12 + 1, true))
}

/// Notifications.timeLabel: "now", "5m ago", else the time it came
pub fn time_label(received_ms: i64, now_ms: i64, received_hm: &str) -> String {
    if received_ms <= 0 {
        return String::new();
    }
    let mins = (now_ms - received_ms).div_euclid(60_000);
    if mins < 1 {
        "now".into()
    } else if mins < 60 {
        format!("{mins}m ago")
    } else {
        received_hm.to_string()
    }
}

/// The apps that sent them, newest first, each once, at most six (`keys`:
/// app name, else desktop entry, else id; newest first)
pub fn shelf_apps(keys: &[String]) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if out.len() >= 6 {
            break;
        }
        if seen.insert(k.as_str()) {
            out.push(i);
        }
    }
    out
}

/// Strips markup from a notification's body (`replace(/<[^>]*>/g, "")`)
pub fn plain(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut tag = false;
    for c in body.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// LockSurface's ramp: `sb` mapped to 0..1 over [a, b], smoothstepped
pub fn ramp(sb: f64, a: f64, b: f64) -> f64 {
    let t = ((sb - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The Material lock clock's digit size (LockMaterial.clockSize)
pub fn material_clock_size(w: f64, h: f64, split: bool, ios: bool) -> f64 {
    (if split { (h * 0.2).min(w * 0.12) } else { (h * if ios { 0.17 } else { 0.15 }).min(w * 0.2) }).round()
}

/// Bedside Mode's clock weights (StandBy: thin, regular, bold), Glass's and
/// Material's (the font's weight axis)
pub fn standby_weight(setting: &str, material: bool) -> i32 {
    match (setting, material) {
        ("bold", false) => 600,
        ("regular", false) => 400,
        (_, false) => 200,
        ("bold", true) => 600,
        ("regular", true) => 400,
        (_, true) => 200,
    }
}

/// Bedside Mode's calendar: how many days of the month before fill its
/// first week, the week starting on `start` (0 Sunday … 6 Saturday:
/// appearance.weekStart or the system's language), when the 1st is on
/// `first_wday` (0 = Sunday)
pub fn lead(first_wday: i32, start: u32) -> i32 {
    locale::column(first_wday.rem_euclid(7) as u32, start) as i32
}

/// Whether to treat the face camera's or the reader's tries as given back
/// (Lock.qml's woke): someone came back after 30 s away or from Bedside Mode
pub fn came_back(idle: bool, now_ms: i64, last_touch_ms: i64) -> bool {
    idle || now_ms - last_touch_ms > 30_000
}

/// Where the clock and date sit on a screen of `screen` px, as (x, y, w,
/// h): the top 6..44 % of its middle 88 %, both the centred and the split
/// layout's clock. Their ink follows the wallpaper under it
/// (services::wallpaper: dark ink on a light one)
pub fn clock_region(screen: (f32, f32)) -> (f32, f32, f32, f32) {
    (screen.0 * 0.06, screen.1 * 0.06, screen.0 * 0.88, screen.1 * 0.38)
}

/// Bedside Mode at night: the date line's ink. Still red, light enough to
/// read (≥ 4.5:1) under the night's dimming (NIGHT_DIM black over it all)
pub const NIGHT_DATE_INK: [u8; 3] = [0xFF, 0x9C, 0x92];
/// The night's black over everything (lock.slint's #00000059)
pub const NIGHT_DIM: f64 = 0x59 as f64 / 255.0;

/// WCAG's contrast of a colour shown at `alpha` over black, then dimmed by
/// `dim` (black over it), against black
pub fn contrast_on_black(rgb: [u8; 3], alpha: f64, dim: f64) -> f64 {
    let lin = |c: u8| {
        let v = c as f64 / 255.0 * alpha * (1.0 - dim);
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let y = 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
    (y + 0.05) / 0.05
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_capped_at_pams_limit_and_wiped() {
        let mut s = "a".repeat(600);
        assert!(cap_password(&mut s));
        assert_eq!(s.len(), MAX_PASSWORD);
        let mut s = "é".repeat(300); // (2 bytes each: cut on a character's edge)
        assert!(cap_password(&mut s));
        assert_eq!(s.len(), MAX_PASSWORD);
        assert!(s.chars().all(|c| c == 'é'));
        let mut s = "short".to_string();
        assert!(!cap_password(&mut s));
        assert_eq!(s, "short");
        wipe(&mut s);
        assert!(s.is_empty());
    }

    #[test]
    fn a_key_that_wakes_bedside_mode_types_only_when_printable() {
        for t in ["a", "Z", "7", " ", "é", "€"] {
            assert!(printable_key(t), "{t:?}");
        }
        // (Return, Escape, Tab, Backspace, Delete, Shift, Control, Up, F1, two characters, none)
        for t in ["\n", "\u{1b}", "\t", "\u{8}", "\u{7f}", "\u{10}", "\u{11}", "\u{F700}", "\u{F704}", "ab", ""] {
            assert!(!printable_key(t), "{t:?}");
        }
    }

    #[test]
    fn the_clock_ink_follows_the_wallpaper_behind_it() {
        use crate::services::wallpaper::{WallTop, GRID};
        let screen = (1920.0, 1080.0);
        let (x, y, w, h) = clock_region(screen);
        assert!(x > 0.0 && y > 0.0 && x + w < screen.0 && y + h < screen.1 * 0.5);
        // a light top, a dark bottom: dark ink on the clock
        let mut g = vec![0.1f32; GRID * GRID];
        for v in g.iter_mut().take(GRID * GRID / 2) {
            *v = 0.9;
        }
        let light_top = WallTop::from_grid("/a.jpg", 16.0 / 9.0, g.clone());
        assert_eq!(light_top.dark_ink_under(screen, clock_region(screen)), Some(true));
        // the other way round: light ink (only what's behind the clock counts)
        g.reverse();
        let dark_top = WallTop::from_grid("/b.jpg", 16.0 / 9.0, g);
        assert_eq!(dark_top.dark_ink_under(screen, clock_region(screen)), Some(false));
        // a bright strip only under the bar doesn't turn the clock dark
        let mut strip = vec![0.1f32; GRID * GRID];
        for v in strip.iter_mut().take(GRID * 2) {
            *v = 1.0;
        }
        let strip = WallTop::from_grid("/c.jpg", 16.0 / 9.0, strip);
        assert!(strip.light);
        assert_eq!(strip.dark_ink_under(screen, clock_region(screen)), Some(false));
    }

    #[test]
    fn bedside_night_date_reads() {
        // the old ink: red at 60 %, the clock's 70 %, then the night's dimming
        assert!(contrast_on_black([0xFF, 0x3B, 0x30], 0.6 * 0.7, NIGHT_DIM) < 2.0);
        // the new: at least 4.5:1, and still red
        assert!(contrast_on_black(NIGHT_DATE_INK, 1.0, NIGHT_DIM) >= 4.5, "{}", contrast_on_black(NIGHT_DATE_INK, 1.0, NIGHT_DIM));
        assert!(NIGHT_DATE_INK[0] > NIGHT_DATE_INK[1] && NIGHT_DATE_INK[0] > NIGHT_DATE_INK[2]);
    }

    #[test]
    fn faillock_settings() {
        assert_eq!(faillock_conf(""), Faillock::default());
        let f = faillock_conf("# deny = 9\ndeny = 5\n  unlock_time=120 # two minutes\nfail_interval = 60\n");
        assert_eq!(f, Faillock { deny: 5, unlock: 120, interval: 60 });
        assert_eq!(faillock_conf("unlock_time = never").unlock, 0);
        // (nonsense keeps the defaults; the last one wins)
        assert_eq!(faillock_conf("deny = 0\ndeny = x").deny, 3);
        assert_eq!(faillock_conf("deny = 4\ndeny = 6").deny, 6);
    }

    #[test]
    fn locked_texts() {
        assert_eq!(locked_text(600), "Too many attempts: try again in about 10 minutes.");
        assert_eq!(locked_text(30), "Too many attempts: try again in about 1 minute.");
        assert!(locked_text(0).contains("faillock --reset"));
    }

    fn local(y: i32, mo: i32, d: i32, h: i32, mi: i32, s: i32) -> i64 {
        // (a fake clock: minutes of the day only)
        let _ = (y, mo, d);
        ((h * 3600 + mi * 60 + s) as i64) * 1000
    }

    #[test]
    fn lockouts() {
        let c = Faillock::default();
        // unreadable: three in a row here
        assert_eq!(lockout(&c, None, 2, 0, local), None);
        assert_eq!(lockout(&c, Some("NOREAD"), 3, 0, local), Some(600));
        let tally = "user:\nWhen                Type  Source                                           Valid\n2026-10-30 13:27:01 TTY /dev/pts/1 V\n2026-10-30 13:27:05 TTY /dev/pts/1 V\n2026-10-30 13:27:09 TTY /dev/pts/1 V\n";
        let now = local(2026, 10, 30, 13, 28, 9);
        assert_eq!(lockout(&c, Some(tally), 0, now, local), Some(540));
        // too long ago: not counted
        assert_eq!(lockout(&c, Some(tally), 0, local(2026, 10, 30, 13, 50, 0), local), None);
        // invalid entries (I) don't count
        let invalid = tally.replace(" V\n", " I\n");
        assert_eq!(lockout(&c, Some(&invalid), 9, now, local), None);
        // never unlocks by itself
        let never = Faillock { unlock: 0, ..c };
        assert_eq!(lockout(&never, Some(tally), 0, now, local), Some(0));
    }

    #[test]
    fn finger_messages() {
        assert_eq!(finger_message("Place your finger on the reader"), Finger::Ask);
        assert_eq!(finger_message("Swipe your right index finger"), Finger::Ask);
        assert_eq!(finger_message("Failed to match fingerprint"), Finger::Miss);
        assert_eq!(finger_message("No match"), Finger::Miss);
        assert_eq!(finger_message("Fingerprint not recognised"), Finger::Miss);
        assert_eq!(finger_message("Verification timed out"), Finger::Other);
    }

    #[test]
    fn island_states() {
        // nothing on: no island, only the field
        assert_eq!(island("", "", false, false, false), ISLAND_NONE);
        let i = island("waiting", "", false, false, false);
        assert_eq!((i.kind, i.phase, i.hint), ("finger", "waiting", "Touch the fingerprint reader"));
        assert_eq!(island("reading", "", false, false, false).hint, "Touch the fingerprint reader");
        let i = island("miss", "", false, false, false);
        assert_eq!((i.kind, i.phase, i.hint), ("finger", "miss", "Not recognised. Try again"));
        let i = island("", "reading", false, false, false);
        assert_eq!((i.kind, i.phase, i.hint), ("face", "reading", "Look at the camera"));
        assert_eq!(island("", "waiting", false, false, false).hint, "Look at the camera");
        assert_eq!(island("", "miss", false, false, false).kind, "face");
        // typing, or a password being checked: back to the field
        assert_eq!(island("waiting", "reading", true, false, false), ISLAND_NONE);
        assert_eq!(island("waiting", "", false, true, false), ISLAND_NONE);
        // a password unlock: none; a reader's match stays on the way out
        assert_eq!(island("waiting", "", false, false, true), ISLAND_NONE);
        let i = island("match", "", true, false, true);
        assert_eq!((i.kind, i.phase, i.hint), ("finger", "match", "Unlocked"));
        assert_eq!(island("waiting", "match", false, false, true).kind, "face");
        // both on: a miss first, then the camera while it looks, else the reader
        assert_eq!(island("miss", "reading", false, false, false).kind, "finger");
        assert_eq!(island("waiting", "reading", false, false, false).kind, "face");
        assert_eq!(island("waiting", "waiting", false, false, false).kind, "finger");
        assert_eq!(island("waiting", "miss", false, false, false).phase, "miss");
        // anything else: none
        assert_eq!(island("busy", "", false, false, false), ISLAND_NONE);
    }

    #[test]
    fn drifts_move_away() {
        let mut seq = [0.6, 0.62, 0.1].into_iter();
        // 0.6 → 0.2 and 0.62 → 0.24 are too near 0; 0.1 → -0.8 is far enough
        let y = next_drift(0.0, || seq.next().unwrap_or(0.5));
        assert!((y + 0.8).abs() < 1e-9);
        for _ in 0..50 {
            let y = next_drift(0.3, random);
            assert!((-1.0..=1.0).contains(&y));
        }
        assert_eq!(room(1920.0, 1080.0, 600.0, 300.0), (612.0, 342.0));
        assert_eq!(room(500.0, 300.0, 600.0, 300.0), (0.0, 0.0));
    }

    #[test]
    fn night_is_late_and_dark() {
        assert!(night(false, 23) && night(false, 3));
        assert!(!night(true, 23) && !night(false, 21) && !night(false, 6));
    }

    #[test]
    fn clocks_and_dates() {
        assert_eq!(clock(21, 5, false), "9:05");
        assert_eq!(clock(0, 0, false), "12:00");
        assert_eq!(clock(9, 41, true), "09:41");
        assert_eq!(hm(13, 7, false), "1:07 PM");
        assert_eq!(hm(0, 7, false), "12:07 AM");
        assert_eq!(hm(13, 7, true), "13:07");
        assert_eq!(date_long(4, 5, 10), "Thursday 5 November");
        assert_eq!(date_short(5, 10), "5 Nov");
        assert_eq!(time_label(1_000, 30_000, "9:00 AM"), "now");
        assert_eq!(time_label(1_000, 1_000 + 5 * 60_000, "9:00 AM"), "5m ago");
        assert_eq!(time_label(1_000, 1_000 + 61 * 60_000, "9:00 AM"), "9:00 AM");
        assert_eq!(time_label(0, 5, "x"), "");
    }

    #[test]
    fn shelf_has_each_app_once() {
        let keys: Vec<String> = ["Mail", "Chat", "Mail", "A", "B", "C", "D", "E"].iter().map(|s| s.to_string()).collect();
        assert_eq!(shelf_apps(&keys), vec![0, 1, 3, 4, 5, 6]);
        assert!(shelf_apps(&[]).is_empty());
        assert_eq!(plain("<b>Hi</b> <i>there</i>"), "Hi there");
        assert_eq!(plain("a < b"), "a ");
    }

    #[test]
    fn ramps_and_sizes() {
        assert_eq!(ramp(0.0, 0.3, 0.8), 0.0);
        assert_eq!(ramp(1.0, 0.3, 0.8), 1.0);
        assert!((ramp(0.55, 0.3, 0.8) - 0.5).abs() < 1e-9);
        assert_eq!(material_clock_size(1920.0, 1080.0, false, true), 184.0);
        assert_eq!(material_clock_size(1920.0, 1080.0, true, true), 216.0);
        assert_eq!(standby_weight("thin", false), 200);
        assert_eq!(standby_weight("bold", true), 600);
    }

    #[test]
    fn months() {
        // November 2026 starts on a Sunday: six days of October first
        assert_eq!(lead(0, 1), 6);
        assert_eq!(lead(1, 1), 0);
        assert_eq!(lead(6, 1), 5);
        // weeks from Sunday: none; from Saturday: one
        assert_eq!(lead(0, 0), 0);
        assert_eq!(lead(0, 6), 1);
        assert_eq!(lead(6, 6), 0);
        assert_eq!(month_short(10, 2026), "Nov 2026");
        assert!(came_back(true, 0, 0));
        assert!(came_back(false, 40_000, 0));
        assert!(!came_back(false, 20_000, 0));
    }
}

#[cfg(test)]
mod lid_tests {
    #[test]
    fn a_shut_lid_types_nothing() {
        assert!(!super::typing_counts(true, true, &["eDP-1"]));
        assert!(!super::typing_counts(true, true, &[]));
        assert!(super::typing_counts(false, true, &["eDP-1"]));
        // (not locked: the lid doesn't matter here)
        assert!(super::typing_counts(true, false, &["eDP-1"]));
        // docked, lid shut: the outside keyboard types
        assert!(super::typing_counts(true, true, &["eDP-1", "DP-2"]));
        assert!(super::typing_counts(true, true, &["HDMI-A-1"]));
        assert!(super::built_in("eDP-1") && super::built_in("LVDS-1") && !super::built_in("DP-1"));
    }
}
