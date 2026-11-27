//! Typing in the Overview searches in place (27 Nov): its field is Search's
//! own (tessera_spotlight::search), and what it types decides the mode.
//!   nothing typed   the spaces: the arrows pick one, Enter goes there, Esc closes
//!   a character     search mode: the spaces fade and step back, the field
//!                   rises to Search's place, the results grow in under it;
//!                   the keys are Search's (Enter runs the row, then the
//!                   Overview closes as Search does)
//!   emptied         back to the spaces (the results shrink away, the field
//!                   comes back down); Esc with text empties it, a second
//!                   Esc closes
//! A space alone isn't something to search for: it's taken out again.

/// What a change to the field's text does
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// nothing
    None,
    /// a space alone: the field emptied again
    Clear,
    /// the first character: search mode, with this text
    Start,
    /// typed on (Backspace too) while searching
    Type,
    /// emptied: back to the spaces (`clear`: spaces are left in the field)
    Stop { clear: bool },
}

/// What Esc does
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Esc {
    /// the text goes (back to the spaces)
    Clear,
    /// the Overview closes
    Close,
}

/// What Enter (and the arrows) act on
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    /// Search's results
    Search,
    /// the spaces and windows
    Spaces,
}

/// The Overview's search mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub on: bool,
}

impl Mode {
    /// The field's text is now `text`
    pub fn edited(&mut self, text: &str) -> Edit {
        let want = !text.trim().is_empty();
        match (self.on, want) {
            (false, true) => {
                self.on = true;
                Edit::Start
            }
            (true, true) => Edit::Type,
            (true, false) => {
                self.on = false;
                Edit::Stop { clear: !text.is_empty() }
            }
            (false, false) if text.is_empty() => Edit::None,
            (false, false) => Edit::Clear,
        }
    }

    pub fn escape(&self) -> Esc {
        if self.on {
            Esc::Clear
        } else {
            Esc::Close
        }
    }

    pub fn keys(&self) -> Keys {
        if self.on {
            Keys::Search
        } else {
            Keys::Spaces
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_character_searches_and_emptying_comes_back() {
        let mut m = Mode::default();
        assert_eq!(m.keys(), Keys::Spaces);
        assert_eq!(m.edited("f"), Edit::Start);
        assert!(m.on);
        assert_eq!(m.keys(), Keys::Search);
        assert_eq!(m.edited("fi"), Edit::Type);
        assert_eq!(m.edited("f"), Edit::Type);
        // Backspace to empty: the spaces again
        assert_eq!(m.edited(""), Edit::Stop { clear: false });
        assert!(!m.on);
        assert_eq!(m.keys(), Keys::Spaces);
        // and nothing more for an empty field
        assert_eq!(m.edited(""), Edit::None);
        // typing again searches again
        assert_eq!(m.edited("w"), Edit::Start);
    }

    #[test]
    fn a_space_alone_isnt_a_search() {
        let mut m = Mode::default();
        assert_eq!(m.edited(" "), Edit::Clear);
        assert!(!m.on);
        assert_eq!(m.edited(" f"), Edit::Start);
        // (" f" back to " ": the spaces, the field emptied)
        assert_eq!(m.edited(" "), Edit::Stop { clear: true });
        assert!(!m.on);
    }

    #[test]
    fn esc_twice_clears_then_closes() {
        let mut m = Mode::default();
        assert_eq!(m.escape(), Esc::Close);
        m.edited("water");
        assert_eq!(m.escape(), Esc::Clear);
        // (the Overview empties the field: that edit stops the search)
        assert_eq!(m.edited(""), Edit::Stop { clear: false });
        assert_eq!(m.escape(), Esc::Close);
    }

    #[test]
    fn enter_runs_the_result_while_searching_else_picks_the_space() {
        let mut m = Mode::default();
        assert_eq!(m.keys(), Keys::Spaces);
        m.edited("fire");
        assert_eq!(m.keys(), Keys::Search);
        m.edited("");
        assert_eq!(m.keys(), Keys::Spaces);
    }
}
