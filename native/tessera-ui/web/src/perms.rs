//! Site permissions: what a page asks WebKit for (the camera, the
//! microphone, sharing the screen, notifications) is asked of you once per
//! site, in a small prompt in the bar ("meet.google.com wants to use your
//! camera and microphone": Deny, Allow), and the answer is remembered per
//! site in the data folder (permissions.json, 0600). The bar's menu has
//! "Site permissions": each site and what it may do, with Forget.
//!
//! A meeting link from the calendar (Meet, Teams, Zoom's web client) needs
//! the camera and the microphone: WebRTC and media streams are on for that.
//! Sharing the screen goes through the desktop portal (WebKit asks it;
//! the portal's own picker chooses the screen or window). What a site's
//! devices are called (enumerateDevices' labels) is told only to a site
//! you let use the camera or the microphone. Anything else a page asks for
//! (location, the clipboard, pointer lock…) gets WebKit's own answer: no.
//!
//! The rules (the store, origins, the words) are pure and tested here; the
//! GTK side is `attach`.

use gtk4 as gtk;
use gtk::{gio, glib, pango};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::prelude::*;
use webkit6::{PermissionRequest, WebView};

pub const CAMERA: &str = "camera";
pub const MICROPHONE: &str = "microphone";
pub const SCREEN: &str = "screen";
pub const NOTIFICATIONS: &str = "notifications";

/// A page's site: scheme://host[:port] (http and https only; None else)
pub fn origin(uri: &str) -> Option<String> {
    let u = uri.trim();
    let (scheme, rest) = u.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h).to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    // (the usual port is the same site)
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("https", Some((h, "443"))) | ("http", Some((h, "80"))) => h.to_string(),
        _ => host,
    };
    Some(format!("{scheme}://{host}"))
}

/// The site as the prompt names it ("meet.google.com")
pub fn shown(origin: &str) -> String {
    let h = origin.split_once("://").map_or(origin, |(_, h)| h);
    h.strip_prefix("www.").unwrap_or(h).to_string()
}

fn noun(kind: &str) -> &'static str {
    match kind {
        CAMERA => "camera",
        MICROPHONE => "microphone",
        SCREEN => "screen",
        _ => "notifications",
    }
}

/// "meet.google.com wants to use your camera and microphone"
pub fn question(origin: &str, kinds: &[&str]) -> String {
    let site = shown(origin);
    if kinds == [NOTIFICATIONS] {
        return format!("{site} wants to show notifications");
    }
    if kinds == [SCREEN] {
        return format!("{site} wants to share your screen");
    }
    let words: Vec<&str> = kinds.iter().map(|k| noun(k)).collect();
    let list = match words.len() {
        0 => String::new(),
        1 => words[0].to_string(),
        n => format!("{} and {}", words[..n - 1].join(", "), words[n - 1]),
    };
    format!("{site} wants to use your {list}")
}

/// What the list says a site may do ("Camera: allowed · Microphone: blocked")
pub fn summary(answers: &BTreeMap<String, bool>) -> String {
    let order = [CAMERA, MICROPHONE, SCREEN, NOTIFICATIONS];
    let mut parts: Vec<String> = Vec::new();
    for k in order {
        if let Some(a) = answers.get(k) {
            let n = noun(k);
            let cap = n[..1].to_uppercase() + &n[1..];
            let (cap, word) = if k == SCREEN { ("Screen sharing".to_string(), if *a { "allowed" } else { "blocked" }) } else { (cap, if *a { "allowed" } else { "blocked" }) };
            parts.push(format!("{cap}: {word}"));
        }
    }
    parts.join(" · ")
}

/// The answers, per site
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    pub sites: BTreeMap<String, BTreeMap<String, bool>>,
}

