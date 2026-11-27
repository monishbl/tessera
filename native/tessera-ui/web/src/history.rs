//! The local history the address field completes from: the pages opened
//! here, kept in the data folder (history.json, readable only by you) and
//! never sent anywhere. Nothing typed leaves the computer until Enter.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// How many pages are kept (the oldest go first)
pub const KEEP: usize = 2000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub visits: u32,
    /// seconds since the epoch
    #[serde(default)]
    pub last: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub items: Vec<Visit>,
}

/// Only web pages are kept (not files, sign-in pages' answers, data: or
/// about: pages), and none with a password in the address
pub fn worth_keeping(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")) && url.len() <= 2048 && !crate::nav::host_of(url).is_empty() && !url.split_once("://").is_some_and(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or("").contains('@'))
}

/// An address for matching: no scheme, no www., lower case
fn key(url: &str) -> String {
    let s = url.split_once("://").map_or(url, |(_, r)| r);
    let s = s.strip_prefix("www.").unwrap_or(s);
    s.to_lowercase()
}

impl History {
    pub fn load(path: &Path) -> Self {
        let items: Vec<Visit> = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { items: items.into_iter().filter(|v| worth_keeping(&v.url)).collect() }
    }

    /// Written whole (a temporary file renamed over it), readable only by you
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(&serde_json::to_vec(&self.items).map_err(std::io::Error::other)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }

    /// A page was opened (its title may come later: `titled`)
    pub fn record(&mut self, url: &str, title: &str, now: u64) {
        if !worth_keeping(url) {
            return;
        }
        if let Some(v) = self.items.iter_mut().find(|v| v.url == url) {
            v.visits = v.visits.saturating_add(1);
            v.last = now;
            if !title.trim().is_empty() {
                v.title = title.trim().to_string();
            }
        } else {
            self.items.push(Visit { url: url.to_string(), title: title.trim().to_string(), visits: 1, last: now });
        }
        if self.items.len() > KEEP {
            self.items.sort_by(|a, b| b.last.cmp(&a.last));
            self.items.truncate(KEEP);
        }
    }

    /// The page's title, once it has one
    pub fn titled(&mut self, url: &str, title: &str) -> bool {
        match self.items.iter_mut().find(|v| v.url == url) {
            Some(v) if !title.trim().is_empty() && v.title != title.trim() => {
                v.title = title.trim().to_string();
                true
            }
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The pages the typed text may mean, best first: an address that starts
    /// with it, then a title word that starts with it, then an address that
    /// has it anywhere; within each, the often and lately visited first
    pub fn suggest(&self, query: &str, limit: usize, now: u64) -> Vec<Visit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let q_key = key(&q);
        let mut scored: Vec<(u8, f64, &Visit)> = Vec::new();
        for v in &self.items {
            let k = key(&v.url);
            let title = v.title.to_lowercase();
            let tier = if k.starts_with(&q_key) {
                3
            } else if title.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(&q)) || title.starts_with(&q) {
                2
            } else if k.contains(&q_key) || (q.len() >= 3 && title.contains(&q)) {
                1
            } else {
                continue;
            };
            let days = now.saturating_sub(v.last) as f64 / 86400.0;
            let frecency = (v.visits.max(1) as f64) / (1.0 + days);
            scored.push((tier, frecency, v));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)).then(a.2.url.len().cmp(&b.2.url.len())));
        scored.into_iter().take(limit).map(|(_, _, v)| v.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86400;

    fn h() -> History {
        let mut h = History::default();
        h.record("https://www.rust-lang.org/learn", "Learn Rust", 10 * DAY);
        h.record("https://docs.rs/webkit6", "webkit6 - Rust", 10 * DAY);
        h.record("https://example.com/", "Example Domain", 9 * DAY);
        h.record("https://news.example.org/rust-release", "Rust 2.0 is out", 1 * DAY);
        h
    }

    #[test]
    fn only_web_pages_are_kept() {
        assert!(worth_keeping("https://example.com/x"));
        assert!(!worth_keeping("file:///home/me/a.html"));
        assert!(!worth_keeping("about:blank"));
        assert!(!worth_keeping("data:text/html,hi"));
        assert!(!worth_keeping("https://user:secret@example.com/"));
        let mut h = History::default();
        h.record("file:///x", "", 1);
        assert!(h.items.is_empty());
    }

    #[test]
    fn an_address_that_starts_with_the_text_comes_first() {
        let s = h().suggest("ru", 5, 10 * DAY);
        assert_eq!(s[0].url, "https://www.rust-lang.org/learn");
        // (then title words starting with it, the lately visited first)
        assert_eq!(s[1].url, "https://docs.rs/webkit6");
        assert_eq!(s[2].url, "https://news.example.org/rust-release");
        assert!(h().suggest("exa", 5, 10 * DAY).iter().any(|v| v.url == "https://example.com/"));
        assert!(h().suggest("www.exa", 5, 10 * DAY)[0].url == "https://example.com/");
        assert!(h().suggest("zzz", 5, 10 * DAY).is_empty());
        assert!(h().suggest("  ", 5, 10 * DAY).is_empty());
        assert_eq!(h().suggest("e", 2, 10 * DAY).len(), 2);
    }

    #[test]
    fn visits_count_and_titles_come_later() {
        let mut h = h();
        h.record("https://example.com/", "", 11 * DAY);
        let v = h.items.iter().find(|v| v.url == "https://example.com/").unwrap();
        assert_eq!((v.visits, v.last, v.title.as_str()), (2, 11 * DAY, "Example Domain"));
        assert!(h.titled("https://example.com/", "Example"));
        assert!(!h.titled("https://example.com/", "Example"));
        assert!(!h.titled("https://nowhere/", "x"));
    }

    #[test]
    fn saved_and_read_back() {
        let dir = std::env::temp_dir().join(format!("tessera-web-test-{}", std::process::id()));
        let path = dir.join("history.json");
        let h = h();
        h.save(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(History::load(&path), h);
        std::fs::write(&path, b"not json").unwrap();
        assert!(History::load(&path).items.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_oldest_go_past_the_limit() {
        let mut h = History::default();
        for i in 0..(KEEP + 5) {
            h.record(&format!("https://e.com/{i}"), "", i as u64);
        }
        assert_eq!(h.items.len(), KEEP);
        assert!(!h.items.iter().any(|v| v.url == "https://e.com/0"));
    }
}
