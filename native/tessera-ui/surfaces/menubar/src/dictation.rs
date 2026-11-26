//! Dictation's live activity in the island (host/desktop dictation.rs and
//! dictation_live.rs say what's happening): it springs open into a card when
//! you start, and shrinks to the pill after 3 s of you speaking (a hover or
//! a click opens it again); "Writing…" and the typed words open it again.
//!
//! The wave: the microphone's level, told 20 times a second, eased (quick up,
//! slow down) and kept for the last ~3 s; drawn as a soft ribbon that rolls
//! right to left (island.slint slides it one step between levels), and the
//! pill's own little one. With Reduce motion the card shows a still level bar.

use std::collections::VecDeque;
use std::fmt::Write;

/// The card's wave: its points (one more than shows: it slides in from the right)
pub const WAVE_N: usize = 64;
/// The pill's little wave
pub const MINI_N: usize = 13;
/// One level each 50 ms
pub const FRAME_MS: f64 = 50.0;
/// A level this high is you speaking (for the shrink)
pub const SPEECH: f32 = 0.18;
/// It shrinks to the pill after this much speech
pub const SHRINK_AFTER_MS: f64 = 3000.0;
/// The last seconds of the time limit, warned about
pub const WARN_S: f64 = 10.0;
/// The ribbon's viewbox: STEP across per point, HEIGHT high
pub const STEP: f32 = 10.0;
pub const HEIGHT: f32 = 100.0;

/// The levels heard, newest last
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wave {
    hist: VecDeque<f32>,
    eased: f32,
}

impl Wave {
    /// A new level (0..1): eased, kept, and the eased one back
    pub fn push(&mut self, raw: f32) -> f32 {
        let raw = if raw.is_finite() { raw.clamp(0.0, 1.0) } else { 0.0 };
        // (up quickly, as a voice starts; down slowly, so it doesn't flicker between words)
        let k = if raw > self.eased { 0.65 } else { 0.25 };
        self.eased += (raw - self.eased) * k;
        self.hist.push_back(self.eased);
        while self.hist.len() > WAVE_N {
            self.hist.pop_front();
        }
        self.eased
    }
    pub fn clear(&mut self) {
        self.hist.clear();
        self.eased = 0.0;
    }
    pub fn level(&self) -> f32 {
        self.eased
    }
    pub fn len(&self) -> usize {
        self.hist.len()
    }
    pub fn is_empty(&self) -> bool {
        self.hist.is_empty()
    }
    /// The newest `n` levels, silence before the first
    pub fn tail(&self, n: usize) -> Vec<f32> {
        let have = self.hist.len().min(n);
        let mut v = vec![0.0; n - have];
        v.extend(self.hist.iter().skip(self.hist.len() - have));
        v
    }
    /// The ribbon of the newest `n` levels (see `ribbon`)
    pub fn path(&self, n: usize) -> String {
        ribbon(&self.tail(n))
    }
}

/// Half the ribbon's height at a level, in the viewbox (a hairline when quiet)
fn half(v: f32) -> f32 {
    1.5 + (HEIGHT / 2.0 - 4.0 - 1.5) * v.clamp(0.0, 1.0)
}

/// A soft ribbon through levels, as an SVG path in a viewbox
/// `(len - 1) * STEP` by `HEIGHT`: mirrored about the middle, its oldest end
/// tapering in, curved through the midpoints between levels
pub fn ribbon(levels: &[f32]) -> String {
    let n = levels.len();
    if n < 2 {
        return String::new();
    }
    let mid = HEIGHT / 2.0;
    let taper = 6.0f32;
    let pts: Vec<(f32, f32)> = levels.iter().enumerate().map(|(i, &v)| (i as f32 * STEP, half(v) * (i as f32 / taper).min(1.0).max(0.25))).collect();
    let top: Vec<(f32, f32)> = pts.iter().map(|&(x, h)| (x, mid - h)).collect();
    let bottom: Vec<(f32, f32)> = pts.iter().rev().map(|&(x, h)| (x, mid + h)).collect();
    let mut s = String::with_capacity(n * 40);
    let _ = write!(s, "M {:.1} {:.1}", top[0].0, top[0].1);
    smooth(&mut s, &top);
    let _ = write!(s, " L {:.1} {:.1}", bottom[0].0, bottom[0].1);
    smooth(&mut s, &bottom);
    s.push_str(" Z");
    s
}

/// Quadratic curves through the midpoints, ending on the last point
fn smooth(s: &mut String, pts: &[(f32, f32)]) {
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let _ = write!(s, " Q {:.1} {:.1} {:.1} {:.1}", a.0, a.1, (a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    }
    if let Some(l) = pts.last() {
        let _ = write!(s, " L {:.1} {:.1}", l.0, l.1);
    }
}

/// The card: open or the pill, and the mic and language list
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    /// the host's stage: listening | writing | done | "" (none)
    pub stage: String,
    session: i64,
    speech_ms: f64,
    open: bool,
    /// the mic and language list is open; its page ("" the mics, "lang")
    pub picking: bool,
    pub page: String,
}

