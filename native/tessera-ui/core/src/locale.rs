//! The system's language for dates and times (LC_TIME), as the rest of the
//! system uses it: day and month names, whether clocks are 12- or 24-hour
//! by default, and the day weeks start on (audit #372 #512 #875).
//!
//! `init` runs once at start, before any thread (setlocale isn't
//! thread-safe): libc's strftime then writes the language's names ("mer. 4
//! nov." in French). With no language set (C or POSIX) everything stays as
//! before: English names, 12-hour, Monday first.
//!
//! The settings still win: appearance.clockHours ("12" | "24"; "auto" or
//! none follows the language; an older appearance.clock24 true still means
//! 24-hour) and appearance.weekStart ("monday" | "sunday" | "saturday";
//! "auto" or none follows the language).

use serde_json::Value;
use std::sync::OnceLock;

/// Take LC_TIME from the environment (LC_ALL, LC_TIME, LANG). Call first, before threads.
pub fn init() {
    unsafe {
        libc::setlocale(libc::LC_TIME, b"\0".as_ptr() as *const libc::c_char);
    }
}

/// What the language says, read once
struct System {
    /// a language is set (not C or POSIX)
    named: bool,
    h24: bool,
    /// 0 Sunday … 6 Saturday
    week_start: u32,
}

fn system() -> &'static System {
    static S: OnceLock<System> = OnceLock::new();
    S.get_or_init(|| {
        let name = unsafe {
            let p = libc::setlocale(libc::LC_TIME, std::ptr::null());
            if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        let named = is_named(&name);
        if !named {
            return System { named, h24: false, week_start: 1 };
        }
        System { named, h24: langinfo_h24().unwrap_or(false), week_start: langinfo_week_start().unwrap_or(1) }
    })
}

/// A real language, not "C", "C.UTF-8" or "POSIX"
fn is_named(name: &str) -> bool {
    !(name.is_empty() || name == "C" || name == "POSIX" || name.starts_with("C.") || name.starts_with("C@"))
}

/// A time format that writes the 12-hour hour (%I, %l or %r)
pub fn is_12h_format(t_fmt: &str) -> bool {
    t_fmt.contains("%I") || t_fmt.contains("%l") || t_fmt.contains("%r")
}

// glibc's LC_TIME items (langinfo.h, numbered from _NL_ITEM(LC_TIME, 0))
#[cfg(target_env = "gnu")]
const LC_TIME_ITEM: i32 = 2 << 16;
#[cfg(target_env = "gnu")]
const T_FMT: i32 = LC_TIME_ITEM | 42;
#[cfg(target_env = "gnu")]
const WEEK_1STDAY: i32 = LC_TIME_ITEM | 102;
#[cfg(target_env = "gnu")]
const FIRST_WEEKDAY: i32 = LC_TIME_ITEM | 104;

#[cfg(target_env = "gnu")]
fn langinfo_h24() -> Option<bool> {
    let p = unsafe { libc::nl_langinfo(T_FMT as libc::nl_item) };
    if p.is_null() {
        return None;
    }
    let t = unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy();
    if t.is_empty() { None } else { Some(!is_12h_format(&t)) }
}

#[cfg(target_env = "gnu")]
fn langinfo_week_start() -> Option<u32> {
    unsafe {
        // (week-1stday is a word stored in the pointer itself, first_weekday a
        // byte it points to: as glibc's `locale` prints them)
        let day1 = libc::nl_langinfo(WEEK_1STDAY as libc::nl_item) as usize as u32;
        let p = libc::nl_langinfo(FIRST_WEEKDAY as libc::nl_item);
        if p.is_null() {
            return None;
        }
        week_start_of(day1, *p as u8)
    }
}

#[cfg(not(target_env = "gnu"))]
fn langinfo_h24() -> Option<bool> {
    None
}

#[cfg(not(target_env = "gnu"))]
fn langinfo_week_start() -> Option<u32> {
    None
}

