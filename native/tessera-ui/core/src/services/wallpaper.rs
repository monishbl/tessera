//! How light is the wallpaper under what's drawn straight on it?
//! (services::wallpaper::WallTop)
//!
//! Decides dark or light ink for what sits on the wallpaper with no surface
//! of its own: the menu bar (Glass's text, Material's pills), the lock
//! screen's clock, date and chips, the desktop's icon names. Panels, menus
//! and windows have their own surface and keep the system's light or dark.
//!
//! One measurement per picture: the picture squeezed into GRID x GRID, each
//! cell's luma kept, so any part asks for the average under any rectangle of
//! its screen (the picture covers the screen, cropped to fit, as the
//! wallpaper draws it). `sample(path, tx)` measures on a thread (cached by
//! path and modification time, so the bar, the lock and the desktop asking
//! for the same picture decode it once) and sends the answer as
//! Msg::Data(WallTop). `Follow` asks again when the picture showing changes:
//! another path, a time-of-day step, the file written anew.
//!
//! The rule: luma (Rec. 709 weights on the sRGB values, 0..1) above 0.55 is
//! a light wallpaper: dark ink. (The QML's Background.qml: the top 3%
//! above 0.6; one rule for every region here.)

use crate::Msg;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// The measurement's cells across and down
pub const GRID: usize = 64;
/// Above this luma (0..1) the wallpaper is light: dark ink on it
pub const DARK_INK_ABOVE: f32 = 0.55;
/// The menu bar's strip, as a share of the picture's height (the QML's top
/// two rows of 64)
const TOP_SHARE: f32 = 2.0 / 64.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallTop {
    pub path: String,
    /// Light under the menu bar (the picture's top 3%): dark ink there.
    /// Parts that know their screen ask `dark_ink_under` instead
    pub light: bool,
    /// The picture's width / height (0: not read)
    pub aspect: f32,
    /// Each cell's luma, GRID x GRID, row by row (empty: not read)
    pub luma: Vec<f32>,
}

/// A colour's luma, 0..1
pub fn luma(p: [u8; 3]) -> f32 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
}

/// The rule: dark ink over this luma?
pub fn dark_ink(luma: f32) -> bool {
    luma > DARK_INK_ABOVE
}

/// The decision from some RGB pixels (their average luma)
pub fn top_is_light(rgb: &[[u8; 3]]) -> bool {
    if rgb.is_empty() {
        return false;
    }
    dark_ink(rgb.iter().map(|p| luma(*p)).sum::<f32>() / rgb.len() as f32)
}

/// How much of cell `i` (spanning i..i+1) lies within a..b
fn overlap(i: usize, a: f32, b: f32) -> f32 {
    ((i + 1) as f32).min(b) - (i as f32).max(a)
}

/// The area-weighted mean of a `n` x `n` grid over u0..u1, v0..v1 (0..1 of
/// the picture); a rectangle thinner than a cell takes the cell it's in
pub fn grid_mean(grid: &[f32], n: usize, u0: f32, v0: f32, u1: f32, v1: f32) -> Option<f32> {
    if n == 0 || grid.len() < n * n {
        return None;
    }
    let nf = n as f32;
    let span = |a: f32, b: f32| {
        let (mut a, mut b) = (a.clamp(0.0, 1.0) * nf, b.clamp(0.0, 1.0) * nf);
        if b - a < 1.0 {
            let c = ((a + b) / 2.0).clamp(0.5, nf - 0.5);
            (a, b) = (c - 0.5, c + 0.5);
        }
        (a, b)
    };
    let (a, b) = span(u0, u1);
    let (c, d) = span(v0, v1);
    let (mut sum, mut wsum) = (0.0f32, 0.0f32);
    for j in (c.floor() as usize)..(d.ceil() as usize).min(n) {
        let wy = overlap(j, c, d);
        if wy <= 0.0 {
            continue;
        }
        for i in (a.floor() as usize)..(b.ceil() as usize).min(n) {
            let w = overlap(i, a, b) * wy;
            if w > 0.0 {
                sum += grid[j * n + i] * w;
                wsum += w;
            }
        }
    }
    (wsum > 0.0).then(|| sum / wsum)
}

/// A point of a screen (px) on a picture of `aspect` covering it (cropped
/// to fill, centred, as the wallpaper draws it), as 0..1 of the picture
pub fn on_picture(aspect: f32, screen: (f32, f32), x: f32, y: f32) -> (f32, f32) {
    let (sw, sh) = (screen.0.max(1.0), screen.1.max(1.0));
    let s = sw / sh;
    let (mut u, mut v) = (x / sw, y / sh);
    if aspect > s {
        // (wider than the screen: its sides are cut)
        let vis = s / aspect;
        u = (1.0 - vis) / 2.0 + u * vis;
    } else if aspect > 0.0 && aspect < s {
        // (taller: its top and bottom are cut)
        let vis = aspect / s;
        v = (1.0 - vis) / 2.0 + v * vis;
    }
    (u, v)
}