impl Card {
    /// The host's stage and dictation, as the state says: true when it
    /// opened or closed
    pub fn follow(&mut self, stage: &str, session: i64) -> bool {
        let was = self.open;
        if stage != self.stage || session != self.session {
            match stage {
                // (a new dictation: open, counting speech from nothing)
                "listening" => {
                    self.open = true;
                    self.speech_ms = 0.0;
                }
                "writing" | "done" => self.open = true,
                _ => self.open = false,
            }
            if stage != "listening" {
                self.picking = false;
                self.page.clear();
            }
            self.stage = stage.to_string();
            self.session = session;
        }
        was != self.open
    }
    /// A level heard (one each FRAME_MS): true when it shrinks to the pill
    pub fn heard(&mut self, level: f32) -> bool {
        if self.stage != "listening" || !self.open || self.picking {
            return false;
        }
        if level >= SPEECH {
            self.speech_ms += FRAME_MS;
        }
        if self.speech_ms >= SHRINK_AFTER_MS {
            self.open = false;
            return true;
        }
        false
    }
    /// Open by itself (not a hover): the island shows the card
    pub fn open(&self) -> bool {
        self.open
    }
    /// The mic and language line clicked: the list opens (and the card stays
    /// open while it is) or closes (open for another 3 s of speech)
    pub fn toggle_pick(&mut self) {
        if self.stage != "listening" {
            return;
        }
        self.picking = !self.picking;
        self.page.clear();
        self.open = true;
        self.speech_ms = 0.0;
    }
    /// Something in the list picked: it closes, the card stays a while
    pub fn picked(&mut self) {
        self.picking = false;
        self.page.clear();
        self.speech_ms = 0.0;
    }
}

/// How much of the time limit is gone (the ring), 0..1
pub fn used(elapsed_s: f64, max_s: f64) -> f32 {
    if max_s <= 0.0 {
        return 0.0;
    }
    (elapsed_s / max_s).clamp(0.0, 1.0) as f32
}

/// In the last WARN_S seconds: how many are left ("8 s left")
pub fn left(elapsed_s: f64, max_s: f64) -> Option<String> {
    let rest = (max_s - elapsed_s).max(0.0);
    (max_s > 0.0 && rest <= WARN_S).then(|| format!("{} s left", rest.ceil() as i64))
}

/// The key hint for how it was started
pub fn hint(mode: &str) -> &'static str {
    if mode == "hold" {
        "Release to type · Esc to cancel"
    } else {
        "Press again to stop · Esc to cancel"
    }
}

/// The newest end of the words so far, about `max` characters (two lines of
/// the card), from a word's start: "…and send it today"
pub fn tail_words(text: &str, max: usize) -> String {
    let text = text.trim();
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let tail: String = text.chars().skip(n - max).collect();
    let from_word = match tail.find(char::is_whitespace) {
        Some(i) if i + 1 < tail.len() => tail[i..].trim_start().to_string(),
        _ => tail,
    };
    format!("…{from_word}")
}

