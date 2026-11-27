//! A widget's options (rust.surfaces: widgetOptions), compare
//! shell/modules/desktop/WidgetOptions.qml: a widget's right-click menu →
//! "Widget Options…" opens a dialog (the panel `widgetOptions`, which widget:
//! the shell state's `panels.widgetOptionsKind`) for what the widget shows,
//! set on the widget instead of in Settings:
//!   photos      the folder its pictures come from (and a button for the
//!               file chooser: this closes for it)
//!   worldClock  the cities ("Mumbai|Asia/Kolkata, London|Europe/London")
//!   stocks      the symbols ("AAPL, ^GSPC, BTC-USD")
//!   stack       Smart Rotate (every Widget Stack)
//!   system      how resource graphs are drawn (Area, Bars, Line; also the
//!               island, the menu bar's popovers and Quick Settings)
//!   webClip     (Rust only; `webClip:<id>`, one clip's) its page, the part
//!               shown (the top; a part chosen by dragging on a first full
//!               render, `tessera-web --render`; a CSS selector), and how
//!               often it's refreshed (clip.rs)
//! Enter, Done, Escape or a click outside closes it (a field keeps what's
//! typed: it's saved as it loses focus, or on closing). Settings are written
//! to config.json at once, as the QML's property writes were.
//!
//! The panel's name is `widgetOptions` (the host's Panels opens it with
//! that, and the part is called the same). Standalone only: with the QML
//! shell running its own dialog draws.

use super::*;
use tessera_core::services::bridge::ShellState;

const OPT_NAME: &str = "widgetoptions"; // (the panel is Panels' "widgetOptions": host/shell PANEL_NAMES)
const OPT_NS: &str = "tessera:widgetOptions";

enum Msg {
    Commit(String),
    Choose,
    Rotate(bool),
    Graph(i32),
    Dismissed,
    Closed,
    ClipUrl(String),
    ClipSelector(String),
    ClipRegion(i32),
    ClipEvery(i32),
    ClipCropped(f32, f32, f32, f32),
    /// the first full render for choosing a part (which one, the picture and its size)
    Preview(u64, Option<(String, (u32, u32))>),
}

struct OptWin {
    output: String,
    surface: Rc<Surface>,
    ui: ui::WidgetOptions,
    shown: bool,
    glass_sent: String,
    /// the kind the words were made for
    kind: String,
    settled: bool,
}

#[derive(Default)]
pub struct WidgetOptions {
    win: Option<OptWin>,
    /// a web clip's preview: the address it's of, its picture's size, which render
    preview_for: String,
    preview_size: (u32, u32),
    preview_gen: u64,
}

/// "webClip:<id>" → ("webClip", id); any other kind as it is
pub fn split_kind(kind: &str) -> (&str, &str) {
    match kind.split_once(':') {
        Some((k, id)) if k == "webClip" => (k, id),
        _ => (kind, ""),
    }
}

/// A clip's options changed in config.json: `patch`'s fields set on its
/// entry (read and written under the settings' lock, so quick changes
/// don't overwrite each other)
fn write_clip(id: &str, patch: Value) {
    let id = id.to_string();
    tessera_core::cfgfile::edit_config_later(move |m| {
        let cfg = Value::Object(m.clone());
        let p = patch.clone();
        let list = crate::clip::with_changed(&cfg, &id, move |v| {
            if let (Some(o), Some(add)) = (v.as_object_mut(), p.as_object()) {
                for (k, x) in add {
                    o.insert(k.clone(), x.clone());
                }
            }
        });
        let sec = m.entry("desktop".to_string()).or_insert_with(|| json!({}));
        if !sec.is_object() {
            *sec = json!({});
        }
        sec["webClips"] = json!(list);
        true
    });
}

/// The region's choice for the Segmented (page, crop, selector)
fn region_index(v: &Value) -> i32 {
    match v["region"].as_str() {
        Some("crop") => 1,
        Some("selector") => 2,
        _ => 0,
    }
}

fn every_index(v: &Value) -> i32 {
    let e = v["every"].as_u64().map(|e| e as u32).unwrap_or(900);
    crate::clip::EVERY.iter().position(|x| *x == e).unwrap_or(1) as i32
}