impl WallTop {
    /// From a picture's grid (luma per cell) and its aspect
    pub fn from_grid(path: &str, aspect: f32, luma: Vec<f32>) -> Self {
        let light = grid_mean(&luma, GRID, 0.0, 0.0, 1.0, TOP_SHARE).is_some_and(dark_ink);
        WallTop { path: path.to_string(), light, aspect, luma }
    }

    /// Read a picture (on a thread: a 4K picture decodes in a few tenths of
    /// a second); one that can't be read gives no measurement (light ink)
    pub fn read(path: &str) -> Self {
        let read = || {
            let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
            let aspect = img.width() as f32 / img.height().max(1) as f32;
            // (a box average: a 4K picture in milliseconds, and the regions are averages anyway)
            let small = img.thumbnail_exact(GRID as u32, GRID as u32).to_rgb8();
            Some((aspect, small.pixels().map(|p| luma(p.0)).collect::<Vec<f32>>()))
        };
        match read() {
            Some((aspect, grid)) => Self::from_grid(path, aspect, grid),
            None => WallTop { path: path.to_string(), ..Default::default() },
        }
    }

    /// The average luma (0..1) under a rectangle (x, y, w, h, px) of a
    /// screen of `screen` px the picture covers; None when not measured
    pub fn luma_under(&self, screen: (f32, f32), rect: (f32, f32, f32, f32)) -> Option<f32> {
        if self.luma.is_empty() {
            return None;
        }
        let (u0, v0) = on_picture(self.aspect, screen, rect.0, rect.1);
        let (u1, v1) = on_picture(self.aspect, screen, rect.0 + rect.2, rect.1 + rect.3);
        grid_mean(&self.luma, GRID, u0, v0, u1, v1)
    }

    /// Dark ink under that rectangle? None when not measured
    pub fn dark_ink_under(&self, screen: (f32, f32), rect: (f32, f32, f32, f32)) -> Option<bool> {
        self.luma_under(screen, rect).map(dark_ink)
    }
}

fn stamp(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The last few pictures measured, by path and modification time
static CACHE: Mutex<Vec<(String, Option<SystemTime>, WallTop)>> = Mutex::new(Vec::new());

/// A picture's measurement, from the cache when its file is unchanged
pub fn measured(path: &str) -> WallTop {
    let key = stamp(path);
    if let Ok(c) = CACHE.lock() {
        if let Some((_, _, w)) = c.iter().find(|(p, t, _)| p == path && *t == key) {
            return w.clone();
        }
    }
    let w = WallTop::read(path);
    if let Ok(mut c) = CACHE.lock() {
        c.retain(|(p, _, _)| p != path);
        c.push((path.to_string(), key, w.clone()));
        if c.len() > 4 {
            c.remove(0);
        }
    }
    w
}

/// Measure a picture on a thread; the answer arrives as Msg::Data(WallTop)
pub fn sample(path: &str, tx: Sender<Msg>) {
    let path = path.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(Msg::Data(Box::new(measured(&path))));
    });
}

/// The picture a part's ink follows: the one showing now
/// (wallpaper::showing_now: the still, the time-of-day step, a live
/// wallpaper's poster). `check` measures it again when it changed, looking
/// at most every 20 s (it's called from clocks) unless forced
#[derive(Default)]
pub struct Follow {
    key: Option<(String, Option<SystemTime>)>,
    looked: Option<Instant>,
}

impl Follow {
    /// The picture followed ("" before the first look)
    pub fn path(&self) -> &str {
        self.key.as_ref().map_or("", |k| k.0.as_str())
    }

    /// Whether this measurement is the followed picture's
    pub fn is(&self, w: &WallTop) -> bool {
        !w.path.is_empty() && w.path == self.path()
    }

    /// Look at what's showing; true when it changed (a measurement is on its way)
    pub fn check(&mut self, cfg: &serde_json::Value, material: bool, tx: &Sender<Msg>, force: bool) -> bool {
        if !force && self.looked.is_some_and(|t| t.elapsed() < Duration::from_secs(20)) {
            return false;
        }
        self.looked = Some(Instant::now());
        let path = crate::wallpaper::showing_now(cfg, material);
        self.follow(&path, tx)
    }

