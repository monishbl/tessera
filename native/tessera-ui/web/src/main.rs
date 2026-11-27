//! tessera-web: Tessera's quick-look browser. One window (a normal Wayland
//! toplevel, app id `tessera-web`), a bar drawn in GTK in the shell's look
//! (back, forward, reload, the address pill, reading mode, "Open in your
//! browser", a menu), and WebKitGTK 6 under it in its own process with
//! WebKit's web-process sandbox on: a page can never take the shell down.
//! Not a main browser: no tabs, extensions, sync or passwords.
//!
//! The shell starts it for Circle to Search's "Search the web" (--lens,
//! --from), links (AI sources, web results, notifications, weather, the bug
//! report page), Wi-Fi sign-in pages (--portal), Quick Look (--quicklook) and
//! account sign-in (--signin/--done); the options are the contract with it
//! (src/args.rs, `tessera-web --help`). When this isn't installed or can't
//! start (exit 1), the shell opens the default browser instead.
//!
//! --render draws a page out of sight for a picture of it (src/render.rs):
//! Search's live web results, desktop web clips.
//!
//! Its data: $XDG_DATA_HOME/tessera/web (site data: cookies, storage; and
//! history.json, the address field's local history: never sent anywhere),
//! $XDG_CACHE_HOME/tessera/web (the web cache, the compiled ad blocker).
//! Settings → Default Apps and the bar's menu clear them.

mod args;
mod history;
mod lens;
mod nav;
mod perms;
mod place;
mod render;
mod theme;

use args::{Args, Mode, Parsed, Place, Rect};
use gtk4 as gtk;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use webkit6::prelude::*;
use webkit6::{NavigationPolicyDecision, NetworkSession, PolicyDecision, PolicyDecisionType, ResponsePolicyDecision, UserContentManager, WebView};

/// The ad and tracker blocker's list (WebKit content-blocker rules: well
/// known ad and tracking hosts when a page loads them from elsewhere, and
/// the usual ad slots hidden). Compiled by WebKit at first start (and again
/// when it changes) into the cache folder.
const BLOCKLIST: &str = include_str!("blocklist.json");
const READER_JS: &str = include_str!("reader.js");

/// The page's selected text, told to this program as it changes (in a
/// script world of its own: the page can't see or fake it), for the
/// menu's "Search Google for “…”"
const SELECTION_WORLD: &str = "tessera-selection";
const SELECTION_HANDLER: &str = "tesseraSelection";
const SELECTION_JS: &str = r#"(function () {
  var t = null;
  document.addEventListener('selectionchange', function () {
    clearTimeout(t);
    t = setTimeout(function () {
      var s = String(window.getSelection() || '').slice(0, 2000);
      try { window.webkit.messageHandlers.tesseraSelection.postMessage(s); } catch (e) {}
    }, 120);
  });
})();"#;

fn data_dir() -> PathBuf {
    tessera_core::util::xdg("XDG_DATA_HOME", ".local/share").join("tessera/web")
}

fn cache_dir() -> PathBuf {
    tessera_core::util::xdg("XDG_CACHE_HOME", ".cache").join("tessera/web")
}

/// A name for the blocker's list that changes when the list does (FNV-1a)
fn filter_id() -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in BLOCKLIST.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("tessera-blocklist-{h:016x}")
}

