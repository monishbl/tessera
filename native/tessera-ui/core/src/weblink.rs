//! Web links from the shell, and tessera-web: Tessera's quick-look browser,
//! a program of its own (one process per call, so a page can never take
//! the shell down; its command line is the contract, ~/tessera-verify/web/
//! SPEC.md). Every web link the shell opens comes here: the AI's sources
//! and links, Search's web results, links in notifications, the weather's
//! forecast, the bug report. Settings → Default apps, "Open links from
//! Tessera in" (`apps.webLinks`: "tessera", the default, or "browser") says
//! where. Only http and https go to tessera-web; everything else (mailto:,
//! tel:, a file) goes to xdg-open as before, and so does a web link when
//! tessera-web isn't installed or fails to start.
//!
//! Also here: Wi-Fi sign-in pages (`portal`, `network.portalAuto`), Quick
//! Look for links and HTML files (`quicklook`), and what Circle to Search
//! needs to wait for its window (`spawn`, `wait_window`).
//!
//! Google's sign-in is not here and never goes to tessera-web: Google
//! refuses OAuth inside embedded web views (403 disallowed_useragent), so it
//! keeps the default browser. The user agent is never changed to get round it.

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The program, and its windows' class (app id)
pub const BIN: &str = "tessera-web";
pub const CLASS: &str = "tessera-web";

/// tessera-web's bar, in logical px from the window's top-left: the one
/// set of numbers both sides use. tessera-web lays its bar out with them
/// (web/src/main.rs, theme.rs) and Circle to Search lands the circled
/// picture on the address pill's thumbnail with them (screentools circle.rs
/// and circle.slint), so the hand-off has no jump.
pub mod bar {
    /// the bar's padding, left/right and top/bottom
    pub const PAD_X: i32 = 10;
    pub const PAD_Y: i32 = 8;
    /// back, forward and reload: square buttons, this far apart
    pub const BUTTON: i32 = 30;
    pub const SPACING: i32 = 4;
    /// the address pill: its margin either side, its inner padding, its height
    pub const PILL_MARGIN: i32 = 6;
    pub const PILL_PAD: i32 = 12;
    pub const PILL_H: i32 = 34;
    /// the gap between the pill's thumbnail (or glyph) and its words
    pub const PILL_GAP: i32 = 8;
    /// the bar's height (its progress line, 2 px, comes under it)
    pub const H: i32 = PAD_Y + PILL_H + PAD_Y;
    /// where the pill starts
    pub const PILL_X: i32 = PAD_X + 3 * BUTTON + 3 * SPACING + PILL_MARGIN;
    pub const PILL_Y: i32 = PAD_Y;
    /// the address pill's picture (--lens): a square, its corners rounded
    pub const THUMB: i32 = 22;
    pub const THUMB_RADIUS: i32 = 6;
    pub const THUMB_X: i32 = PILL_X + PILL_PAD;
    pub const THUMB_Y: i32 = PILL_Y + (PILL_H - THUMB) / 2;
}

/// A start that ends with an error this soon failed to start (no WebKitGTK,
/// no display): the default browser opens the link instead
pub const FAIL_WITHIN: Duration = Duration::from_secs(5);

/// NetworkManager's connectivity when a sign-in page is in the way
/// (NM_CONNECTIVITY_PORTAL: 0 unknown, 1 none, 2 portal, 3 limited, 4 full)
pub const PORTAL: u32 = 2;

/// The page asked for when NetworkManager names no check address of its own
pub const PROBE_URL: &str = "http://nmcheck.gnome.org/check_network_status.txt";

/// "Open links from Tessera in": the quick-look browser (the default) or yours
pub fn links_in_tessera(cfg: &Value) -> bool {
    // (read with its default, so tools/rust-defaults.json carries it)
    cfg["apps"]["webLinks"].as_str().unwrap_or("tessera") != "browser"
}

/// "Open Wi-Fi sign-in pages automatically" (on unless turned off)
pub fn portal_auto(cfg: &Value) -> bool {
    cfg["network"]["portalAuto"].as_bool().unwrap_or(true)
}