    /// Follow this picture; true when it changed
    pub fn follow(&mut self, path: &str, tx: &Sender<Msg>) -> bool {
        let key = (path.to_string(), stamp(path));
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.key = Some(key);
        if !path.is_empty() {
            sample(path, tx.clone());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(l: f32) -> Vec<f32> {
        vec![l; GRID * GRID]
    }

    #[test]
    fn light_above_the_threshold() {
        assert!(top_is_light(&[[255, 255, 255]; 128]));
        assert!(!top_is_light(&[[20, 30, 40]; 128]));
        // a pale sky: light; a saturated blue (low luminance) isn't
        assert!(top_is_light(&[[200, 220, 240]; 128]));
        assert!(!top_is_light(&[[30, 90, 230]; 128]));
        // half white, half black averages 0.5: dark
        let mut half = vec![[255, 255, 255]; 64];
        half.extend(vec![[0, 0, 0]; 64]);
        assert!(!top_is_light(&half));
        assert!(!top_is_light(&[]));
        // the rule's edge
        assert!(!dark_ink(0.55));
        assert!(dark_ink(0.56));
        assert!(!dark_ink(0.3));
    }

    #[test]
    fn regions_of_a_split_picture() {
        // top half white, bottom half black
        let mut g = flat(0.0);
        for v in g.iter_mut().take(GRID * GRID / 2) {
            *v = 1.0;
        }
        let w = WallTop::from_grid("/p.jpg", 16.0 / 9.0, g);
        assert!(w.light, "the bar's strip is in the white");
        let screen = (1920.0, 1080.0);
        assert_eq!(w.dark_ink_under(screen, (0.0, 0.0, 1920.0, 32.0)), Some(true));
        assert_eq!(w.dark_ink_under(screen, (0.0, 700.0, 1920.0, 300.0)), Some(false));
        // straddling the middle evenly: 0.5, light ink
        let m = w.luma_under(screen, (0.0, 270.0, 1920.0, 540.0)).unwrap();
        assert!((m - 0.5).abs() < 0.02, "{m}");
        // a rectangle thinner than a cell still reads its cell
        assert_eq!(w.dark_ink_under(screen, (100.0, 10.0, 1.0, 1.0)), Some(true));
        // nothing measured: no answer
        assert_eq!(WallTop::default().luma_under(screen, (0.0, 0.0, 10.0, 10.0)), None);
        assert!(!WallTop::default().light);
    }

    #[test]
    fn the_picture_covers_the_screen() {
        // a picture as the screen: the same place
        let (u, v) = on_picture(16.0 / 9.0, (1920.0, 1080.0), 960.0, 540.0);
        assert!((u - 0.5).abs() < 1e-5 && (v - 0.5).abs() < 1e-5);
        // twice as wide as the screen: half of it shows, the middle half
        let (u, v) = on_picture(2.0 * 16.0 / 9.0, (1920.0, 1080.0), 0.0, 0.0);
        assert!((u - 0.25).abs() < 1e-5 && v.abs() < 1e-5);
        let (u, _) = on_picture(2.0 * 16.0 / 9.0, (1920.0, 1080.0), 1920.0, 0.0);
        assert!((u - 0.75).abs() < 1e-5);
        // a square picture on a wide screen: its top and bottom are cut, so
        // the screen's top edge is (1 - 9/16) / 2 of the way down it
        let (u, v) = on_picture(1.0, (1920.0, 1080.0), 0.0, 0.0);
        let vis = 1.0 / (16.0 / 9.0);
        assert!(u.abs() < 1e-5 && (v - (1.0 - vis) / 2.0).abs() < 1e-5);
        // a bright stripe cut off the top of a square picture doesn't reach the bar
        let mut g = flat(0.1);
        for v in g.iter_mut().take(GRID * 8) {
            *v = 1.0; // the top 8 rows of 64: within the cut-off 21.9%
        }
        let w = WallTop::from_grid("/sq.jpg", 1.0, g);
        assert!(w.light, "the picture's own top is light");
        assert_eq!(w.dark_ink_under((1920.0, 1080.0), (0.0, 0.0, 1920.0, 32.0)), Some(false), "but not the screen's");
    }

    #[test]
    fn grid_mean_edges() {
        assert_eq!(grid_mean(&[], GRID, 0.0, 0.0, 1.0, 1.0), None);
        let g = flat(0.4);
        assert!((grid_mean(&g, GRID, 0.0, 0.0, 1.0, 1.0).unwrap() - 0.4).abs() < 1e-5);
        // out of range clamps to the picture
        assert!((grid_mean(&g, GRID, -1.0, -1.0, 2.0, 2.0).unwrap() - 0.4).abs() < 1e-5);
        assert!(grid_mean(&g, GRID, 1.0, 1.0, 1.0, 1.0).is_some(), "the corner cell");
    }

    #[test]
    fn measured_once_per_picture() {
        let d = std::env::temp_dir().join(format!("tessera-wall-cache-{}.png", std::process::id()));
        image::RgbImage::from_pixel(320, 180, image::Rgb([240, 240, 235])).save(&d).unwrap();
        let p = d.to_string_lossy().into_owned();
        let a = measured(&p);
        assert!(a.light && (a.aspect - 16.0 / 9.0).abs() < 1e-3 && a.luma.len() == GRID * GRID);
        // written anew (another modification time): measured again
        std::thread::sleep(std::time::Duration::from_millis(1100));
        image::RgbImage::from_pixel(320, 180, image::Rgb([10, 10, 10])).save(&d).unwrap();
        let b = measured(&p);
        assert!(!b.light, "a rewritten file is read again");
        let _ = std::fs::remove_file(&d);
        let none = measured("/nonexistent.jpg");
        assert!(none.luma.is_empty() && !none.light && none.path == "/nonexistent.jpg");
    }

    #[test]
    fn the_film_pictures() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../site/film/hero/assets");
        let leaves = WallTop::read(&root.join("leaves.jpg").to_string_lossy());
        if leaves.luma.is_empty() {
            eprintln!("film pictures missing: skipped");
            return;
        }
        assert!(!leaves.light, "the leaves are dark at the top");
    }
}
