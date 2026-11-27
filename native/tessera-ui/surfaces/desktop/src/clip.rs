//! Web clips: a desktop widget showing part of a web page, live. Its
//! options (Widget Options…, options.rs) are kept per widget in config.json's
//! `desktop.webClips`, a list of strings, one JSON object a clip: {id, url,
//! region: "page" | "crop" | "selector", crop: "X,Y,W,H" (page px at
//! CROP_WIDTH), selector: CSS, every: seconds (300, 900, 3600; 0 by hand)}
//! (strings, so the QML fallback's Config.qml keeps them as a list<string>).
//!
//! Each clip on screen runs one `tessera-web --render URL --out DIR … --every
//! S` (the shared contract, ~/tessera-verify/web/SPEC.md): it renders the
//! page in tessera-web's own session, off screen, writes DIR/shot.png and
//! DIR/links.json, and prints `{"event":"rendered",…}` each time. The
//! desktop starts it while the widget shows (on the desktop, or on top of
//! its stack) and stops it when it doesn't, when the clip's options change
//! (it starts again with the new ones), and while battery saver is on. By
//! hand: one render, and again from the widget's menu (Refresh now).
//!
//! The picture is drawn natively, covering the widget from its top left; a
//! click goes through links.json (x, y, w, h in the picture's own pixels,
//! whatever its scale: web/src/render.rs) to the link under it, else the page,
//! in the quick-look browser (weblink::open: where Settings says). With no
//! tessera-web, or one whose --render fails, the widget says so and a
//! click opens the page.

use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A part of the page is chosen on a render this wide (CSS px)
pub const CROP_WIDTH: i32 = 1280;
pub const CROP_HEIGHT: i32 = 960;
/// --width and --height go from 16 to this (tessera-web's args.rs)
pub const RENDER_MOST: i32 = 8192;
/// The refresh choices, in Widget Options' order (seconds; 0 by hand)
pub const EVERY: [u32; 4] = [300, 900, 3600, 0];

#[derive(Clone, Debug, PartialEq)]
pub enum Region {
    /// the top of the page, laid out for the widget's shape
    Page,
    /// this rectangle of the page (CSS px, at CROP_WIDTH)
    Crop(i32, i32, i32, i32),
    /// the first element this CSS selector finds
    Selector(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipCfg {
    pub url: String,
    pub region: Region,
    pub every: u32,
}

/// "X,Y,W,H", whole numbers, W and H above zero
pub fn parse_rect(s: &str) -> Option<(i32, i32, i32, i32)> {
    let n: Vec<i32> = s.split(',').map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() < 1e6).map(|v| v.round() as i32)).collect::<Option<Vec<_>>>()?;
    match n[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some((x.max(0), y.max(0), w, h)),
        _ => None,
    }
}

impl ClipCfg {
    /// `desktop.webClips.<id>` (None: no address yet)
    pub fn read(v: &Value) -> Option<ClipCfg> {
        let url = v["url"].as_str().unwrap_or("").trim().to_string();
        if !tessera_core::weblink::is_web(&url) {
            return None;
        }
        let region = match v["region"].as_str().unwrap_or("page") {
            "crop" => v["crop"].as_str().and_then(parse_rect).map_or(Region::Page, |(x, y, w, h)| Region::Crop(x, y, w, h)),
            "selector" => {
                let s = v["selector"].as_str().unwrap_or("").trim().to_string();
                if s.is_empty() { Region::Page } else { Region::Selector(s) }
            }
            _ => Region::Page,
        };
        let every = v["every"].as_u64().map(|e| e as u32).filter(|e| EVERY.contains(e)).unwrap_or(900);
        Some(ClipCfg { url, region, every })
    }
}

/// The clip's options in config.json (Null when none)
pub fn cfg_value(cfg: &Value, id: &str) -> Value {
    if id.is_empty() {
        return Value::Null;
    }
    cfg["desktop"]["webClips"]
        .as_array()
        .and_then(|a| a.iter().filter_map(|e| e.as_str()).filter_map(|e| serde_json::from_str::<Value>(e).ok()).find(|v| v["id"].as_str() == Some(id)))
        .unwrap_or(Value::Null)
}