/// Web searches: every one Tessera makes (Search's web row and its live
/// results, tessera-web's address field, a selection searched with the
/// shortcut or a page's menu) goes to the engine Settings → Default apps
/// chose (`search.engine`, Google unless changed), its address built here
pub mod search {
    use serde_json::Value;

    /// (setting value, the name shown, the query address before the words)
    pub const ENGINES: &[(&str, &str, &str)] = &[
        ("google", "Google", "https://www.google.com/search?q="),
        ("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q="),
        ("bing", "Bing", "https://www.bing.com/search?q="),
        ("brave", "Brave Search", "https://search.brave.com/search?q="),
        ("startpage", "Startpage", "https://www.startpage.com/do/search?q="),
    ];

    /// The engine chosen; anything unknown (an older config, a typo) is Google
    pub fn engine(cfg: &Value) -> &'static str {
        let e = cfg["search"]["engine"].as_str().unwrap_or("google");
        ENGINES.iter().find(|x| x.0 == e).map_or("google", |x| x.0)
    }

    /// Its name, as menus write it ("Search Google for …")
    pub fn name(engine: &str) -> &'static str {
        ENGINES.iter().find(|x| x.0 == engine).map_or("Google", |x| x.1)
    }

    /// The query's words for an address: everything but A-Z a-z 0-9 - _ . ~
    /// percent-encoded (so a "&", "#" or "+" stays part of the words)
    pub fn encode(q: &str) -> String {
        let mut out = String::with_capacity(q.len() * 3);
        for b in q.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// The results page for these words on this engine (unknown: Google)
    pub fn url_for(engine: &str, q: &str) -> String {
        let base = ENGINES.iter().find(|x| x.0 == engine).map_or(ENGINES[0].2, |x| x.2);
        format!("{base}{}", encode(q.trim()))
    }

    /// The results page for these words on the chosen engine
    pub fn url(cfg: &Value, q: &str) -> String {
        url_for(engine(cfg), q)
    }

    /// "Web results in Search": Search's live results panel (on unless
    /// turned off)
    pub fn web_results(cfg: &Value) -> bool {
        cfg["search"]["webResults"].as_bool().unwrap_or(true)
    }

    /// Selected text as words to search for: spaces and lines run together,
    /// at most `most` characters (cut at a word where it can be); "" for
    /// nothing worth searching
    pub fn words(text: &str, most: usize) -> String {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() <= most {
            return flat;
        }
        let cut: String = flat.chars().take(most).collect();
        match cut.rfind(' ') {
            Some(i) if i > most / 2 => cut[..i].to_string(),
            _ => cut,
        }
    }

    /// A short form for a menu's label: the first `most` characters and "…"
    pub fn short(text: &str, most: usize) -> String {
        let w = words(text, usize::MAX);
        if w.chars().count() <= most {
            w
        } else {
            format!("{}…", w.chars().take(most).collect::<String>().trim_end())
        }
    }
}

/// tessera-web --render's windows' class, should one ever be mapped (it
/// renders out of sight, its window never shown: web/src/render.rs). The
/// Hyprland rule for it keeps it on a hidden workspace, and the shell's
/// window lists leave it out (`is_render_class`).
pub const RENDER_CLASS: &str = "tessera-web-render";

/// A window no list of the shell's shows (the dock, Alt-Tab, the Overview,
/// All Windows, the window list): tessera-web --render's
pub fn is_render_class(class: &str) -> bool {
    class == RENDER_CLASS
}

/// Open a link in tessera-web, Tessera's quick-look browser, whatever
/// "Open links from Tessera in" says (Search's web results panel: its
/// results and Expand); the default browser when it isn't installed or
/// fails to start
pub fn open_quick(url: &str) {
    let url = url.trim().to_string();
    if !is_web(&url) {
        return xdg_open(&url);
    }
    std::thread::spawn(move || {
        if bin().is_none() {
            return xdg_open(&url);
        }
        let u = url.clone();
        start_or(vec![url], move || xdg_open(&u));
    });
}

