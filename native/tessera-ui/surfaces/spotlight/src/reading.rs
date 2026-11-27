//! Search's "Reading list" section: the links kept with the share sheet's
//! Read later (tessera_core::readlist), unread, newest first. Shown when the
//! field is empty or asks for it ("reading list", "read later"); a link
//! whose title has what's typed shows after the other results. Enter opens
//! it in reading mode (tessera-web --reader); Ctrl+D marks it read and
//! Ctrl+Delete takes it off the list, while it's the selected row.
//!
//! Kept apart from the rest of Search (rank.rs ranks everything else): the
//! rows are added to the ranking's here, in one place.

use tessera_core::rank::{Row, Run};
use tessera_core::readlist::{self, Item};

pub const SECTION: &str = "Reading list";
/// The most shown for an empty field or the section asked for
const MOST: usize = 8;
/// The most shown after other results, for a title that matches
const MATCHED: usize = 3;

/// Whether what's typed asks for the list itself
pub fn asks_for_it(q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.len() < 4 {
        return false;
    }
    ["reading list", "read later"].iter().any(|w| w.starts_with(&q) || q.starts_with(w))
}

fn row(i: &Item, now: i64) -> Row {
    let when = readlist::added_words(i.added, now);
    let site = i.site();
    let sub = if when.is_empty() { site } else { format!("{site} · {when}") };
    Row {
        section: SECTION.into(),
        title: i.shown_title(),
        subtitle: format!("{sub} · Ctrl+D marks it read"),
        glyph: "book-open".into(),
        run: Run::Read(i.url.clone()),
        ..Default::default()
    }
}

/// The ranking's rows with the reading list's: first for an empty field or
/// when asked for, else the ones whose titles match, last
pub fn merge(text: &str, items: &[Item], rows: &mut Vec<Row>, now: i64) {
    let unread: Vec<&Item> = items.iter().filter(|i| !i.read).collect();
    if unread.is_empty() {
        return;
    }
    let q = text.trim();
    // (the other prefixes are Search's own modes: AI, commands, emoji, files)
    if q.starts_with(['?', '>', ':', '/']) {
        return;
    }
    if q.is_empty() || asks_for_it(q) {
        let mine: Vec<Row> = unread.iter().take(MOST).map(|i| row(i, now)).collect();
        rows.splice(0..0, mine);
        return;
    }
    let low = q.to_lowercase();
    if low.chars().count() < 3 {
        return;
    }
    let found: Vec<Row> = unread.iter().filter(|i| i.shown_title().to_lowercase().contains(&low) || i.site().contains(&low)).take(MATCHED).map(|i| row(i, now)).collect();
    rows.extend(found);
}

/// The selected row's link, when it's one of the list's
pub fn url_of(r: &Row) -> Option<&str> {
    match &r.run {
        Run::Read(u) if r.section == SECTION => Some(u.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(url: &str, title: &str, read: bool) -> Item {
        Item { url: url.into(), title: title.into(), added: 1000, read }
    }

    #[test]
    fn shown_for_an_empty_field_or_when_asked() {
        let items = [item("https://a.b/x", "Rust on Wayland", false), item("https://c.d/y", "", true)];
        let mut rows = Vec::new();
        merge("", &items, &mut rows, 2000);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].section.as_str(), rows[0].title.as_str(), &rows[0].run), (SECTION, "Rust on Wayland", &Run::Read("https://a.b/x".into())));
        assert_eq!(rows[0].subtitle, "a.b · Today · Ctrl+D marks it read");
        let mut rows = vec![Row { section: "Applications".into(), ..Default::default() }];
        merge("reading", &items, &mut rows, 2000);
        assert_eq!(rows[0].section, SECTION);
        assert!(asks_for_it("Read later") && asks_for_it("reading list please") && !asks_for_it("rea"));
    }

    #[test]
    fn a_matching_title_comes_last() {
        let items = [item("https://a.b/x", "Rust on Wayland", false)];
        let mut rows = vec![Row { section: "Applications".into(), ..Default::default() }];
        merge("wayl", &items, &mut rows, 2000);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].section, SECTION);
        assert_eq!(url_of(&rows[1]), Some("https://a.b/x"));
        assert_eq!(url_of(&rows[0]), None);
        // (too short, another mode, or nothing unread: nothing added)
        let mut rows = Vec::new();
        merge("wa", &items, &mut rows, 0);
        merge("> wayland", &items, &mut rows, 0);
        merge("", &[item("https://a.b", "", true)], &mut rows, 0);
        assert!(rows.is_empty());
    }
}
