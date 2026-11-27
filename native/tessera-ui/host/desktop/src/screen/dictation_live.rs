//! Dictation's live side, while you speak: the level of your voice for the
//! island's bars. (No words while you speak: they come once, at the end, from
//! the full transcription.)
//!
//! The recorder (dictation.rs) writes the WAV as before; the final
//! transcription reads that same file. Beside it, one thread (a `Tap`) reads
//! the file as it grows, every 50 ms, and hands the level of what's new
//! straight to the menu bar's island (`level_to`: not through the host, so no
//! host turn, nor every service's `settled`, 20 times a second). If nothing
//! can be read from the recording within a second, it reads the microphone
//! itself (`parec`) instead.
//!
//! The level listens for a voice, not the room (`VoiceMeter`): the samples go
//! through a band-pass (300–3400 Hz: no hum, no rumble, no hiss), their RMS in
//! dBFS is compared with the room's own level (a floor that falls at once and
//! rises slowly, ~1.5 dB a second), and only what's clearly above it (12 dB)
//! counts, over a 26 dB range with a soft knee; then it rises at once and
//! falls over ~250 ms. Quiet rooms, fans, hum and breathing are a flat 0.
//! It all ends when dictation stops, is cancelled or fails (the Tap is
//! stopped or dropped). Nothing runs while you're not dictating.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The recording: 16 kHz mono, 16-bit
pub const RATE: usize = 16_000;
/// One level each 50 ms (20 a second)
pub const FRAME_MS: u64 = 50;
pub const FRAME: usize = RATE * FRAME_MS as usize / 1000;
/// Ticks (of FRAME_MS) without a sample from the recording before the
/// microphone is read directly
const SWITCH_TICKS: u32 = 20;
/// The voice band
pub const VOICE_LOW_HZ: f64 = 300.0;
pub const VOICE_HIGH_HZ: f64 = 3400.0;
/// Only this far above the room's own level counts, over this range
pub const GATE_DB: f64 = 12.0;
pub const RANGE_DB: f64 = 26.0;
/// How fast the room's level may rise (dB a frame: ~1.5 dB a second, so a
/// voice doesn't become the room; it falls at once in a pause)
pub const FLOOR_RISE_DB: f64 = 1.5 * FRAME_MS as f64 / 1000.0;
/// The release: what's left of the level after each frame (it falls
/// to nothing over ~250-300 ms), and below what it's nothing
pub const RELEASE: f64 = 0.6;
pub const QUIET: f64 = 0.03;

/// A second-order filter (RBJ's cookbook), direct form I
#[derive(Clone, Debug, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(high_pass: bool, hz: f64) -> Biquad {
        let w = 2.0 * std::f64::consts::PI * hz / RATE as f64;
        let (cos, alpha) = (w.cos(), w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2));
        let a0 = 1.0 + alpha;
        let b = if high_pass { [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0] } else { [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0] };
        Biquad { b: [b[0] / a0, b[1] / a0, b[2] / a0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The level of a voice (0..1), frame by frame, against the room's own
#[derive(Clone, Debug)]
pub struct VoiceMeter {
    high: Biquad,
    low: Biquad,
    /// the room's level (dBFS), once there's been a frame
    floor: Option<f64>,
    out: f64,
}

impl Default for VoiceMeter {
    fn default() -> Self {
        VoiceMeter { high: Biquad::new(true, VOICE_LOW_HZ), low: Biquad::new(false, VOICE_HIGH_HZ), floor: None, out: 0.0 }
    }
}

impl VoiceMeter {
    /// The samples since the last frame: the level now
    pub fn frame(&mut self, samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return self.out as f32;
        }
        let mut sum = 0.0;
        for &s in samples {
            let v = self.low.run(self.high.run(s as f64 / 32768.0));
            sum += v * v;
        }
        let rms = (sum / samples.len() as f64).sqrt();
        let db = if rms > 1e-9 { 20.0 * rms.log10() } else { -180.0 };
        let floor = match self.floor {
            // (falls at once, rises slowly)
            Some(f) if db >= f => f + (db - f).min(FLOOR_RISE_DB),
            _ => db.max(-120.0),
        };
        self.floor = Some(floor);
        let x = ((db - floor - GATE_DB) / RANGE_DB).clamp(0.0, 1.0);
        let target = x * x * (3.0 - 2.0 * x); // (a soft knee at both ends)
        self.out = if target >= self.out { target } else { target.max(self.out * RELEASE) };
        if self.out < QUIET {
            self.out = 0.0;
        }
        self.out as f32
    }
}

/// The live side of one dictation: stopped when dropped
pub struct Tap {
    stop: Arc<AtomicBool>,
}

impl Tap {
    /// Follow `file` (the recorder's WAV); `level_to`: where each level goes
    /// (the menu bar)
    pub fn spawn<L>(file: String, level_to: L) -> Tap
    where
        L: Fn(f32) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::spawn(move || follow(file, s2, level_to));
        Tap { stop }
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop();
    }
}

