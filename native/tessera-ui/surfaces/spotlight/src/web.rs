//! Search's live web results: under the local results, a moment after you
//! stop typing, the top of the engine's own results page (Google's unless
//! Settings chose another), drawn by `tessera-web --render` out of sight
//! and shown here as a picture; a click goes through its links.json to
//! the quick-look browser at that link. Settings → Default apps, "Web
//! results in Search" (`search.webResults`, on); never for paths,
//! commands or sums (rank::wants_web). One render at a time: a new query
//! stops the last, and closing Search stops it too.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};

/// How long typing has to pause before the words go out
pub const DEBOUNCE_MS: u64 = 500;
/// The page's width (CSS px): wide enough that Google puts its knowledge
/// panel in a column of its own, so the results' column is the one shown
pub const RENDER_W: i32 = 1200;
/// The most of the results' column shown (CSS px, about what the panel shows)
pub const RENDER_H: i32 = 380;

/// A link in the picture (its pixels), from links.json
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub href: String,
    pub title: String,
    /// one of the page's search results (its title link)
    pub result: bool,
    pub snippet: String,
}

impl Link {
    /// Where it goes: the result's own address when the page gave it
    pub fn target(&self) -> &str {
        &self.href
    }
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// links.json, as tessera-web --render writes it (web/src/render.rs)
pub fn parse_links(v: &Value) -> Vec<Link> {
    let Some(a) = v.as_array() else { return Vec::new() };
    a.iter()
        .filter_map(|l| {
            let f = |k: &str| l[k].as_f64().unwrap_or(0.0) as f32;
            let s = |k: &str| l[k].as_str().unwrap_or("").to_string();
            let result = l["result"].as_bool() == Some(true);
            // (a result's own address when it's there, else the link's)
            let href = if result && tessera_core::weblink::is_web(&s("url")) { s("url") } else { s("href") };
            (tessera_core::weblink::is_web(&href) && f("w") > 0.0 && f("h") > 0.0).then(|| Link { x: f("x"), y: f("y"), w: f("w"), h: f("h"), href, title: s("title"), result, snippet: s("snippet") })
        })
        .collect()
}

/// The link at a point of the picture (its pixels): the smallest one there
pub fn link_at(links: &[Link], x: f32, y: f32) -> Option<&Link> {
    links.iter().filter(|l| l.contains(x, y)).min_by(|a, b| (a.w * a.h).partial_cmp(&(b.w * b.h)).unwrap_or(std::cmp::Ordering::Equal))
}

/// The results the arrow keys go through, top to bottom (one per address)
pub fn results(links: &[Link]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..links.len()).filter(|i| links[*i].result).collect();
    idx.sort_by(|a, b| links[*a].y.partial_cmp(&links[*b].y).unwrap_or(std::cmp::Ordering::Equal).then(links[*a].x.partial_cmp(&links[*b].x).unwrap_or(std::cmp::Ordering::Equal)));
    let mut seen = std::collections::HashSet::new();
    idx.retain(|i| seen.insert(links[*i].href.clone()));
    idx
}

/// The part of the engine's page shown: Google's results column (no
/// header, no side panel); the page's top for the others
pub fn selector(engine: &str) -> Option<&'static str> {
    match engine {
        "google" => Some("#center_col"),
        _ => None,
    }
}

/// tessera-web's arguments for one picture of `url`
pub fn args(engine: &str, url: &str, out: &Path, scale: f32) -> Vec<String> {
    let scale = if scale.is_finite() { scale.clamp(1.0, 2.0) } else { 1.0 };
    let mut a = vec![
        "--render".to_string(),
        url.to_string(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
        "--width".into(),
        RENDER_W.to_string(),
        "--height".into(),
        RENDER_H.to_string(),
        "--scale".into(),
        format!("{scale:.2}"),
    ];
    if let Some(s) = selector(engine) {
        a.push("--selector".into());
        a.push(s.into());
    }
    a
}

/// Google's robot check (tessera-web --render says `{"event":"blocked"}`
/// for google.*/sorry or a reCAPTCHA page, never a picture of it: it shows
/// the address): no more renders of Google's results for this long, or
/// until you've passed the check in the quick-look browser (it writes
/// `google-ok.json` in its data folder when a Google results page loads
/// there and stays). Meanwhile the panel is the plain "Search Google for …"
pub const BACKOFF_SECS: u64 = 30 * 60;

/// tessera-web's note that Google let it through: {"at": unix seconds}
pub fn google_ok_path() -> PathBuf {
    tessera_core::util::xdg("XDG_DATA_HOME", ".local/share").join("tessera/web/google-ok.json")
}

/// When Google last let the quick-look browser through (unix seconds)
pub fn google_ok_at(text: &str) -> Option<u64> {
    serde_json::from_str::<Value>(text).ok()?["at"].as_u64()
}

/// Backing off Google's results now: blocked within BACKOFF_SECS, and
/// not let through since
pub fn backed_off(blocked_at: Option<u64>, ok_at: Option<u64>, now: u64) -> bool {
    blocked_at.is_some_and(|b| now < b.saturating_add(BACKOFF_SECS) && !ok_at.is_some_and(|o| o >= b))
}

/// Where the pictures go: the session's runtime folder (memory, gone at log out)
pub fn out_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("tessera/search-web")
}