fn version() -> String {
    let channel = option_env!("TESSERA_BUILD_CHANNEL").filter(|s| !s.is_empty()).unwrap_or("local build");
    let commit = option_env!("TESSERA_BUILD_COMMIT").map(|c| &c[..c.len().min(7)]).filter(|s| !s.is_empty());
    let variant = option_env!("TESSERA_BUILD_VARIANT").filter(|s| !s.is_empty());
    let mut parts = vec![channel.to_string()];
    parts.extend(commit.map(str::to_string));
    parts.extend(variant.map(str::to_string));
    format!("tessera-web {} ({})", env!("CARGO_PKG_VERSION"), parts.join(", "))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// One JSON line on stdout (the answering modes: --signin, --portal)
fn emit(v: serde_json::Value) {
    use std::io::Write;
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

/// Run a command and forget it (waited for on a thread: no zombies)
fn detach(cmd: &str, args: &[&str]) {
    let mut c = std::process::Command::new(cmd);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Ok(mut child) = c.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// "Open in your browser": Settings → Default Apps' browser when one is
/// chosen (a command line), else the system's default (xdg-open)
fn open_in_browser(uri: &str) {
    let cfg = tessera_core::theme::read_json();
    let chosen = cfg["defaultApps"]["browser"].as_str().unwrap_or("").trim().to_string();
    if chosen.is_empty() {
        detach("xdg-open", &[uri]);
    } else {
        detach("sh", &["-c", &format!("{chosen} \"$1\""), "sh", uri]);
    }
}

/// Where a download goes: the Downloads folder, a name not taken yet
fn download_path(name: &str) -> PathBuf {
    let dir = tessera_core::util::user_dir("DOWNLOAD", "Downloads");
    let clean: String = name.chars().map(|c| if c == '/' || c == '\0' { '_' } else { c }).collect();
    let clean = clean.trim_start_matches('.').trim();
    let clean = if clean.is_empty() { "download" } else { clean };
    let first = dir.join(clean);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match clean.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (clean.to_string(), String::new()),
    };
    (2..1000).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// The Lens picture as the pill's thumbnail: its middle square (it's
/// covered, as Circle to Search's picture lands), `px` across
fn thumb_texture(p: &std::path::Path, px: i32) -> Option<gdk::Texture> {
    use gtk::gdk_pixbuf::Pixbuf;
    let (_, w, h) = Pixbuf::file_info(p)?;
    if w <= 0 || h <= 0 || px <= 0 {
        return None;
    }
    let k = px as f64 / w.min(h) as f64;
    let (sw, sh) = (((w as f64 * k).round() as i32).max(px), ((h as f64 * k).round() as i32).max(px));
    let full = Pixbuf::from_file_at_scale(p, sw, sh, false).ok()?;
    let square = full.new_subpixbuf((sw - px) / 2, (sh - px) / 2, px, px);
    Some(gdk::Texture::for_pixbuf(&square))
}

fn icon(names: &[&str]) -> gtk::Image {
    gtk::Image::from_gicon(&gio::ThemedIcon::from_names(names))
}

fn icon_button(names: &[&str], tip: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&icon(names)));
    b.set_tooltip_text(Some(tip));
    b.set_focus_on_click(false);
    b.set_valign(gtk::Align::Center);
    b
}

const BACK: &[&str] = &["go-previous-symbolic", "pan-start-symbolic"];
const FORWARD: &[&str] = &["go-next-symbolic", "pan-end-symbolic"];
const RELOAD: &[&str] = &["view-refresh-symbolic"];
const STOP: &[&str] = &["process-stop-symbolic", "window-close-symbolic"];
const SEARCH: &[&str] = &["system-search-symbolic", "edit-find-symbolic"];
const SECURE: &[&str] = &["channel-secure-symbolic", "security-high-symbolic", "system-lock-screen-symbolic"];
const READER: &[&str] = &["view-reader-symbolic", "format-justify-left-symbolic", "text-x-generic-symbolic"];
const BROWSER: &[&str] = &["web-browser-symbolic", "applications-internet-symbolic", "emblem-shared-symbolic"];
const MORE: &[&str] = &["view-more-symbolic", "open-menu-symbolic"];

struct App {
    args: Args,
    palette: theme::Palette,
    window: gtk::Window,
    root: gtk::Box,
    view: WebView,
    session: NetworkSession,
    content: UserContentManager,
    back: gtk::Button,
    forward: gtk::Button,
    reload: gtk::Button,
    glyph: gtk::Image,
    thumb: Option<gtk::Image>,
    stack: gtk::Stack,
    label: gtk::Label,
    entry: gtk::Entry,
    reader: gtk::ToggleButton,
    progress: gtk::ProgressBar,
    sheet: gtk::Box,
    list: gtk::ListBox,
    shown: RefCell<Vec<history::Visit>>,
    history: RefCell<history::History>,
    history_path: PathBuf,
    /// a word in the pill for a moment ("Downloaded …"), over the title
    note: RefCell<Option<String>>,
    /// the reader button changed by this program, not by a press
    quiet: Cell<bool>,
    /// the page is full screen (Esc leaves that first)
    fullscreen: Cell<bool>,
    /// the answer is out (--signin, --portal): nothing more on stdout
    answered: Cell<bool>,
    /// the first page is loading (once the blocker is ready)
    begun: Cell<bool>,
    /// the start was a file (only then may file:// pages load)
    from_file: Cell<bool>,
    /// the --from/--at rule is on in Hyprland until the window is placed
    placing: Cell<bool>,
    /// the page's selected text now (SELECTION_JS)
    selection: RefCell<String>,
    main_loop: glib::MainLoop,
}

impl App {
    fn answers(&self) -> bool {
        matches!(self.args.mode, Mode::SignIn { .. } | Mode::Portal)
    }

    /// The address field takes typing (not in sign-in, Quick Look or a
    /// Wi-Fi sign-in page)
    fn editable(&self) -> bool {
        matches!(self.args.mode, Mode::Browse | Mode::Lens(_))
    }

    fn lens(&self) -> bool {
        matches!(self.args.mode, Mode::Lens(_))
    }

    fn uri(&self) -> String {
        self.view.uri().map(|s| s.to_string()).unwrap_or_default()
    }

    fn keeps_history(&self, uri: &str) -> bool {
        match self.args.mode {
            Mode::Browse => true,
            Mode::Lens(_) => !nav::is_google(uri),
            _ => false,
        }
    }

    fn editing(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("edit")
    }

    /// The bar as the page is now: the pill's words, its glyph and picture,
    /// the buttons that can do something
    fn refresh(&self) {
        let uri = self.uri();
        let title = self.view.title().map(|s| s.to_string()).unwrap_or_default();
        let text = match self.note.borrow().as_ref() {
            Some(n) => n.clone(),
            None => nav::bar_text(self.lens(), self.args.title.as_deref(), &title, &uri),
        };
        self.label.set_text(&text);
        self.label.set_tooltip_text((!uri.is_empty() && uri != "about:blank").then_some(nav::display(&uri)).as_deref());
        let picture = self.lens() && text == "Search by picture";
        if let Some(t) = &self.thumb {
            t.set_visible(picture);
        }
        self.glyph.set_visible(!picture);
        self.glyph.set_from_gicon(&gio::ThemedIcon::from_names(if nav::scheme_of(&uri) == "https" { SECURE } else { SEARCH }));
        self.back.set_sensitive(self.view.can_go_back());
        self.forward.set_sensitive(self.view.can_go_forward());
        let loading = self.view.is_loading();
        self.reload.set_child(Some(&icon(if loading { STOP } else { RELOAD })));
        self.reload.set_tooltip_text(Some(if loading { "Stop" } else { "Reload" }));
        let page = matches!(nav::scheme_of(&uri).as_str(), "http" | "https" | "file");
        self.reader.set_sensitive(page && !loading || self.reader.is_active());
        let t = if title.trim().is_empty() { "Tessera Web".to_string() } else { title.trim().to_string() };
        self.window.set_title(Some(&t));
    }

    /// A word in the pill for a few seconds
    fn say(self: &Rc<Self>, text: &str) {
        *self.note.borrow_mut() = Some(text.to_string());
        self.refresh();
        let app = self.clone();
        let mine = text.to_string();
        glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if app.note.borrow().as_deref() == Some(mine.as_str()) {
                *app.note.borrow_mut() = None;
                app.refresh();
            }
        });
    }

    fn go(&self, uri: &str) {
        self.end_edit(true);
        self.view.load_uri(uri);
    }

    /// The pill becomes the address field, its address selected
    fn edit(&self) {
        if !self.editable() || self.editing() {
            return;
        }
        let uri = self.uri();
        self.entry.set_text(if uri == "about:blank" { "" } else { &uri });
        self.stack.set_visible_child_name("edit");
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        self.sheet.set_visible(false);
    }

    /// Back to the pill's words; `to_page`: the page takes the keys again
    fn end_edit(&self, to_page: bool) {
        self.sheet.set_visible(false);
        self.shown.borrow_mut().clear();
        if self.editing() {
            self.stack.set_visible_child_name("show");
            if to_page {
                self.view.grab_focus();
            }
        }
        self.refresh();
    }

    /// The history's pages for what's typed, under the bar
    fn suggest(&self) {
        let q = self.entry.text().to_string();
        let found = if q.trim().is_empty() || q == self.uri() { Vec::new() } else { self.history.borrow().suggest(&q, 6, now()) };
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for v in &found {
            let b = gtk::Box::new(gtk::Orientation::Vertical, 1);
            let t = gtk::Label::new(Some(if v.title.is_empty() { v.url.as_str() } else { v.title.as_str() }));
            t.set_xalign(0.0);
            t.set_ellipsize(pango::EllipsizeMode::End);
            let u = gtk::Label::new(Some(nav::display(&v.url).as_str()));
            u.set_xalign(0.0);
            u.set_ellipsize(pango::EllipsizeMode::Middle);
            u.add_css_class("url");
            b.append(&t);
            b.append(&u);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            row.set_focusable(false);
            self.list.append(&row);
        }
        self.sheet.set_visible(!found.is_empty() && self.editing());
        *self.shown.borrow_mut() = found;
    }

    /// Up and down through the suggestions (the field keeps the keys)
    fn step(&self, by: i32) {
        let n = self.shown.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let cur = self.list.selected_row().map_or(-1, |r| r.index());
        let next = if cur < 0 { if by > 0 { 0 } else { n - 1 } } else { (cur + by).rem_euclid(n) };
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
        }
    }

    /// Enter in the address field: the suggestion picked, else what's typed
    fn enter(&self) {
        let picked = self.list.selected_row().and_then(|r| self.shown.borrow().get(r.index() as usize).map(|v| v.url.clone()));
        let target = picked.or_else(|| nav::typed(&self.entry.text()));
        match target {
            Some(t) => self.go(&t),
            None => self.end_edit(true),
        }
    }

    /// The answer, once (--signin: where it ended; --portal: online)
    fn finish(&self, uri: &str) {
        if self.answers() && !self.answered.get() {
            self.answered.set(true);
            emit(serde_json::json!({"event": "done", "url": uri}));
        }
        self.close();
    }

    fn close(&self) {
        if self.answers() && !self.answered.get() {
            self.answered.set(true);
            emit(serde_json::json!({"event": "closed"}));
        }
        if self.placing.replace(false) {
            place::eval(place::rule_off_lua());
        }
        self.window.set_visible(false);
        self.main_loop.quit();
    }

    fn save_history(&self) {
        if let Err(e) = self.history.borrow().save(&self.history_path) {
            eprintln!("tessera-web: the history wasn't saved: {e}");
        }
    }

    /// Settings' and the menu's "Clear history and website data": cookies,
    /// storage, the cache and the history
    fn clear_data(self: &Rc<Self>) {
        if let Some(m) = self.session.website_data_manager() {
            m.clear(webkit6::WebsiteDataTypes::all(), glib::TimeSpan(0), None::<&gio::Cancellable>, |r| {
                if let Err(e) = r {
                    eprintln!("tessera-web: website data wasn't cleared: {e}");
                }
            });
        }
        self.history.borrow_mut().clear();
        self.save_history();
        self.say("History and website data cleared");
    }

    /// Reading mode on: the article alone (src/reader.js), else a word why not
    fn reader_on(self: &Rc<Self>) {
        let cfg = serde_json::json!({ "css": theme::reader_css(&self.palette) });
        let script = format!("var TESSERA_READER = {cfg};\n{READER_JS}");
        let app = self.clone();
        self.view.evaluate_javascript(&script, Some("tessera-reader"), None, None::<&gio::Cancellable>, move |r| {
            let ok = matches!(&r, Ok(v) if v.to_str().as_str() == "ok");
            if !ok {
                app.quiet.set(true);
                app.reader.set_active(false);
                app.quiet.set(false);
                app.say("There's no article to read on this page");
            }
        });
    }

    /// A navigation or a response: --done, the schemes a page may open,
    /// Quick Look's links, new windows (this one), downloads
    fn decide(self: &Rc<Self>, decision: &PolicyDecision, kind: PolicyDecisionType) -> bool {
        match kind {
            PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                let Some(nav_decision) = decision.downcast_ref::<NavigationPolicyDecision>() else { return false };
                let Some(action) = nav_decision.navigation_action() else { return false };
                let uri = action.request().and_then(|r| r.uri()).map(|s| s.to_string()).unwrap_or_default();
                if let Mode::SignIn { done, .. } = &self.args.mode {
                    if nav::done_matches(&uri, done) {
                        decision.ignore();
                        self.finish(&uri);
                        return true;
                    }
                }
                let scheme = nav::scheme_of(&uri);
                if !matches!(scheme.as_str(), "http" | "https" | "about" | "data" | "blob" | "file" | "") {
                    // (mailto:, tel:, an app's own scheme: the system's handler)
                    decision.ignore();
                    if action.is_user_gesture() {
                        detach("xdg-open", &[&uri]);
                    }
                    return true;
                }
                if scheme == "file" && !self.from_file.get() {
                    // (a web page never opens the computer's files)
                    decision.ignore();
                    return true;
                }
                if self.args.mode == Mode::QuickLook && action.navigation_type() == webkit6::NavigationType::LinkClicked && !same_page(&uri, &self.uri()) {
                    // (a preview stays one page: a link opens in your browser)
                    decision.ignore();
                    open_in_browser(&uri);
                    return true;
                }
                if kind == PolicyDecisionType::NewWindowAction {
                    // (one window: a link meant for a new one opens here)
                    decision.ignore();
                    if !uri.is_empty() {
                        self.view.load_uri(&uri);
                    }
                    return true;
                }
                false
            }
            PolicyDecisionType::Response => {
                let Some(r) = decision.downcast_ref::<ResponsePolicyDecision>() else { return false };
                if r.is_main_frame_main_resource() && !r.is_mime_type_supported() {
                    decision.download();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// The first page, once the blocker is ready (or not wanted)
    fn begin(self: &Rc<Self>) {
        if self.begun.replace(true) {
            return;
        }
        match &self.args.mode {
            Mode::Lens(path) => match lens::read(path) {
                Ok(data) => {
                    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                    let html = lens::page(&data, lens::mime(&data, path), ms, self.palette.dark);
                    self.view.load_html(&html, None);
                }
                Err(why) => self.view.load_html(&message_page(why, &self.palette), None),
            },
            Mode::SignIn { url, .. } => self.view.load_uri(url),
            Mode::Portal => {
                let start = self.args.target.as_deref().and_then(nav::typed).unwrap_or_else(portal_check_uri);
                self.view.load_uri(&start);
                watch_online(self.clone());
            }
            Mode::Browse | Mode::QuickLook => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
                match self.args.target.as_deref().and_then(|t| args::start_uri(t, &cwd, |p| p.is_file())) {
                    Some(uri) => {
                        self.from_file.set(uri.starts_with("file://"));
                        if self.from_file.get() && self.args.mode == Mode::QuickLook {
                            // (a downloaded HTML file previewed: no scripts)
                            if let Some(s) = WebViewExt::settings(&self.view) {
                                s.set_enable_javascript(false);
                            }
                        }
                        self.view.load_uri(&uri);
                    }
                    None => {
                        self.view.load_uri("about:blank");
                        self.edit();
                    }
                }
            }
        }
    }
}

/// The page menu's search item: "Search Google for “water lily”" (the
/// selection cut short)
fn search_label(engine_name: &str, selection: &str) -> String {
    format!("Search {engine_name} for “{}”", tessera_core::weblink::search::short(selection, 28))
}

/// Two addresses that differ only after # are one page
fn same_page(a: &str, b: &str) -> bool {
    let strip = |s: &str| s.split('#').next().unwrap_or("").to_string();
    !a.is_empty() && strip(a) == strip(b)
}

/// A few words on a page of their own, in the look's colours
fn message_page(text: &str, p: &theme::Palette) -> String {
    let k = |c: theme::Rgba| theme::css_color([c[0], c[1], c[2], 255]);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>body {{ margin: 0; height: 100vh; display: grid; place-items: center; font: 15px/1.4 system-ui, sans-serif; background: {}; color: {}; }}</style></head><body><p>{}</p></body></html>",
        k(p.page),
        k(p.text),
        lens::escape(text)
    )
}