/// An http or https link (any case, spaces round it allowed)
pub fn is_web(url: &str) -> bool {
    let u = url.trim();
    let head: String = u.chars().take(8).collect::<String>().to_ascii_lowercase();
    head.starts_with("http://") || head.starts_with("https://")
}

/// An HTML file Quick Look hands to tessera-web (a path or a file:// URL)
pub fn is_html_file(path: &str) -> bool {
    let p = path.trim();
    let p = p.strip_prefix("file://").unwrap_or(p);
    if !p.starts_with('/') {
        return false;
    }
    let ext = Path::new(p).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ext == "html" || ext == "htm"
}

/// Where a link goes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// tessera-web
    Web,
    /// xdg-open: the default browser, mail app, phone app…
    Default,
}

/// The decision: tessera-web for http(s) only, when the setting wants it
/// and it's installed
pub fn route(url: &str, in_tessera: bool, have_web: bool) -> Route {
    if in_tessera && have_web && is_web(url) {
        Route::Web
    } else {
        Route::Default
    }
}

/// Quick Look's decision for a path or link: tessera-web --quicklook for a
/// web link or an HTML file (while it's installed); None: Quick Look's own
/// preview (an HTML file without tessera-web shows as text, as before)
pub fn quicklook_route(target: &str, have_web: bool) -> Option<Route> {
    if is_web(target) {
        Some(if have_web { Route::Web } else { Route::Default })
    } else if is_html_file(target) && have_web {
        Some(Route::Web)
    } else {
        None
    }
}

/// The captive portal's page: NetworkManager's own check address, else GNOME's
pub fn probe_url(check_uri: &str) -> String {
    let u = check_uri.trim();
    if is_web(u) {
        u.to_string()
    } else {
        PROBE_URL.to_string()
    }
}

/// Opening a sign-in page by itself, once per connection: NetworkManager's
/// connectivity and primary connection (its D-Bus path: a new one each time
/// a network is joined, so joining the same café again opens it again)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortalOnce {
    /// the connection whose sign-in page was opened
    opened_for: String,
}

impl PortalOnce {
    /// A new reading: true when the sign-in page should open now
    pub fn step(&mut self, connectivity: u32, primary: &str, auto: bool) -> bool {
        // (that connection went: the next one may open it again)
        if !self.opened_for.is_empty() && self.opened_for != primary {
            self.opened_for.clear();
        }
        if !auto || connectivity != PORTAL || primary.is_empty() || primary == "/" || self.opened_for == primary {
            return false;
        }
        self.opened_for = primary.to_string();
        true
    }
}

/// tessera-web's path: $TESSERA_WEB, beside this program, Tessera's bin
/// folder (~/.local/share/tessera/bin), else on $PATH
pub fn bin() -> Option<PathBuf> {
    let file = |p: PathBuf| p.is_file().then_some(p);
    std::env::var_os("TESSERA_WEB")
        .map(PathBuf::from)
        .and_then(file)
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(BIN))).and_then(file))
        .or_else(|| file(crate::util::xdg("XDG_DATA_HOME", ".local/share").join("tessera/bin").join(BIN)))
        .or_else(|| std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(BIN)).find(|p| p.is_file())))
}

/// Start tessera-web with these arguments, in a scope of its own (as the
/// shell's apps: app-scope.sh) and a session of its own (setsid without -f
/// execs in place, so the child here is tessera-web and its exit is seen)
pub fn spawn(args: &[String]) -> std::io::Result<Child> {
    spawn_with(args, Stdio::null())
}

/// `spawn` with its stdout piped: tessera-web --render, whose JSON lines
/// say each picture is ready (Search's web results read them)
pub fn spawn_render(args: &[String]) -> std::io::Result<Child> {
    spawn_with(args, Stdio::piped())
}

fn spawn_with(args: &[String], stdout: Stdio) -> std::io::Result<Child> {
    let exe = bin().ok_or(std::io::ErrorKind::NotFound)?;
    let mut c = Command::new("setsid");
    if let Some(scope) = crate::util::asset("shell/assets/scripts/app-scope.sh") {
        c.arg(scope).args(["--id", BIN, "--"]);
    }
    c.arg(exe).args(args).stdin(Stdio::null()).stdout(stdout).stderr(Stdio::null());
    // (every signal unblocked in it: the shell's blocked ones are inherited)
    unsafe {
        c.pre_exec(|| {
            crate::util::child_signals();
            Ok(())
        });
    }
    c.spawn()
}