impl Store {
    pub fn parse(text: &str) -> Store {
        let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let mut sites = BTreeMap::new();
        if let Some(o) = v.as_object() {
            for (site, answers) in o {
                let Some(a) = answers.as_object() else { continue };
                let m: BTreeMap<String, bool> = a.iter().filter(|(k, _)| [CAMERA, MICROPHONE, SCREEN, NOTIFICATIONS].contains(&k.as_str())).filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect();
                if !m.is_empty() && origin(site).as_deref() == Some(site.as_str()) {
                    sites.insert(site.clone(), m);
                }
            }
        }
        Store { sites }
    }

    pub fn to_json(&self) -> String {
        let mut o = serde_json::Map::new();
        for (site, m) in &self.sites {
            o.insert(site.clone(), json!(m));
        }
        serde_json::to_string_pretty(&Value::Object(o)).unwrap_or_else(|_| "{}".into())
    }

    pub fn load(p: &Path) -> Store {
        std::fs::read_to_string(p).map(|t| Store::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, p: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if !p.exists() {
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)?;
        }
        tessera_core::util::write_atomic(p, self.to_json().as_bytes())?;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
    }

    /// The remembered answer for all of these: Some(true) when each was
    /// allowed, Some(false) when any was blocked, None to ask
    pub fn answer(&self, origin: &str, kinds: &[&str]) -> Option<bool> {
        let m = self.sites.get(origin)?;
        if kinds.iter().any(|k| m.get(*k) == Some(&false)) {
            return Some(false);
        }
        if !kinds.is_empty() && kinds.iter().all(|k| m.get(*k) == Some(&true)) {
            return Some(true);
        }
        None
    }

    pub fn set(&mut self, origin: &str, kinds: &[&str], allow: bool) {
        let m = self.sites.entry(origin.to_string()).or_default();
        for k in kinds {
            m.insert(k.to_string(), allow);
        }
    }

    pub fn forget(&mut self, origin: &str) {
        self.sites.remove(origin);
    }

    /// The site may use the camera or the microphone (and so hear what
    /// they're called)
    pub fn media_allowed(&self, origin: &str) -> bool {
        self.sites.get(origin).is_some_and(|m| m.get(CAMERA) == Some(&true) || m.get(MICROPHONE) == Some(&true))
    }
}

/// What a WebKit request asks for (empty: not one asked about here)
fn kinds_of(r: &PermissionRequest) -> Vec<&'static str> {
    if let Some(m) = r.downcast_ref::<webkit6::UserMediaPermissionRequest>() {
        if webkit6::functions::user_media_permission_is_for_display_device(m) {
            return vec![SCREEN];
        }
        let mut k = Vec::new();
        if m.is_for_video_device() {
            k.push(CAMERA);
        }
        if m.is_for_audio_device() {
            k.push(MICROPHONE);
        }
        return k;
    }
    if r.is::<webkit6::NotificationPermissionRequest>() {
        return vec![NOTIFICATIONS];
    }
    Vec::new()
}

struct Asked {
    request: PermissionRequest,
    origin: String,
    kinds: Vec<&'static str>,
}

struct Ui {
    store: RefCell<Store>,
    path: PathBuf,
    queue: RefCell<VecDeque<Asked>>,
    prompt: gtk::Box,
    words: gtk::Label,
    sheet: gtk::Box,
    list: gtk::ListBox,
}

impl Ui {
    fn save(&self) {
        if let Err(e) = self.store.borrow().save(&self.path) {
            eprintln!("tessera-web: the site permissions weren't saved: {e}");
        }
    }

    /// The first request still waiting, in the prompt (or none: hidden)
    fn show_next(&self) {
        let q = self.queue.borrow();
        match q.front() {
            Some(a) => {
                self.words.set_text(&question(&a.origin, &a.kinds));
                self.prompt.set_visible(true);
            }
            None => self.prompt.set_visible(false),
        }
    }

