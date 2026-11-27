//! Search (rust.surfaces: spotlight): apps, actions, files, sums and answers,
//! conversions, definitions, emoji, commands, the clipboard, and the "Just
//! ask" hand-off to the AI, as you type. Search itself (the view and its
//! controller) is search.rs and ui/search.slint, shared with the Overview,
//! which hosts the same Search as you type in it; this is its own window. Glass: a floating glass field, the
//! results in a second glass panel below; Material: the docked search view,
//! segmented lists under a hero card. Compare shell/modules/spotlight/
//! (SpotlightGlass.qml, SearchView.qml, SearchModel.qml and their rows) and
//! shell/services/Search.qml.
//!
//! It opens as the QML one did, through Panels ("spotlight": its shortcut,
//! the menu bar's search button, `qs ipc call`): RustUi.qml forwards the
//! open and close, and it says when it closed itself.
//!
//! The ranking is Rust's (rank.rs: Search.query's order, tested as pure
//! functions); the rows only the QML shell knows (plain-language requests,
//! the calendar, contacts, notes, screenshots, the App Store, the clipboard,
//! Chromecasts) come from it on stdin (`{"to":"spotlight","extras":…}`,
//! asked with `search.query`) and run there by key (`search.run`). The AI's
//! quick answer runs in the shell too (`ai.quickAsk`; its progress arrives
//! as `{"quick":…}` lines).
//!
//! Not like the QML yet, where Slint can't do the same:
//! - Material's icons are Material Symbols in the font's default fill (Slint
//!   can't set the FILL axis); shapes are polar outlines near the QML's
//!   shader shapes (cookie, sunny), not the same curves;
//! - the AI answer is CommonMark through Slint's StyledText (no headings,
//!   tables or code boxes, which the QML's rich text draws); links open only
//!   when they're https (the shell's launch.open);
//! - springs are Slint's spring() easing with the QML SpringAnimation's ω and
//!   ζ (Qt steps 16 ms: ω = √(62.5·spring), ζ = 62.5·damping / 2ω); the rows'
//!   entrance is that spring's closed form, staggered as the QML's;
//! - no tooltips on the field's buttons; Glass's Tab to the buttons is a
//!   focus ring the panel draws (the field keeps the keyboard);
//! - Material's loading row shows the shared Busy arc, not the QML's
//!   morphing loading indicator.
//! And one not Slint's: apps with exactly the same score keep the order of
//! the desktop-file scan; Quickshell's list is in hash order (not the same
//! from one run to the next), so such ties can come out in another order.

pub mod ui {
    slint::include_modules!();
}
tessera_core::theme_global!(ui);

// tessera-core's modules (shell, wayland, theme, services, util…) under the
// names they had when everything was one crate: `crate::shell::Part`
#[allow(unused_imports)]
use tessera_core::*;

pub mod layout;
pub mod providers;
pub mod web;
pub mod reading;
pub mod search;
// (Search's ranking lives in tessera-core: the Overview ranks with it too)
pub use tessera_core::rank;
pub use search::{glass_fill, parse_extras, sub_line};

// (for search_view!, expanded in other crates)
#[doc(hidden)]
pub use slint as __slint;
#[doc(hidden)]
pub use tessera_core as __core;

crate::search_view!(ui, SpotlightPanel);

use search::{Search, View};
use tessera_core::services::hypr::Glass;
use tessera_core::shell::{Command, Part};
use tessera_core::wayland::{Spec, State, Surface};
use slint::ComponentHandle;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use smithay_client_toolkit::shell::WaylandSurface;
use std::any::Any;
use std::rc::Rc;
use std::time::Duration;

const NAME: &str = "spotlight";
pub const NS: &str = "tessera:spotlight";

/// The window's own messages (Search's are search::Msg)
enum W {
    Dismissed,
    Closed,
    /// make the window ahead, so the first open draws at once
    Prepare,
}

struct Win {
    output: String,
    surface: Rc<Surface>,
    ui: ui::SpotlightPanel,
    view: Rc<SearchUi>,
    a11y: Option<crate::a11y::A11y>,
    glass_sent: String,
}

pub struct Spotlight {
    win: Option<Win>,
    open: bool,
    search: Search,
    prep: slint::Timer,
}