/// `desktop.webClips` with this clip's options changed (`f` on its object;
/// a new clip added), every other entry as it was
pub fn with_changed(cfg: &Value, id: &str, f: impl FnOnce(&mut Value)) -> Vec<String> {
    let mut list: Vec<String> = cfg["desktop"]["webClips"].as_array().map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect()).unwrap_or_default();
    let at = list.iter().position(|e| serde_json::from_str::<Value>(e).ok().is_some_and(|v| v["id"].as_str() == Some(id)));
    let mut v = at.and_then(|n| serde_json::from_str::<Value>(&list[n]).ok()).filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    f(&mut v);
    v["id"] = Value::String(id.to_string());
    let text = v.to_string();
    match at {
        Some(n) => list[n] = text,
        None => list.push(text),
    }
    list
}

/// Where a clip's pictures go: ~/.cache/tessera/web/clips/<id>
pub fn dir(id: &str) -> PathBuf {
    let safe: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    tessera_core::util::xdg("XDG_CACHE_HOME", ".cache").join("tessera/web/clips").join(if safe.is_empty() { "clip".to_string() } else { safe })
}

/// tessera-web's arguments for a widget `pw` × `ph` (logical px) big
pub fn render_args(c: &ClipCfg, out: &Path, pw: f32, ph: f32) -> Vec<String> {
    let mut a = vec!["--render".to_string(), c.url.clone(), "--out".into(), out.to_string_lossy().into_owned()];
    match &c.region {
        Region::Page => {
            // (laid out about twice the widget's size: a narrow page, as a phone sees it)
            let w = ((pw * 2.0).round() as i32).clamp(360, RENDER_MOST);
            let h = ((w as f32 * ph / pw.max(1.0)).round() as i32).clamp(200, RENDER_MOST);
            a.extend(["--width".into(), w.to_string(), "--height".into(), h.to_string()]);
        }
        Region::Crop(x, y, w, h) => {
            let vh = CROP_HEIGHT.max(y + h).min(RENDER_MOST);
            a.extend(["--width".into(), CROP_WIDTH.to_string(), "--height".into(), vh.to_string(), "--clip".into(), format!("{x},{y},{w},{h}")]);
        }
        Region::Selector(s) => {
            a.extend(["--width".into(), CROP_WIDTH.to_string(), "--height".into(), CROP_HEIGHT.to_string(), "--selector".into(), s.clone()]);
        }
    }
    if c.every > 0 {
        a.extend(["--every".into(), c.every.to_string()]);
    }
    a
}

/// The first full render, for choosing a part of the page in Widget Options
pub fn preview_args(url: &str, out: &Path) -> Vec<String> {
    vec!["--render".into(), url.to_string(), "--out".into(), out.to_string_lossy().into_owned(), "--width".into(), CROP_WIDTH.to_string(), "--height".into(), CROP_HEIGHT.to_string()]
}

/// A part chosen on the preview (fractions of the picture, 0..1) as page
/// px: the preview is CROP_WIDTH CSS px across, however many pixels it has
pub fn crop_from(fx: f32, fy: f32, fw: f32, fh: f32, png_w: u32, png_h: u32) -> Option<(i32, i32, i32, i32)> {
    if png_w == 0 || png_h == 0 {
        return None;
    }
    let page_h = CROP_WIDTH as f32 * png_h as f32 / png_w as f32;
    let (x0, y0) = (fx.min(fx + fw).clamp(0.0, 1.0), fy.min(fy + fh).clamp(0.0, 1.0));
    let (x1, y1) = (fx.max(fx + fw).clamp(0.0, 1.0), fy.max(fy + fh).clamp(0.0, 1.0));
    let (x, y) = ((x0 * CROP_WIDTH as f32).round() as i32, (y0 * page_h).round() as i32);
    let (w, h) = (((x1 - x0) * CROP_WIDTH as f32).round() as i32, ((y1 - y0) * page_h).round() as i32);
    (w >= 16 && h >= 16).then_some((x, y, w, h))
}