/// The first weekday (0 Sunday … 6 Saturday) from glibc's week-1stday (a
/// date, YYYYMMDD: 19971130 a Sunday, 19971201 a Monday) and first_weekday
/// (1 = that day, 2 = the one after…)
pub fn week_start_of(week_1stday: u32, first_weekday: u8) -> Option<u32> {
    if !(1..=7).contains(&first_weekday) {
        return None;
    }
    let (y, m, d) = ((week_1stday / 10000) as i64, ((week_1stday / 100) % 100) as i64, (week_1stday % 100) as i64);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // (days from 1970-01-01, a Thursday)
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let wd = (days + 4).rem_euclid(7) as u32;
    Some((wd + first_weekday as u32 - 1) % 7)
}

/// The system language's default: a 24-hour clock
pub fn system_clock24() -> bool {
    system().h24
}

/// The system language's first weekday: 0 Sunday … 6 Saturday (Monday with no language)
pub fn system_week_start() -> u32 {
    system().week_start
}

/// Whether a language is set (names come from it)
pub fn named() -> bool {
    system().named
}

/// 24-hour clocks: appearance.clockHours, else an older clock24 true, else the language's
pub fn clock24(cfg: &Value) -> bool {
    clock24_with(cfg, system_clock24())
}

/// clock24 with the language's default given (tests)
pub fn clock24_with(cfg: &Value, system: bool) -> bool {
    let a = &cfg["appearance"];
    match a["clockHours"].as_str() {
        Some("24") => true,
        Some("12") => false,
        // (before clockHours: false was also everyone's default, so only true was chosen)
        _ => a["clock24"].as_bool() == Some(true) || system,
    }
}

/// The day weeks start on, 0 Sunday … 6 Saturday: appearance.weekStart, else the language's
pub fn week_start(cfg: &Value) -> u32 {
    week_start_with(cfg, system_week_start())
}

/// week_start with the language's default given (tests)
pub fn week_start_with(cfg: &Value, system: u32) -> u32 {
    match cfg["appearance"]["weekStart"].as_str() {
        Some("sunday") => 0,
        Some("monday") => 1,
        Some("saturday") => 6,
        _ => system % 7,
    }
}

/// strftime on a made-up date: only the fields the names need
fn fmt_tm(fmt: &str, wday: i32, mon: i32) -> String {
    let fmt = std::ffi::CString::new(fmt).unwrap_or_default();
    let mut buf = [0u8; 128];
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_wday = wday;
        tm.tm_mon = mon;
        tm.tm_mday = 1;
        tm.tm_year = 126;
        let n = libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), fmt.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

const DAYS_EN: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// A weekday's name in the language (0 Sunday … 6 Saturday): "Wednesday", or short "Wed"
pub fn day_name(wd: u32, short: bool) -> String {
    let wd = wd % 7;
    let s = fmt_tm(if short { "%a" } else { "%A" }, wd as i32, 0);
    if s.is_empty() {
        let n = DAYS_EN[wd as usize];
        return if short { n[..3].to_string() } else { n.to_string() };
    }
    s
}

/// A month's name in the language (1 … 12), as a date writes it ("4 November"); short "Nov"
pub fn month_name(m: u32, short: bool) -> String {
    let m = (m.clamp(1, 12) - 1) as i32;
    let s = fmt_tm(if short { "%b" } else { "%B" }, 0, m);
    if s.is_empty() {
        let n = MONTHS_EN[m as usize];
        return if short { n[..3].to_string() } else { n.to_string() };
    }
    s
}

/// A month's name on its own, as a title ("November 2026"): the
/// nominative where the language has one (%OB: Russian, Polish…)
pub fn month_title(m: u32) -> String {
    let s = fmt_tm("%OB", 0, (m.clamp(1, 12) - 1) as i32);
    if s.is_empty() || s.starts_with('%') { month_name(m, false) } else { s }
}

/// The calendar's header letters, a week from `start` (0 Sunday): "M T W T F S S"
pub fn weekday_letters(start: u32) -> Vec<String> {
    letters_of(&(0..7).map(|i| day_name((start + i) % 7, true)).collect::<Vec<_>>())
}