/// Whether a child that was started ended with an error within `within`
/// (it failed to start); false when it's still running then, or ended well
pub fn failed_early(child: &mut Child, within: Duration) -> bool {
    let until = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return !s.success(),
            Ok(None) if Instant::now() >= until => return false,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return true,
        }
    }
}

/// xdg-open, as before (a thread reaps it)
pub fn xdg_open(target: &str) {
    let t = target.trim().to_string();
    if t.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let mut c = Command::new("xdg-open");
        c.arg(&t).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        unsafe {
            c.pre_exec(|| {
                crate::util::child_signals();
                Ok(())
            });
        }
        if let Ok(mut c) = c.spawn() {
            let _ = c.wait();
        }
    });
}

/// tessera-web with these arguments; `fallback` (on the thread) when it
/// isn't installed or fails to start
pub fn start_or(args: Vec<String>, fallback: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || match spawn(&args) {
        Err(_) => fallback(),
        Ok(mut c) => {
            if failed_early(&mut c, FAIL_WITHIN) {
                eprintln!("tessera-ui: tessera-web didn't start: the default browser instead");
                fallback();
            } else {
                // (reaped when it ends)
                let _ = c.wait();
            }
        }
    });
}

/// Open a link from the shell where Settings says (the settings read here)
pub fn open(url: &str) {
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    // (the settings file and the PATH looked at off the UI thread)
    std::thread::spawn(move || open_with(&crate::cfgfile::config_value(), &url));
}

/// `open` with these settings
pub fn open_with(cfg: &Value, url: &str) {
    let url = url.trim().to_string();
    match route(&url, links_in_tessera(cfg), bin().is_some()) {
        Route::Web => {
            let u = url.clone();
            start_or(vec![url], move || xdg_open(&u));
        }
        Route::Default => xdg_open(&url),
    }
}

/// A Wi-Fi network's sign-in page (tessera-web --portal, which closes by
/// itself once online); the default browser without it
pub fn portal(url: &str) {
    let url = url.trim().to_string();
    if !is_web(&url) {
        return;
    }
    if bin().is_none() {
        return xdg_open(&url);
    }
    let u = url.clone();
    start_or(vec!["--portal".into(), url], move || xdg_open(&u));
}

/// Quick Look for a web link or an HTML file (tessera-web --quicklook); the
/// default browser when it fails to start. False: not one of those, or no
/// tessera-web for an HTML file (Quick Look's own preview shows it)
pub fn quicklook(target: &str) -> bool {
    let t = target.trim().to_string();
    match quicklook_route(&t, bin().is_some()) {
        None => false,
        Some(Route::Default) => {
            xdg_open(&t);
            true
        }
        Some(Route::Web) => {
            let u = t.clone();
            start_or(vec!["--quicklook".into(), t], move || xdg_open(&u));
            true
        }
    }
}

/// A reading-list link in reading mode (tessera-web --reader); where Settings
/// says otherwise (your browser), or without tessera-web, the default browser
pub fn reader(url: &str) {
    let url = url.trim().to_string();
    if !is_web(&url) {
        return;
    }
    std::thread::spawn(move || {
        let cfg = crate::cfgfile::config_value();
        match route(&url, links_in_tessera(&cfg), bin().is_some()) {
            Route::Web => {
                let u = url.clone();
                start_or(vec!["--reader".into(), url], move || xdg_open(&u));
            }
            Route::Default => xdg_open(&url),
        }
    });
}

/// A video meeting's link in an event (its location or notes): Google Meet,
/// Microsoft Teams, Zoom (its web client), Webex, Jitsi, Whereby
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meeting {
    pub url: String,
    /// "Google Meet", "Teams", "Zoom"…
    pub service: &'static str,
}

