//! What the resources popover shows (ResourceRow.qml on Resources.qml's
//! stats): the samples come from the desktop's sampler (desktop::sysstat:
//! CPU, memory, swap, the package temperature, the network), plus memory in
//! GB and the GPU's load where the driver reports it (amdgpu's
//! gpu_busy_percent, Intel's idle residency), as Resources.qml reads them.

use crate::sysstat::{self, Stat};

/// A row: a group of stats (ResourceRow.qml)
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub group: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    /// the figure on the right, and the network's second one
    pub value: String,
    pub value2: String,
    /// the line under the title
    pub detail: String,
    pub tone: &'static str,
    pub tone2: &'static str,
    pub main: Vec<f64>,
    pub second: Vec<f64>,
    /// the main graph's top (0: scale to the largest)
    pub max: f64,
}

/// The group a hovered figure belongs to (ResourcePopover.group)
pub fn group_of(key: &str) -> &'static str {
    match key {
        "cpu" | "temperature" => "processor",
        "gpu" => "graphics",
        "memory" | "swap" => "memory",
        k if k.starts_with("net") => "network",
        _ => "processor",
    }
}

/// The groups shown (Resources.groups): graphics only where there's a GPU reading
pub fn groups(gpu: bool) -> Vec<&'static str> {
    if gpu { vec!["processor", "graphics", "memory", "network"] } else { vec!["processor", "memory", "network"] }
}

/// Material's role for a stat's tone (ResourceTones.qml): 0 primary,
/// 1 secondary, 2 tertiary
pub fn role(tone: &str) -> i32 {
    match tone {
        "green" | "teal" => 2,
        "purple" | "orange" => 1,
        _ => 0,
    }
}

/// The network's shared top: a little room over the peak of both lines
pub fn net_top(a: &[f64], b: &[f64]) -> f64 {
    a.iter().chain(b.iter()).cloned().fold(65536.0, f64::max) * 1.15
}

/// The rows, from the stats (Resources.stats' keys) and the memory's
/// "used of total GB"
pub fn rows(stats: &[Stat], mem_detail: &str, gpu: Option<&Stat>) -> Vec<Row> {
    let find = |k: &str| stats.iter().find(|s| s.key == k);
    let empty = Stat { key: "cpu", label: "", tone: "blue", text: String::new(), history: Vec::new(), max: 1.0 };
    let mut out = Vec::new();
    for g in groups(gpu.is_some()) {
        let (main, second) = match g {
            "processor" => (find("cpu"), find("temperature")),
            "graphics" => (gpu, None),
            "memory" => (find("memory"), find("swap")),
            _ => (find("netDown"), find("netUp")),
        };
        let main = main.unwrap_or(&empty);
        let detail = match g {
            "processor" => second.map(|s| s.text.replace('°', "°C")).unwrap_or_default(),
            "memory" => {
                let swap = second.and_then(|s| s.history.last().copied()).unwrap_or(0.0);
                let t = second.map(|s| s.text.clone()).unwrap_or_default();
                format!("{mem_detail}{}", if swap > 0.005 { format!(" · swap {t}") } else { String::new() })
            }
            "graphics" if main.label == AWAKE => "Time awake, not load".into(),
            _ => String::new(),
        };
        let net = g == "network";
        out.push(Row {
            group: g,
            title: match g {
                "processor" => "Processor",
                "graphics" => "Graphics",
                "memory" => "Memory",
                _ => "Network",
            },
            icon: match g {
                "processor" => "cpu",
                "graphics" => "graphics-card",
                "memory" => "memory",
                _ => "arrows-down-up",
            },
            value: if net { format!("↓ {}", main.text) } else { main.text.clone() },
            value2: if net { format!("↑ {}", second.map(|s| s.text.as_str()).unwrap_or("")) } else { String::new() },
            detail,
            tone: main.tone,
            tone2: second.map_or(main.tone, |s| s.tone),
            main: main.history.clone(),
            second: if net { second.map(|s| s.history.clone()).unwrap_or_default() } else { Vec::new() },
            max: main.max,
        });
    }
    out
}

/// /proc/meminfo: "6.1 of 15.4 GB" (in core: the overlay shows it too)
pub use crate::surfaces::kit::mem_detail;

/// The label of a GPU figure that is time awake, not load (Intel)
pub const AWAKE: &str = "GPU awake";