fn follow<L>(file: String, stop: Arc<AtomicBool>, level_to: L)
where
    L: Fn(f32) + Send + 'static,
{
    let tick = std::time::Duration::from_millis(FRAME_MS);
    let mut f: Option<std::fs::File> = None;
    let mut wav = WavTail::default();
    // (the fallback: the microphone itself, when the recording doesn't grow)
    let mut meter: Option<Meter> = None;
    let mut ticks = 0u32;
    let mut heard_any = false;
    let mut samples: Vec<i16> = Vec::with_capacity(RATE);
    let mut buf = vec![0u8; 32 * 1024];
    let mut voice = VoiceMeter::default();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(tick);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        ticks = ticks.saturating_add(1);
        samples.clear();
        if let Some(m) = meter.as_mut() {
            m.read_new(&mut buf, &mut samples);
        } else {
            if f.is_none() {
                f = std::fs::File::open(&file).ok();
            }
            if let Some(file_now) = f.as_mut() {
                read_new(file_now, &mut buf, &mut wav, &mut samples);
            }
            // (nothing readable after a second, or not a WAV it knows: the
            // microphone through parec instead)
            if !heard_any && samples.is_empty() && (ticks >= SWITCH_TICKS || wav.foreign()) {
                f = None;
                meter = Meter::start();
                if meter.is_none() {
                    break; // (no levels; dictation itself goes on)
                }
            }
        }
        // (only what's new: a level that didn't change isn't sent again)
        if samples.is_empty() {
            continue;
        }
        heard_any = true;
        level_to(voice.frame(&samples));
    }
    if let Some(m) = meter.take() {
        m.end();
    }
}

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

/// A WAV file read as it's written: its header skipped, then its samples
#[derive(Debug, Default)]
pub struct WavTail {
    head: Vec<u8>,
    in_data: bool,
    odd: Option<u8>,
    foreign: bool,
}

impl WavTail {
    /// Bytes as they were appended to the file: the samples among them
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<i16>) {
        if self.foreign {
            return;
        }
        if self.in_data {
            return push_pcm(bytes, &mut self.odd, out);
        }
        self.head.extend_from_slice(bytes);
        if let Some(at) = data_offset(&self.head) {
            self.in_data = true;
            let head = std::mem::take(&mut self.head);
            push_pcm(&head[at.min(head.len())..], &mut self.odd, out);
        } else if self.head.len() > 1 << 16 {
            // (no header is that long: not a WAV it can read)
            self.foreign = true;
            self.head = Vec::new();
        }
    }
    pub fn foreign(&self) -> bool {
        self.foreign
    }
}

/// Everything appended to a file since the last read (a regular file at its
/// end reads 0 bytes, then whatever the recorder adds after)
pub fn read_new(f: &mut impl Read, buf: &mut [u8], tail: &mut WavTail, out: &mut Vec<i16>) {
    loop {
        match f.read(buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => tail.feed(&buf[..n], out),
        }
    }
}

/// The microphone read directly (`parec`, 16 kHz mono raw, the default
/// source), when the recording can't be followed: its output never blocks
struct Meter {
    child: std::process::Child,
    out: std::process::ChildStdout,
    odd: Option<u8>,
}

