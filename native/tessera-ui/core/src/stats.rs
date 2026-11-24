//! Measurement hooks: the first presented frame, and per-surface frame times
//! (the time Slint took to draw, the compositor's presentation intervals).
//! SIGUSR1 prints them as one JSON line per surface and starts over.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct Frames {
    render_us: Vec<u32>,
    intervals_ms: Vec<u32>,
    last_time: Option<u32>,
}

impl Frames {
    pub fn rendered(&mut self, d: Duration) {
        keep_recent(&mut self.render_us);
        self.render_us.push(d.as_micros() as u32);
    }
    /// A wl_surface.frame callback: `time` is the compositor's ms clock.
    /// Gaps over 250 ms are idle time, not frames.
    pub fn presented(&mut self, time: u32) {
        if let Some(last) = self.last_time {
            let gap = time.wrapping_sub(last);
            if gap < 250 {
                keep_recent(&mut self.intervals_ms);
                self.intervals_ms.push(gap);
            }
        }
        self.last_time = Some(time);
    }
    pub fn report(&mut self, name: &str) -> String {
        let r = summary(&mut self.render_us);
        let i = summary(&mut self.intervals_ms);
        let line = format!(
            "{{\"surface\":\"{name}\",\"frames\":{},\"render_us\":{r},\"interval_ms\":{i}}}",
            self.render_us.len()
        );
        *self = Self::default();
        line
    }
}

/// The samples a surface keeps between reports: the newest MAX_SAMPLES (a
/// steadily animating surface kept every frame for as long as the shell
/// ran, megabytes an hour; audit #203)
const MAX_SAMPLES: usize = 20_000;

fn keep_recent(v: &mut Vec<u32>) {
    if v.len() >= MAX_SAMPLES {
        v.drain(..MAX_SAMPLES / 2);
    }
}

fn summary(v: &mut [u32]) -> String {
    if v.is_empty() {
        return "null".into();
    }
    v.sort_unstable();
    let p = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let mean = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
    format!(
        "{{\"n\":{},\"mean\":{mean:.1},\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        v.len(),
        p(0.5),
        p(0.95),
        p(0.99),
        v[v.len() - 1]
    )
}

/// Wall clock (for the start-up measurement: the launcher prints its own
/// wall clock just before exec) and time since main() began.
pub fn log_first_frame(started: Instant) {
    let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    eprintln!("tessera-ui: first-frame wall={wall:.6} since-main-ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
}