impl Default for Spotlight {
    fn default() -> Self {
        Self { win: None, open: false, search: Search::new(NAME), prep: slint::Timer::default() }
    }
}

impl Spotlight {
    fn send(st: &State) -> impl Fn(Box<dyn Any + Send>) + Clone + 'static {
        st.shell.sender(NAME)
    }

    /// The window on this output (made the first time; kept while closed)
    fn window(&mut self, st: &mut State, output: &str) -> bool {
        if self.win.as_ref().is_some_and(|w| w.output != output) {
            let w = self.win.take().unwrap();
            self.search.attach(st, None);
            Glass::clear_now(&w.output, NS);
            st.drop_surface(&w.surface);
        }
        if self.win.is_some() {
            return true;
        }
        let Some(wl_output) = st.output_named(output) else { return false };
        let surface = st.prepare_surface(Spec {
            layer: Layer::Overlay,
            namespace: NS,
            output: Some((&wl_output, output)),
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            size: (0, 0),
            exclusive: -1,
            keyboard: KeyboardInteractivity::None,
            margin_left: 0,
        });
        let ui = ui::SpotlightPanel::new().expect("SpotlightPanel");
        st.shell.apply_theme(&ui.global::<ui::Theme>());
        let view = Rc::new(SearchUi::new(&ui));
        {
            let ctx = self.search.key_ctx();
            view.wire(self.search.sender(st), Rc::new(move |text: &str, shift: bool, ctrl: bool| search::handles(ctx.get(), text, shift, ctrl)));
        }
        {
            let to = Self::send(st);
            ui.on_dismissed(move || to(Box::new(W::Dismissed)));
        }
        {
            let to = Self::send(st);
            ui.on_closed(move || to(Box::new(W::Closed)));
        }
        ui.show().expect("show spotlight");
        let a11y = crate::a11y::A11y::new(&surface, st.tx.clone());
        self.win = Some(Win { output: output.into(), surface, ui, view: view.clone(), a11y, glass_sent: String::new() });
        self.search.attach(st, Some(view as Rc<dyn View>));
        true
    }

    fn open(&mut self, st: &mut State, screen: Option<String>) {
        let output = screen.or_else(|| self.win.as_ref().map(|w| w.output.clone())).or_else(|| st.outputs_now().first().map(|o| o.1.clone()));
        let Some(output) = output else { return };
        if !self.window(st, &output) {
            return;
        }
        self.open = true;
        self.search.activate(st);
        let w = self.win.as_ref().unwrap();
        w.view.open();
        w.view.set_list_y(0.0);
        w.ui.set_shown(true);
        w.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        w.surface.show(0, 0);
    }

    /// Close it (`tell`: it closed itself, so the shell's Panels follows)
    fn close(&mut self, tell: bool) {
        if !self.open {
            return;
        }
        self.open = false;
        self.search.deactivate();
        if let Some(w) = &self.win {
            w.ui.set_shown(false);
            w.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            w.surface.layer.commit();
        }
        if tell {
            crate::shell::closed(NAME);
        }
    }

    fn closed(&mut self, st: &mut State) {
        if self.open {
            return;
        }
        if let Some(w) = &mut self.win {
            w.surface.hide();
            st.shell.glass.set(&w.output, NS, &[]);
            w.glass_sent.clear();
        }
    }

    /// Search closed itself (a row run, Esc, a link opened)
    fn after_search(&mut self) {
        if self.search.take_done() {
            self.close(true);
        }
    }
}

