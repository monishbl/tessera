//! The reading list: links kept to read later (the share sheet's "Read
//! later"), shown in Search's "Reading list" section and on the Today
//! screen, opened in reading mode in tessera-web (`tessera-web --reader
//! URL`, weblink::reader). One file, ~/.local/share/tessera/web/
//! reading-list.json (0600, beside tessera-web's own data): a list of
//! {url, title, added (ms), read (bool, left out while unread)}. Never sent
//! anywhere.
//!
//! Every reader reads the file again when it shows the list (it's small),
//! so a link added from the share sheet is there the next time Search or
//! Today opens; the writes are whole-file (util::write_atomic) under a lock
//! in this process.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How many links it keeps (the oldest read ones go first)
pub const MAX: usize = 500;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub url: String,
    pub title: String,
    /// when it was added (ms since the epoch)
    pub added: i64,
    pub read: bool,
}

impl Item {
    /// The title to show: its own, else one made from the link
    pub fn shown_title(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.trim().to_string();
        }
        title_from_url(&self.url)
    }

    /// The site, for the line under the title ("example.com")
    pub fn site(&self) -> String {
        host(&self.url)
    }
}

/// ~/.local/share/tessera/web/reading-list.json
pub fn path() -> PathBuf {
    crate::util::xdg("XDG_DATA_HOME", ".local/share").join("tessera/web/reading-list.json")
}

/// The list as written (newest first); a missing or broken file is empty
pub fn parse(text: &str) -> Vec<Item> {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let Some(a) = v.as_array() else { return Vec::new() };
    a.iter()
        .filter_map(|e| {
            let url = e["url"].as_str()?.trim().to_string();
            if !crate::weblink::is_web(&url) {
                return None;
            }
            Some(Item { url, title: e["title"].as_str().unwrap_or("").to_string(), added: e["added"].as_i64().unwrap_or(0), read: e["read"].as_bool().unwrap_or(false) })
        })
        .collect()
}

pub fn to_json(list: &[Item]) -> String {
    let a: Vec<Value> = list
        .iter()
        .map(|i| {
            let mut o = json!({"url": i.url, "title": i.title, "added": i.added});
            if i.read {
                o["read"] = json!(true);
            }
            o
        })
        .collect();
    serde_json::to_string_pretty(&a).unwrap_or_else(|_| "[]".into())
}

pub fn load_from(p: &Path) -> Vec<Item> {
    std::fs::read_to_string(p).map(|t| parse(&t)).unwrap_or_default()
}

/// The list now
pub fn load() -> Vec<Item> {
    load_from(&path())
}

/// The unread ones, newest first (what Search and Today show)
pub fn unread() -> Vec<Item> {
    load().into_iter().filter(|i| !i.read).collect()
}

/// Add a link (at the top; one already there moves up and is unread again)
pub fn added(mut list: Vec<Item>, url: &str, title: &str, now: i64) -> Vec<Item> {
    let url = url.trim().to_string();
    let old = list.iter().position(|i| i.url == url).map(|n| list.remove(n));
    let title = if title.trim().is_empty() { old.map(|o| o.title).unwrap_or_default() } else { title.trim().to_string() };
    list.insert(0, Item { url, title, added: now, read: false });
    while list.len() > MAX {
        // (the oldest read one, else the oldest)
        match list.iter().rposition(|i| i.read) {
            Some(n) => {
                list.remove(n);
            }
            None => {
                list.pop();
            }
        }
    }
    list
}

pub fn marked_read(mut list: Vec<Item>, url: &str) -> Vec<Item> {
    for i in list.iter_mut().filter(|i| i.url == url) {
        i.read = true;
    }
    list
}

pub fn removed(mut list: Vec<Item>, url: &str) -> Vec<Item> {
    list.retain(|i| i.url != url);
    list
}

static LOCK: Mutex<()> = Mutex::new(());

/// The file changed by `f`, written whole and private (0600)
pub fn change_at(p: &Path, f: impl FnOnce(Vec<Item>) -> Vec<Item>) -> std::io::Result<()> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let list = f(load_from(p));
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    if !p.exists() {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)?;
    }
    crate::util::write_atomic(p, to_json(&list).as_bytes())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(crate::util::link_target(p), std::fs::Permissions::from_mode(0o600))
}

fn change(f: impl FnOnce(Vec<Item>) -> Vec<Item>) {
    if let Err(e) = change_at(&path(), f) {
        eprintln!("tessera-ui: the reading list wasn't saved: {e}");
    }
}