/// A saved crop as fractions of a preview `size` (x, y, w, h; w 0: none)
pub fn crop_fracs(crop: &str, size: (u32, u32), shown_ratio: f32) -> (f32, f32, f32, f32) {
    let Some((x, y, w, h)) = crate::clip::parse_rect(crop) else { return (0.0, 0.0, 0.0, 0.0) };
    if size.0 == 0 || size.1 == 0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let cw = crate::clip::CROP_WIDTH as f32;
    // (the box shows the preview's top, cw × cw·shown_ratio page px)
    let bh = cw * shown_ratio;
    (x as f32 / cw, y as f32 / bh, w as f32 / cw, h as f32 / bh)
}

/// The dialog's heading
pub fn title(kind: &str) -> &'static str {
    match kind {
        "photos" => "Photos",
        "worldClock" => "World clock",
        "stocks" => "Stocks",
        "stack" => "Widget Stack",
        "system" => "System",
        "webClip" => "Web clip",
        _ => "Widget",
    }
}

/// What a typed field becomes in the settings
pub fn parse_field(kind: &str, text: &str, home: &str) -> Option<Value> {
    let list = |upper: bool| -> Vec<String> { text.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).map(|x| if upper { x.to_uppercase() } else { x.to_string() }).collect() };
    match kind {
        "photos" => {
            let t = text.trim();
            Some(json!(if let Some(rest) = t.strip_prefix('~') { format!("{home}{rest}") } else { t.to_string() }))
        }
        "worldClock" => Some(json!(list(false))),
        "stocks" => Some(json!(list(true))),
        _ => None,
    }
}

/// The settings' key for a field of that kind
fn field_key(kind: &str) -> &'static str {
    match kind {
        "photos" => "photosFolder",
        "worldClock" => "worldClocks",
        _ => "stocks",
    }
}

const GRAPHS: [&str; 3] = ["area", "bars", "line"];

/// The widget kind Widget Options… was for (Panels.widgetOptionsKind)
fn kind_now(st: &State) -> String {
    st.shell
        .get::<ShellState>()
        .map(|s| {
            let k = s.str("panels.widgetOptionsKind");
            (if k.is_empty() { s.str("widgetOptionsKind") } else { k }).to_string()
        })
        .unwrap_or_default()
}

impl WidgetOptions {
    /// The words for this kind, and what the settings say now
    fn fill(&mut self, st: &State) {
        let cfg = Desktop::cfg(st);
        let Some(w) = self.win.as_mut() else { return };
        let full = kind_now(st);
        let new_kind = w.kind != full;
        w.kind = full.clone();
        let (kind, clip_id) = split_kind(&full);
        let (kind, clip_id) = (kind.to_string(), clip_id.to_string());
        let ui = &w.ui;
        ui.set_kind(kind.as_str().into());
        ui.set_heading(format!("{} options", title(&kind)).into());
        let (label, placeholder, hint) = match kind.as_str() {
            "photos" => ("Folder", "~/Pictures", "A picture from this folder, changing every 15 minutes. Leave it empty for ~/Pictures."),
            "worldClock" => ("Cities", "Mumbai|Asia/Kolkata, London|Europe/London", "City|Area/Zone, separated by commas."),
            _ => ("Symbols", "AAPL, ^GSPC, BTC-USD", "Symbols separated by commas, as on Yahoo Finance (e.g. RELIANCE.NS)."),
        };
        ui.set_field_label(label.into());
        ui.set_placeholder(placeholder.into());
        ui.set_field_hint(hint.into());
        // (what is typed isn't overwritten while the dialog stays on one kind)
        if new_kind {
            let text = match kind.as_str() {
                "photos" => st.shell.cfg["desktop"]["photosFolder"].as_str().unwrap_or("").to_string(),
                "worldClock" => cfg.cities.join(", "),
                "stocks" => cfg.stocks.join(", "),
                _ => String::new(),
            };
            ui.set_field_edit(text.into());
        }
        ui.set_smart_rotate(cfg.smart_rotate);
        ui.set_graph(GRAPHS.iter().position(|g| *g == cfg.graph).unwrap_or(0) as i32);
        ui.set_on_glass(st.shell.plugin && !st.shell.look.material);
        if kind == "webClip" {
            let v = crate::clip::cfg_value(&st.shell.cfg, &clip_id);
            if new_kind {
                ui.set_clip_url(v["url"].as_str().unwrap_or("").into());
                ui.set_clip_selector(v["selector"].as_str().unwrap_or("").into());
                ui.set_clip_has_preview(false);
                self.preview_for.clear();
            }
            self.clip_fill(st, &clip_id);
        }
    }