impl Meter {
    fn start() -> Option<Meter> {
        use std::os::unix::io::AsRawFd;
        let mut c = std::process::Command::new("setpriv");
        c.args(["--pdeathsig", "TERM", "--", "parec", "--raw", "--format=s16le", "--channels=1", "--rate=16000", "--latency-msec=50", "-d", "@DEFAULT_SOURCE@"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        super::util::default_signals(&mut c);
        let mut child = c.spawn().ok()?;
        let Some(out) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let fd = out.as_raw_fd();
        unsafe {
            let fl = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
        Some(Meter { child, out, odd: None })
    }

    /// What it has said since the last look (never waits)
    fn read_new(&mut self, buf: &mut [u8], out: &mut Vec<i16>) {
        loop {
            match self.out.read(buf) {
                Ok(0) => break,
                Ok(n) => push_pcm(&buf[..n], &mut self.odd, out),
                Err(_) => break, // (WouldBlock: nothing more yet)
            }
        }
    }

    fn end(self) {
        let Meter { mut child, out, .. } = self;
        drop(out);
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file (16 kHz mono, 16-bit) of some samples, as a recorder writes one
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
    fn a_growing_recording_is_followed() {
        // as a recorder writes one: the header first (sizes not known yet),
        // then the samples as they come, in pieces of any length
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("tessera-dictation-test-{}.wav", std::process::id()));
        let mut w = std::fs::File::create(&path).unwrap();
        let mut header = wav_bytes(&[]);
        header[4..8].copy_from_slice(&0u32.to_le_bytes());
        w.write_all(&header[..20]).unwrap(); // (not even the whole header yet)
        let mut r = std::fs::File::open(&path).unwrap();
        let (mut tail, mut out, mut buf) = (WavTail::default(), Vec::new(), vec![0u8; 64]);
        read_new(&mut r, &mut buf, &mut tail, &mut out);
        assert!(out.is_empty());
        w.write_all(&header[20..]).unwrap();
        read_new(&mut r, &mut buf, &mut tail, &mut out);
        assert!(out.is_empty());
        // a loud 50 ms, written in two pieces split inside a sample
        let loud: Vec<u8> = std::iter::repeat(8000i16).take(FRAME).flat_map(|s| s.to_le_bytes()).collect();
        w.write_all(&loud[..701]).unwrap();
        read_new(&mut r, &mut buf, &mut tail, &mut out);
        assert_eq!(out.len(), 350);
        w.write_all(&loud[701..]).unwrap();
        read_new(&mut r, &mut buf, &mut tail, &mut out);
        assert_eq!(out.len(), FRAME);
        assert!(out.iter().all(|&s| s == 8000));
        // nothing new: nothing more
        read_new(&mut r, &mut buf, &mut tail, &mut out);
        assert_eq!(out.len(), FRAME);
        assert!(!tail.foreign());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn not_a_wav_is_given_up_on() {
        let mut t = WavTail::default();
        let mut out = Vec::new();
        t.feed(&vec![7u8; 70_000], &mut out);
        assert!(t.foreign() && out.is_empty());
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


    /// `n` frames of a signal (`f(t)` in -1..1, t in seconds from `t0`)
    fn frames(m: &mut VoiceMeter, t0: f64, n: usize, f: impl Fn(f64) -> f64) -> Vec<f32> {
        (0..n)
            .map(|k| {
                let s: Vec<i16> = (0..FRAME).map(|i| ((f(t0 + (k * FRAME + i) as f64 / RATE as f64)) * 32767.0).clamp(-32768.0, 32767.0) as i16).collect();
                m.frame(&s)
            })
            .collect()
    }

    /// A fixed noise (a small LCG): -1..1
    fn noise(t: f64) -> f64 {
        let mut x = (t * RATE as f64) as u64 ^ 0x9E37_79B9_7F4A_7C15;
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        x ^= x >> 33;
        (x % 20001) as f64 / 10000.0 - 1.0
    }

    #[test]
    fn silence_and_a_quiet_room_are_nothing() {
        let mut m = VoiceMeter::default();
        assert!(frames(&mut m, 0.0, 20, |_| 0.0).iter().all(|&l| l == 0.0));
        let mut m = VoiceMeter::default();
        // (room noise, with breathing-sized swells of a few dB)
        let l = frames(&mut m, 0.0, 80, |t| noise(t) * 0.01 * (1.0 + 0.4 * (t * 0.7 * std::f64::consts::TAU).sin()));
        assert!(l.iter().all(|&x| x == 0.0), "{l:?}");
    }

    #[test]
    fn hum_and_a_fan_are_nothing() {
        // a loud 50 Hz hum (cut by the band), then a steady fan-like noise (the room's level)
        let mut m = VoiceMeter::default();
        let hum = frames(&mut m, 0.0, 60, |t| 0.5 * (t * 50.0 * std::f64::consts::TAU).sin());
        assert!(hum.iter().all(|&x| x == 0.0), "{hum:?}");
        let mut m = VoiceMeter::default();
        let fan = frames(&mut m, 0.0, 60, |t| 0.2 * noise(t));
        assert!(fan.iter().all(|&x| x == 0.0), "{fan:?}");
    }

    #[test]
    fn a_voice_well_above_the_room_is_high_then_falls_off() {
        let mut m = VoiceMeter::default();
        let room = |t: f64| noise(t) * 0.01;
        frames(&mut m, 0.0, 40, room);
        // a voice-band burst (1 kHz, ~36 dB over the room) for 300 ms
        let burst = frames(&mut m, 2.0, 6, |t| room(t) + 0.3 * (t * 1000.0 * std::f64::consts::TAU).sin());
        assert!(burst.iter().skip(1).all(|&x| x > 0.7), "{burst:?}");
        // then quiet again: it falls over ~250 ms, and is nothing after half a second
        let after = frames(&mut m, 2.3, 12, room);
        assert!(after[0] > 0.3 && after[0] < burst[5], "{after:?}");
        assert!(after[11] == 0.0, "{after:?}");
        // a level is never outside 0..1
        assert!(burst.iter().chain(after.iter()).all(|&x| (0.0..=1.0).contains(&x)));
    }

    #[test]
    fn a_rumble_under_the_voice_band_is_nothing() {
        let mut m = VoiceMeter::default();
        frames(&mut m, 0.0, 40, |t| noise(t) * 0.01);
        // a sudden 60 Hz thump as loud as the voice above: cut by the band
        let thump = frames(&mut m, 2.0, 6, |t| noise(t) * 0.01 + 0.3 * (t * 60.0 * std::f64::consts::TAU).sin());
        assert!(thump.iter().all(|&x| x < 0.2), "{thump:?}");
    }
}