/// The one render process: started for a query, stopped (killed, by its
/// own handle: never a name) when the text changes or Search closes
#[derive(Default)]
pub struct Renderer {
    child: Option<Arc<Mutex<Option<Child>>>>,
}

impl Renderer {
    /// Start one (the last one stopped first). `on_line`: each JSON line it
    /// prints; `on_end`: it ended (after its last line). False: it couldn't start
    pub fn start(&mut self, args: Vec<String>, on_line: impl Fn(Value) + Send + 'static, on_end: impl FnOnce() + Send + 'static) -> bool {
        self.stop();
        let mut child = match tessera_core::weblink::spawn_render(&args) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let Some(out) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        };
        let held = Arc::new(Mutex::new(Some(child)));
        self.child = Some(held.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    on_line(v);
                }
            }
            // (its output closed: it ended or was stopped; reaped here)
            let c = held.lock().ok().and_then(|mut g| g.take());
            if let Some(mut c) = c {
                let _ = c.wait();
            }
            on_end();
        });
        true
    }

    /// Stop the one running (if any); its thread reaps it
    pub fn stop(&mut self) {
        if let Some(held) = self.child.take() {
            if let Ok(mut g) = held.lock() {
                if let Some(c) = g.as_mut() {
                    let _ = c.kill();
                }
            }
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn links() -> Vec<Link> {
        parse_links(&json!([
            {"x": 0, "y": 70, "w": 1100, "h": 90, "href": "https://www.google.com/goto?url=b", "title": "Second", "result": true, "url": "https://b.example/", "snippet": "B"},
            {"x": 0, "y": 0, "w": 1100, "h": 60, "href": "https://www.google.com/goto?url=a", "title": "First", "result": true, "url": "", "snippet": ""},
            {"x": 20, "y": 10, "w": 100, "h": 20, "href": "https://small.example/", "title": "Small"},
            {"x": 0, "y": 200, "w": 100, "h": 20, "href": "javascript:void(0)", "title": "Script"},
            {"x": 0, "y": 300, "w": 1100, "h": 60, "href": "https://www.google.com/goto?url=b2", "title": "Second again", "result": true, "url": "https://b.example/"},
        ]))
    }

    #[test]
    fn links_from_the_render() {
        let l = links();
        assert_eq!(l.len(), 4);
        // (a result goes to its own address when the page gave it)
        assert_eq!(l[0].target(), "https://b.example/");
        assert_eq!(l[1].target(), "https://www.google.com/goto?url=a");
        assert!(parse_links(&json!({"not": "a list"})).is_empty());
    }

    #[test]
    fn a_click_finds_the_smallest_link_there() {
        let l = links();
        assert_eq!(link_at(&l, 30.0, 15.0).map(|x| x.title.as_str()), Some("Small"));
        assert_eq!(link_at(&l, 500.0, 15.0).map(|x| x.title.as_str()), Some("First"));
        assert_eq!(link_at(&l, 500.0, 100.0).map(|x| x.title.as_str()), Some("Second"));
        assert!(link_at(&l, 500.0, 65.0).is_none());
    }

    #[test]
    fn the_keys_go_through_the_results_in_order() {
        let l = links();
        let r = results(&l);
        // (top to bottom; the same address once)
        assert_eq!(r.iter().map(|i| l[*i].title.as_str()).collect::<Vec<_>>(), ["First", "Second"]);
    }

    #[test]
    fn google_is_left_alone_for_a_while_after_its_robot_check() {
        let b = 1_000_000;
        // never blocked: renders as usual
        assert!(!backed_off(None, None, b));
        // blocked: 30 minutes off
        assert!(backed_off(Some(b), None, b));
        assert!(backed_off(Some(b), None, b + BACKOFF_SECS - 1));
        assert!(!backed_off(Some(b), None, b + BACKOFF_SECS));
        // passed the check in the browser since: renders again
        assert!(!backed_off(Some(b), Some(b + 60), b + 61));
        // a pass from before the block doesn't count
        assert!(backed_off(Some(b), Some(b - 60), b + 61));
        // google-ok.json
        assert_eq!(google_ok_at(r#"{"at": 1764000000}"#), Some(1_764_000_000));
        assert_eq!(google_ok_at("{}"), None);
        assert_eq!(google_ok_at("not json"), None);
        assert!(google_ok_path().ends_with("tessera/web/google-ok.json"));
    }

    #[test]
    fn what_is_asked_of_tessera_web() {
        let a = args("google", "https://www.google.com/search?q=x", Path::new("/run/user/1/tessera/search-web"), 1.25);
        assert_eq!(a[..2], ["--render".to_string(), "https://www.google.com/search?q=x".to_string()]);
        assert!(a.windows(2).any(|w| w[0] == "--selector" && w[1] == "#center_col"));
        assert!(a.windows(2).any(|w| w[0] == "--scale" && w[1] == "1.25"));
        assert!(a.windows(2).any(|w| w[0] == "--width" && w[1] == "1200"));
        let b = args("duckduckgo", "https://duckduckgo.com/?q=x", Path::new("/tmp"), 9.0);
        assert!(!b.contains(&"--selector".to_string()));
        assert!(b.windows(2).any(|w| w[0] == "--scale" && w[1] == "2.00"));
        assert!(out_dir().ends_with("tessera/search-web"));
    }
}