    /// A web clip's choices as the settings have them, and its preview
    fn clip_fill(&mut self, st: &State, id: &str) {
        let v = crate::clip::cfg_value(&st.shell.cfg, id);
        let Some(w) = self.win.as_ref() else { return };
        let ui = &w.ui;
        let region = region_index(&v);
        ui.set_clip_region(region);
        ui.set_clip_every(every_index(&v));
        let url = v["url"].as_str().unwrap_or("").trim().to_string();
        let ratio = if self.preview_size.0 > 0 { self.preview_size.1 as f32 / self.preview_size.0 as f32 } else { 0.75 };
        let shown = ratio.min(1.2);
        ui.set_clip_ratio(ratio);
        let (x, y, cw, ch) = if region == 1 { crop_fracs(v["crop"].as_str().unwrap_or(""), self.preview_size, shown) } else { (0.0, 0.0, 0.0, 0.0) };
        ui.set_crop_x(x);
        ui.set_crop_y(y);
        ui.set_crop_w(cw);
        ui.set_crop_h(ch);
        if region != 1 {
            return;
        }
        if !tessera_core::weblink::is_web(&url) {
            ui.set_clip_has_preview(false);
            ui.set_clip_note("Type the page's address first.".into());
            return;
        }
        if self.preview_for == url {
            return;
        }
        // (a first full render, to drag the part on)
        self.preview_for = url.clone();
        self.preview_gen += 1;
        let gen = self.preview_gen;
        ui.set_clip_has_preview(false);
        if tessera_core::weblink::bin().is_none() {
            ui.set_clip_note("Tessera's web browser isn't installed, so the page can't be shown.".into());
            return;
        }
        ui.set_clip_note("Loading the page…".into());
        let to = st.shell.sender(OPT_NAME);
        let out = crate::clip::dir(id).join("preview");
        crate::clip::render_once(crate::clip::preview_args(&url, &out), move |got| to(Box::new(Msg::Preview(gen, got))));
    }

    /// The clip these options are for (its id)
    fn clip_id(&self) -> Option<String> {
        let w = self.win.as_ref()?;
        let (k, id) = split_kind(&w.kind);
        (k == "webClip" && !id.is_empty()).then(|| id.to_string())
    }

    fn open(&mut self, st: &mut State, screen: Option<String>) {
        if !tessera_core::host::standalone() {
            return;
        }
        let output = screen.or_else(|| self.win.as_ref().map(|w| w.output.clone())).or_else(|| Desktop::primary_for(st, None).map(|p| p.1));
        let Some(output) = output else { return };
        if self.win.as_ref().is_some_and(|w| w.output != output) {
            self.drop_win(st);
        }
        if self.win.is_none() {
            let Some(wl) = st.output_named(&output) else { return };
            let surface = st.prepare_surface(Spec {
                layer: Layer::Overlay,
                namespace: OPT_NS,
                output: Some((&wl, &output)),
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                size: (0, 0),
                exclusive: -1,
                keyboard: KeyboardInteractivity::Exclusive,
                margin_left: 0,
            });
            let ui = ui::WidgetOptions::new().expect("WidgetOptions");
            st.shell.apply_theme(&ui.global::<ui::Theme>());
            // (Material: the kit's icons as Material Symbols)
            ui.global::<ui::Symbols>().on_of(|g| tessera_core::icons::symbol(&g).into());
            let to = st.shell.sender(OPT_NAME);
            let t = to.clone();
            ui.on_commit(move |s| t(Box::new(Msg::Commit(s.to_string()))));
            let t = to.clone();
            ui.on_choose(move || t(Box::new(Msg::Choose)));
            let t = to.clone();
            ui.on_rotate(move |on| t(Box::new(Msg::Rotate(on))));
            let t = to.clone();
            ui.on_graph_picked(move |i| t(Box::new(Msg::Graph(i))));
            let t = to.clone();
            ui.on_clip_url_commit(move |s| t(Box::new(Msg::ClipUrl(s.to_string()))));
            let t = to.clone();
            ui.on_clip_selector_commit(move |s| t(Box::new(Msg::ClipSelector(s.to_string()))));
            let t = to.clone();
            ui.on_clip_region_picked(move |i| t(Box::new(Msg::ClipRegion(i))));
            let t = to.clone();
            ui.on_clip_every_picked(move |i| t(Box::new(Msg::ClipEvery(i))));
            let t = to.clone();
            ui.on_clip_cropped(move |x, y, w, h| t(Box::new(Msg::ClipCropped(x, y, w, h))));
            let t = to.clone();
            ui.on_dismissed(move || t(Box::new(Msg::Dismissed)));
            ui.on_closed(move || to(Box::new(Msg::Closed)));
            ui.show().expect("show widget options");
            self.win = Some(OptWin { output, surface, ui, shown: false, glass_sent: String::new(), kind: String::from("\u{0}"), settled: false });
        }
        self.fill(st);
        let Some(w) = self.win.as_mut() else { return };
        w.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        w.surface.show(0, 0);
        st.set_input_region(&w.surface, None);
        if !w.shown {
            w.shown = true;
            w.ui.set_shown(true);
            w.ui.invoke_focus_first();
        }
    }

