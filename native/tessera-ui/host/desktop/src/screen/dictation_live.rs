//! Dictation's live side, while you speak: the microphone's level for the
//! island's wave, and the words so far (drafts, corrected at the end).
//!
//! The recorder (dictation.rs) writes the WAV as before; the final
//! transcription reads that same file, so it's exactly as reliable as it was.
//! Beside it, one thread (a `Tap`) reads the file as it grows, every 50 ms:
//!   - the level of the newest 50 ms (RMS, in decibels, 0..1) goes to the
//!     service (`Level`), which hands it to the island: 20 a second at most;
//!   - every 1.2 s at the soonest, when there's new speech and the last draft
//!     has been answered, the audio so far (its last 28 s) is written to a
//!     small WAV in the runtime folder and handed to ONE long-lived
//!     `dictate.py stream` (started on the first draft), which answers with
//!     the words (`Partial`). One draft at a time: none pile up, none wait.
//! Drafts use the chosen model if it's a small one (Tiny, Base), else Base or
//! Tiny if downloaded, else there are no drafts (the island has no words
//! until the end; everything else works the same).
//! It all ends when dictation stops, is cancelled or fails (the Tap is
//! stopped or dropped): the thread ends, the helper is told to stop (SIGTERM:
//! its whisper-cli is ended with it) and the draft file is removed. Nothing
//! runs while you're not dictating.

use serde_json::Value;
use std::any::Any;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The recording: 16 kHz mono, 16-bit
pub const RATE: usize = 16_000;
/// One level each 50 ms (20 a second)
pub const FRAME_MS: u64 = 50;
pub const FRAME: usize = RATE * FRAME_MS as usize / 1000;
/// A draft at most this often, after at least 1 s of audio, with half a
/// second new since the last, and only the last 28 s (whisper's window is 30)
pub const PARTIAL_EVERY_MS: i64 = 1200;
pub const PARTIAL_FIRST: usize = RATE;
pub const PARTIAL_MIN_NEW: usize = RATE / 2;
pub const PARTIAL_TAIL: usize = RATE * 28;
/// A level this high is speech (worth a new draft)
pub const VOICED: f32 = 0.12;
/// The models small enough for a draft every second or so
const DRAFT_MODELS: &[&str] = &["tiny", "base"];

/// The newest level (0..1) of a dictation (its session)
pub struct Level(pub u64, pub f32);
/// The words so far of a dictation (its session)
pub struct Partial(pub u64, pub String);

/// Where the samples start in a WAV file's first bytes: after the "data"
/// chunk's header (None: not there yet, or not a WAV). The sizes a recorder
/// writes before it closes the file are placeholders: the data runs to the end.
pub fn data_offset(head: &[u8]) -> Option<usize> {
    if head.len() < 12 || &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12usize;
    while at + 8 <= head.len() {
        let size = u32::from_le_bytes([head[at + 4], head[at + 5], head[at + 6], head[at + 7]]) as usize;
        if &head[at..at + 4] == b"data" {
            return Some(at + 8);
        }
        // (chunks are padded to an even length)
        at = at.checked_add(8)?.checked_add(size)?.checked_add(size & 1)?;
        if at > 1 << 16 {
            return None; // (no header is that long)
        }
    }
    None
}

/// Little-endian 16-bit samples from bytes read in any pieces (`odd`: a byte
/// left over from the last piece)
pub fn push_pcm(bytes: &[u8], odd: &mut Option<u8>, out: &mut Vec<i16>) {
    let mut rest = bytes;
    if let Some(lo) = odd.take() {
        match rest.split_first() {
            Some((hi, tail)) => {
                out.push(i16::from_le_bytes([lo, *hi]));
                rest = tail;
            }
            None => {
                *odd = Some(lo);
                return;
            }
        }
    }
    let mut chunks = rest.chunks_exact(2);
    for c in &mut chunks {
        out.push(i16::from_le_bytes([c[0], c[1]]));
    }
    if let [b] = chunks.remainder() {
        *odd = Some(*b);
    }
}

/// How loud some samples are, 0..1: their RMS in decibels, -60 dB (a quiet
/// room) to -12 dB (speaking up close)
pub fn level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 48.0).clamp(0.0, 1.0) as f32
}