/// NetworkManager's connectivity check address (a plain http page a
/// captive portal answers with its sign-in page), from its config
fn portal_check_uri() -> String {
    let mut files = Vec::new();
    for dir in ["/usr/lib/NetworkManager/conf.d", "/run/NetworkManager/conf.d", "/etc/NetworkManager/conf.d"] {
        let mut names: Vec<PathBuf> = std::fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "conf")).collect()).unwrap_or_default();
        names.sort();
        if dir == "/etc/NetworkManager/conf.d" {
            if let Ok(t) = std::fs::read_to_string("/etc/NetworkManager/NetworkManager.conf") {
                files.push(t);
            }
        }
        files.extend(names.iter().filter_map(|p| std::fs::read_to_string(p).ok()));
    }
    nav::connectivity_uri(&files).unwrap_or_else(|| "http://ping.archlinux.org/nm-check.txt".into())
}

/// --portal: NetworkManager asked again every few seconds; once it says
/// the computer is fully online, the answer goes out and the window closes
fn watch_online(app: Rc<App>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let online = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (online, stop) = (online.clone(), stop.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            while !stop.load(Ordering::Relaxed) {
                let out = std::process::Command::new("nmcli").args(["-t", "networking", "connectivity", "check"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output();
                match out {
                    Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "full" => {
                        online.store(true, Ordering::Relaxed);
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => return, // (no nmcli: the window stays until closed)
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        });
    }
    glib::timeout_add_local(Duration::from_millis(500), move || {
        if online.load(Ordering::Relaxed) {
            let uri = app.uri();
            app.finish(&uri);
            return glib::ControlFlow::Break;
        }
        if app.answered.get() {
            stop.store(true, Ordering::Relaxed);
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

/// The blocker's compiled list (compiled now when it isn't yet), then `then`
fn with_blocker(content: &UserContentManager, then: impl Fn() + Clone + 'static) {
    let dir = cache_dir().join("filters");
    if std::fs::create_dir_all(&dir).is_err() {
        then();
        return;
    }
    let store = webkit6::UserContentFilterStore::new(&dir.to_string_lossy());
    let id = filter_id();
    let (content, store2, id2, then2) = (content.clone(), store.clone(), id.clone(), then.clone());
    store.load(&id, None::<&gio::Cancellable>, move |r| match r {
        Ok(filter) => {
            content.add_filter(&filter);
            then();
        }
        Err(_) => {
            let content = content.clone();
            store2.save(&id2, &glib::Bytes::from_static(BLOCKLIST.as_bytes()), None::<&gio::Cancellable>, move |r| {
                match r {
                    Ok(filter) => content.add_filter(&filter),
                    Err(e) => eprintln!("tessera-web: the ad blocker's list didn't compile: {e}"),
                }
                then2();
            });
        }
    });
}

fn build(args: Args, main_loop: glib::MainLoop) -> Rc<App> {
    let palette = theme::current();
    if let Some(display) = gdk::Display::default() {
        let css = gtk::CssProvider::new();
        css.load_from_string(&theme::css(&palette));
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    if let Some(s) = gtk::Settings::default() {
        // (GTK's own parts: the menu, the field's selection, in the look's light or dark)
        s.set_property("gtk-application-prefer-dark-theme", palette.dark);
    }

    // The session: site data in the data folder (the portal's never kept)
    let data = data_dir();
    let cache = cache_dir();
    for d in [&data, &cache] {
        let _ = std::fs::create_dir_all(d);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    let session = if args.mode == Mode::Portal {
        NetworkSession::new_ephemeral()
    } else {
        let (site, site_cache) = (data.join("site").to_string_lossy().into_owned(), cache.join("site").to_string_lossy().into_owned());
        NetworkSession::new(Some(site.as_str()), Some(site_cache.as_str()))
    };
    session.set_itp_enabled(true);
    let content = UserContentManager::new();
    if matches!(args.mode, Mode::Browse | Mode::Lens(_)) {
        // (the selected text, for the menu's search: in a world of its own)
        content.register_script_message_handler(SELECTION_HANDLER, Some(SELECTION_WORLD));
        content.add_script(&webkit6::UserScript::for_world(SELECTION_JS, webkit6::UserContentInjectedFrames::AllFrames, webkit6::UserScriptInjectionTime::End, SELECTION_WORLD, &[], &[]));
    }
    let settings = webkit6::Settings::new();
    settings.set_enable_developer_extras(false);
    settings.set_enable_back_forward_navigation_gestures(true);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_media_playback_requires_user_gesture(true);
    settings.set_allow_file_access_from_file_urls(false);
    settings.set_allow_universal_access_from_file_urls(false);
    settings.set_enable_smooth_scrolling(true);
    let view = WebView::builder().network_session(&session).user_content_manager(&content).settings(&settings).build();
    view.set_vexpand(true);
    view.set_hexpand(true);
    view.add_css_class("page");
    let pg = palette.page;
    // (pages are never see-through: only the bar is glass)
    view.set_background_color(&gdk::RGBA::new(pg[0] as f32 / 255.0, pg[1] as f32 / 255.0, pg[2] as f32 / 255.0, 1.0));

    // The bar: back, forward, reload, the pill, reading mode, your browser, the menu
    // (laid out with tessera_core::weblink::bar: Circle to Search lands its
    // picture on the thumbnail with the same numbers)
    use tessera_core::weblink::bar as dims;
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, dims::SPACING);
    bar.add_css_class("bar");
    let back = icon_button(BACK, "Back");
    let forward = icon_button(FORWARD, "Forward");
    let reload = icon_button(RELOAD, "Reload");
    let pill = gtk::Box::new(gtk::Orientation::Horizontal, dims::PILL_GAP);
    pill.add_css_class("pill");
    pill.set_hexpand(true);
    pill.set_valign(gtk::Align::Center);
    pill.set_margin_start(dims::PILL_MARGIN);
    pill.set_margin_end(dims::PILL_MARGIN);
    let glyph = icon(SEARCH);
    glyph.add_css_class("glyph");
    let thumb = match &args.mode {
        Mode::Lens(p) => {
            // (a gtk::Image at a fixed pixel size, not a gtk::Picture: a
            // Picture asks for the picture's own width and would be as wide)
            let t = gtk::Image::new();
            if let Some(tex) = thumb_texture(p, dims::THUMB * 3) {
                t.set_paintable(Some(&tex));
            }
            t.set_pixel_size(dims::THUMB);
            t.set_size_request(dims::THUMB, dims::THUMB);
            t.set_valign(gtk::Align::Center);
            t.set_overflow(gtk::Overflow::Hidden);
            t.add_css_class("thumb");
            Some(t)
        }
        _ => None,
    };
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label.add_css_class("where");
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Search or enter address"));
    entry.set_input_purpose(gtk::InputPurpose::Url);
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.add_named(&label, Some("show"));
    stack.add_named(&entry, Some("edit"));
    stack.set_visible_child_name("show");
    pill.append(&glyph);
    if let Some(t) = &thumb {
        pill.append(t);
    }
    pill.append(&stack);
    let reader = gtk::ToggleButton::new();
    reader.set_child(Some(&icon(READER)));
    reader.set_tooltip_text(Some("Reading mode"));
    reader.set_focus_on_click(false);
    reader.set_valign(gtk::Align::Center);
    let browser = icon_button(BROWSER, "Open in your browser");
    let menu = gio::Menu::new();
    menu.append(Some("Copy link"), Some("web.copy"));
    menu.append(Some("Open in your browser"), Some("web.browser"));
    menu.append(Some("Clear history and website data"), Some("web.clear"));
    let more = gtk::MenuButton::new();
    more.set_child(Some(&icon(MORE)));
    more.set_tooltip_text(Some("More"));
    more.set_menu_model(Some(&menu));
    more.set_valign(gtk::Align::Center);
    more.set_focus_on_click(false);
    for w in [back.upcast_ref::<gtk::Widget>(), forward.upcast_ref(), reload.upcast_ref(), pill.upcast_ref(), reader.upcast_ref(), browser.upcast_ref(), more.upcast_ref()] {
        bar.append(w);
    }
    let progress = gtk::ProgressBar::new();
    progress.add_css_class("progress");
    progress.set_opacity(0.0);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(&bar);
    column.append(&progress);
    column.append(&view);
    // The suggestions, a sheet over the page under the pill
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(true);
    list.set_focusable(false);
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sheet.add_css_class("suggest");
    sheet.append(&list);
    sheet.set_valign(gtk::Align::Start);
    sheet.set_halign(gtk::Align::Fill);
    sheet.set_margin_top(48);
    sheet.set_margin_start(104);
    sheet.set_margin_end(118);
    sheet.set_visible(false);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&column));
    overlay.add_overlay(&sheet);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("root");
    root.append(&overlay);
    if matches!(args.place, Place::From(_)) {
        root.add_css_class("entering");
    }

    let (w, h) = match args.place {
        Place::From(r) | Place::At(r) => (r.w, r.h),
        Place::Rules => (1180, 780),
    };
    let window = gtk::Window::builder().title(args.title.clone().unwrap_or_else(|| "Tessera Web".into())).default_width(w).default_height(h).decorated(false).build();
    window.add_css_class("tessera-web");
    window.set_child(Some(&root));
    // (site permissions: the camera, the microphone, the screen, notifications)
    perms::attach(&view, &overlay, &window, &menu, &palette, &data);

    let history_path = data.join("history.json");
    let history = if matches!(args.mode, Mode::Browse | Mode::Lens(_)) { history::History::load(&history_path) } else { history::History::default() };
    let app = Rc::new(App {
        args,
        palette,
        window,
        root,
        view,
        session,
        content,
        back,
        forward,
        reload,
        glyph,
        thumb,
        stack,
        label,
        entry,
        reader,
        progress,
        sheet,
        list,
        shown: RefCell::new(Vec::new()),
        history: RefCell::new(history),
        history_path,
        note: RefCell::new(None),
        quiet: Cell::new(false),
        fullscreen: Cell::new(false),
        answered: Cell::new(false),
        begun: Cell::new(false),
        from_file: Cell::new(false),
        placing: Cell::new(false),
        selection: RefCell::new(String::new()),
        main_loop,
    });
    wire(&app, &pill, &browser);
    reader_at_start(&app);
    app.refresh();
    app
}

/// --reader (a reading-list link): reading mode once the first page is in
fn reader_at_start(app: &Rc<App>) {
    if !app.args.reader {
        return;
    }
    let once = Rc::new(Cell::new(false));
    let a = app.clone();
    app.view.connect_load_changed(move |_, ev| {
        if ev == webkit6::LoadEvent::Finished && !once.replace(true) && !a.reader.is_active() {
            a.reader.set_active(true);
        }
    });
}

/// Every signal: the page's, the bar's, the keys
fn wire(app: &Rc<App>, pill: &gtk::Box, browser: &gtk::Button) {
    let v = &app.view;
    {
        let a = app.clone();
        v.connect_uri_notify(move |view| {
            let uri = view.uri().map(|s| s.to_string()).unwrap_or_default();
            if let Mode::SignIn { done, .. } = &a.args.mode {
                if nav::done_matches(&uri, done) {
                    view.stop_loading();
                    a.finish(&uri);
                    return;
                }
            }
            a.refresh();
        });
    }
    {
        let a = app.clone();
        v.connect_title_notify(move |view| {
            let (uri, title) = (a.uri(), view.title().map(|s| s.to_string()).unwrap_or_default());
            if a.keeps_history(&uri) && a.history.borrow_mut().titled(&uri, &title) {
                a.save_history();
            }
            a.refresh();
        });
    }
    {
        let a = app.clone();
        v.connect_load_changed(move |view, ev| {
            match ev {
                webkit6::LoadEvent::Started => {
                    if a.reader.is_active() {
                        a.quiet.set(true);
                        a.reader.set_active(false);
                        a.quiet.set(false);
                    }
                    a.progress.set_opacity(1.0);
                }
                webkit6::LoadEvent::Committed => {
                    let uri = a.uri();
                    if a.keeps_history(&uri) {
                        let title = view.title().map(|s| s.to_string()).unwrap_or_default();
                        a.history.borrow_mut().record(&uri, &title, now());
                        a.save_history();
                    }
                }
                webkit6::LoadEvent::Finished => {
                    a.progress.set_opacity(0.0);
                    a.progress.set_fraction(0.0);
                }
                _ => {}
            }
            a.refresh();
        });
    }
    {
        let a = app.clone();
        v.connect_estimated_load_progress_notify(move |view| {
            a.progress.set_fraction(view.estimated_load_progress());
        });
    }
    {
        let a = app.clone();
        v.connect_decide_policy(move |_, decision, kind| a.decide(decision, kind));
    }
    {
        // (window.open(): opens here, as one window)
        v.connect_create(move |view, action| {
            let uri = action.request().and_then(|r| r.uri()).map(|s| s.to_string()).unwrap_or_default();
            if !uri.is_empty() && !uri.starts_with("javascript:") {
                view.load_uri(&uri);
            }
            None
        });
    }
    {
        // The selected text, and the page's menu: "Search Google for “…”"
        // (the engine Settings chose) for a selection, here
        let a = app.clone();
        app.content.connect_script_message_received(Some(SELECTION_HANDLER), move |_, value| {
            *a.selection.borrow_mut() = value.to_str().to_string();
        });
        let a = app.clone();
        v.connect_context_menu(move |_, menu, hit| {
            let words = tessera_core::weblink::search::words(&a.selection.borrow(), 400);
            if !a.editable() || !hit.context_is_selection() || words.is_empty() {
                return false;
            }
            let engine = nav::engine();
            let label = search_label(tessera_core::weblink::search::name(engine), &a.selection.borrow());
            let action = gio::SimpleAction::new("search-selection", None);
            let a2 = a.clone();
            let url = tessera_core::weblink::search::url_for(engine, &words);
            action.connect_activate(move |_, _| a2.go(&url));
            menu.append(&webkit6::ContextMenuItem::new_separator());
            menu.append(&webkit6::ContextMenuItem::from_gaction(&action, &label, None));
            false
        });
    }
    {
        let a = app.clone();
        v.connect_enter_fullscreen(move |_| {
            a.fullscreen.set(true);
            false
        });
        let a = app.clone();
        v.connect_leave_fullscreen(move |_| {
            a.fullscreen.set(false);
            false
        });
    }
    {
        let a = app.clone();
        v.connect_web_process_terminated(move |view, _| {
            let uri = a.uri();
            view.load_alternate_html(&message_page("This page stopped working. Reload to try again.", &a.palette), &uri, None);
        });
    }
    {
        let a = app.clone();
        app.session.connect_download_started(move |_, download| {
            let a2 = a.clone();
            download.connect_decide_destination(move |d, name| {
                let path = download_path(name);
                d.set_destination(&path.to_string_lossy());
                let shown = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let a3 = a2.clone();
                d.connect_finished(move |_| a3.say(&format!("Downloaded {shown}")));
                true
            });
            let a2 = a.clone();
            download.connect_failed(move |_, e| {
                eprintln!("tessera-web: a download failed: {e}");
                a2.say("The download didn't finish");
            });
        });
    }

    // The bar
    {
        let a = app.clone();
        app.back.connect_clicked(move |_| a.view.go_back());
        let a = app.clone();
        app.forward.connect_clicked(move |_| a.view.go_forward());
        let a = app.clone();
        app.reload.connect_clicked(move |_| {
            if a.view.is_loading() {
                a.view.stop_loading();
            } else {
                a.view.reload();
            }
        });
        let a = app.clone();
        browser.connect_clicked(move |_| {
            let uri = a.uri();
            if !uri.is_empty() && uri != "about:blank" {
                open_in_browser(&uri);
                a.close();
            }
        });
        let a = app.clone();
        app.reader.connect_toggled(move |b| {
            if a.quiet.get() {
                return;
            }
            if b.is_active() {
                a.reader_on();
            } else {
                a.view.reload();
            }
        });
        let click = gtk::GestureClick::new();
        let a = app.clone();
        click.connect_released(move |_, _, _, _| a.edit());
        pill.add_controller(click);
        let a = app.clone();
        app.entry.connect_changed(move |_| {
            if a.editing() {
                a.suggest();
            }
        });
        let a = app.clone();
        app.entry.connect_activate(move |_| a.enter());
        let a = app.clone();
        app.list.connect_row_activated(move |_, row| {
            let url = a.shown.borrow().get(row.index() as usize).map(|v| v.url.clone());
            if let Some(u) = url {
                a.go(&u);
            }
        });
        // (leaving the field ends editing: a moment later, so a click on a
        // suggestion lands first)
        let focus = gtk::EventControllerFocus::new();
        let a = app.clone();
        focus.connect_leave(move |_| {
            let a = a.clone();
            glib::timeout_add_local_once(Duration::from_millis(180), move || {
                let inside = gtk::prelude::GtkWindowExt::focus(&a.window).is_some_and(|w| w == *a.entry.upcast_ref::<gtk::Widget>() || w.is_ancestor(&a.entry));
                if a.editing() && !inside {
                    a.end_edit(false);
                }
            });
        });
        app.entry.add_controller(focus);
    }

    // The menu's actions
    {
        let group = gio::SimpleActionGroup::new();
        let copy = gio::SimpleAction::new("copy", None);
        let a = app.clone();
        copy.connect_activate(move |_, _| {
            let uri = a.uri();
            if let Some(d) = gdk::Display::default() {
                d.clipboard().set_text(&uri);
                a.say("Link copied");
            }
        });
        let open = gio::SimpleAction::new("browser", None);
        let a = app.clone();
        open.connect_activate(move |_, _| {
            let uri = a.uri();
            if !uri.is_empty() && uri != "about:blank" {
                open_in_browser(&uri);
                a.close();
            }
        });
        let clear = gio::SimpleAction::new("clear", None);
        let a = app.clone();
        clear.connect_activate(move |_, _| a.clear_data());
        group.add_action(&copy);
        group.add_action(&open);
        group.add_action(&clear);
        app.window.insert_action_group("web", Some(&group));
    }

    // Keys, before the page or the field gets them
    {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let a = app.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);
            let stop = glib::Propagation::Stop;
            if a.editing() {
                match key {
                    gdk::Key::Escape => {
                        a.end_edit(true);
                        return stop;
                    }
                    gdk::Key::Down => {
                        a.step(1);
                        return stop;
                    }
                    gdk::Key::Up => {
                        a.step(-1);
                        return stop;
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter if a.list.selected_row().is_some() => {
                        a.enter();
                        return stop;
                    }
                    _ => {}
                }
            }
            match key {
                gdk::Key::Escape if !a.fullscreen.get() => {
                    a.close();
                    stop
                }
                gdk::Key::space if a.args.mode == Mode::QuickLook && !ctrl && !alt => {
                    a.close();
                    stop
                }
                gdk::Key::l | gdk::Key::L if ctrl && a.editable() => {
                    a.edit();
                    stop
                }
                gdk::Key::F6 if a.editable() => {
                    a.edit();
                    stop
                }
                gdk::Key::d | gdk::Key::D if alt && a.editable() => {
                    a.edit();
                    stop
                }
                gdk::Key::w | gdk::Key::W if ctrl => {
                    a.close();
                    stop
                }
                gdk::Key::Left if alt => {
                    a.view.go_back();
                    stop
                }
                gdk::Key::Right if alt => {
                    a.view.go_forward();
                    stop
                }
                gdk::Key::F5 => {
                    a.view.reload();
                    stop
                }
                gdk::Key::r | gdk::Key::R if ctrl && !a.editing() => {
                    a.view.reload();
                    stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        app.window.add_controller(keys);
    }
    {
        let a = app.clone();
        app.window.connect_close_request(move |_| {
            a.close();
            glib::Propagation::Proceed
        });
    }
}

/// --from/--at through Hyprland: the rule before the window opens
/// (true: one is on), the move once it's shown
fn place_before(args: &Args) -> Option<Rect> {
    let r = match args.place {
        Place::Rules => return None,
        Place::From(r) | Place::At(r) => r,
    };
    if !place::in_hyprland() {
        return None;
    }
    let mons = place::monitors();
    let m = place::monitor_for(&r, &mons)?;
    let still = matches!(args.place, Place::At(_));
    if !place::eval(&place::rule_lua(&r, &m, still)) {
        return None;
    }
    Some(if still { r } else { place::usual(&m) })
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&argv) {
        Ok(Parsed::Run(a)) => a,
        Ok(Parsed::Help) => {
            println!("{}", args::USAGE);
            return;
        }
        Ok(Parsed::Version) => {
            println!("{}", version());
            return;
        }
        Ok(Parsed::Render(r)) => {
            // (its own class, should a window of it ever be mapped: the
            // Hyprland rule hides it and the shell's lists leave it out)
            glib::set_prgname(Some(tessera_core::weblink::RENDER_CLASS));
            glib::set_application_name("Tessera Web");
            if let Err(e) = gtk::init() {
                emit(serde_json::json!({"event": "error", "message": format!("can't start: {e}"), "url": r.url}));
                std::process::exit(1);
            }
            std::process::exit(render::run(r));
        }
        Err(e) => {
            eprintln!("tessera-web: {e}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    };
    // (the Wayland app id, and so Hyprland's class: tessera-web)
    glib::set_prgname(Some("tessera-web"));
    glib::set_application_name("Tessera Web");
    if let Err(e) = gtk::init() {
        eprintln!("tessera-web: can't start: {e}");
        std::process::exit(1);
    }
    let main_loop = glib::MainLoop::new(None, false);
    let target = place_before(&args);
    let app = build(args, main_loop.clone());
    app.placing.set(target.is_some());
    {
        let a = app.clone();
        app.window.connect_map(move |_| {
            let a = a.clone();
            glib::timeout_add_local_once(Duration::from_millis(40), move || {
                if let Some(to) = target {
                    if a.placing.replace(false) {
                        let lua = format!("{}{}", place::move_lua(std::process::id(), &to), place::rule_off_lua());
                        std::thread::spawn(move || {
                            place::eval(&lua);
                        });
                    }
                }
                a.root.remove_css_class("entering");
            });
        });
    }
    // The blocker first (not for sign-in or Wi-Fi pages: nothing may break those)
    let blocked = matches!(app.args.mode, Mode::Browse | Mode::Lens(_) | Mode::QuickLook);
    if blocked {
        let a = app.clone();
        with_blocker(&app.content, move || a.begin());
        // (a store that never answers doesn't keep the page waiting)
        let a = app.clone();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || a.begin());
    } else {
        app.begin();
    }
    app.window.present();
    if !app.editing() {
        app.view.grab_focus();
    }
    main_loop.run();
    app.save_history();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocklist_is_webkits_format() {
        let v: serde_json::Value = serde_json::from_str(BLOCKLIST).expect("blocklist.json is JSON");
        let rules = v.as_array().unwrap();
        assert!(rules.len() > 20);
        for r in rules {
            assert!(r["trigger"]["url-filter"].as_str().is_some_and(|f| !f.is_empty()), "{r}");
            let t = r["action"]["type"].as_str().unwrap();
            assert!(["block", "css-display-none"].contains(&t), "{r}");
            // (WebKit's filters have no alternation or counted repeats)
            let f = r["trigger"]["url-filter"].as_str().unwrap();
            assert!(!f.contains('|') && !f.contains('{'), "{f}");
        }
        // (Google's own pages stay: Lens and sign-in need them)
        assert!(!BLOCKLIST.contains("accounts\\\\.google") && !BLOCKLIST.contains("lens\\\\.google"));
        assert!(filter_id().starts_with("tessera-blocklist-") && filter_id().len() == "tessera-blocklist-".len() + 16);
    }

    #[test]
    fn downloads_never_overwrite() {
        let p = download_path("../../etc/passwd");
        assert!(!p.to_string_lossy().contains("/../"));
        assert!(p.file_name().unwrap().to_string_lossy().contains("passwd"));
        assert_eq!(download_path("").file_name().unwrap(), "download");
    }

    #[test]
    fn the_menus_search_item() {
        assert_eq!(search_label("Google", "  water\n lily "), "Search Google for “water lily”");
        assert_eq!(search_label("DuckDuckGo", "a very long piece of selected text indeed"), "Search DuckDuckGo for “a very long piece of selecte…”");
    }

    #[test]
    fn one_page_by_its_fragment() {
        assert!(same_page("https://a.b/x#y", "https://a.b/x"));
        assert!(!same_page("https://a.b/y", "https://a.b/x"));
        assert!(!same_page("", ""));
        assert!(version().starts_with("tessera-web "));
        assert!(message_page("<b>", &theme::current()).contains("&lt;b&gt;"));
    }
}