/// One letter per short name: the first, upper case; when they'd all be the
/// same (Chinese "周一" … "周日"), the last
pub fn letters_of(names: &[String]) -> Vec<String> {
    let first: Vec<String> = names.iter().map(|n| n.trim_matches('.').chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()).collect();
    if first.len() > 1 && first.iter().all(|f| *f == first[0]) {
        return names.iter().map(|n| n.trim_matches('.').chars().last().map(String::from).unwrap_or_default()).collect();
    }
    first
}

/// Where a day falls in a week starting on `start`: 0 … 6 (weekday 0 Sunday … 6 Saturday)
pub fn column(weekday: u32, start: u32) -> u32 {
    (weekday % 7 + 7 - start % 7) % 7
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn week_start_from_glibc() {
        // en_US: 19971130 (a Sunday), first_weekday 1: Sunday
        assert_eq!(week_start_of(19971130, 1), Some(0));
        // de_DE: 19971130, 2: Monday
        assert_eq!(week_start_of(19971130, 2), Some(1));
        // a locale that numbers from Monday: 19971201, 1: Monday
        assert_eq!(week_start_of(19971201, 1), Some(1));
        // fa_IR/ar_EG: Saturday
        assert_eq!(week_start_of(19971130, 7), Some(6));
        assert_eq!(week_start_of(19971130, 0), None);
        assert_eq!(week_start_of(0, 1), None);
    }

    #[test]
    fn twelve_hour_formats() {
        assert!(is_12h_format("%r"));
        assert!(is_12h_format("%I:%M:%S %p %Z"));
        assert!(is_12h_format("%l:%M %p"));
        assert!(!is_12h_format("%H:%M:%S"));
        assert!(!is_12h_format("%T"));
        assert!(!is_named("C"));
        assert!(!is_named("C.UTF-8"));
        assert!(!is_named("POSIX"));
        assert!(is_named("en_US.UTF-8"));
    }

    #[test]
    fn settings_win_over_the_language() {
        assert!(clock24_with(&json!({}), true));
        assert!(!clock24_with(&json!({}), false));
        // (an old false is everyone's old default: the language decides)
        assert!(clock24_with(&json!({"appearance": {"clock24": false}}), true));
        assert!(clock24_with(&json!({"appearance": {"clock24": true}}), false));
        assert!(!clock24_with(&json!({"appearance": {"clock24": true, "clockHours": "12"}}), true));
        assert!(clock24_with(&json!({"appearance": {"clockHours": "24"}}), false));
        assert!(clock24_with(&json!({"appearance": {"clockHours": "auto"}}), true));
        assert_eq!(week_start_with(&json!({}), 0), 0);
        assert_eq!(week_start_with(&json!({"appearance": {"weekStart": "auto"}}), 1), 1);
        assert_eq!(week_start_with(&json!({"appearance": {"weekStart": "sunday"}}), 1), 0);
        assert_eq!(week_start_with(&json!({"appearance": {"weekStart": "monday"}}), 0), 1);
        assert_eq!(week_start_with(&json!({"appearance": {"weekStart": "saturday"}}), 0), 6);
    }

    #[test]
    fn names_and_letters() {
        // (tests run in the C locale: init isn't called)
        assert_eq!(day_name(3, false), "Wednesday");
        assert_eq!(day_name(0, true), "Sun");
        assert_eq!(month_name(11, false), "November");
        assert_eq!(month_name(1, true), "Jan");
        assert_eq!(month_title(11), "November");
        assert_eq!(weekday_letters(1), vec!["M", "T", "W", "T", "F", "S", "S"]);
        assert_eq!(weekday_letters(0), vec!["S", "M", "T", "W", "T", "F", "S"]);
        let zh: Vec<String> = ["周日", "周一", "周二"].iter().map(|s| s.to_string()).collect();
        assert_eq!(letters_of(&zh), vec!["日", "一", "二"]);
        let fr: Vec<String> = ["lun.", "mar."].iter().map(|s| s.to_string()).collect();
        assert_eq!(letters_of(&fr), vec!["L", "M"]);
        assert_eq!(column(1, 1), 0);
        assert_eq!(column(0, 1), 6);
        assert_eq!(column(0, 0), 0);
        assert_eq!(column(5, 6), 6);
    }
}
