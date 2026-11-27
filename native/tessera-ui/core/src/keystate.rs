//! Caps Lock and the keyboard layout, as the seat's keyboard has them now
//! (#761): the password fields (the lock, the login screen, the admin
//! password dialog) say "Caps Lock is on", and the layout's name when more
//! than one is set up, under the field. With Caps Lock on or the wrong
//! layout, every password was refused with "Incorrect password" and
//! nothing said why; three tries lock the account for 10 minutes.
//!
//! wayland.rs fills it from wl_keyboard's modifiers (the Caps Lock modifier,
//! the active group) and keymap (the groups' names, `name[GroupN]` in its
//! xkb_symbols), then tells the parts (Part::keys_changed). The on-screen
//! keyboard's own keymap names no groups: it's ignored, so the names stay
//! those of the real keyboard while it types.

use std::cell::RefCell;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    /// Caps Lock is on
    pub caps: bool,
    /// the active layout (xkb group, from 0)
    pub group: u32,
    /// the layouts' names, in group order ("English (US)", "Russian")
    pub names: Vec<String>,
}

impl Keys {
    /// The active layout's name when there are several, else ""
    pub fn layout(&self) -> String {
        if self.names.len() < 2 {
            return String::new();
        }
        self.names.get(self.group as usize).cloned().unwrap_or_default()
    }

    /// The line under a password field: "Caps Lock is on", the layout's
    /// name, both, or "" (nothing to say)
    pub fn hint(&self) -> String {
        hint(self.caps, &self.layout())
    }
}

/// A password as Slint's TextInput draws it (one ● a character): what a
/// field measures to keep its end in sight (#760)
pub fn dots(text: &str) -> String {
    "\u{25cf}".repeat(text.chars().count())
}

/// "Caps Lock is on · English (US)", "Caps Lock is on", "English (US)" or ""
pub fn hint(caps: bool, layout: &str) -> String {
    match (caps, layout.is_empty()) {
        (true, false) => format!("Caps Lock is on · {layout}"),
        (true, true) => "Caps Lock is on".into(),
        (false, false) => layout.to_string(),
        (false, true) => String::new(),
    }
}

thread_local! {
    static KEYS: RefCell<Keys> = RefCell::new(Keys::default());
}

/// The keyboard's state now (the UI thread's)
pub fn now() -> Keys {
    KEYS.with(|k| k.borrow().clone())
}

/// The seat's modifiers changed: true when Caps Lock or the layout did
pub fn set_modifiers(caps: bool, group: u32) -> bool {
    KEYS.with(|k| {
        let mut k = k.borrow_mut();
        let changed = k.caps != caps || k.group != group;
        k.caps = caps;
        k.group = group;
        changed
    })
}

/// A new keymap: true when the layouts' names changed. One that names no
/// groups (the on-screen keyboard's) leaves them as they were
pub fn set_keymap(text: &str) -> bool {
    let names = layout_names(text);
    if names.is_empty() {
        return false;
    }
    KEYS.with(|k| {
        let mut k = k.borrow_mut();
        let changed = k.names != names;
        k.names = names;
        changed
    })
}

/// The groups' names in an xkb keymap's text: its `name[Group1]="…";` lines
/// (xkb_symbols), in group order
pub fn layout_names(keymap: &str) -> Vec<String> {
    let mut found: Vec<(u32, String)> = Vec::new();
    for line in keymap.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("name[") else { continue };
        let Some((idx, value)) = rest.split_once(']') else { continue };
        let Some(n) = idx.to_ascii_lowercase().strip_prefix("group").and_then(|n| n.trim().parse::<u32>().ok()) else { continue };
        let Some(v) = value.split_once('"').and_then(|(_, v)| v.rsplit_once('"')).map(|(v, _)| v.to_string()) else { continue };
        if n >= 1 && !found.iter().any(|(i, _)| *i == n) {
            found.push((n, v));
        }
    }
    found.sort_by_key(|(i, _)| *i);
    found.into_iter().map(|(_, v)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "xkb_keymap {\nxkb_types \"complete\" {\n\tlevel_name[Level1]= \"Base\";\n};\nxkb_symbols \"pc+us+ru:2+inet(evdev)\" {\n\tname[Group1]=\"English (US)\";\n\tname[Group2]=\"Russian\";\n\tkey <AE01> { [ 1, exclam ] };\n};\n};\n";

    #[test]
    fn dots_as_the_field_draws_them() {
        assert_eq!(dots(""), "");
        assert_eq!(dots("pässwörd"), "\u{25cf}".repeat(8));
    }

    #[test]
    fn names_from_the_keymap() {
        assert_eq!(layout_names(TWO), vec!["English (US)", "Russian"]);
        // (a level's name is no layout's; lower case and out of order are read)
        assert_eq!(layout_names("name[group2]=\"B\";\nname[Group1] = \"A\";\nlevel_name[Level2]=\"Shift\";"), vec!["A", "B"]);
        assert!(layout_names("xkb_symbols \"(unnamed)\" {\nkey <K1> {[ a ]};\n};").is_empty());
    }

    #[test]
    fn the_line_under_the_field() {
        let mut k = Keys { caps: false, group: 1, names: vec!["English (US)".into(), "Russian".into()] };
        assert_eq!(k.hint(), "Russian");
        k.caps = true;
        assert_eq!(k.hint(), "Caps Lock is on · Russian");
        // one layout: its name isn't said
        k.names.truncate(1);
        k.group = 0;
        assert_eq!(k.hint(), "Caps Lock is on");
        k.caps = false;
        assert_eq!(k.hint(), "");
        // (a group past the names: nothing)
        assert_eq!(Keys { caps: false, group: 5, names: vec!["A".into(), "B".into()] }.layout(), "");
    }

    #[test]
    fn the_on_screen_keyboards_keymap_keeps_the_names() {
        assert!(set_keymap(TWO));
        assert!(!set_keymap(TWO), "the same again: no change");
        assert!(!set_keymap("xkb_symbols \"(unnamed)\" {\nkey <K1> {[ a ]};\n};"));
        assert_eq!(now().names.len(), 2);
        assert!(set_modifiers(true, 1));
        assert!(!set_modifiers(true, 1));
        assert_eq!(now().hint(), "Caps Lock is on · Russian");
    }
}