/// The service a link is a meeting on (None: not one)
pub fn meeting_service(url: &str) -> Option<&'static str> {
    let u = url.trim().to_ascii_lowercase();
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("");
    let on = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    if host == "meet.google.com" && !path.is_empty() {
        Some("Google Meet")
    } else if (on("teams.microsoft.com") || on("teams.live.com")) && (path.contains("meet") || path.starts_with("l/")) {
        Some("Teams")
    } else if on("zoom.us") && (path.starts_with("j/") || path.starts_with("my/") || path.starts_with("w/") || path.starts_with("wc/")) {
        Some("Zoom")
    } else if on("webex.com") && path.contains("meet") {
        Some("Webex")
    } else if host == "meet.jit.si" && !path.is_empty() {
        Some("Jitsi")
    } else if on("whereby.com") && !path.is_empty() {
        Some("Whereby")
    } else {
        None
    }
}

/// A Zoom meeting's link for its web client (zoom.us/j/ID → zoom.us/wc/join/ID),
/// so it opens in the browser rather than asking for the app; others as they are
pub fn meeting_web(url: &str, service: &str) -> String {
    if service != "Zoom" {
        return url.to_string();
    }
    let Some(at) = url.find("/j/") else { return url.to_string() };
    let (head, tail) = url.split_at(at);
    let tail = &tail[3..];
    let (id, query) = tail.split_once('?').map_or((tail, ""), |(i, q)| (i, q));
    // (the passcode rides along: pwd=)
    let pwd = query.split('&').find(|kv| kv.starts_with("pwd="));
    format!("{head}/wc/join/{id}{}", pwd.map(|p| format!("?{p}")).unwrap_or_default())
}

/// The first meeting link in an event's location, then its notes
pub fn meeting_link(location: &str, notes: &str) -> Option<Meeting> {
    for text in [location, notes] {
        for word in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '|')) {
            let w = word.trim_end_matches(['.', ',', ';', ':', '!']);
            if !is_web(w) {
                continue;
            }
            if let Some(service) = meeting_service(w) {
                return Some(Meeting { url: meeting_web(w, service), service });
            }
        }
    }
    None
}

/// Whether a Hyprland event line says a window of `class` opened
/// (`openwindow>>ADDRESS,WORKSPACE,CLASS,TITLE`)
pub fn opened(line: &str, class: &str) -> bool {
    line.trim_end().strip_prefix("openwindow>>").and_then(|r| r.splitn(4, ',').nth(2)).is_some_and(|c| c == class)
}

/// Hyprland's event socket, opened before the window is started so its
/// openwindow can't be missed (None: not on Hyprland)
pub fn hypr_events() -> Option<UnixStream> {
    UnixStream::connect(crate::services::hypr::dir()?.join(".socket2.sock")).ok()
}

/// What became of a window that was waited for
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// its window opened
    Opened,
    /// the program ended first (true: without an error)
    Ended(bool),
    /// neither, in time
    TimedOut,
}