/// "Read later": a web link onto the list (false: not a web link)
pub fn add(url: &str, title: &str) -> bool {
    if !crate::weblink::is_web(url) {
        return false;
    }
    let (u, t) = (url.trim().to_string(), title.to_string());
    change(move |l| added(l, &u, &t, crate::util::now_ms()));
    true
}

pub fn mark_read(url: &str) {
    change(|l| marked_read(l, url));
}

pub fn remove(url: &str) {
    change(|l| removed(l, url));
}

/// The link's host, without "www."
pub fn host(url: &str) -> String {
    let u = url.trim();
    let rest = u.split_once("://").map_or(u, |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    let h = h.rsplit_once('@').map_or(h, |(_, h)| h);
    h.strip_prefix("www.").unwrap_or(h).to_ascii_lowercase()
}

/// A title from the link alone: its last path part, words for the dashes
/// ("/2026/11/how-to-bake-bread.html" → "How to bake bread"), else the site
pub fn title_from_url(url: &str) -> String {
    let u = url.trim();
    let rest = u.split_once("://").map_or(u, |(_, r)| r);
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let last = path.split('/').skip(1).filter(|s| !s.is_empty()).last().unwrap_or("");
    let stem = last.rsplit_once('.').map_or(last, |(s, ext)| if ext.len() <= 5 && !s.is_empty() { s } else { last });
    let words: String = stem.chars().map(|c| if c == '-' || c == '_' || c == '+' { ' ' } else { c }).collect();
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    // (an id or a number is no title)
    let letters = words.chars().filter(|c| c.is_alphabetic()).count();
    if words.is_empty() || letters < 3 || words.chars().filter(|c| c.is_ascii_digit()).count() > letters {
        return host(url);
    }
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => host(url),
    }
}

/// When it was added, for the line under the title ("Today", "Yesterday",
/// "3 days ago", else the date)
pub fn added_words(added: i64, now: i64) -> String {
    if added <= 0 {
        return String::new();
    }
    let days = (now - added).max(0) / 86_400_000;
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => format!("{days} days ago"),
        _ => crate::util::strftime("%-d %b", added / 1000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_links() {
        assert_eq!(title_from_url("https://www.example.com/2026/11/how-to-bake-bread.html"), "How to bake bread");
        assert_eq!(title_from_url("https://example.com/"), "example.com");
        assert_eq!(title_from_url("https://news.site/a/12345678"), "news.site");
        assert_eq!(title_from_url("https://blog.dev/posts/rust_on_wayland?ref=x#top"), "Rust on wayland");
        assert_eq!(host("https://user@WWW.Example.com:8080/x"), "example.com:8080");
    }

    #[test]
    fn adding_marking_removing() {
        let l = added(Vec::new(), "https://a.b/one", "One", 1);
        let l = added(l, "https://a.b/two", "", 2);
        assert_eq!(l.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), ["https://a.b/two", "https://a.b/one"]);
        let l = marked_read(l, "https://a.b/one");
        assert!(l[1].read);
        // (added again: up top, unread, its title kept)
        let l = added(l, "https://a.b/one", "", 3);
        assert_eq!((l[0].url.as_str(), l[0].title.as_str(), l[0].read, l.len()), ("https://a.b/one", "One", false, 2));
        let l = removed(l, "https://a.b/two");
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn the_file_round_trips_and_stays_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tessera-readlist-{}", std::process::id()));
        let p = dir.join("web/reading-list.json");
        change_at(&p, |l| added(l, "https://a.b/x", "X", 5)).unwrap();
        change_at(&p, |l| added(l, "javascript:alert(1)", "bad", 6)).unwrap();
        let l = load_from(&p);
        // (only web links are read back)
        assert_eq!(l, vec![Item { url: "https://a.b/x".into(), title: "X".into(), added: 5, read: false }]);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(parse("not json").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_is_kept_to_its_most() {
        let mut l = Vec::new();
        for n in 0..MAX + 3 {
            l = added(l, &format!("https://a.b/{n}"), "", n as i64);
        }
        assert_eq!(l.len(), MAX);
        assert_eq!(l[0].url, format!("https://a.b/{}", MAX + 2));
    }

    #[test]
    fn when_it_was_added() {
        let day = 86_400_000;
        assert_eq!(added_words(10 * day, 10 * day + 5), "Today");
        assert_eq!(added_words(10 * day, 11 * day + 5), "Yesterday");
        assert_eq!(added_words(10 * day, 13 * day), "3 days ago");
        assert_eq!(added_words(0, 1), "");
    }
}