/// The GPU's reading: amdgpu's busy percentage, or Intel's idle residency
/// (busy = 1 - idle time over the interval)
#[derive(Default)]
pub struct Gpu {
    found: Option<Option<(bool, String)>>, // (busy kind, file)
    last_idle: Option<(f64, f64)>,
    history: Vec<f64>,
}

/// The DRM cards, the one the firmware booted on first
fn cards() -> Vec<std::path::PathBuf> {
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else { return Vec::new() };
    let mut cards: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("card") && !n.to_string_lossy().contains('-'))).collect();
    // (the card the firmware booted on first: on a hybrid laptop that's the
    // integrated one that drives the screens, not a discrete one asleep)
    cards.sort_by_key(|c| (std::fs::read_to_string(c.join("device/boot_vga")).map_or(true, |v| v.trim() != "1"), c.clone()));
    cards
}

/// Intel's idle residency files (gt0's RC6, or Xe's gtidle)
const IDLE_FILES: [&str; 2] = ["gt/gt0/rc6_residency_ms", "device/tile0/gt0/gtidle/idle_residency_ms"];

/// The first card, in order, with a reading: busy percentage, else idle
/// residency, decided per card (an Intel card driving the screens has no
/// busy file, and must not lose to a discrete GPU asleep beside it)
fn first_reading(cards: &[std::path::PathBuf]) -> Option<(bool, String)> {
    for c in cards {
        let busy = c.join("device/gpu_busy_percent");
        if std::fs::File::open(&busy).is_ok() {
            return Some((true, busy.to_string_lossy().into_owned()));
        }
        for rel in IDLE_FILES {
            let f = c.join(rel);
            if std::fs::File::open(&f).is_ok() {
                return Some((false, f.to_string_lossy().into_owned()));
            }
        }
    }
    None
}

/// `<card>/device/gpu_busy_percent`'s device is runtime-suspended
fn asleep(busy_file: &str) -> bool {
    std::path::Path::new(busy_file).parent().and_then(|d| std::fs::read_to_string(d.join("power/runtime_status")).ok()).is_some_and(|s| suspended(&s))
}

/// A runtime_status that means the device is asleep
pub fn suspended(status: &str) -> bool {
    matches!(status.trim(), "suspended" | "suspending")
}

impl Gpu {
    fn find() -> Option<(bool, String)> {
        first_reading(&cards())
    }

    /// Which card is read (once), without reading it: while only the menu
    /// bar's figures are sampled (they don't show the GPU) the card is left
    /// alone, so a discrete GPU can stay asleep
    pub fn locate(&mut self) {
        self.found.get_or_insert_with(Self::find);
        // (the next reading starts afresh: no idle time over the gap)
        self.last_idle = None;
    }