/// Wait until a window of `class` opens (Hyprland's openwindow), `child`
/// ends, or `timeout` passes. Without Hyprland's events: a moment, then
/// taken as opened while the program still runs
pub fn wait_window(events: Option<UnixStream>, child: &mut Child, class: &str, timeout: Duration) -> Shown {
    let until = Instant::now() + timeout;
    let ended = |c: &mut Child| match c.try_wait() {
        Ok(Some(s)) => Some(Shown::Ended(s.success())),
        Err(_) => Some(Shown::Ended(false)),
        Ok(None) => None,
    };
    let Some(events) = events else {
        let wait = Instant::now() + Duration::from_millis(1500).min(timeout);
        while Instant::now() < wait {
            if let Some(e) = ended(child) {
                return e;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        return Shown::Opened;
    };
    let _ = events.set_read_timeout(Some(Duration::from_millis(100)));
    let mut r = BufReader::new(events);
    // (bytes kept across a timeout: a line may come in two reads)
    let mut line: Vec<u8> = Vec::new();
    loop {
        if Instant::now() >= until {
            return Shown::TimedOut;
        }
        if let Some(e) = ended(child) {
            return e;
        }
        match r.read_until(b'\n', &mut line) {
            Ok(0) => {
                // (the socket closed: Hyprland went; the window is taken as there)
                return Shown::Opened;
            }
            Ok(_) => {
                if line.ends_with(b"\n") {
                    if opened(&String::from_utf8_lossy(&line), class) {
                        return Shown::Opened;
                    }
                    line.clear();
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
            Err(_) => return Shown::Opened,
        }
    }
}

/// A rectangle for --from / --at: "X,Y,W,H" in whole logical pixels
pub fn rect_arg(x: f32, y: f32, w: f32, h: f32) -> String {
    format!("{},{},{},{}", x.round() as i64, y.round() as i64, w.round().max(1.0) as i64, h.round().max(1.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_web_links_go_to_tessera_web() {
        assert_eq!(route("https://example.com", true, true), Route::Web);
        assert_eq!(route("  HTTP://example.com/a ", true, true), Route::Web);
        assert_eq!(route("mailto:a@b.c", true, true), Route::Default);
        assert_eq!(route("tel:123", true, true), Route::Default);
        assert_eq!(route("file:///home/a.html", true, true), Route::Default);
        assert_eq!(route("ftp://example.com", true, true), Route::Default);
        // (your browser chosen, or tessera-web not installed)
        assert_eq!(route("https://example.com", false, true), Route::Default);
        assert_eq!(route("https://example.com", true, false), Route::Default);
        assert!(!is_web("http"));
        assert!(!is_web("javascript:alert(1)"));
    }

    #[test]
    fn the_settings_and_their_defaults() {
        assert!(links_in_tessera(&json!({})));
        assert!(links_in_tessera(&json!({"apps": {"webLinks": "tessera"}})));
        assert!(!links_in_tessera(&json!({"apps": {"webLinks": "browser"}})));
        assert!(portal_auto(&json!({})));
        assert!(!portal_auto(&json!({"network": {"portalAuto": false}})));
    }

    #[test]
    fn searches_go_to_the_chosen_engine() {
        assert_eq!(search::engine(&json!({})), "google");
        assert_eq!(search::engine(&json!({"search": {"engine": "bing"}})), "bing");
        assert_eq!(search::engine(&json!({"search": {"engine": "askjeeves"}})), "google");
        assert_eq!(search::url(&json!({}), " water lily "), "https://www.google.com/search?q=water%20lily");
        assert_eq!(search::url(&json!({"search": {"engine": "duckduckgo"}}), "a&b #1+"), "https://duckduckgo.com/?q=a%26b%20%231%2B");
        assert_eq!(search::url_for("brave", "rust"), "https://search.brave.com/search?q=rust");
        assert_eq!(search::url_for("startpage", "x"), "https://www.startpage.com/do/search?q=x");
        assert_eq!(search::url_for("nonsense", "x"), "https://www.google.com/search?q=x");
        assert_eq!(search::url_for("google", "é"), "https://www.google.com/search?q=%C3%A9");
        assert_eq!(search::name("duckduckgo"), "DuckDuckGo");
        assert_eq!(search::name(""), "Google");
        assert!(search::web_results(&json!({})));
        assert!(!search::web_results(&json!({"search": {"webResults": false}})));
        // (every engine's address is https and ends where the words go)
        for (_, _, base) in search::ENGINES {
            assert!(base.starts_with("https://") && base.ends_with("q="), "{base}");
        }
    }

    #[test]
    fn selected_text_as_words() {
        assert_eq!(search::words("  water\n\tlily  ", 100), "water lily");
        assert_eq!(search::words("one two three four", 12), "one two");
        assert_eq!(search::words("abcdefghijkl", 5), "abcde");
        assert_eq!(search::words("   ", 10), "");
        assert_eq!(search::short("a quite long piece of text", 10), "a quite lo…");
        assert_eq!(search::short("short", 10), "short");
        assert!(is_render_class("tessera-web-render") && !is_render_class("tessera-web"));
    }

    #[test]
    fn quick_look_takes_links_and_html_files() {
        assert_eq!(quicklook_route("https://a.b", true), Some(Route::Web));
        assert_eq!(quicklook_route("https://a.b", false), Some(Route::Default));
        assert_eq!(quicklook_route("/home/me/page.HTML", true), Some(Route::Web));
        assert_eq!(quicklook_route("file:///home/me/page.htm", true), Some(Route::Web));
        // (no tessera-web: Quick Look shows an HTML file as text, as before)
        assert_eq!(quicklook_route("/home/me/page.html", false), None);
        assert_eq!(quicklook_route("/home/me/photo.png", true), None);
        assert_eq!(quicklook_route("page.html", true), None);
    }

    #[test]
    fn a_sign_in_page_opens_once_per_connection() {
        let mut p = PortalOnce::default();
        let a = "/org/freedesktop/NetworkManager/ActiveConnection/4";
        // (still checking, then the portal: opened once)
        assert!(!p.step(1, a, true));
        // (limited, 3, is no portal: NetworkManager's numbering)
        assert!(!p.step(3, a, true));
        assert_eq!(PORTAL, 2);
        assert!(p.step(PORTAL, a, true));
        assert!(!p.step(PORTAL, a, true));
        // (signed in: full; the portal again on the same connection: not again)
        assert!(!p.step(4, a, true));
        assert!(!p.step(PORTAL, a, true));
        // (left, joined again: a new connection, opened again)
        assert!(!p.step(0, "", true));
        let b = "/org/freedesktop/NetworkManager/ActiveConnection/5";
        assert!(p.step(PORTAL, b, true));
        // (the switch off: never)
        let mut q = PortalOnce::default();
        assert!(!q.step(PORTAL, a, false));
        assert!(!q.step(PORTAL, "/", true));
        assert!(!q.step(PORTAL, "", true));
    }

    #[test]
    fn the_probe_is_networkmanagers_own() {
        assert_eq!(probe_url("http://ping.archlinux.org/nm-check.txt"), "http://ping.archlinux.org/nm-check.txt");
        assert_eq!(probe_url(""), PROBE_URL);
        assert_eq!(probe_url("not a url"), PROBE_URL);
    }

    #[test]
    fn the_bars_thumbnail() {
        assert_eq!((bar::THUMB_X, bar::THUMB_Y, bar::THUMB, bar::THUMB_RADIUS), (130, 14, 22, 6));
        assert_eq!(bar::H, 50);
    }

    #[test]
    fn meeting_links_in_events() {
        let m = meeting_link("", "Join with Google Meet: https://meet.google.com/abc-defg-hij.\nOr dial in").unwrap();
        assert_eq!((m.url.as_str(), m.service), ("https://meet.google.com/abc-defg-hij", "Google Meet"));
        let t = meeting_link("Microsoft Teams Meeting", "Join: <https://teams.microsoft.com/l/meetup-join/19%3ameeting_x/0?context=y>").unwrap();
        assert_eq!(t.service, "Teams");
        assert!(t.url.starts_with("https://teams.microsoft.com/l/meetup-join/"));
        let z = meeting_link("https://us02web.zoom.us/j/8123456789?pwd=abc123&from=addon", "").unwrap();
        assert_eq!((z.url.as_str(), z.service), ("https://us02web.zoom.us/wc/join/8123456789?pwd=abc123", "Zoom"));
        // (the location first; plain sites and other links aren't meetings)
        assert_eq!(meeting_link("https://meet.jit.si/Standup", "https://meet.google.com/x").unwrap().service, "Jitsi");
        assert_eq!(meeting_link("Main St", "See https://example.com/agenda and https://meet.google.com/"), None);
        assert_eq!(meeting_service("https://evil.com/meet.google.com/x"), None);
        assert_eq!(meeting_service("https://zoom.us/pricing"), None);
        assert_eq!(meeting_service("ftp://meet.google.com/x"), None);
    }

    #[test]
    fn its_window_is_heard_opening() {
        assert!(opened("openwindow>>55aa,1,tessera-web,Search by picture\n", CLASS));
        assert!(!opened("openwindow>>55aa,1,firefox,tessera-web\n", CLASS));
        assert!(!opened("closewindow>>55aa\n", CLASS));
        assert_eq!(rect_arg(10.4, 20.6, 300.0, 0.2), "10,21,300,1");
    }
}