    fn close(&mut self, st: &mut State, tell_shell: bool) {
        let Some(w) = self.win.as_mut() else { return };
        if w.shown {
            w.shown = false;
            w.ui.set_shown(false);
            w.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // (while closing, clicks pass through: the layer stays for the fade)
            st.set_input_region(&w.surface, Some(&[]));
            w.surface.wl.commit();
        }
        if tell_shell {
            shell::closed(OPT_NAME);
        }
    }

    fn drop_win(&mut self, st: &mut State) {
        if let Some(w) = self.win.take() {
            st.shell.glass.set(&w.output, OPT_NS, &[]);
            w.surface.hide(); // (unmapped now: dropping the window alone leaves it on screen)
            st.drop_surface(&w.surface);
        }
    }

    /// A field's text as it was left: saved if it changed
    fn commit(&mut self, st: &State, text: &str) {
        let Some(w) = &self.win else { return };
        let home = std::env::var("HOME").unwrap_or_default();
        let Some(v) = parse_field(&w.kind, text, &home) else { return };
        let key = field_key(&w.kind);
        if st.shell.cfg["desktop"][key] != v && !(st.shell.cfg["desktop"][key].is_null() && v == json!("")) {
            write_cfg("desktop", key, v);
        }
    }

    /// The photos folder, from the file chooser (the dialog closes for it)
    fn choose(&mut self, st: &mut State) {
        self.close(st, true);
        let start = {
            let f = st.shell.cfg["desktop"]["photosFolder"].as_str().unwrap_or("");
            if f.is_empty() { "~/Pictures".to_string() } else { f.to_string() }
        };
        let Some(script) = crate::services::apps::shell_file("shell/assets/scripts/pick-file.py") else { return };
        std::thread::spawn(move || {
            let home = std::env::var("HOME").unwrap_or_default();
            let start = if let Some(rest) = start.strip_prefix('~') { format!("{home}{rest}") } else { start };
            let out = std::process::Command::new("python3")
                .arg(script)
                .args(["--folder", "--title", "Pictures for the Photos widget", "--start", &start])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            if let Ok(o) = out {
                let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if o.status.success() && !path.is_empty() {
                    write_cfg("desktop", "photosFolder", json!(path));
                }
            }
        });
    }
}