    pub fn sample(&mut self, now: f64) {
        let found = self.found.get_or_insert_with(Self::find).clone();
        let Some((busy, path)) = found else { return };
        // (a card in runtime suspend is idle: reading its busy file could
        // wake it, on some kernels, every second)
        if busy && asleep(&path) {
            self.push(0.0);
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let Ok(v) = text.trim().parse::<f64>() else { return };
        let load = if busy {
            Some((v / 100.0).clamp(0.0, 1.0))
        } else {
            let prev = self.last_idle.replace((v, now));
            prev.filter(|(_, t)| now > *t).map(|(pv, t)| (1.0 - (v - pv) / ((now - t) * 1000.0)).clamp(0.0, 1.0))
        };
        if let Some(l) = load {
            self.push(l);
        }
    }

    fn push(&mut self, l: f64) {
        self.history.push(l);
        if self.history.len() > sysstat::HISTORY {
            self.history.remove(0);
        }
    }

    pub fn stat(&self) -> Option<Stat> {
        if !matches!(self.found, Some(Some(_))) {
            return None;
        }
        let last = self.history.last().copied().unwrap_or(0.0);
        // (Intel's figure is time out of idle (RC6), not load: said so)
        let label = if matches!(self.found, Some(Some((false, _)))) { AWAKE } else { "GPU" };
        Some(Stat { key: "gpu", label, tone: "indigo", text: format!("{}%", (last * 100.0).round()), history: self.history.clone(), max: 1.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(key: &'static str, tone: &'static str, text: &str, history: Vec<f64>, max: f64) -> Stat {
        Stat { key, label: "", tone, text: text.into(), history, max }
    }

    #[test]
    fn groups_and_tones() {
        assert_eq!(group_of("temperature"), "processor");
        assert_eq!(group_of("netUp"), "network");
        assert_eq!(group_of("swap"), "memory");
        assert_eq!(group_of("gpu"), "graphics");
        assert_eq!(groups(false), vec!["processor", "memory", "network"]);
        assert_eq!((role("blue"), role("purple"), role("teal")), (0, 1, 2));
        assert_eq!(net_top(&[], &[]), 65536.0 * 1.15);
        assert_eq!(net_top(&[1e6], &[2e6]), 2e6 * 1.15);
    }

    #[test]
    fn rows_read_as_resource_row() {
        let stats = vec![
            stat("cpu", "blue", "12%", vec![0.1, 0.12], 1.0),
            stat("memory", "green", "40%", vec![0.4], 1.0),
            stat("swap", "purple", "3%", vec![0.03], 1.0),
            stat("temperature", "orange", "52°", vec![52.0], 100.0),
            stat("netDown", "teal", "1.2 KB/s", vec![1200.0], 0.0),
            stat("netUp", "pink", "300 B/s", vec![300.0], 0.0),
        ];
        let r = rows(&stats, "6.1 of 15.4 GB", None);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].title, r[0].value.as_str(), r[0].detail.as_str()), ("Processor", "12%", "52°C"));
        assert_eq!(r[1].detail, "6.1 of 15.4 GB · swap 3%");
        assert_eq!((r[2].value.as_str(), r[2].value2.as_str(), r[2].tone2), ("↓ 1.2 KB/s", "↑ 300 B/s", "pink"));
        // (no swap in use: none said)
        let mut s2 = stats.clone();
        s2[2].history = vec![0.0];
        assert_eq!(rows(&s2, "1.0 of 2.0 GB", None)[1].detail, "1.0 of 2.0 GB");
        let g = stat("gpu", "indigo", "7%", vec![0.07], 1.0);
        let r = rows(&stats, "", Some(&g));
        assert_eq!((r[1].title, r[1].icon, r.len()), ("Graphics", "graphics-card", 4));
        assert_eq!(r[1].detail, "");
        let g = Stat { label: AWAKE, ..g };
        assert_eq!(rows(&stats, "", Some(&g))[1].detail, "Time awake, not load");
        assert_eq!(mem_detail("MemTotal: 16148844 kB\nMemAvailable: 9759008 kB\n"), "6.1 of 15.4 GB");
    }

    #[test]
    fn the_booted_card_is_read_first_whatever_its_kind() {
        let d = std::env::temp_dir().join(format!("tessera-gpu-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (intel, amd) = (d.join("card1"), d.join("card0"));
        std::fs::create_dir_all(intel.join("gt/gt0")).unwrap();
        std::fs::write(intel.join("gt/gt0/rc6_residency_ms"), "100\n").unwrap();
        std::fs::create_dir_all(amd.join("device")).unwrap();
        std::fs::write(amd.join("device/gpu_busy_percent"), "0\n").unwrap();
        // (the Intel card first, as cards() puts the boot_vga one)
        let got = first_reading(&[intel.clone(), amd.clone()]).unwrap();
        assert_eq!(got, (false, intel.join("gt/gt0/rc6_residency_ms").to_string_lossy().into_owned()));
        let got = first_reading(&[amd.clone(), intel]).unwrap();
        assert!(got.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_sleeping_gpu_is_left_asleep() {
        assert!(suspended("suspended\n") && suspended("suspending"));
        assert!(!suspended("active\n") && !suspended("resuming") && !suspended("unsupported"));
        // a card in runtime suspend reads as idle, its busy file not read
        let d = std::env::temp_dir().join(format!("tessera-gpu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("power")).unwrap();
        std::fs::write(d.join("power/runtime_status"), "suspended\n").unwrap();
        let busy = d.join("gpu_busy_percent");
        std::fs::write(&busy, "80\n").unwrap();
        let mut g = Gpu { found: Some(Some((true, busy.to_string_lossy().into_owned()))), ..Default::default() };
        g.sample(1.0);
        assert_eq!(g.history, vec![0.0]);
        std::fs::write(d.join("power/runtime_status"), "active\n").unwrap();
        g.sample(2.0);
        assert_eq!(g.history, vec![0.0, 0.8]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