/// A WAV file (16 kHz mono, 16-bit) of some samples: a draft's recording
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&(RATE as u32).to_le_bytes());
    v.extend_from_slice(&((RATE * 2) as u32).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

fn write_wav(path: &str, samples: &[i16]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(&wav_bytes(samples))
}

/// The model for drafts: the chosen one if it's small, else a small one
/// that's downloaded (None: no drafts)
pub fn draft_model(chosen: &str, downloaded: &Value) -> Option<String> {
    let has = |m: &str| downloaded[m].as_bool().unwrap_or(false);
    if DRAFT_MODELS.contains(&chosen) && has(chosen) {
        return Some(chosen.to_string());
    }
    ["base", "tiny"].iter().find(|m| has(m)).map(|m| m.to_string())
}

/// When to ask for the next draft: one at a time, not more often than
/// PARTIAL_EVERY_MS, only with new speech
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PartialGate {
    busy: bool,
    last_at: i64,
    last_len: usize,
    asked: bool,
}

impl PartialGate {
    /// `len`: samples recorded so far; `voiced`: speech since the last draft
    pub fn want(&self, now: i64, len: usize, voiced: bool) -> bool {
        !self.busy && voiced && len >= PARTIAL_FIRST && len >= self.last_len + PARTIAL_MIN_NEW && (!self.asked || now - self.last_at >= PARTIAL_EVERY_MS)
    }
    pub fn sent(&mut self, now: i64, len: usize) {
        self.busy = true;
        self.asked = true;
        self.last_at = now;
        self.last_len = len;
    }
    /// Its answer came (or it failed)
    pub fn done(&mut self) {
        self.busy = false;
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
}

/// What drafts are made with (the language can change while you speak)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub model: Option<String>,
    pub lang: String,
}

/// The live side of one dictation: stopped when dropped
pub struct Tap {
    stop: Arc<AtomicBool>,
    pick: Arc<Mutex<Pick>>,
}

impl Tap {
    /// Follow `file` (the recorder's WAV) for dictation `session`;
    /// `helper`: the command for `dictate.py stream`; `draft`: the draft's file
    pub fn spawn<F>(file: String, session: u64, pick: Pick, send: F, helper: Vec<String>, draft: String) -> Tap
    where
        F: Fn(Box<dyn Any + Send>) + Send + Clone + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let pick = Arc::new(Mutex::new(pick));
        let (s2, p2) = (stop.clone(), pick.clone());
        std::thread::spawn(move || follow(file, session, p2, s2, send, helper, draft));
        Tap { stop, pick }
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    pub fn set_lang(&self, lang: &str) {
        if let Ok(mut p) = self.pick.lock() {
            p.lang = lang.to_string();
        }
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The long-lived `dictate.py stream`
struct Helper {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
}

impl Helper {
    fn start<F>(cmd: &[String], session: u64, send: F, answered: Arc<AtomicBool>) -> Option<Helper>
    where
        F: Fn(Box<dyn Any + Send>) + Send + Clone + 'static,
    {
        let (prog, args) = cmd.split_first()?;
        let mut c = std::process::Command::new(prog);
        c.args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        super::util::default_signals(&mut c); // (so SIGTERM reaches it)
        let mut child = c.spawn().ok()?;
        let stdin = child.stdin.take()?;
        let out = child.stdout.take()?;
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&l) {
                    if v["ok"].as_bool() == Some(true) {
                        send(Box::new(Partial(session, v["text"].as_str().unwrap_or("").to_string())));
                    }
                    answered.store(true, Ordering::SeqCst);
                }
            }
        });
        Some(Helper { child, stdin })
    }

    fn ask(&mut self, id: u64, wav: &str, pick: &Pick, model: &str) -> bool {
        let line = serde_json::json!({"id": id, "wav": wav, "model": model, "lang": if pick.lang.is_empty() { "auto" } else { pick.lang.as_str() }});
        writeln!(self.stdin, "{line}").and_then(|_| self.stdin.flush()).is_ok()
    }

    /// Stop it (and the whisper-cli it may be running), reaped off this thread
    fn end(self) {
        let Helper { mut child, stdin } = self;
        drop(stdin);
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn follow<F>(file: String, session: u64, pick: Arc<Mutex<Pick>>, stop: Arc<AtomicBool>, send: F, helper_cmd: Vec<String>, draft: String)
where
    F: Fn(Box<dyn Any + Send>) + Send + Clone + 'static,
{
    let tick = std::time::Duration::from_millis(FRAME_MS);
    let mut f: Option<std::fs::File> = None;
    let mut head: Vec<u8> = Vec::new();
    let mut in_data = false;
    let mut odd = None;
    let mut samples: Vec<i16> = Vec::with_capacity(RATE * 16);
    let mut buf = vec![0u8; 32 * 1024];
    let mut gate = PartialGate::default();
    let answered = Arc::new(AtomicBool::new(false));
    let mut helper: Option<Helper> = None;
    let mut no_drafts = false;
    let mut voiced = false;
    let mut id = 0u64;
    'outer: while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(tick);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if f.is_none() {
            f = std::fs::File::open(&file).ok();
        }
        let Some(file_now) = f.as_mut() else { continue };
        // (everything new since the last look)
        loop {
            match file_now.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if in_data {
                        push_pcm(&buf[..n], &mut odd, &mut samples);
                    } else {
                        head.extend_from_slice(&buf[..n]);
                        if let Some(at) = data_offset(&head) {
                            in_data = true;
                            push_pcm(&head[at.min(head.len())..], &mut odd, &mut samples);
                            head = Vec::new();
                        } else if head.len() > 1 << 16 {
                            break 'outer; // (not a WAV it can read: no levels, no drafts)
                        }
                    }
                }
            }
        }
        if samples.is_empty() {
            continue;
        }
        let l = level(&samples[samples.len().saturating_sub(FRAME)..]);
        send(Box::new(Level(session, l)));
        voiced |= l >= VOICED;
        if answered.swap(false, Ordering::SeqCst) {
            gate.done();
        }
        if no_drafts {
            continue;
        }
        let now = tessera_core::util::now_ms();
        if !gate.want(now, samples.len(), voiced) {
            continue;
        }
        let p = pick.lock().map(|p| p.clone()).unwrap_or_default();
        let Some(model) = p.model.clone() else {
            no_drafts = true;
            continue;
        };
        if helper.is_none() {
            helper = Helper::start(&helper_cmd, session, send.clone(), answered.clone());
            if helper.is_none() {
                no_drafts = true;
                continue;
            }
        }
        let tail = &samples[samples.len().saturating_sub(PARTIAL_TAIL)..];
        id += 1;
        let ok = write_wav(&draft, tail).is_ok() && helper.as_mut().is_some_and(|h| h.ask(id, &draft, &p, &model));
        if !ok {
            // (the helper went away, or the runtime folder is full: no more drafts)
            no_drafts = true;
            continue;
        }
        gate.sent(now, samples.len());
        voiced = false;
    }
    if let Some(h) = helper.take() {
        h.end();
    }
    let _ = std::fs::remove_file(&draft);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_samples_start_after_the_data_header() {
        let w = wav_bytes(&[1, 2, 3]);
        assert_eq!(data_offset(&w), Some(44));
        assert_eq!(w.len(), 44 + 6);
        // (another chunk first, odd-sized: padded)
        let mut v = b"RIFF\0\0\0\0WAVE".to_vec();
        v.extend_from_slice(b"LIST");
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(&[9, 9, 9, 0]);
        v.extend_from_slice(b"data");
        v.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(data_offset(&v), Some(12 + 8 + 4 + 8));
        // not there yet, not a WAV
        assert_eq!(data_offset(&w[..30]), None);
        assert_eq!(data_offset(b"OggS0000000000000000"), None);
    }

    #[test]
    fn samples_read_in_odd_pieces() {
        let bytes: Vec<u8> = [100i16, -2, 32767].iter().flat_map(|s| s.to_le_bytes()).collect();
        let (mut odd, mut out) = (None, Vec::new());
        push_pcm(&bytes[..1], &mut odd, &mut out);
        assert!(out.is_empty());
        push_pcm(&bytes[1..1], &mut odd, &mut out);
        push_pcm(&bytes[1..4], &mut odd, &mut out);
        assert_eq!(out, vec![100, -2]);
        push_pcm(&bytes[4..], &mut odd, &mut out);
        assert_eq!(out, vec![100, -2, 32767]);
        assert_eq!(odd, None);
    }

    #[test]
    fn levels_from_quiet_to_loud() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0; 800]), 0.0);
        assert_eq!(level(&[32767; 800]), 1.0);
        // -36 dB is half way; louder is higher
        let a = (32768.0 * 10f64.powf(-36.0 / 20.0)) as i16;
        let half = level(&vec![a; 800]);
        assert!((half - 0.5).abs() < 0.01, "{half}");
        assert!(level(&vec![a / 4; 800]) < half);
        // (a hum below -60 dB is nothing)
        assert_eq!(level(&[20; 800]), 0.0);
        assert!(level(&[-3000; 800]) > VOICED);
    }

    #[test]
    fn drafts_one_at_a_time_not_too_often() {
        let mut g = PartialGate::default();
        // not before a second of audio, nor without speech
        assert!(!g.want(0, PARTIAL_FIRST - 1, true));
        assert!(!g.want(0, PARTIAL_FIRST, false));
        assert!(g.want(0, PARTIAL_FIRST, true));
        g.sent(1000, PARTIAL_FIRST);
        // one at a time: none while it's being written
        assert!(g.busy());
        assert!(!g.want(5000, RATE * 5, true));
        g.done();
        // not sooner than PARTIAL_EVERY_MS, nor without half a second more
        assert!(!g.want(1000 + PARTIAL_EVERY_MS - 1, RATE * 5, true));
        assert!(!g.want(9000, PARTIAL_FIRST + PARTIAL_MIN_NEW - 1, true));
        assert!(g.want(1000 + PARTIAL_EVERY_MS, PARTIAL_FIRST + PARTIAL_MIN_NEW, true));
    }

    #[test]
    fn drafts_with_a_small_model() {
        let d = json!({"tiny": true, "base": true, "small": true, "medium": true});
        assert_eq!(draft_model("tiny", &d).as_deref(), Some("tiny"));
        assert_eq!(draft_model("base", &d).as_deref(), Some("base"));
        assert_eq!(draft_model("medium", &d).as_deref(), Some("base"));
        assert_eq!(draft_model("large-v3-turbo", &json!({"tiny": true, "large-v3-turbo": true})).as_deref(), Some("tiny"));
        assert_eq!(draft_model("small", &json!({"small": true})), None);
        assert_eq!(draft_model("base", &json!({})), None);
    }
}
