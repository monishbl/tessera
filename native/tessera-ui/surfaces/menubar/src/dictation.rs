//! Dictation's live activity in the island (host/desktop dictation.rs and
//! dictation_live.rs say what's happening), as iOS's recorder in the Dynamic
//! Island: one slim row (a tile with your voice as bars, the time in bold
//! with "Listening" / "Writing…" / "Typed" under it, Cancel and Done) and
//! under it up to two lines of your words, each word arriving blurred and
//! sharpening into place. It opens by itself as you start and shrinks to the
//! pill after 3 s of you speaking (a hover or a click opens it again);
//! "Writing…" and "Typed" open it again.
//!
//! The level comes 20 times a second; it's eased (quick up, slow down) and
//! the newest few are the bars (the tile's seven, the pill's five).

use std::collections::VecDeque;

/// Levels kept (the tile's bars are the newest of them)
pub const KEEP: usize = 16;
/// The tile's bars and the pill's
pub const TILE_BARS: usize = 7;
pub const PILL_BARS: usize = 5;
/// One level each 50 ms
pub const FRAME_MS: f64 = 50.0;
/// A level this high is you speaking (for the shrink)
pub const SPEECH: f32 = 0.18;
/// It shrinks to the pill after this much speech
pub const SHRINK_AFTER_MS: f64 = 3000.0;
/// The last seconds of the time limit, warned about
pub const WARN_S: f64 = 10.0;
/// The words: characters a line (a little more than fit: the oldest end of a
/// line that's too long is cut off on the left), and how long a word is new
/// (it arrives blurred) or still a guess (dim)
pub const LINE_CHARS: usize = 50;
pub const FRESH_MS: f64 = 600.0;
pub const SETTLE_MS: f64 = 1500.0;

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
        while self.hist.len() > KEEP {
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
}

/// `n` bars: the newest levels (newest on the right), each at least a dot's
/// height (0..1 of the bar's room), so they're visibly alive when you're quiet
pub fn bars(w: &Wave, n: usize) -> Vec<f32> {
    w.tail(n).iter().map(|v| (0.22 + 0.78 * v.clamp(0.0, 1.0)).min(1.0)).collect()
}

/// The card: open by itself or the pill
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    /// the host's stage: listening | writing | done | "" (none)
    pub stage: String,
    session: i64,
    speech_ms: f64,
    open: bool,
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
            self.stage = stage.to_string();
            self.session = session;
        }
        was != self.open
    }
    /// A level heard (one each FRAME_MS): true when it shrinks to the pill
    pub fn heard(&mut self, level: f32) -> bool {
        if self.stage != "listening" || !self.open {
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
}

/// One word shown: kept (its id) while it stays the same word in the same
/// place, so only new and corrected words arrive (blurred, sharpening)
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub id: u64,
    pub text: String,
    /// when it arrived (ms)
    pub born: f64,
}

/// The words so far, as drafts come and are corrected
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Words {
    list: Vec<Word>,
    next: u64,
}

impl Words {
    /// The newest words: each one the same as before (allowing for a word
    /// put in or left out) keeps its id and its arrival; the others are new
    pub fn update(&mut self, text: &str, now: f64) {
        let new: Vec<&str> = text.split_whitespace().collect();
        let old = std::mem::take(&mut self.list);
        let mut out = Vec::with_capacity(new.len());
        let mut j = 0usize;
        for (i, w) in new.iter().enumerate() {
            if old.get(j).is_some_and(|o| o.text == *w) {
                out.push(old[j].clone());
                j += 1;
            } else if old.get(j + 1).is_some_and(|o| o.text == *w) {
                // (the old word here was left out)
                out.push(old[j + 1].clone());
                j += 2;
            } else {
                self.next += 1;
                out.push(Word { id: self.next, text: w.to_string(), born: now });
                // (a word put in: the old one comes next; else this one replaced it)
                let inserted = new.get(i + 1).is_some_and(|n| old.get(j).is_some_and(|o| o.text == *n));
                if !inserted {
                    j += 1;
                }
            }
        }
        self.list = out;
    }
    pub fn clear(&mut self) {
        self.list.clear();
    }
    pub fn list(&self) -> &[Word] {
        &self.list
    }
}

/// The two lines shown: the newest words that fit one line (the bottom one),
/// and the words before them that fit another (the top one); the older ones
/// have scrolled off. Indices into `words`.
pub fn two_lines(words: &[Word], per_line: usize) -> (Vec<usize>, Vec<usize>) {
    let mut lines: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    let mut line = 1usize;
    let mut used = 0usize;
    for i in (0..words.len()).rev() {
        let n = words[i].text.chars().count();
        let need = if used == 0 { n } else { used + 1 + n };
        if need > per_line && used > 0 {
            if line == 0 {
                break;
            }
            line = 0;
            used = n;
        } else {
            used = need;
        }
        lines[line].insert(0, i);
    }
    let [top, bottom] = lines;
    (top, bottom)
}