    /// Allow or Deny pressed: this answer for the site, remembered; any
    /// other request from that site the answer covers goes with it
    fn answer(self: &Rc<Self>, allow: bool) {
        let Some(a) = self.queue.borrow_mut().pop_front() else { return };
        self.store.borrow_mut().set(&a.origin, &a.kinds, allow);
        self.save();
        if allow {
            a.request.allow();
        } else {
            a.request.deny();
        }
        let store = self.store.borrow().clone();
        self.queue.borrow_mut().retain(|o| match store.answer(&o.origin, &o.kinds) {
            Some(true) => {
                o.request.allow();
                false
            }
            Some(false) => {
                o.request.deny();
                false
            }
            None => true,
        });
        self.show_next();
        self.fill();
    }

    /// A page left with its question unanswered: it's withdrawn (WebKit
    /// takes a request never answered as a no)
    fn drop_waiting(&self) {
        self.queue.borrow_mut().clear();
        self.show_next();
    }

    /// The Site permissions list
    fn fill(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let sites: Vec<(String, BTreeMap<String, bool>)> = self.store.borrow().sites.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if sites.is_empty() {
            let l = gtk::Label::new(Some("No site has asked for anything yet."));
            l.set_xalign(0.0);
            l.add_css_class("url");
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&l));
            row.set_activatable(false);
            row.set_selectable(false);
            self.list.append(&row);
            return;
        }
        for (site, answers) in sites {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let words = gtk::Box::new(gtk::Orientation::Vertical, 1);
            words.set_hexpand(true);
            let t = gtk::Label::new(Some(shown(&site).as_str()));
            t.set_xalign(0.0);
            t.set_ellipsize(pango::EllipsizeMode::Middle);
            let u = gtk::Label::new(Some(summary(&answers).as_str()));
            u.set_xalign(0.0);
            u.set_ellipsize(pango::EllipsizeMode::End);
            u.add_css_class("url");
            words.append(&t);
            words.append(&u);
            let forget = gtk::Button::with_label("Forget");
            forget.add_css_class("perm-button");
            forget.set_valign(gtk::Align::Center);
            forget.set_tooltip_text(Some("Ask again next time"));
            let me = self.clone();
            forget.connect_clicked(move |_| {
                me.store.borrow_mut().forget(&site);
                me.save();
                me.fill();
            });
            b.append(&words);
            b.append(&forget);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            row.set_activatable(false);
            row.set_selectable(false);
            self.list.append(&row);
        }
    }
}

/// The prompt's and the list's style (the look's colours, as the bar's)
fn css(p: &crate::theme::Palette) -> String {
    let k = crate::theme::css_color;
    let font = p.font.replace(['"', '\\', ';', '{', '}'], "");
    let radius = if p.material { 20 } else { 999 };
    format!(
        r#".permit {{ background-color: {menu}; color: {text}; border-radius: {radius}px; padding: 0 6px 0 14px; min-height: {h}px; box-shadow: 0 4px 14px rgba(0, 0, 0, 0.22); font-family: "{font}", "Inter", sans-serif; font-size: 13px; }}
.permit button, .perms button.perm-button {{ border: none; box-shadow: none; outline: none; border-radius: {radius}px; padding: 2px 12px; min-height: 26px; background-color: {hover}; color: {text}; }}
.permit button.allow {{ background-color: {accent}; color: {on_accent}; }}
.permit button:hover, .perms button.perm-button:hover {{ opacity: 0.85; }}
.perms {{ background-color: {menu}; color: {text}; border-radius: 14px; padding: 10px; box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28); font-family: "{font}", "Inter", sans-serif; font-size: 13px; }}
.perms .heading {{ font-weight: 600; font-size: 14px; padding: 2px 6px 6px 6px; }}
.perms list {{ background: none; }}
.perms row {{ border-radius: 9px; padding: 6px 6px; color: {text}; background: none; }}
.perms .url {{ font-size: 11px; opacity: 0.75; }}
"#,
        menu = k(p.menu),
        text = k(p.text),
        hover = k(p.hover),
        accent = k(p.accent),
        on_accent = k(p.on_selected),
        h = tessera_core::weblink::bar::PILL_H,
    )
}

