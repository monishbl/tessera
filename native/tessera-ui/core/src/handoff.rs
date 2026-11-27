//! Typing in the Overview hands over to Search (27 Nov): the first letter
//! typed there opens Search with it, and the Overview goes.
//!
//! The Overview (surfaces/overview) sends what's typed to Search's part
//! (surfaces/spotlight) as `Typed` messages, in the order typed, and asks
//! Panels for Search (`panels.search [text, screen]`: the text also rides on
//! Search's open command, as "query"). Until Search has the keyboard (the
//! Overview's window loses it to Search's) or `LET_GO` passes, the Overview
//! keeps it, so nothing typed meanwhile reaches the window underneath: its
//! field takes the keys and they go on to Search too. Search holds what
//! came before it opened (`Inbox`) and takes it as it opens.

use std::time::{Duration, Instant};

/// Search's part (the surface's name)
pub const SEARCH: &str = "spotlight";

/// The most the Overview keeps the keyboard for, handing over
pub const LET_GO: Duration = Duration::from_millis(400);

/// What came before Search opened is for this opening only if this fresh
pub const HOLD: Duration = Duration::from_secs(2);

/// What the Overview passes on to Search, in the order typed
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    /// the field's text now (after each key typed into it, Backspace too)
    Text(String),
    /// a key the field doesn't type (Return, Escape, Up, Down)
    Key(String),
}

/// Text typed into the Overview's field starts the hand-over (a space
/// alone doesn't: there's nothing to search for)
pub fn starts(text: &str) -> bool {
    !text.trim().is_empty()
}

/// Search's field after the Overview's newer text: `Some` to show, `None` to
/// keep what's there. `last` is the text the Overview sent before: while
/// the field still shows it, the newer one replaces it (Backspace too);
/// once something was typed into Search itself, only a text that carries it
/// on does (never anything that would undo a key typed there).
pub fn merge(current: &str, last: &str, newer: &str) -> Option<String> {
    if newer == current {
        return None;
    }
    if current == last || newer.starts_with(current) {
        return Some(newer.to_string());
    }
    None
}

/// Search's side: what came from the Overview before it opened, held for
/// the opening that follows (`HOLD`)
#[derive(Default, Debug)]
pub struct Inbox {
    items: Vec<Typed>,
    at: Option<Instant>,
}

impl Inbox {
    pub fn push(&mut self, t: Typed, now: Instant) {
        if self.at.is_some_and(|a| now.duration_since(a) > HOLD) {
            self.items.clear();
        }
        // (a newer text replaces the one before it, as long as no key came between)
        if let (Typed::Text(_), Some(Typed::Text(_))) = (&t, self.items.last()) {
            self.items.pop();
        }
        self.items.push(t);
        self.at = Some(now);
    }

    /// What's for this opening, in order (nothing if it's old)
    pub fn take(&mut self, now: Instant) -> Vec<Typed> {
        let fresh = self.at.is_some_and(|a| now.duration_since(a) <= HOLD);
        self.at = None;
        let items = std::mem::take(&mut self.items);
        if fresh {
            items
        } else {
            Vec::new()
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_starts_it_a_space_alone_doesnt() {
        assert!(starts("f"));
        assert!(starts(" f"));
        assert!(starts("1"));
        assert!(starts("é"));
        assert!(!starts(""));
        assert!(!starts(" "));
        assert!(!starts("\t "));
    }

    #[test]
    fn the_overviews_text_follows_until_search_is_typed_into() {
        // the first text, then each key
        assert_eq!(merge("", "", "f").as_deref(), Some("f"));
        assert_eq!(merge("f", "f", "fi").as_deref(), Some("fi"));
        // Backspace in the Overview's field
        assert_eq!(merge("fir", "fir", "fi").as_deref(), Some("fi"));
        // the same again: nothing to do
        assert_eq!(merge("fi", "fi", "fi"), None);
        // typed into Search itself ("fix"): a text that carries it on still comes...
        assert_eq!(merge("fix", "fi", "fixe").as_deref(), Some("fixe"));
        // ...one that would undo it doesn't
        assert_eq!(merge("fix", "fi", "fir"), None);
        assert_eq!(merge("fix", "fi", "f"), None);
    }

    #[test]
    fn keys_typed_before_search_opens_wait_for_it_in_order() {
        let t0 = Instant::now();
        let mut b = Inbox::default();
        b.push(Typed::Text("f".into()), t0);
        b.push(Typed::Text("fi".into()), t0 + Duration::from_millis(30));
        b.push(Typed::Text("fir".into()), t0 + Duration::from_millis(60));
        b.push(Typed::Key("\n".into()), t0 + Duration::from_millis(90));
        b.push(Typed::Text("fire".into()), t0 + Duration::from_millis(120));
        // (texts in a row: the newest; a key keeps its place)
        assert_eq!(b.take(t0 + Duration::from_millis(150)), vec![Typed::Text("fir".into()), Typed::Key("\n".into()), Typed::Text("fire".into())]);
        // taken: nothing for the next opening
        assert!(b.take(t0 + Duration::from_millis(160)).is_empty());
    }

    #[test]
    fn what_came_long_ago_isnt_for_this_opening() {
        let t0 = Instant::now();
        let mut b = Inbox::default();
        b.push(Typed::Text("old".into()), t0);
        assert!(b.take(t0 + HOLD + Duration::from_millis(1)).is_empty());
        // an old one is dropped when a new one comes
        b.push(Typed::Text("old".into()), t0);
        b.push(Typed::Key("\n".into()), t0 + HOLD + Duration::from_secs(1));
        assert_eq!(b.take(t0 + HOLD + Duration::from_secs(1)), vec![Typed::Key("\n".into())]);
        b.push(Typed::Text("x".into()), t0);
        b.clear();
        assert!(b.take(t0).is_empty());
    }

    #[test]
    fn the_keyboard_is_let_go_soon_after() {
        assert!(LET_GO <= Duration::from_millis(500) && LET_GO < HOLD);
    }
}