/// In the last WARN_S seconds: how many are left ("8 s left")
pub fn left(elapsed_s: f64, max_s: f64) -> Option<String> {
    let rest = (max_s - elapsed_s).max(0.0);
    (max_s > 0.0 && rest <= WARN_S).then(|| format!("{} s left", rest.ceil() as i64))
}

/// The line under the time
pub fn status(stage: &str, left: Option<&str>) -> String {
    match stage {
        "writing" => "Writing…".into(),
        "done" => "Typed".into(),
        _ => left.map(str::to_string).unwrap_or_else(|| "Listening".into()),
    }
}

/// The key hint for how it was started (the row's accessible description)
pub fn hint(mode: &str) -> &'static str {
    if mode == "hold" {
        "Release to type · Esc to cancel"
    } else {
        "Press again to stop · Esc to cancel"
    }
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
        assert_eq!(w.len(), KEEP);
        assert!((w.level() - 0.5).abs() < 1e-3);
        w.clear();
        assert!(w.is_empty());
        assert_eq!(w.level(), 0.0);
    }

    #[test]
    fn the_bars_stay_alive() {
        let mut w = Wave::default();
        let quiet = bars(&w, TILE_BARS);
        assert_eq!(quiet.len(), TILE_BARS);
        assert!(quiet.iter().all(|&b| b > 0.2 && b < 0.3));
        for _ in 0..3 {
            w.push(1.0);
        }
        let loud = bars(&w, PILL_BARS);
        assert_eq!(loud.len(), PILL_BARS);
        // the newest on the right, taller than the silence before it
        assert!(loud[4] > loud[1] && loud[4] <= 1.0);
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

    fn ids(w: &Words) -> Vec<u64> {
        w.list().iter().map(|x| x.id).collect()
    }

    #[test]
    fn only_new_and_corrected_words_arrive() {
        let mut w = Words::default();
        w.update("send the", 0.0);
        let first = ids(&w);
        assert_eq!(first.len(), 2);
        // more words: the first two stay as they were
        w.update("send the report", 100.0);
        assert_eq!(&ids(&w)[..2], &first[..]);
        assert_eq!(w.list()[2].born, 100.0);
        // a correction: only that word is new
        w.update("send a report", 200.0);
        let now = ids(&w);
        assert_eq!(now[0], first[0]);
        assert_ne!(now[1], first[1]);
        assert_eq!(w.list()[2].born, 100.0);
        // a word put in, one left out
        w.update("please send a report", 300.0);
        assert_eq!(&ids(&w)[1..], &now[..]);
        w.update("please send report", 400.0);
        assert_eq!(ids(&w)[2], now[2]);
        // (the same text twice: nothing new)
        let same = ids(&w);
        w.update("please send report", 500.0);
        assert_eq!(ids(&w), same);
        w.clear();
        assert!(w.list().is_empty());
    }

    fn words(s: &str) -> Vec<Word> {
        s.split_whitespace().enumerate().map(|(i, t)| Word { id: i as u64, text: t.into(), born: 0.0 }).collect()
    }

    #[test]
    fn two_lines_newest_at_the_bottom() {
        assert_eq!(two_lines(&[], 10), (vec![], vec![]));
        assert_eq!(two_lines(&words("one two"), 10), (vec![], vec![0, 1]));
        // "one two" / "three four" (9 a line)
        assert_eq!(two_lines(&words("one two three four"), 10), (vec![0, 1], vec![2, 3]));
        // older ones scroll off the top
        assert_eq!(two_lines(&words("a b c d e f g"), 3), (vec![3, 4], vec![5, 6]));
        // a word longer than a line still shows (cut on the left)
        assert_eq!(two_lines(&words("x extraordinarily"), 5), (vec![0], vec![1]));
    }

    #[test]
    fn status_and_the_time_limit() {
        assert_eq!(left(100.0, 120.0), None);
        assert_eq!(left(110.0, 120.0).as_deref(), Some("10 s left"));
        assert_eq!(left(111.5, 120.0).as_deref(), Some("9 s left"));
        assert_eq!(left(130.0, 120.0).as_deref(), Some("0 s left"));
        assert_eq!(status("listening", None), "Listening");
        assert_eq!(status("listening", Some("8 s left")), "8 s left");
        assert_eq!(status("writing", Some("8 s left")), "Writing…");
        assert_eq!(status("done", None), "Typed");
        assert_eq!(hint("hold"), "Release to type · Esc to cancel");
        assert_eq!(hint("toggle"), "Press again to stop · Esc to cancel");
    }
}