impl Part for Spotlight {
    // (the window it keeps while closed, made ahead and left unmapped)
    fn prewarm(&mut self, st: &mut State) {
        let Some(out) = st.likely_output() else { return };
        if self.win.is_none() && !self.open && self.window(st, &out) {
            if let Some(w) = &self.win {
                w.surface.hide();
            }
        }
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn start(&mut self, st: &mut State) {
        self.search.start(st);
        let to = Self::send(st);
        self.prep.start(slint::TimerMode::SingleShot, Duration::from_millis(1500), move || to(Box::new(W::Prepare)));
    }

    fn command(&mut self, st: &mut State, cmd: &Command) {
        match cmd.cmd.as_str() {
            "open" => {
                self.open(st, cmd.screen.clone());
                // (with text typed in: `panels.search`, `tessera-ui ipc call panels search "…"`)
                if self.open {
                    if let Some(q) = cmd.args["query"].as_str().filter(|q| !q.is_empty()) {
                        if let Some(w) = &self.win {
                            w.view.set_text_end(q);
                        }
                        self.search.edited(st, q.to_string());
                    }
                }
            }
            "close" => self.close(false),
            _ => {}
        }
    }

    fn config_changed(&mut self, st: &mut State, look: bool) {
        if let Some(w) = &mut self.win {
            if look {
                st.shell.apply_theme(&w.ui.global::<ui::Theme>());
                w.glass_sent.clear();
            }
        }
        self.search.config_changed(st);
    }

    fn plugin_changed(&mut self, st: &mut State) {
        if let Some(w) = &mut self.win {
            st.shell.apply_theme(&w.ui.global::<ui::Theme>());
            w.glass_sent.clear();
        }
        self.search.plugin_changed(st);
    }

    fn data(&mut self, st: &mut State, what: std::any::TypeId) {
        self.search.data(st, what);
    }

    fn message(&mut self, st: &mut State, msg: Box<dyn Any + Send>) {
        // A line from the shell: its rows for a query, the AI's progress
        let msg = match msg.downcast::<serde_json::Value>() {
            Ok(v) => {
                self.search.shell_line(st, &v);
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<search::Msg>() {
            Ok(m) => {
                self.search.message(st, *m);
                self.after_search();
                return;
            }
            Err(m) => m,
        };
        let Ok(m) = msg.downcast::<W>() else { return };
        match *m {
            W::Dismissed => self.close(true),
            W::Closed => self.closed(st),
            W::Prepare => {
                // (made, then hidden at once: left shown, its full-screen
                // layer took every click on that screen)
                if self.win.is_none() && !self.open {
                    if let Some(o) = st.outputs_now().first().map(|o| o.1.clone()) {
                        if self.window(st, &o) {
                            if let Some(w) = &self.win {
                                w.surface.hide();
                            }
                        }
                    }
                }
            }
        }
    }

    fn keyboard_left(&mut self, _st: &mut State, s: &Rc<Surface>) {
        if self.open && self.win.as_ref().is_some_and(|w| Rc::ptr_eq(&w.surface, s)) {
            self.close(true);
        }
    }

    fn output_removed(&mut self, st: &mut State, name: &str) {
        if self.win.as_ref().is_some_and(|w| w.output == name) {
            let was = self.open;
            self.close(was);
            let w = self.win.take().unwrap();
            self.search.attach(st, None);
            st.drop_surface(&w.surface);
        }
    }

    fn surface_closed(&mut self, st: &mut State, s: &Rc<Surface>) {
        if self.win.as_ref().is_some_and(|w| Rc::ptr_eq(&w.surface, s)) {
            let name = s.output.clone();
            self.output_removed(st, &name);
        }
    }

    /// Glass: the field's and the results' glass for the plugin, as they
    /// move; the accessibility tree
    fn after_frame(&mut self, st: &mut State) {
        let kbd = st.keyboard_surface();
        let Some(w) = &mut self.win else { return };
        let f = kbd.as_ref().is_some_and(|k| Rc::ptr_eq(k, &w.surface));
        if let Some(a) = w.a11y.as_mut() {
            a.update(false, f);
        }
        if !w.surface.mapped.get() || !st.shell.plugin || st.shell.look.material {
            return;
        }
        let o = w.ui.get_openness().clamp(0.0, 1.0);
        let shapes = search::glass_shapes(&*w.view, &st.shell.look);
        let key = tessera_core::theme::glass_key(&shapes);
        if key != w.glass_sent {
            st.shell.glass.set(&w.output, NS, if o > 0.002 { &shapes } else { &[] });
            w.glass_sent = key;
        }
    }

    fn a11y(&mut self) -> Vec<&mut crate::a11y::A11y> {
        self.win.as_mut().and_then(|w| w.a11y.as_mut()).into_iter().collect()
    }

    fn stats(&self) {
        if let Some(w) = &self.win {
            eprintln!("tessera-ui: stats {}", w.surface.stats.borrow_mut().report("spotlight"));
        }
    }

    fn quit(&mut self, _: &mut State) {
        self.search.quit();
        if let Some(w) = &self.win {
            Glass::clear_now(&w.output, NS);
        }
    }
}