/// The prompt over the bar's right end, the list as a sheet, the menu's
/// "Site permissions", and WebKit's requests answered (call once, while
/// building the window; `overlay` holds the bar and the page)
pub fn attach(view: &WebView, overlay: &gtk::Overlay, window: &gtk::Window, menu: &gio::Menu, palette: &crate::theme::Palette, data: &Path) {
    use tessera_core::weblink::bar as dims;
    if let Some(display) = gtk::gdk::Display::default() {
        let p = gtk::CssProvider::new();
        p.load_from_string(&css(palette));
        gtk::style_context_add_provider_for_display(&display, &p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
    // (calls in the page: WebRTC and the camera and microphone's streams)
    if let Some(s) = WebViewExt::settings(view) {
        s.set_enable_media_stream(true);
        s.set_enable_webrtc(true);
    }

    // The prompt: the question, Deny, Allow; over the bar's right end
    let prompt = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    prompt.add_css_class("permit");
    prompt.set_halign(gtk::Align::End);
    prompt.set_valign(gtk::Align::Start);
    prompt.set_margin_top(dims::PAD_Y);
    prompt.set_margin_end(dims::PAD_X);
    prompt.set_margin_start(dims::PILL_X + 40);
    prompt.set_size_request(-1, dims::PILL_H);
    let words = gtk::Label::new(None);
    words.set_ellipsize(pango::EllipsizeMode::Middle);
    words.set_hexpand(true);
    words.set_xalign(0.0);
    let deny = gtk::Button::with_label("Deny");
    deny.set_valign(gtk::Align::Center);
    deny.set_focus_on_click(false);
    let allow = gtk::Button::with_label("Allow");
    allow.add_css_class("allow");
    allow.set_valign(gtk::Align::Center);
    allow.set_focus_on_click(false);
    prompt.append(&words);
    prompt.append(&deny);
    prompt.append(&allow);
    prompt.set_visible(false);
    overlay.add_overlay(&prompt);

    // The list: a sheet under the bar's right end
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sheet.add_css_class("perms");
    sheet.set_halign(gtk::Align::End);
    sheet.set_valign(gtk::Align::Start);
    sheet.set_margin_top(dims::H + 4);
    sheet.set_margin_end(dims::PAD_X);
    sheet.set_size_request(360, -1);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Site permissions"));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let done = gtk::Button::with_label("Done");
    done.add_css_class("perm-button");
    head.append(&title);
    head.append(&done);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(320);
    scroll.set_child(Some(&list));
    sheet.append(&head);
    sheet.append(&scroll);
    sheet.set_visible(false);
    overlay.add_overlay(&sheet);

    let path = data.join("permissions.json");
    let ui = Rc::new(Ui { store: RefCell::new(Store::load(&path)), path, queue: RefCell::new(VecDeque::new()), prompt, words, sheet, list });
    {
        let u = ui.clone();
        allow.connect_clicked(move |_| u.answer(true));
        let u = ui.clone();
        deny.connect_clicked(move |_| u.answer(false));
        let u = ui.clone();
        done.connect_clicked(move |_| u.sheet.set_visible(false));
    }

    // WebKit's requests
    {
        let u = ui.clone();
        view.connect_permission_request(move |view, request| {
            let uri = view.uri().map(|s| s.to_string()).unwrap_or_default();
            let Some(site) = origin(&uri) else {
                request.deny();
                return true;
            };
            // (what a site's devices are called: only to a site that may use them)
            if request.is::<webkit6::DeviceInfoPermissionRequest>() {
                if u.store.borrow().media_allowed(&site) {
                    request.allow();
                } else {
                    request.deny();
                }
                return true;
            }
            let kinds = kinds_of(request);
            if kinds.is_empty() {
                // (WebKit's own answer)
                return false;
            }
            match u.store.borrow().answer(&site, &kinds) {
                Some(true) => {
                    request.allow();
                    return true;
                }
                Some(false) => {
                    request.deny();
                    return true;
                }
                None => {}
            }
            u.queue.borrow_mut().push_back(Asked { request: request.clone(), origin: site, kinds });
            u.show_next();
            true
        });
        // (another page: its questions go)
        let u = ui.clone();
        view.connect_load_changed(move |_, ev| {
            if ev == webkit6::LoadEvent::Committed {
                u.drop_waiting();
            }
        });
    }

    // The menu's "Site permissions"
    menu.append(Some("Site permissions"), Some("perm.list"));
    let group = gio::SimpleActionGroup::new();
    let show = gio::SimpleAction::new("list", None);
    let u = ui.clone();
    show.connect_activate(move |_, _| {
        u.fill();
        u.sheet.set_visible(!u.sheet.is_visible());
    });
    group.add_action(&show);
    window.insert_action_group("perm", Some(&group));
    // (Esc while the list is open closes the list first)
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let u = ui;
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape && u.sheet.is_visible() {
            u.sheet.set_visible(false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_by_origin() {
        assert_eq!(origin("https://meet.google.com/abc-defg?x=1").as_deref(), Some("https://meet.google.com"));
        assert_eq!(origin("HTTPS://Meet.Google.com:443/x").as_deref(), Some("https://meet.google.com"));
        assert_eq!(origin("http://a.b:8080/").as_deref(), Some("http://a.b:8080"));
        assert_eq!(origin("https://user:pw@zoom.us/wc/join/1").as_deref(), Some("https://zoom.us"));
        assert_eq!(origin("file:///home/me/a.html"), None);
        assert_eq!(origin("about:blank"), None);
        assert_eq!(shown("https://www.example.com"), "example.com");
    }

    #[test]
    fn the_questions() {
        assert_eq!(question("https://meet.google.com", &[CAMERA, MICROPHONE]), "meet.google.com wants to use your camera and microphone");
        assert_eq!(question("https://zoom.us", &[MICROPHONE]), "zoom.us wants to use your microphone");
        assert_eq!(question("https://a.b", &[SCREEN]), "a.b wants to share your screen");
        assert_eq!(question("https://a.b", &[NOTIFICATIONS]), "a.b wants to show notifications");
    }

    #[test]
    fn answers_are_remembered_per_site() {
        let mut s = Store::default();
        let meet = "https://meet.google.com";
        assert_eq!(s.answer(meet, &[CAMERA, MICROPHONE]), None);
        s.set(meet, &[CAMERA, MICROPHONE], true);
        assert_eq!(s.answer(meet, &[CAMERA, MICROPHONE]), Some(true));
        assert_eq!(s.answer(meet, &[MICROPHONE]), Some(true));
        // (asked for more than was answered: asked again)
        assert_eq!(s.answer(meet, &[CAMERA, SCREEN]), None);
        s.set(meet, &[SCREEN], false);
        assert_eq!(s.answer(meet, &[CAMERA, SCREEN]), Some(false));
        assert!(s.media_allowed(meet) && !s.media_allowed("https://other.site"));
        // (another site knows nothing of it)
        assert_eq!(s.answer("https://other.site", &[CAMERA]), None);
        assert_eq!(summary(&s.sites[meet]), "Camera: allowed · Microphone: allowed · Screen sharing: blocked");
        s.forget(meet);
        assert_eq!(s.answer(meet, &[CAMERA]), None);
    }

    #[test]
    fn the_file_round_trips_and_stays_private() {
        use std::os::unix::fs::PermissionsExt;
        let mut s = Store::default();
        s.set("https://meet.google.com", &[CAMERA, MICROPHONE], true);
        s.set("https://ads.example", &[NOTIFICATIONS], false);
        let dir = std::env::temp_dir().join(format!("tessera-web-perms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("permissions.json");
        s.save(&p).unwrap();
        assert_eq!(Store::load(&p), s);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        // (hand-edited nonsense is left out)
        let odd = Store::parse(r#"{"https://a.b": {"camera": true, "telepathy": true}, "not a site": {"camera": true}, "https://c.d": {"camera": "yes"}}"#);
        assert_eq!(odd.sites.len(), 1);
        assert_eq!(odd.sites["https://a.b"].len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