/// One clickable area of a render (links.json)
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub href: String,
}

pub fn parse_links(text: &str) -> Vec<Link> {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let n = |e: &Value, k: &str| e[k].as_f64().unwrap_or(0.0) as f32;
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let href = e["href"].as_str()?.trim().to_string();
                    if !tessera_core::weblink::is_web(&href) {
                        return None;
                    }
                    Some(Link { x: n(e, "x"), y: n(e, "y"), w: n(e, "w"), h: n(e, "h"), href })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A tap at (fx, fy), fractions of the widget (`bw` × `bh`), on a picture
/// `pw` × `ph` drawn covering it from its top left: the picture's pixel
pub fn tap_on_picture(fx: f32, fy: f32, bw: f32, bh: f32, pw: u32, ph: u32) -> (f32, f32) {
    let s = (bw / pw.max(1) as f32).max(bh / ph.max(1) as f32).max(1e-6);
    (fx.clamp(0.0, 1.0) * bw / s, fy.clamp(0.0, 1.0) * bh / s)
}

/// The link under a point of the picture (the smallest area there)
pub fn link_at(links: &[Link], x: f32, y: f32) -> Option<&Link> {
    links.iter().filter(|l| l.w > 0.0 && l.h > 0.0 && x >= l.x && x < l.x + l.w && y >= l.y && y < l.y + l.h).min_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
}

/// The part of a picture `iw` × `ih` that covers a `w` × `h` box from its
/// top left (x, y, cw, ch in the picture's pixels)
pub fn top_cover(iw: u32, ih: u32, w: u32, h: u32) -> (u32, u32) {
    let s = (w as f32 / iw.max(1) as f32).max(h as f32 / ih.max(1) as f32);
    (((w as f32 / s).round() as u32).clamp(1, iw.max(1)), ((h as f32 / s).round() as u32).clamp(1, ih.max(1)))
}

/// A render's picture for a widget `w` × `h` device px
pub fn picture(path: &str, w: u32, h: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let img = crate::files::open_limited(path)?;
    let (cw, ch) = top_cover(img.width(), img.height(), w.max(1), h.max(1));
    let out = img.crop_imm(0, 0, cw, ch).resize_exact(w.max(1), h.max(1), image::imageops::FilterType::Triangle).to_rgba8();
    Some(slint::SharedPixelBuffer::clone_from_slice(out.as_raw(), w.max(1), h.max(1)))
}

/// What a render printed: {"event":"rendered","png":…,"links":…}
pub fn rendered(line: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["event"].as_str() != Some("rendered") {
        return None;
    }
    let png = v["png"].as_str()?.to_string();
    Some((png, v["links"].as_str().unwrap_or("").to_string()))
}

// ── The renderers ────────────────────────────────────────────────────────

/// What a renderer told the desktop
pub enum Event {
    /// a new picture (its path, its size, the links in it)
    Shot { png: String, size: (u32, u32), links: Vec<Link> },
    /// it ended (true: well, false: it failed or couldn't start)
    Ended(bool),
}

/// The newest picture of a clip
#[derive(Clone, Debug, Default)]
pub struct Shot {
    pub png: String,
    pub size: (u32, u32),
    pub links: Vec<Link>,
    /// counts the pictures (each one decoded afresh)
    pub seq: u64,
}

struct Running {
    child: Child,
    /// the arguments it was started with (another set: started again)
    args: Vec<String>,
    /// which start this is (an event from an older one is dropped)
    gen: u64,
}

#[derive(Default)]
pub struct Clips {
    running: HashMap<String, Running>,
    pub shots: HashMap<String, Shot>,
    /// clips whose renderer failed (or tessera-web is missing): the widget says so
    pub failed: HashMap<String, bool>,
    /// a by-hand clip rendered once already (not again until Refresh now)
    done_once: HashMap<String, Vec<String>>,
    gen: u64,
}

/// A renderer stopped: its exact PID sent SIGTERM (a no-op once it's gone),
/// reaped on a thread
fn drop_child(mut child: Child) {
    if let Ok(None) = child.try_wait() {
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn spawn(args: &[String]) -> std::io::Result<Child> {
    let exe = tessera_core::weblink::bin().ok_or(std::io::ErrorKind::NotFound)?;
    let mut c = Command::new(exe);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    unsafe {
        c.pre_exec(|| {
            tessera_core::util::child_signals();
            // (the shell gone: its renderers go too)
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    c.spawn()
}

impl Clips {
    /// Each clip in `want` (id → its arguments) running, the rest stopped;
    /// `send` hands a renderer's events to the desktop
    pub fn sync(&mut self, want: &HashMap<String, Vec<String>>, send: &(impl Fn(String, u64, Event) + Clone + Send + 'static)) {
        let stale: Vec<String> = self.running.iter().filter(|(id, r)| want.get(*id) != Some(&r.args)).map(|(id, _)| id.clone()).collect();
        for id in stale {
            self.stop(&id);
        }
        // (a by-hand clip whose options changed renders again)
        self.done_once.retain(|id, a| want.get(id) == Some(a));
        for (id, args) in want {
            if self.running.contains_key(id) {
                continue;
            }
            let by_hand = !args.iter().any(|a| a == "--every");
            if by_hand && self.done_once.get(id) == Some(args) {
                continue;
            }
            if self.failed.get(id) == Some(&true) && self.done_once.get(id) == Some(args) {
                continue;
            }
            self.start(id, args.clone(), send.clone());
        }
    }

    fn start(&mut self, id: &str, args: Vec<String>, send: impl Fn(String, u64, Event) + Send + 'static) {
        self.gen += 1;
        let gen = self.gen;
        if let Some(out) = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
            let _ = std::fs::create_dir_all(out);
        }
        self.done_once.insert(id.to_string(), args.clone());
        match spawn(&args) {
            Err(_) => {
                self.failed.insert(id.to_string(), true);
            }
            Ok(mut child) => {
                let out = child.stdout.take();
                let idc = id.to_string();
                std::thread::spawn(move || {
                    if let Some(o) = out {
                        for line in BufReader::new(o).lines().map_while(Result::ok) {
                            let Some((png, links)) = rendered(&line) else { continue };
                            let size = image::image_dimensions(&png).unwrap_or((0, 0));
                            let links = std::fs::read_to_string(&links).map(|t| parse_links(&t)).unwrap_or_default();
                            send(idc.clone(), gen, Event::Shot { png, size, links });
                        }
                    }
                    // (stdout closed: it ended, or is ending; the desktop reaps it)
                    send(idc, gen, Event::Ended(true));
                });
                self.running.insert(id.to_string(), Running { child, args, gen });
            }
        }
    }

    /// An event from a renderer: true when the widget should be drawn again
    pub fn event(&mut self, id: &str, gen: u64, ev: Event) -> bool {
        match ev {
            Event::Shot { png, size, links } => {
                if self.running.get(id).is_none_or(|r| r.gen != gen) {
                    return false;
                }
                let seq = self.shots.get(id).map_or(0, |s| s.seq) + 1;
                self.shots.insert(id.to_string(), Shot { png, size, links, seq });
                self.failed.remove(id);
                true
            }
            Event::Ended(_) => {
                let Some(r) = self.running.get_mut(id).filter(|r| r.gen == gen) else { return false };
                // (its exit status says how it went: it has closed stdout, so
                // it's ending; one that lingers is stopped)
                let mut ok = false;
                for _ in 0..10 {
                    match r.child.try_wait() {
                        Ok(Some(s)) => {
                            ok = s.success();
                            break;
                        }
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
                        Err(_) => break,
                    }
                }
                if let Some(r) = self.running.remove(id) {
                    if !ok {
                        drop_child(r.child);
                    }
                }
                if !ok {
                    self.failed.insert(id.to_string(), true);
                }
                !ok
            }
        }
    }

    /// Stop a clip's renderer
    pub fn stop(&mut self, id: &str) {
        if let Some(r) = self.running.remove(id) {
            drop_child(r.child);
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.running.keys().cloned().collect();
        for id in ids {
            self.stop(&id);
        }
    }

    /// Refresh now (and try again after a failure)
    pub fn refresh(&mut self, id: &str) {
        self.stop(id);
        self.done_once.remove(id);
        self.failed.remove(id);
    }

    /// The clips no longer on the desktop forget their pictures
    pub fn keep_only(&mut self, ids: &[String]) {
        self.shots.retain(|k, _| ids.contains(k));
        self.failed.retain(|k, _| ids.contains(k));
    }
}

impl Drop for Clips {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// A one-off render (the options' preview) on a thread: `done` gets the
/// picture's path and size, or None when it failed
pub fn render_once(args: Vec<String>, done: impl FnOnce(Option<(String, (u32, u32))>) + Send + 'static) {
    std::thread::spawn(move || {
        if let Some(out) = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)) {
            let _ = std::fs::create_dir_all(out);
        }
        let Ok(mut child) = spawn(&args) else { return done(None) };
        let mut got = None;
        if let Some(o) = child.stdout.take() {
            for line in BufReader::new(o).lines().map_while(Result::ok) {
                if let Some((png, _)) = rendered(&line) {
                    let size = image::image_dimensions(&png).unwrap_or((0, 0));
                    got = Some((png, size));
                }
            }
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        done(if ok { got } else { None });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn options_as_config_keeps_them() {
        assert_eq!(ClipCfg::read(&json!({})), None);
        assert_eq!(ClipCfg::read(&json!({"url": "javascript:x"})), None);
        let c = ClipCfg::read(&json!({"url": " https://a.b/x ", "region": "crop", "crop": "10,20,300,200", "every": 300})).unwrap();
        assert_eq!(c, ClipCfg { url: "https://a.b/x".into(), region: Region::Crop(10, 20, 300, 200), every: 300 });
        // (a broken crop or an empty selector: the page; an odd interval: 15 minutes)
        let c = ClipCfg::read(&json!({"url": "https://a.b", "region": "crop", "crop": "1,2,0,4", "every": 7})).unwrap();
        assert_eq!((c.region, c.every), (Region::Page, 900));
        assert_eq!(ClipCfg::read(&json!({"url": "https://a.b", "region": "selector", "selector": " "})).unwrap().region, Region::Page);
        assert_eq!(ClipCfg::read(&json!({"url": "https://a.b", "every": 0})).unwrap().every, 0);
    }

    #[test]
    fn kept_as_strings_in_the_config() {
        let cfg = json!({"desktop": {"webClips": [r#"{"id":"a1","url":"https://a.b"}"#, "broken", r#"{"id":"b2","url":"https://c.d","every":300}"#]}});
        assert_eq!(cfg_value(&cfg, "b2")["every"], json!(300));
        assert!(cfg_value(&cfg, "zz").is_null() && cfg_value(&cfg, "").is_null());
        let l = with_changed(&cfg, "a1", |v| v["every"] = json!(0));
        assert_eq!(l.len(), 3);
        let c = json!({"desktop": {"webClips": l}});
        assert_eq!(cfg_value(&c, "a1")["every"], json!(0));
        assert_eq!(cfg_value(&c, "a1")["url"], json!("https://a.b"));
        let l = with_changed(&c, "new", |v| v["url"] = json!("https://n.ew"));
        assert_eq!(l.len(), 4);
    }

    #[test]
    fn the_render_command_follows_the_contract() {
        let out = Path::new("/c/clips/ab");
        let page = ClipCfg { url: "https://a.b".into(), region: Region::Page, every: 900 };
        assert_eq!(render_args(&page, out, 344.0, 164.0), ["--render", "https://a.b", "--out", "/c/clips/ab", "--width", "688", "--height", "328", "--every", "900"]);
        let crop = ClipCfg { region: Region::Crop(0, 900, 400, 300), every: 0, ..page.clone() };
        assert_eq!(render_args(&crop, out, 164.0, 164.0)[4..], ["--width", "1280", "--height", "1200", "--clip", "0,900,400,300"]);
        let sel = ClipCfg { region: Region::Selector("#score".into()), every: 3600, ..page };
        let a = render_args(&sel, out, 164.0, 164.0);
        assert_eq!(a[8..], ["--selector", "#score", "--every", "3600"]);
        assert!(!preview_args("https://a.b", out).contains(&"--every".to_string()));
        // (never past tessera-web's most size: it would refuse the command)
        let deep = ClipCfg { region: Region::Crop(0, 9000, 400, 300), every: 0, url: "https://a.b".into() };
        assert_eq!(render_args(&deep, out, 164.0, 164.0)[4..8], ["--width", "1280", "--height", "8192"]);
        let wide = ClipCfg { region: Region::Page, every: 0, url: "https://a.b".into() };
        assert_eq!(render_args(&wide, out, 6000.0, 100.0)[4..8], ["--width", "8192", "--height", "200"]);
    }

    #[test]
    fn a_part_chosen_on_the_preview() {
        // (a 2× preview: 2560 px across is still 1280 page px)
        assert_eq!(crop_from(0.25, 0.5, 0.5, 0.25, 2560, 1920), Some((320, 480, 640, 240)));
        // (dragged up and to the left)
        assert_eq!(crop_from(0.75, 0.75, -0.5, -0.5, 1280, 960), Some((320, 240, 640, 480)));
        assert_eq!(crop_from(0.1, 0.1, 0.001, 0.5, 1280, 960), None);
        assert_eq!(crop_from(0.0, 0.0, 1.0, 1.0, 0, 0), None);
    }

    #[test]
    fn a_click_finds_its_link() {
        let links = parse_links(r#"[{"x":0,"y":0,"w":400,"h":300,"href":"https://a.b/all"},{"x":10,"y":10,"w":50,"h":20,"href":"https://a.b/one","title":"One"},{"x":0,"y":0,"w":9,"h":9,"href":"javascript:void(0)"}]"#);
        assert_eq!(links.len(), 2);
        // a 200 × 100 widget over an 800 × 300 picture: covered from the top left, a third
        let (x, y) = tap_on_picture(0.1, 0.3, 200.0, 100.0, 800, 300);
        assert!((x - 60.0).abs() < 0.01 && (y - 90.0).abs() < 0.01);
        assert_eq!(link_at(&links, x, y).map(|l| l.href.as_str()), Some("https://a.b/all"));
        assert_eq!(link_at(&links, 20.0, 15.0).map(|l| l.href.as_str()), Some("https://a.b/one"));
        assert_eq!(link_at(&links, 500.0, 15.0), None);
        assert_eq!(top_cover(800, 300, 200, 100), (600, 300));
        assert_eq!(top_cover(400, 1000, 200, 100), (400, 200));
    }

    #[test]
    fn render_lines() {
        assert_eq!(rendered(r#"{"event":"rendered","png":"/d/shot.png","links":"/d/links.json"}"#), Some(("/d/shot.png".into(), "/d/links.json".into())));
        assert_eq!(rendered(r#"{"event":"closed"}"#), None);
        assert_eq!(rendered("garbage"), None);
        assert_eq!(parse_rect(" 1, 2.4 ,3,4"), Some((1, 2, 3, 4)));
        assert_eq!(parse_rect("1,2,3"), None);
        assert!(dir("../../x").ends_with("clips/x"));
    }
}