impl Part for WidgetOptions {
    fn name(&self) -> &'static str {
        OPT_NAME
    }

    fn command(&mut self, st: &mut State, cmd: &shell::Command) {
        match cmd.cmd.as_str() {
            "open" => self.open(st, cmd.screen.clone()),
            "close" => self.close(st, false),
            _ => {}
        }
    }

    fn data(&mut self, st: &mut State, what: TypeId) {
        // (the kind arrives in the shell state, which can come just after the command to open)
        if what == TypeId::of::<ShellState>() && self.win.as_ref().is_some_and(|w| w.shown && w.kind != kind_now(st)) {
            self.fill(st);
        }
    }

    fn message(&mut self, st: &mut State, msg: Box<dyn Any + Send>) {
        let Ok(m) = msg.downcast::<Msg>() else { return };
        match *m {
            Msg::Commit(t) => self.commit(st, &t),
            Msg::Choose => self.choose(st),
            Msg::Rotate(on) => {
                write_cfg("desktop", "smartRotate", json!(on));
                if let Some(w) = &self.win {
                    w.ui.set_smart_rotate(on);
                }
            }
            Msg::Graph(i) => {
                write_cfg("resources", "graphStyle", json!(GRAPHS[(i.max(0) as usize).min(2)]));
                if let Some(w) = &self.win {
                    w.ui.set_graph(i);
                }
            }
            Msg::ClipUrl(u) => {
                let Some(id) = self.clip_id() else { return };
                let t = u.trim();
                // (an address typed without https://: as the browser takes it)
                let url = if t.is_empty() || tessera_core::weblink::is_web(t) { t.to_string() } else { format!("https://{t}") };
                let now = crate::clip::cfg_value(&st.shell.cfg, &id);
                if now["url"].as_str().unwrap_or("") != url {
                    write_clip(&id, json!({"url": url}));
                    if let Some(w) = &self.win {
                        w.ui.set_clip_url(url.as_str().into());
                    }
                }
            }
            Msg::ClipSelector(sel) => {
                let Some(id) = self.clip_id() else { return };
                let sel = sel.trim().to_string();
                if crate::clip::cfg_value(&st.shell.cfg, &id)["selector"].as_str().unwrap_or("") != sel {
                    write_clip(&id, json!({"selector": sel, "region": "selector"}));
                }
            }
            Msg::ClipRegion(i) => {
                let Some(id) = self.clip_id() else { return };
                let region = ["page", "crop", "selector"][(i.max(0) as usize).min(2)];
                write_clip(&id, json!({"region": region}));
                if let Some(w) = &self.win {
                    w.ui.set_clip_region(i);
                }
                // (the URL typed but not yet saved: the preview is of it)
                let typed = self.win.as_ref().map(|w| w.ui.get_clip_url().to_string()).unwrap_or_default();
                if i == 1 && tessera_core::weblink::is_web(typed.trim()) {
                    write_clip(&id, json!({"url": typed.trim()}));
                }
            }
            Msg::ClipEvery(i) => {
                let Some(id) = self.clip_id() else { return };
                let every = crate::clip::EVERY[(i.max(0) as usize).min(3)];
                write_clip(&id, json!({"every": every}));
                if let Some(w) = &self.win {
                    w.ui.set_clip_every(i);
                }
            }
            Msg::ClipCropped(x, y, w, h) => {
                let Some(id) = self.clip_id() else { return };
                let ratio = if self.preview_size.0 > 0 { self.preview_size.1 as f32 / self.preview_size.0 as f32 } else { 0.75 };
                // (the box shows the preview's top: its fractions of the whole picture)
                let k = ratio.min(1.2) / ratio.max(1e-3);
                match crate::clip::crop_from(x, y * k, w, h * k, self.preview_size.0, self.preview_size.1) {
                    Some((cx, cy, cw, ch)) => {
                        write_clip(&id, json!({"region": "crop", "crop": format!("{cx},{cy},{cw},{ch}")}));
                        if let Some(win) = &self.win {
                            win.ui.set_clip_note("Drag again to choose another part.".into());
                        }
                    }
                    None => {
                        if let Some(win) = &self.win {
                            win.ui.set_clip_note("Drag over a larger part of the page.".into());
                        }
                    }
                }
            }
            Msg::Preview(gen, got) => {
                if gen != self.preview_gen {
                    return;
                }
                let Some(w) = &self.win else { return };
                match got.and_then(|(png, size)| slint::Image::load_from_path(std::path::Path::new(&png)).ok().map(|i| (i, size))) {
                    Some((img, size)) => {
                        self.preview_size = size;
                        w.ui.set_clip_preview(img);
                        w.ui.set_clip_has_preview(true);
                        w.ui.set_clip_note("Drag over the part of the page to show.".into());
                        if let Some(id) = self.clip_id() {
                            self.clip_fill(st, &id);
                        }
                    }
                    None => {
                        w.ui.set_clip_has_preview(false);
                        w.ui.set_clip_note("Couldn't load the page. Check the address.".into());
                        // (asked again when the address changes, or the options open again)
                        self.preview_for.clear();
                        self.preview_for.push('\u{0}');
                    }
                }
            }
            Msg::Dismissed => self.close(st, true),
            Msg::Closed => {
                if self.win.as_ref().is_some_and(|w| !w.shown) {
                    self.drop_win(st);
                }
            }
        }
    }

    fn config_changed(&mut self, st: &mut State, look: bool) {
        if self.win.is_none() {
            return;
        }
        if look {
            if let Some(w) = self.win.as_mut() {
                st.shell.apply_theme(&w.ui.global::<ui::Theme>());
                w.glass_sent.clear();
            }
        }
        // (a web clip's choices follow the settings: its writes land there)
        if let Some(id) = self.clip_id() {
            self.clip_fill(st, &id);
        }
        // (the switch and the graphs follow the settings; a field keeps what is typed)
        let cfg = Desktop::cfg(st);
        if let Some(w) = &self.win {
            w.ui.set_smart_rotate(cfg.smart_rotate);
            w.ui.set_graph(GRAPHS.iter().position(|g| *g == cfg.graph).unwrap_or(0) as i32);
            w.ui.set_on_glass(st.shell.plugin && !st.shell.look.material);
        }
    }

    fn plugin_changed(&mut self, st: &mut State) {
        self.config_changed(st, true);
    }

    fn output_removed(&mut self, st: &mut State, name: &str) {
        if self.win.as_ref().is_some_and(|w| w.output == name) {
            self.drop_win(st);
        }
    }

    fn surface_closed(&mut self, st: &mut State, s: &Rc<Surface>) {
        if self.win.as_ref().is_some_and(|w| Rc::ptr_eq(&w.surface, s)) {
            self.drop_win(st);
        }
    }

    fn after_frame(&mut self, st: &mut State) {
        let Some(w) = self.win.as_mut() else { return };
        crate::picture::second_frame(&w.surface, &mut w.settled);
        if !st.shell.plugin || st.shell.look.material || !w.surface.mapped.get() {
            return;
        }
        let ui = &w.ui;
        let (p, s) = (ui.get_fade(), ui.get_scale());
        let (bx, by, bw, bh) = (ui.get_body_x(), ui.get_body_y(), ui.get_body_w(), ui.get_body_h());
        let (sw, sh) = (bw * s, bh * s);
        let shape = theme::glass_shape(&st.shell.look, bx + (bw - sw) / 2.0, by + (bh - sh) / 2.0, sw, sh, 28.0 * s, p, 1.0);
        let key = format!("{shape:?}");
        if key != w.glass_sent {
            st.shell.glass.set(&w.output, OPT_NS, if p > 0.002 { std::slice::from_ref(&shape) } else { &[] });
            w.glass_sent = key;
        }
    }

    fn quit(&mut self, _: &mut State) {
        if let Some(w) = &self.win {
            crate::services::hypr::Glass::clear_now(&w.output, OPT_NS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_field_becomes() {
        assert_eq!(parse_field("photos", " ~/Pictures/Trips ", "/home/a"), Some(json!("/home/a/Pictures/Trips")));
        assert_eq!(parse_field("photos", "/srv/pics", "/home/a"), Some(json!("/srv/pics")));
        assert_eq!(parse_field("worldClock", "Mumbai|Asia/Kolkata, , London|Europe/London", ""), Some(json!(["Mumbai|Asia/Kolkata", "London|Europe/London"])));
        assert_eq!(parse_field("stocks", "aapl, ^gspc ,btc-usd", ""), Some(json!(["AAPL", "^GSPC", "BTC-USD"])));
        assert_eq!(parse_field("system", "x", ""), None);
        assert_eq!(parse_field("stocks", "", ""), Some(json!([])));
    }

    #[test]
    fn a_web_clips_options() {
        assert_eq!(split_kind("webClip:ab12"), ("webClip", "ab12"));
        assert_eq!(split_kind("photos"), ("photos", ""));
        assert_eq!(split_kind("stocks:x"), ("stocks:x", ""));
        assert_eq!(region_index(&json!({"region": "crop"})), 1);
        assert_eq!(region_index(&json!({})), 0);
        assert_eq!(every_index(&json!({"every": 0})), 3);
        assert_eq!(every_index(&json!({})), 1);
        // (a crop of the page's top half, on a 4:3 preview)
        let (x, y, w, h) = crop_fracs("0,0,640,480", (2560, 1920), 0.75);
        assert!((x, w) == (0.0, 0.5) && y == 0.0 && (h - 0.5).abs() < 1e-6);
        assert_eq!(crop_fracs("bad", (1280, 960), 0.75), (0.0, 0.0, 0.0, 0.0));
        assert_eq!(title("webClip"), "Web clip");
    }

    #[test]
    fn headings() {
        assert_eq!(title("worldClock"), "World clock");
        assert_eq!(title("something"), "Widget");
    }
}