/// The languages Settings → Dictation offers (pages/input/dictation.rs's)
pub const LANGUAGES: &[(&str, &str)] = &[
    ("", "Same as the system"),
    ("auto", "Automatic"),
    ("en", "English"),
    ("hi", "Hindi"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("ta", "Tamil"),
    ("te", "Telugu"),
    ("bn", "Bengali"),
    ("ur", "Urdu"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
];

/// A language's name, for the code whisper listens for ("en": English)
pub fn language_name(code: &str) -> String {
    if code.is_empty() || code == "auto" {
        return "Automatic".into();
    }
    LANGUAGES.iter().find(|l| l.0 == code).map(|l| l.1.to_string()).unwrap_or_else(|| code.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_eased_and_kept() {
        let mut w = Wave::default();
        let up = w.push(1.0);
        assert!(up > 0.6 && up < 0.7, "{up}");
        let down = w.push(0.0);
        assert!(down < up && down > 0.4, "slow down: {down}");
        let nonsense = w.push(f32::NAN); // (is silence)
        assert!(nonsense < down && nonsense == w.level());
        for _ in 0..200 {
            w.push(0.5);
        }
        assert_eq!(w.len(), WAVE_N);
        assert!((w.level() - 0.5).abs() < 1e-3);
        w.clear();
        assert!(w.is_empty());
        assert_eq!(w.level(), 0.0);
    }

    #[test]
    fn the_tail_is_padded_with_silence() {
        let mut w = Wave::default();
        w.push(1.0);
        w.push(1.0);
        let t = w.tail(5);
        assert_eq!(t.len(), 5);
        assert!(t[..3].iter().all(|&x| x == 0.0));
        assert!(t[4] > t[3]);
        assert_eq!(w.tail(1).len(), 1);
    }

    #[test]
    fn the_ribbon_spans_its_viewbox() {
        assert_eq!(ribbon(&[0.5]), "");
        let p = ribbon(&[0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
        assert!(p.starts_with("M 0.0 "), "{p}");
        assert!(p.ends_with(" Z"));
        // its right end at (n-1) steps, never taller than the box
        assert!(p.contains(&format!(" {:.1} ", 7.0 * STEP)));
        for n in p.split(' ').filter_map(|t| t.parse::<f32>().ok()) {
            assert!((0.0..=HEIGHT.max(7.0 * STEP)).contains(&n), "{n}");
        }
        // louder is taller: the top edge of a loud wave goes higher
        let quiet = ribbon(&[0.1; 10]);
        let loud = ribbon(&[0.9; 10]);
        let top = |p: &str| p.split(' ').filter_map(|t| t.parse::<f32>().ok()).skip(1).step_by(2).fold(HEIGHT, f32::min);
        assert!(top(&loud) < top(&quiet));
        // silence is a hairline, not nothing
        assert!(top(&ribbon(&[0.0; 10])) < HEIGHT / 2.0);
    }

    #[test]
    fn it_opens_then_shrinks_after_three_seconds_of_speech() {
        let mut c = Card::default();
        assert!(c.follow("listening", 1));
        assert!(c.open());
        // quiet doesn't count
        for _ in 0..200 {
            assert!(!c.heard(0.05));
        }
        assert!(c.open());
        // 3 s of speech (60 levels), with pauses between: then the pill
        let mut shrunk_at = None;
        for i in 0..200 {
            let l = if i % 2 == 0 { 0.6 } else { 0.0 };
            if c.heard(l) {
                shrunk_at = Some(i);
                break;
            }
        }
        assert_eq!(shrunk_at, Some(2 * (SHRINK_AFTER_MS / FRAME_MS) as usize - 2));
        assert!(!c.open());
        assert!(!c.heard(0.9)); // (once)
        // the same dictation again: stays as it is
        assert!(!c.follow("listening", 1));
        // writing opens it again, then done, then it goes
        assert!(c.follow("writing", 1));
        assert!(!c.follow("done", 1));
        assert!(c.open());
        assert!(c.follow("", 1));
        // the next dictation starts open
        assert!(c.follow("listening", 2));
    }

    #[test]
    fn the_list_keeps_it_open() {
        let mut c = Card::default();
        c.follow("listening", 1);
        c.toggle_pick();
        assert!(c.picking);
        for _ in 0..200 {
            assert!(!c.heard(0.9));
        }
        c.picked();
        assert!(!c.picking && c.open());
        // writing closes the list
        c.toggle_pick();
        c.follow("writing", 1);
        assert!(!c.picking);
        c.toggle_pick(); // (not while writing)
        assert!(!c.picking);
    }

    #[test]
    fn the_time_limit() {
        assert_eq!(used(30.0, 120.0), 0.25);
        assert_eq!(used(500.0, 120.0), 1.0);
        assert_eq!(used(5.0, 0.0), 0.0);
        assert_eq!(left(100.0, 120.0), None);
        assert_eq!(left(110.0, 120.0).as_deref(), Some("10 s left"));
        assert_eq!(left(111.5, 120.0).as_deref(), Some("9 s left"));
        assert_eq!(left(130.0, 120.0).as_deref(), Some("0 s left"));
    }

    #[test]
    fn the_newest_words_show() {
        assert_eq!(tail_words("  Hello there  ", 90), "Hello there");
        // (the last 9 are "hree four": from the next whole word)
        assert_eq!(tail_words("one two three four", 9), "…four");
        assert_eq!(tail_words("one two three four", 11), "…three four");
        let long = "word ".repeat(60);
        let t = tail_words(&long, 90);
        assert!(t.starts_with("…word") && t.chars().count() <= 91, "{t}");
        // (one long word: cut, not lost)
        assert_eq!(tail_words("abcdefghij", 4), "…ghij");
        // (not on a character's middle)
        assert_eq!(tail_words("ééééé ü", 3), "…ü");
    }

    #[test]
    fn hints_and_names() {
        assert_eq!(hint("hold"), "Release to type · Esc to cancel");
        assert_eq!(hint("toggle"), "Press again to stop · Esc to cancel");
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("auto"), "Automatic");
        assert_eq!(language_name("xx"), "XX");
    }
}
