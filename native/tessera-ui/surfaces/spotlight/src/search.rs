//! Search itself, shared (27 Nov): what Search's window (lib.rs) and the
//! Overview (surfaces/overview) both draw and run. One `Search` per host:
//! the text, the ranking on the rank lane, the slow answers (sums, units,
//! the dictionary, files), the shell's rows, the inline AI answer, the live
//! web results, the icons and pictures, the keys and running a row. The
//! host owns its window and surface; Search draws through a `View` (the
//! host's window, by `search_view!`, over ui/search.slint's SearchView and
//! its SearchState global) and asks the host to close when it closed
//! itself (`take_done`: a row run, Esc, a web link opened).
//!
//! Its messages (`Msg`) go to the host's part (`Search::new(part)`), which
//! hands them back with `message`; the shell's lines for Search
//! (`{"to":"spotlight",…}`: its rows, the AI's progress) reach whichever
//! host asked last (tessera_core::search_route) and come in by `shell_line`.
//!
//! Slint makes each crate's structs and globals its own types, so the view
//! is a trait with plain Rust values (`RowView`, `Glyph`, `Pushed`…), and
//! `search_view!` writes its one implementation into each host crate, over
//! that crate's `ui` module.

use crate::layout::{self, Entry};
use crate::{providers, reading, web};
use slint::platform::Key;
use slint::{Color, Image, SharedPixelBuffer, SharedString};
use std::any::Any;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tessera_core::appicon::{self, Style};
use tessera_core::rank::{self, AiMeta, Answer, Definition, Extras, Known, Row, Run};
use tessera_core::services::apps::Apps;
use tessera_core::services::icons::Icons;
use tessera_core::theme::{self, GlassShape, Look};
use tessera_core::wayland::State;

/// A picture made off the UI thread (an icon, a web results page)
pub type Px = SharedPixelBuffer<slint::Rgba8Pixel>;

// ── The view ────────────────────────────────────────────────────────────────

/// A glyph as Icon.qml picks it (SGlyph)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    pub text: SharedString,
    pub family: SharedString,
    pub scale: f32,
}

/// One result, laid out (SRow)
#[derive(Clone, Default)]
pub struct RowView {
    pub y: f32,
    pub h: f32,
    pub head: SharedString,
    pub head_y: f32,
    pub row_y: f32,
    pub row_h: f32,
    pub card: bool,
    pub first: bool,
    pub last: bool,
    pub index: i32,
    pub section: SharedString,
    pub title: SharedString,
    pub sub: SharedString,
    pub glyph: Glyph,
    pub glyph_fill: Glyph,
    pub icon: Image,
    pub has_icon: bool,
    pub emoji: SharedString,
    pub picture: Image,
    pub has_picture: bool,
    pub has_path: bool,
    pub loading: bool,
    pub store: bool,
    pub tone: Color,
    pub hero_kind: i32,
    pub key: Glyph,
    pub key_label: SharedString,
}

/// What follows the look and settings
pub struct LookView {
    pub g_search: Glyph,
    pub g_clear: Glyph,
    pub g_ask: Glyph,
    pub g_ask_on: Glyph,
    pub g_back: Glyph,
    pub g_eye: Glyph,
    pub g_spark: Glyph,
    pub g_globe: Glyph,
    pub cookie9: SharedString,
    pub cookie12: SharedString,
    pub sunny: SharedString,
    pub emoji_font: SharedString,
    /// Material's roles the shared Theme lacks (Glass leaves them)
    pub surface_bright: Option<Color>,
    pub on_tertiary: Option<Color>,
    pub accent_tint: Color,
    pub accent_ink: Color,
    pub scrim: Color,
    pub glass_fill: Color,
    pub placeholder: SharedString,
}

/// The selection, the answer, the field's buttons and the web results
pub struct Pushed {
    pub sel: i32,
    pub answering: bool,
    pub button_focus: i32,
    pub quick_busy: bool,
    pub quick_shown: bool,
    pub quick_error: SharedString,
    pub quick_answer: slint::StyledText,
    pub quick_done: bool,
    pub model_name: SharedString,
    pub ask_label: SharedString,
    pub web_shown: bool,
    pub web_loading: bool,
    pub web_name: SharedString,
    pub web_mode: i32,
    pub web_title: SharedString,
    /// the picture and its height / width
    pub web_shot: Option<(Image, f32)>,
    /// the arrow keys' result, as fractions of the picture (x, y, w, h)
    pub web_hl: Option<(f32, f32, f32, f32)>,
    pub web_notice: bool,
}

/// The Theme's colours the rows are drawn with
#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    pub accent: Color,
    pub m3_scrim: Color,
    pub m3_primary_container: Color,
    pub m3_on_primary_container: Color,
}

/// Where Glass's panels are now (the plugin's glass follows them)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlassGeo {
    /// the field's and the panels' opacity
    pub alpha: f32,
    pub body_alpha: f32,
    pub x: f32,
    pub top: f32,
    pub w: f32,
    /// the results' (or answer's) panel, 64 px under the top; 0 none
    pub body_h: f32,
    /// the web results' panel, from the top
    pub web_y: f32,
    pub web_h: f32,
}

/// A window that draws Search (ui/search.slint's SearchView with its
/// SearchState): written into each host crate by `search_view!`
pub trait View {
    /// Its callbacks to `to` (the host's part) as `Msg`; the field's keys
    /// go only when `keys` says they're taken
    fn wire(&self, to: Rc<dyn Fn(Box<dyn Any + Send>)>, keys: Rc<dyn Fn(&str, bool, bool) -> bool>);
    fn theme(&self) -> ThemeColors;
    fn scale(&self) -> f32;
    fn set_look(&self, l: LookView);
    fn set_placeholder(&self, p: &str);
    fn set_glass_fill(&self, c: Color);
    fn set_rows(&self, rows: Vec<RowView>, content_h: f32);
    /// An icon or picture that arrived, into row `i` if it has none yet
    fn update_row(&self, i: usize, icon: Option<&Image>, picture: Option<&Image>);
    fn push(&self, p: Pushed);
    fn list_y(&self) -> f32;
    fn set_list_y(&self, y: f32);
    fn list_max(&self) -> f32;
    fn set_introducing(&self, on: bool);
    fn start_intro(&self);
    /// the field emptied and focused
    fn open(&self);
    fn set_text(&self, t: &str);
    /// the text, the caret after it
    fn set_text_end(&self, t: &str);
    fn focus(&self);
    fn glass(&self) -> GlassGeo;
}

/// Glass's field and panels on the plugin's glass, as they are now
pub fn glass_shapes(v: &dyn View, look: &Look) -> Vec<GlassShape> {
    let g = v.glass();
    let o = g.alpha.clamp(0.0, 1.0);
    let ob = o * g.body_alpha.clamp(0.0, 1.0);
    let mut shapes = vec![theme::glass_shape(look, g.x, g.top, g.w, 56.0, 28.0, o, 1.0)];
    if g.body_h > 0.0 {
        shapes.push(theme::glass_shape(look, g.x, g.top + 64.0, g.w, g.body_h, 24.0, ob, 1.0));
    }
    // (the live web results' panel, under them)
    if g.web_h > 0.0 {
        shapes.push(theme::glass_shape(look, g.x, g.top + g.web_y, g.w, g.web_h, 24.0, ob, 1.0));
    }
    shapes
}

/// This crate's window as Search's `View`, over its `ui` module (each crate's
/// Slint structs and globals are its own types). At a crate's root, after
/// `pub mod ui`: `tessera_spotlight::search_view!(ui, SpotlightPanel);` makes
/// `SearchUi`. The window has SearchView as `sv` and forwards its parts as
/// spotlight.slint's SpotlightPanel does (search-list-y, search-g-x…,
/// search-open()…); main.slint exports SearchState, SRow and SGlyph.
#[macro_export]
macro_rules! search_view {
    ($ui:ident, $comp:ident) => {
        mod __search_ui {
            use super::$ui;
            use $crate::__slint::ComponentHandle as _;
            use $crate::search::{Glyph, GlassGeo, LookView, Msg, Pushed, RowView, ThemeColors};
            use std::any::Any;
            use std::rc::Rc;

            fn g(x: &Glyph) -> $ui::SGlyph {
                $ui::SGlyph { text: x.text.clone(), family: x.family.clone(), scale: x.scale }
            }

            /// This window as Search's view (tessera_spotlight::search::View)
            pub struct SearchUi {
                comp: $crate::__slint::Weak<$ui::$comp>,
                rows: $crate::__core::rows::Rows<$ui::SRow>,
            }

            impl SearchUi {
                pub fn new(c: &$ui::$comp) -> Self {
                    let rows = $crate::__core::rows::Rows::new();
                    c.global::<$ui::SearchState>().set_rows(rows.model());
                    Self { comp: c.as_weak(), rows }
                }
            }

            impl $crate::search::View for SearchUi {
                fn wire(&self, to: Rc<dyn Fn(Box<dyn Any + Send>)>, keys: Rc<dyn Fn(&str, bool, bool) -> bool>) {
                    let Some(c) = self.comp.upgrade() else { return };
                    let s = c.global::<$ui::SearchState>();
                    {
                        let to = to.clone();
                        s.on_key(move |text, shift, ctrl| {
                            let ok = keys(&text, shift, ctrl);
                            if ok {
                                to(Box::new(Msg::Key(text.to_string(), shift, ctrl)));
                            }
                            ok
                        });
                    }
                    {
                        let to = to.clone();
                        s.on_edited(move |t| to(Box::new(Msg::Edited(t.to_string()))));
                    }
                    {
                        let to = to.clone();
                        s.on_activate(move |i| to(Box::new(Msg::Activate(i))));
                    }
                    {
                        let to = to.clone();
                        s.on_quick_look(move |i| to(Box::new(Msg::Look(i))));
                    }
                    {
                        let to = to.clone();
                        s.on_cleared(move || to(Box::new(Msg::Cleared)));
                    }
                    {
                        let to = to.clone();
                        s.on_ask(move || to(Box::new(Msg::Ask)));
                    }
                    {
                        let to = to.clone();
                        s.on_back(move || to(Box::new(Msg::Back)));
                    }
                    {
                        let to = to.clone();
                        s.on_continue_chat(move || to(Box::new(Msg::Continue)));
                    }
                    {
                        let to = to.clone();
                        s.on_copy_answer(move || to(Box::new(Msg::CopyAnswer)));
                    }
                    {
                        let to = to.clone();
                        s.on_link(move |l| to(Box::new(Msg::Link(l.to_string()))));
                    }
                    {
                        let to = to.clone();
                        s.on_web_click(move |x, y| to(Box::new(Msg::WebClick(x, y))));
                    }
                    {
                        let to = to.clone();
                        s.on_web_expand(move || to(Box::new(Msg::WebExpand)));
                    }
                    {
                        let to = to.clone();
                        s.on_web_browser(move || to(Box::new(Msg::WebBrowser)));
                    }
                    s.on_web_notice_off(move || to(Box::new(Msg::WebNoticeOff)));
                }

                fn theme(&self) -> ThemeColors {
                    let Some(c) = self.comp.upgrade() else {
                        let k = $crate::__slint::Color::default();
                        return ThemeColors { accent: k, m3_scrim: k, m3_primary_container: k, m3_on_primary_container: k };
                    };
                    let t = c.global::<$ui::Theme>();
                    ThemeColors { accent: t.get_accent(), m3_scrim: t.get_m3_scrim(), m3_primary_container: t.get_m3_primary_container(), m3_on_primary_container: t.get_m3_on_primary_container() }
                }

                fn scale(&self) -> f32 {
                    self.comp.upgrade().map_or(1.0, |c| c.window().scale_factor())
                }

                fn set_look(&self, l: LookView) {
                    let Some(c) = self.comp.upgrade() else { return };
                    let s = c.global::<$ui::SearchState>();
                    s.set_g_search(g(&l.g_search));
                    s.set_g_clear(g(&l.g_clear));
                    s.set_g_ask(g(&l.g_ask));
                    s.set_g_ask_on(g(&l.g_ask_on));
                    s.set_g_back(g(&l.g_back));
                    s.set_g_eye(g(&l.g_eye));
                    s.set_g_spark(g(&l.g_spark));
                    s.set_g_globe(g(&l.g_globe));
                    s.set_cookie9(l.cookie9);
                    s.set_cookie12(l.cookie12);
                    s.set_sunny(l.sunny);
                    s.set_emoji_font(l.emoji_font);
                    if let Some(x) = l.surface_bright {
                        s.set_surface_bright(x);
                    }
                    if let Some(x) = l.on_tertiary {
                        s.set_on_tertiary(x);
                    }
                    s.set_accent_tint(l.accent_tint);
                    s.set_accent_ink(l.accent_ink);
                    s.set_scrim(l.scrim);
                    s.set_glass_fill(l.glass_fill);
                    s.set_placeholder(l.placeholder);
                }

                fn set_placeholder(&self, p: &str) {
                    if let Some(c) = self.comp.upgrade() {
                        c.global::<$ui::SearchState>().set_placeholder(p.into());
                    }
                }

                fn set_glass_fill(&self, x: $crate::__slint::Color) {
                    if let Some(c) = self.comp.upgrade() {
                        c.global::<$ui::SearchState>().set_glass_fill(x);
                    }
                }

                fn set_rows(&self, rows: Vec<RowView>, content_h: f32) {
                    let out = rows
                        .into_iter()
                        .map(|r| $ui::SRow {
                            y: r.y,
                            h: r.h,
                            head: r.head,
                            head_y: r.head_y,
                            row_y: r.row_y,
                            row_h: r.row_h,
                            card: r.card,
                            first: r.first,
                            last: r.last,
                            index: r.index,
                            section: r.section,
                            title: r.title,
                            sub: r.sub,
                            glyph: g(&r.glyph),
                            glyph_fill: g(&r.glyph_fill),
                            icon: r.icon,
                            has_icon: r.has_icon,
                            emoji: r.emoji,
                            picture: r.picture,
                            has_picture: r.has_picture,
                            has_path: r.has_path,
                            loading: r.loading,
                            store: r.store,
                            tone: r.tone,
                            hero_kind: r.hero_kind,
                            key: g(&r.key),
                            key_label: r.key_label,
                        })
                        .collect();
                    self.rows.set(out);
                    if let Some(c) = self.comp.upgrade() {
                        c.global::<$ui::SearchState>().set_content_h(content_h);
                    }
                }

                fn update_row(&self, i: usize, icon: Option<&$crate::__slint::Image>, picture: Option<&$crate::__slint::Image>) {
                    let Some(mut row) = self.rows.get(i) else { return };
                    let mut changed = false;
                    if let Some(img) = icon {
                        if row.icon.size().width == 0 {
                            row.icon = img.clone();
                            changed = true;
                        }
                    }
                    if let Some(p) = picture {
                        if !row.has_picture {
                            row.picture = p.clone();
                            row.has_picture = true;
                            changed = true;
                        }
                    }
                    if changed {
                        let mut all: Vec<$ui::SRow> = (0..self.rows.len()).filter_map(|j| self.rows.get(j)).collect();
                        all[i] = row;
                        self.rows.set(all);
                    }
                }

                fn push(&self, p: Pushed) {
                    let Some(c) = self.comp.upgrade() else { return };
                    let s = c.global::<$ui::SearchState>();
                    s.set_sel(p.sel);
                    s.set_answering(p.answering);
                    s.set_button_focus(p.button_focus);
                    s.set_quick_busy(p.quick_busy);
                    s.set_quick_shown(p.quick_shown);
                    s.set_quick_error(p.quick_error);
                    s.set_quick_answer(p.quick_answer);
                    s.set_quick_done(p.quick_done);
                    s.set_model_name(p.model_name);
                    s.set_ask_label(p.ask_label);
                    s.set_web_shown(p.web_shown);
                    s.set_web_loading(p.web_loading);
                    s.set_web_name(p.web_name);
                    s.set_web_mode(p.web_mode);
                    s.set_web_title(p.web_title);
                    match p.web_shot {
                        Some((img, aspect)) => {
                            s.set_web_shot(img);
                            s.set_web_has_shot(true);
                            s.set_web_aspect(aspect);
                        }
                        None => {
                            s.set_web_has_shot(false);
                            s.set_web_shot($crate::__slint::Image::default());
                            s.set_web_aspect(0.0);
                        }
                    }
                    s.set_web_hl(p.web_hl.is_some());
                    if let Some((x, y, w, h)) = p.web_hl {
                        s.set_web_hl_x(x);
                        s.set_web_hl_y(y);
                        s.set_web_hl_w(w);
                        s.set_web_hl_h(h);
                    }
                    s.set_web_notice(p.web_notice);
                }

                fn list_y(&self) -> f32 {
                    self.comp.upgrade().map_or(0.0, |c| c.get_search_list_y())
                }

                fn set_list_y(&self, y: f32) {
                    if let Some(c) = self.comp.upgrade() {
                        c.set_search_list_y(y);
                    }
                }

                fn list_max(&self) -> f32 {
                    self.comp.upgrade().map_or(0.0, |c| c.get_search_list_max())
                }

                fn set_introducing(&self, on: bool) {
                    if let Some(c) = self.comp.upgrade() {
                        c.global::<$ui::SearchState>().set_introducing(on);
                    }
                }

                fn start_intro(&self) {
                    if let Some(c) = self.comp.upgrade() {
                        c.invoke_search_start_intro();
                    }
                }

                fn open(&self) {
                    if let Some(c) = self.comp.upgrade() {
                        c.invoke_search_open();
                    }
                }

                fn set_text(&self, t: &str) {
                    if let Some(c) = self.comp.upgrade() {
                        c.invoke_search_set_text(t.into());
                    }
                }

                fn set_text_end(&self, t: &str) {
                    if let Some(c) = self.comp.upgrade() {
                        c.invoke_search_set_text_end(t.into());
                    }
                }

                fn focus(&self) {
                    if let Some(c) = self.comp.upgrade() {
                        c.invoke_search_focus();
                    }
                }

                fn glass(&self) -> GlassGeo {
                    let Some(c) = self.comp.upgrade() else { return GlassGeo::default() };
                    GlassGeo {
                        alpha: c.get_search_g_alpha(),
                        body_alpha: c.get_search_g_body_alpha(),
                        x: c.get_search_g_x(),
                        top: c.get_search_g_top(),
                        w: c.get_search_g_w(),
                        body_h: c.get_search_g_body_h(),
                        web_y: c.get_search_g_web_y(),
                        web_h: c.get_search_g_web_h(),
                    }
                }
            }
        }
        pub use __search_ui::SearchUi;
    };
}

// ── Messages ────────────────────────────────────────────────────────────────

/// Search's own messages, to its host's part and back by `Search::message`
pub enum Msg {
    Key(String, bool, bool),
    Edited(String),
    Activate(i32),
    Look(i32),
    Cleared,
    Ask,
    Back,
    Continue,
    CopyAnswer,
    Link(String),
    AskAnswer,
    Answered(u64, Answer),
    Converted(String, String),
    Defined(String, Definition),
    FindFiles,
    Files(String, Vec<(bool, String)>),
    Contents(String, Vec<String>),
    Hold,
    IntroEnd,
    Idle,
    Icons(Vec<(IconKey, Option<Px>)>),
    Picture(String, Option<Px>),
    Fetched(String, Option<PathBuf>),
    /// a ranking from the rank lane (Search::refresh), by its number
    Ranked(u64, Box<Ranked>),
    /// the web results: typing paused (their number), a picture came (or
    /// None: it couldn't be drawn), the render ended
    WebGo(u64),
    WebShot(u64, Option<Px>, Vec<web::Link>),
    WebEnded(u64),
    /// Google's robot check instead of results (no picture of it)
    WebBlocked(u64),
    /// a click on the picture (fractions of it), its buttons, the note's "Turn off"
    WebClick(f32, f32),
    WebExpand,
    WebBrowser,
    WebNoticeOff,
}

// ── Search's live web results now (src/web.rs) ──────────────────────────────

#[derive(Default)]
struct Web {
    /// the setting on and tessera-web there (read at each opening)
    on: bool,
    engine: &'static str,
    /// the words they're for, and their results page
    query: String,
    url: String,
    /// this query's number: anything for an older one is dropped
    gen: u64,
    /// being drawn (the shimmer)
    loading: bool,
    /// the picture (its pixels' width and height) and its links
    shot: Option<(slint::Image, u32, u32)>,
    links: Vec<web::Link>,
    /// the links the arrow keys go through, and the one they're on
    results: Vec<usize>,
    sel: Option<usize>,
    /// the one-time note: still to be seen, and shown at this opening
    notice: bool,
    noticed: bool,
    renderer: web::Renderer,
    /// Google's robot check came instead (the row with Check in browser)
    blocked: bool,
    /// no picture while backing off from Google (the plain row)
    plain: bool,
    /// when the robot check last came (unix seconds): web::backed_off
    blocked_at: Option<u64>,
    /// when the last searches went out (unix ms, oldest first): web::wait_ms
    starts: Vec<u64>,
}

/// The state key of the one-time note under the web results
const WEB_NOTICE_KEY: &str = "searchWebNotice";

// ── Ranking ─────────────────────────────────────────────────────────────────

/// What ranking reads, owned: a keystroke's rows are worked out on the rank
/// lane (tessera_core::work), not the UI thread, which only applies them
pub(crate) struct Inputs {
    pub(crate) text: String,
    pub(crate) apps: Arc<Vec<rank::App>>,
    /// Search.qml's actions, then the Chromecasts'
    pub(crate) actions: Arc<Vec<rank::Action>>,
    pub(crate) answer: Option<Answer>,
    pub(crate) qalc: bool,
    pub(crate) conversion: Option<String>,
    pub(crate) definition: Option<Definition>,
    pub(crate) extras: Option<Extras>,
    pub(crate) ai: AiMeta,
    pub(crate) files: Vec<(bool, String)>,
    pub(crate) contents: Vec<String>,
    pub(crate) home: String,
    pub(crate) material: bool,
    /// the web search engine (the web row's address)
    pub(crate) engine: &'static str,
    /// the reading list (reading.rs)
    pub(crate) reading: Vec<tessera_core::readlist::Item>,
}

/// The rows for one ranking, laid out
pub struct Ranked {
    pub(crate) rows: Vec<Row>,
    pub(crate) wants: rank::Wants,
    have_extras: bool,
    pub(crate) entries: Vec<Entry>,
    pub(crate) content_h: f32,
    material: bool,
}

impl Inputs {
    /// rank::query and the layout (pure: any thread)
    pub(crate) fn rank(&self) -> Ranked {
        let raw = self.text.trim();
        let k = Known {
            apps: &self.apps,
            actions: &self.actions,
            answer: self.answer.as_ref(),
            qalc: self.qalc,
            conversion: self.conversion.as_deref(),
            definition: self.definition.as_ref(),
            extras: self.extras.as_ref(),
            ai: Some(&self.ai),
            emoji: if raw.starts_with(':') { providers::emoji() } else { &[] },
            files: &self.files,
            contents: &self.contents,
            home: &self.home,
            engine: self.engine,
        };
        let (mut rows, wants) = rank::query(&self.text, &k);
        reading::merge(&self.text, &self.reading, &mut rows, tessera_core::util::now_ms());
        let (entries, content_h) = if self.material { layout::material(&rows) } else { layout::glass(&rows) };
        Ranked { rows, wants, have_extras: self.extras.is_some(), entries, content_h, material: self.material }
    }
}

/// The lane keystrokes are ranked on: one thread, in order, so a ranking
/// is never overtaken by an older one; a ranking a newer keystroke has
/// replaced is skipped before it starts (and dropped if it finishes late).
/// (Shared by every host: each ranking checks its own Search's number.)
static RANK: tessera_core::work::Lane = tessera_core::work::Lane::new("rank");

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IconKey {
    path: PathBuf,
    px: u32,
    style: Style,
}

// ── Keys ────────────────────────────────────────────────────────────────────

/// What the field's key callback has to know at once (Slint wants the answer
/// synchronously)
#[derive(Clone, Copy, Default)]
pub struct KeyCtx {
    pub material: bool,
    pub has_path: bool,
    pub focus: i32,
    /// the selected row is a reading-list link (Ctrl+D, Ctrl+Delete)
    pub reading: bool,
}

/// The keys Search takes from the field (the field gets the rest)
pub fn handles(c: KeyCtx, text: &str, shift: bool, ctrl: bool) -> bool {
    let is = |k: Key| text == SharedString::from(k).as_str();
    if is(Key::Escape) || is(Key::Return) || is(Key::UpArrow) || is(Key::DownArrow) || is(Key::Tab) {
        return true;
    }
    if is(Key::Backtab) {
        return true;
    }
    if c.focus != 0 && text == " " {
        return true;
    }
    if c.reading && ctrl && (text.eq_ignore_ascii_case("d") || is(Key::Delete)) {
        return true;
    }
    c.has_path && ((text == " " && shift) || (ctrl && text.eq_ignore_ascii_case("y")))
}

/// Glass's Tab and Shift+Tab: the field (0) → through the results, or to
/// Clear (1) and Ask (2), and back (SpotlightGlass.qml's KeyNavigation)
pub(crate) fn glass_tab(focus: i32, back: bool, rows: usize, has_text: bool) -> (i32, bool) {
    match (focus, back) {
        (_, true) if focus == 0 => (2, false),
        (2, true) => (if has_text { 1 } else { 0 }, false),
        (1, true) => (0, false),
        (0, false) if rows > 1 => (0, true), // (next result)
        (0, false) => (if has_text { 1 } else { 2 }, false),
        (1, false) => (2, false),
        _ => (0, false),
    }
}

// ── How rows look ───────────────────────────────────────────────────────────

/// Glass's answer card colour (AnswerCard.qml's tone)
pub(crate) fn tone(r: &Row, dark: bool, accent: Color) -> Color {
    let a = |light: &str, dk: &str| hexc(if dark { dk } else { light });
    if r.section == "Do" {
        return accent;
    }
    match r.kind.as_str() {
        "math" => return a("#FF9500", "#FF9F0A"),
        "date" => return a("#FF3B30", "#FF453A"),
        "time" => return a("#007AFF", "#0A84FF"),
        "unit" | "currency" => return a("#34C759", "#30D158"),
        _ => {}
    }
    match r.section.as_str() {
        "Calculator" => a("#FF9500", "#FF9F0A"),
        "Conversion" => a("#34C759", "#30D158"),
        "Dictionary" => a("#AF52DE", "#BF5AF2"),
        _ => accent,
    }
}

pub(crate) fn hexc(s: &str) -> Color {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// What Enter does with a card (AnswerCard.qml, SearchHero.qml)
fn card_hint(r: &Row) -> &'static str {
    match r.section.as_str() {
        "Top Hit" => "",
        "Do" => "Enter to do it",
        "Dictionary" => "Enter opens it",
        _ => "Enter copies it",
    }
}

/// The line under (or beside) a row's title, as each look writes it
pub fn sub_line(r: &Row, material: bool, card: bool) -> String {
    let store_row = if card { r.section == "Top Hit" && r.subtitle.starts_with("Get · ") } else { r.section == "Software" && !r.loading };
    let body = if material && store_row { r.subtitle.trim_start_matches("Get · ").to_string() } else { r.subtitle.clone() };
    if !card {
        return body;
    }
    [body.as_str(), card_hint(r)].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
}

/// The shell's rows, from its extras line
fn shell_rows(v: &serde_json::Value) -> Vec<Row> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
                    Row {
                        section: s("section"),
                        title: s("title"),
                        subtitle: s("subtitle"),
                        glyph: s("glyph"),
                        icon: s("icon"),
                        icon_fallback: "application-x-executable",
                        picture: s("picture"),
                        path: s("path"),
                        loading: r["loading"].as_bool().unwrap_or(false),
                        run: Run::Shell(s("key")),
                        ..Default::default()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_extras(v: &serde_json::Value) -> Extras {
    Extras {
        q: v["q"].as_str().unwrap_or("").trim().to_string(),
        intents: shell_rows(&v["intents"]),
        reminder: shell_rows(&v["reminder"]),
        store: shell_rows(&v["store"]),
        calendar: shell_rows(&v["calendar"]),
        contacts: shell_rows(&v["contacts"]),
        notes: shell_rows(&v["notes"]),
        shots: shell_rows(&v["shots"]),
        clipboard: shell_rows(&v["clipboard"]),
    }
}

/// Glass's panels: over the plugin's glass nothing (the plugin tints it, the
/// QML item draws only its shadow); without it, the frosted fallback's tint
/// (Theme.glass.frost) over Hyprland's blur
pub fn glass_fill(plugin: bool, dark: bool) -> Color {
    let c = |r: f32, g: f32, b: f32, a: f32| Color::from_argb_f32(a, r, g, b);
    match (plugin, dark) {
        (true, _) => c(0.0, 0.0, 0.0, 0.0),
        (false, true) => c(0.11, 0.11, 0.13, 0.62),
        (false, false) => c(0.97, 0.97, 0.98, 0.66),
    }
}

fn rgb(c: Color) -> [u8; 3] {
    [c.red(), c.green(), c.blue()]
}

fn mixc(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn glyph(name: &str, material: bool, fill: bool, bold: bool) -> Glyph {
    let (text, family, scale) = providers::glyph(name, material, fill, bold);
    Glyph { text: text.into(), family: family.into(), scale }
}

// ── Search ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
struct Quick {
    busy: bool,
    answer: String,
    error: String,
}

// (the timers, by job)
const T_ANSWER: usize = 0;
const T_CONV: usize = 1;
const T_DEF: usize = 2;
const T_FILES: usize = 3;
const T_HOLD: usize = 4;
const T_INTRO: usize = 5;
const T_IDLE: usize = 6;
const T_WEB: usize = 7;

pub struct Search {
    /// the host's part (its messages go there)
    part: &'static str,
    /// the window it draws in now
    view: Option<Rc<dyn View>>,
    /// shown and taking keys (Search open; the Overview searching)
    active: bool,
    /// it closed itself: the host closes (take_done)
    done: bool,
    text: String,
    rows: Vec<Row>,
    entries: Vec<Entry>,
    sel: usize,
    scroll: f32,
    focus: i32,
    key_ctx: Rc<Cell<KeyCtx>>,
    answering: bool,
    quick: Quick,
    ai: AiMeta,
    cast: Vec<rank::Action>,
    actions: Vec<rank::Action>,
    /// actions, then cast (handed to the rank lane as they are)
    all_actions: Arc<Vec<rank::Action>>,
    apps: Arc<Vec<rank::App>>,
    extras: Option<Extras>,
    /// the newest ranking asked for (the lane skips older ones), the
    /// newest applied, and whether one asked since was for typing (its
    /// selection and scroll start over)
    rank_gen: u64,
    rank_latest: Arc<AtomicU64>,
    rank_applied: u64,
    rank_typed: bool,
    asked: Option<(String, bool)>,
    hold_until: Option<Instant>,
    had_rows: bool,
    // slow answers
    answerer: providers::Answerer,
    ans_query: String,
    answer: Option<(String, Answer)>,
    conv_query: String,
    conv: Option<(String, String)>,
    def_word: String,
    def: Option<(String, Definition)>,
    file_query: String,
    files: Vec<(bool, String)>,
    contents: Vec<String>,
    qalc: bool,
    plocate: bool,
    rg: bool,
    home: String,
    /// the reading list's unread links, read as it opens (reading.rs)
    reading: Vec<tessera_core::readlist::Item>,
    timers: [slint::Timer; 8],
    // pictures
    icons: HashMap<IconKey, slint::Image>,
    /// each row's icon render
    row_keys: Vec<Option<IconKey>>,
    pending: HashSet<IconKey>,
    pictures: HashMap<String, Option<slint::Image>>,
    /// App Store logos on the web (AppStore.iconFor's URL), fetched once into the cache
    remote: HashMap<String, Option<PathBuf>>,
    /// the live web results under the rows
    web: Web,
}

impl Search {
    pub fn new(part: &'static str) -> Self {
        Self {
            part,
            view: None,
            active: false,
            done: false,
            text: String::new(),
            rows: Vec::new(),
            entries: Vec::new(),
            sel: 0,
            scroll: 0.0,
            focus: 0,
            key_ctx: Rc::new(Cell::new(KeyCtx::default())),
            answering: false,
            quick: Quick::default(),
            ai: AiMeta::default(),
            cast: Vec::new(),
            actions: rank::actions(),
            all_actions: Arc::new(rank::actions()),
            apps: Arc::new(Vec::new()),
            extras: None,
            rank_gen: 0,
            rank_latest: Arc::new(AtomicU64::new(0)),
            rank_applied: 0,
            rank_typed: false,
            asked: None,
            hold_until: None,
            had_rows: false,
            answerer: providers::Answerer::new(),
            ans_query: String::new(),
            answer: None,
            conv_query: String::new(),
            conv: None,
            def_word: String::new(),
            def: None,
            file_query: String::new(),
            files: Vec::new(),
            contents: Vec::new(),
            qalc: false,
            plocate: false,
            rg: false,
            home: std::env::var("HOME").unwrap_or_default(),
            reading: Vec::new(),
            timers: Default::default(),
            icons: HashMap::new(),
            row_keys: Vec::new(),
            pending: HashSet::new(),
            pictures: HashMap::new(),
            remote: HashMap::new(),
            web: Web::default(),
        }
    }

    fn send(&self, st: &State) -> impl Fn(Box<dyn Any + Send>) + Clone + 'static {
        st.shell.sender(self.part)
    }

    /// For a view's `wire`: messages to this host's part
    pub fn sender(&self, st: &State) -> Rc<dyn Fn(Box<dyn Any + Send>)> {
        Rc::new(self.send(st))
    }

    fn after(&self, st: &State, t: usize, ms: u64, m: impl Fn() -> Msg + 'static) {
        let to = self.send(st);
        self.timers[t].start(slint::TimerMode::SingleShot, Duration::from_millis(ms), move || to(Box::new(m())));
    }

    fn material(st: &State) -> bool {
        st.shell.look.material
    }

    /// The key context the field's callback reads (handles)
    pub fn key_ctx(&self) -> Rc<Cell<KeyCtx>> {
        self.key_ctx.clone()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// What's typed (as Search last heard it)
    pub fn text(&self) -> &str {
        &self.text
    }

    /// It closed itself (a row run, Esc, a link opened): the host closes now
    pub fn take_done(&mut self) -> bool {
        std::mem::take(&mut self.done)
    }

    /// Draw into this window from now (None: no window)
    pub fn attach(&mut self, st: &State, view: Option<Rc<dyn View>>) {
        let same = match (&self.view, &view) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.view = view;
        self.icons.clear();
        self.pending.clear();
        if self.view.is_some() {
            self.look(st);
        }
    }

    /// Everything that follows the look and settings, into the window
    pub fn look(&mut self, st: &State) {
        if let Some(v) = &self.view {
            Self::look_view(st, &**v);
        }
        self.icons.clear();
        self.pending.clear();
    }

    /// The look into any window that draws Search (the Overview's other
    /// displays show the field too)
    pub fn look_view(st: &State, v: &dyn View) {
        let m = Self::material(st);
        let look = &st.shell.look;
        let t = v.theme();
        let accent = t.accent;
        let (surface_bright, on_tertiary, accent_tint, accent_ink) = if m {
            let roles = theme::look_roles(look);
            let r = |k: &str| hexc(roles.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or("#ff00ff"));
            (Some(r("surfaceBright")), Some(r("onTertiary")), r("secondaryContainer"), r("primary"))
        } else {
            // (Theme.accentTint: the accent faintly in dark mode, a pale shade of it in light)
            let pale = Color::from_argb_u8(230, mixc(accent.red(), 255, 0.8), mixc(accent.green(), 255, 0.8), mixc(accent.blue(), 255, 0.8));
            (None, None, if look.dark { accent.with_alpha(0.26) } else { pale }, accent)
        };
        v.set_look(LookView {
            g_search: glyph("magnifying-glass", m, false, true),
            g_clear: glyph(if m { "x" } else { "x-circle" }, m, true, false),
            g_ask: glyph("sparkle", m, false, false),
            g_ask_on: glyph("sparkle", m, true, false),
            g_back: glyph("caret-left", m, false, false),
            g_eye: glyph("eye", m, false, false),
            g_spark: glyph("sparkle", false, true, false),
            g_globe: glyph("globe", m, false, false),
            cookie9: tessera_core::surfaces::shapes::outline("cookie9").into(),
            cookie12: tessera_core::surfaces::shapes::outline("cookie12").into(),
            sunny: tessera_core::surfaces::shapes::outline("sunny").into(),
            emoji_font: tessera_core::settings::emoji_font(&st.shell.cfg, &[]).into(),
            surface_bright,
            on_tertiary,
            accent_tint,
            accent_ink,
            scrim: t.m3_scrim,
            glass_fill: glass_fill(st.shell.plugin, look.dark),
            placeholder: if m { "Search apps, files and the web" } else { "Search" }.into(),
        });
    }

    /// The host started: what's on the system
    pub fn start(&mut self, st: &mut State) {
        st.shell.want("apps");
        st.shell.want("icons");
        self.qalc = tessera_core::util::on_path("qalc");
        self.plocate = tessera_core::util::on_path("plocate");
        self.rg = tessera_core::util::on_path("rg");
    }

    /// Shown: SearchModel.reset (no answer, an empty field, no results), the
    /// web results' setting, the reading list, the shell asked for its rows
    pub fn activate(&mut self, st: &mut State) {
        // (the last session's files go: they belong to its query)
        self.file_query.clear();
        self.files.clear();
        self.contents.clear();
        st.shell.want("apps");
        st.shell.want("icons");
        self.active = true;
        self.done = false;
        if self.answering || self.quick.busy {
            tessera_core::shell::ask("ai.stopQuick", serde_json::json!([]));
        }
        self.answering = false;
        self.quick = Quick::default();
        self.text.clear();
        self.rows.clear();
        self.entries.clear();
        self.sel = 0;
        self.scroll = 0.0;
        self.focus = 0;
        self.had_rows = false;
        self.asked = None;
        // (the web results: the setting, the engine, tessera-web there; the note once)
        self.web_stop();
        let cfg = &st.shell.cfg;
        self.web.on = tessera_core::weblink::search::web_results(cfg) && tessera_core::weblink::bin().is_some();
        self.web.engine = tessera_core::weblink::search::engine(cfg);
        self.web.notice = !tessera_core::cfgfile::state_get(WEB_NOTICE_KEY).as_bool().unwrap_or(false);
        self.web.noticed = false;
        // (a ranking still on its way is for the text before)
        self.drop_rankings();
        // (the reading list, read now: an empty field shows it)
        self.reading = tessera_core::readlist::unread();
        if !self.reading.is_empty() {
            self.refresh(st, false);
        }
        tessera_core::shell::ask("clipboard.refresh", serde_json::json!([]));
        // (the AI's name and the Chromecasts, before anything's typed)
        self.ask_extras(String::new(), false);
        self.push(st);
    }

    /// The window shows nothing of the last time (no rows, no answer, no
    /// web results): for a host that shows the field before it searches
    /// (the Overview, opening, or about to search again)
    pub fn reset_view(&mut self, st: &State) {
        if self.active {
            return;
        }
        self.rows.clear();
        self.entries.clear();
        self.sel = 0;
        self.focus = 0;
        self.answering = false;
        self.quick = Quick::default();
        self.web_clear();
        self.rebuild(st);
    }

    /// No ranking asked for before now is applied
    fn drop_rankings(&mut self) {
        self.rank_gen += 1;
        self.rank_latest.store(self.rank_gen, Ordering::SeqCst);
        self.rank_applied = self.rank_gen;
        self.rank_typed = false;
    }

    /// Hidden (the host closed, or the Overview's field emptied): what's
    /// under way stops. The window keeps what it shows (it fades or shrinks
    /// away as it is); nothing more is drawn into it until it's shown again.
    pub fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // (a quick answer still coming stops here, not at the next opening:
        // its tokens and the network were spent on an answer thrown away;
        // nor do the lookups for the text typed run after it's gone)
        if self.answering || self.quick.busy {
            tessera_core::shell::ask("ai.stopQuick", serde_json::json!([]));
        }
        self.answering = false;
        self.quick = Quick::default();
        // (the web results' render stops with it; the note, once seen, is done)
        self.web_stop();
        if self.web.noticed {
            self.web.notice = false;
            self.web.noticed = false;
            tessera_core::cfgfile::set_state(WEB_NOTICE_KEY, serde_json::json!(true));
        }
        // (a lookup stopped before it was asked is forgotten too: else the
        // same text typed again never asks it, as the query looks seen)
        for t in [T_ANSWER, T_CONV, T_DEF, T_FILES] {
            if self.timers[t].running() {
                match t {
                    T_ANSWER => self.ans_query.clear(),
                    T_CONV => self.conv_query.clear(),
                    T_DEF => self.def_word.clear(),
                    _ => self.file_query.clear(),
                }
            }
            self.timers[t].stop();
        }
        // (a ranking on its way is for what was shown: not drawn now)
        self.drop_rankings();
    }

    /// It closes itself: stops, and the host closes (take_done)
    fn close_self(&mut self) {
        self.deactivate();
        self.done = true;
    }

    fn ask_extras(&mut self, q: String, store: bool) {
        let want = (q, store);
        if self.asked.as_ref() != Some(&want) {
            // (the shell's answer comes back to this host)
            tessera_core::search_route::asked_by(self.part);
            tessera_core::shell::ask("search.query", serde_json::json!([want.0, want.1]));
            self.asked = Some(want);
        }
    }

    /// What ranking reads now, owned
    fn inputs(&self, st: &State) -> Inputs {
        let raw = self.text.trim().to_string();
        let word = rank::word_to_define(&raw);
        Inputs {
            text: self.text.clone(),
            apps: self.apps.clone(),
            actions: self.all_actions.clone(),
            answer: self.answer.as_ref().filter(|a| a.0 == raw).map(|a| a.1.clone()),
            qalc: self.qalc,
            conversion: self.conv.as_ref().filter(|c| c.0 == raw).map(|c| c.1.clone()),
            definition: self.def.as_ref().filter(|d| !word.is_empty() && d.0 == word).map(|d| d.1.clone()),
            extras: self.extras.as_ref().filter(|e| e.q == raw).cloned(),
            ai: self.ai.clone(),
            files: self.files.clone(),
            contents: self.contents.clone(),
            home: self.home.clone(),
            material: Self::material(st),
            engine: tessera_core::weblink::search::engine(&st.shell.cfg),
            reading: self.reading.clone(),
        }
    }

    /// The rows again, for the field's text and what's known: ranked and
    /// laid out on the rank lane (a keystroke under load cost its ranking
    /// on the UI thread before its frame, 17 Nov), applied when they come
    /// back if nothing newer was asked meanwhile
    fn refresh(&mut self, st: &State, typed: bool) {
        self.rank_gen += 1;
        self.rank_typed |= typed;
        let gen = self.rank_gen;
        let latest = self.rank_latest.clone();
        latest.store(gen, Ordering::SeqCst);
        let inputs = self.inputs(st);
        let to = self.send(st);
        RANK.run(move || {
            if latest.load(Ordering::SeqCst) != gen {
                return; // (another keystroke came: this one's rows would never show)
            }
            let r = inputs.rank();
            if latest.load(Ordering::SeqCst) == gen {
                to(Box::new(Msg::Ranked(gen, Box::new(r))));
            }
        });
    }

    /// A ranking still on the lane, worked out here and applied now: what
    /// Enter, the arrows and Tab act on is the rows for what's typed
    fn rank_now(&mut self, st: &State) {
        if self.rank_applied >= self.rank_gen {
            return;
        }
        let r = tessera_core::perf::time(|| "spotlight rank now".into(), || self.inputs(st).rank());
        self.ranked(st, self.rank_gen, r);
    }

    /// A ranking back from the lane (or rank_now): applied if it's the
    /// newest asked for and none as new was applied (never out of order)
    fn ranked(&mut self, st: &State, gen: u64, r: Ranked) {
        if gen != self.rank_gen || gen <= self.rank_applied {
            return;
        }
        self.rank_applied = gen;
        let typed = std::mem::take(&mut self.rank_typed);
        self.apply_ranked(st, typed, r);
    }

    fn apply_ranked(&mut self, st: &State, typed: bool, r: Ranked) {
        let raw = self.text.trim().to_string();
        let Ranked { rows, wants, have_extras, entries, content_h, material } = r;
        // What's still to ask, as Search.qml's debounces
        if let Some(q) = wants.answer {
            if q != self.ans_query {
                self.ans_query = q;
                self.answer = None;
                self.after(st, T_ANSWER, 90, || Msg::AskAnswer);
            }
        }
        if let Some(q) = wants.conversion {
            if q != self.conv_query {
                self.conv_query = q;
                self.conv = None;
                let q2 = self.conv_query.clone();
                let to = self.send(st);
                self.timers[T_CONV].start(slint::TimerMode::SingleShot, Duration::from_millis(180), move || {
                    let (q, to) = (q2.clone(), to.clone());
                    std::thread::spawn(move || to(Box::new(Msg::Converted(q.clone(), providers::convert(&q)))));
                });
            }
        }
        if let Some(wd) = wants.define {
            if wd != self.def_word {
                self.def_word = wd;
                self.def = None;
                let w2 = self.def_word.clone();
                let to = self.send(st);
                self.timers[T_DEF].start(slint::TimerMode::SingleShot, Duration::from_millis(300), move || {
                    let (w, to) = (w2.clone(), to.clone());
                    std::thread::spawn(move || to(Box::new(Msg::Defined(w.clone(), providers::define(&w)))));
                });
            }
        }
        if let Some(fq) = wants.files {
            if fq != self.file_query {
                self.file_query = fq;
                // (the last query's files never show under this one while
                // it's looked for: Enter opened them; audit #143)
                self.files.clear();
                self.contents.clear();
                self.after(st, T_FILES, 200, || Msg::FindFiles);
            }
        }
        if let Some((q, store)) = wants.extras {
            self.ask_extras(q, store);
        }
        // (while the shell's rows for new text are on their way, what's shown
        // stays a moment: no flicker of a list without them)
        if typed {
            self.hold_until = (!have_extras && self.asked.as_ref().is_some_and(|a| a.0 == raw) && !raw.is_empty()).then(|| Instant::now() + Duration::from_millis(120));
            if self.hold_until.is_some() {
                self.after(st, T_HOLD, 130, || Msg::Hold);
            }
        }
        if !have_extras && self.hold_until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.hold_until = None;
        self.sel = if typed { 0 } else { rank::keep_selection(&self.rows, self.sel, &rows) };
        self.rows = rows;
        if typed {
            self.scroll = 0.0;
            if let Some(v) = &self.view {
                v.set_list_y(0.0);
            }
        }
        // Material: a list that just appeared comes in row by row
        let has = !self.rows.is_empty();
        if has && !self.had_rows && Self::material(st) {
            if let Some(v) = &self.view {
                v.start_intro();
                v.set_introducing(true);
            }
            self.after(st, T_INTRO, 700, || Msg::IntroEnd);
        }
        self.had_rows = has;
        // (laid out on the lane, unless the look changed meanwhile)
        let laid = (material == Self::material(st)).then_some((entries, content_h));
        self.rebuild_laid(st, laid);
    }

    /// The rows into the window
    fn rebuild(&mut self, st: &State) {
        self.rebuild_laid(st, None);
    }

    /// The rows into the window, with their layout if it's known
    fn rebuild_laid(&mut self, st: &State, laid: Option<(Vec<Entry>, f32)>) {
        let m = Self::material(st);
        let (entries, h) = laid.unwrap_or_else(|| if m { layout::material(&self.rows) } else { layout::glass(&self.rows) });
        self.entries = entries;
        let Some(v) = self.view.clone() else { return };
        let t = v.theme();
        let accent = t.accent;
        let dark = st.shell.look.dark;
        let mut want_icons = Vec::new();
        let mut want_remote: Vec<String> = Vec::new();
        let mut row_keys = Vec::new();
        let mut want_pics = Vec::new();
        let icons = st.shell.get::<Icons>().cloned();
        let scale = v.scale().max(1.0);
        let style_plain = Style::from_settings(&serde_json::json!({"appearance": {"iconStyle": "plain", "iconShape": "none"}}), m, dark, [0, 0, 0], ([0, 0, 0], [0, 0, 0]));
        let style_app = {
            let themed = (rgb(t.m3_primary_container), rgb(t.m3_on_primary_container));
            Style::from_settings(&st.shell.cfg, m, dark, rgb(accent), themed)
        };
        let out: Vec<RowView> = self
            .rows
            .iter()
            .zip(&self.entries)
            .enumerate()
            .map(|(i, (r, e))| {
                // the icon: looked up in the theme, drawn as the look draws it
                let size = if m && e.card { 40.0 } else { 26.0 };
                let styled = !m && (r.section == "Applications" || (r.section == "Top Hit" && matches!(r.run, Run::App(_))));
                let mut icon = None;
                let mut has_icon = false;
                let mut row_key = None;
                let web = r.icon.starts_with("https://") || r.icon.starts_with("http://");
                if web && !r.loading && !self.remote.contains_key(&r.icon) {
                    want_remote.push(r.icon.clone());
                }
                let local = if web { self.remote.get(&r.icon).cloned().flatten() } else { None };
                if !r.icon.is_empty() && !r.loading && (!web || local.is_some()) {
                    let found = match local {
                        Some(p) => Some(p),
                        None => icons.as_ref().and_then(|ic| ic.path(&[r.icon.as_str()], &[r.icon_fallback])),
                    };
                    if let Some(p) = found {
                        let key = IconKey { path: p, px: (size * scale).ceil() as u32, style: if styled { style_app.clone() } else { style_plain.clone() } };
                        has_icon = true;
                        match self.icons.get(&key) {
                            Some(img) => icon = Some(img.clone()),
                            None => want_icons.push(key.clone()),
                        }
                        row_key = Some(key);
                    }
                }
                let mut picture = None;
                if !r.picture.is_empty() {
                    match self.pictures.get(&r.picture) {
                        Some(p) => picture = p.clone(),
                        None => want_pics.push(r.picture.clone()),
                    }
                }
                row_keys.push(row_key);
                let g = if r.glyph.is_empty() { if e.card { "sparkle" } else { "circle" } } else { r.glyph.as_str() };
                let (hero_kind, key, key_label) = match r.section.as_str() {
                    "Top Hit" => (0, "arrow-up-right", "Open"),
                    "Do" => (1, "arrow-up-right", "Open"),
                    "Dictionary" => (2, "arrow-square-out", "Open"),
                    _ => (2, "copy", "Copy"),
                };
                RowView {
                    y: e.y,
                    h: e.h,
                    head: e.head.as_str().into(),
                    head_y: e.head_y,
                    row_y: e.row_y,
                    row_h: e.row_h,
                    card: e.card,
                    first: e.first,
                    last: e.last,
                    index: i as i32,
                    section: r.section.as_str().into(),
                    title: r.title.as_str().into(),
                    sub: sub_line(r, m, e.card).into(),
                    glyph: glyph(g, m, false, false),
                    glyph_fill: glyph(g, m, true, false),
                    icon: icon.unwrap_or_default(),
                    has_icon,
                    emoji: r.emoji.as_str().into(),
                    picture: picture.clone().unwrap_or_default(),
                    has_picture: picture.is_some(),
                    has_path: !r.path.is_empty(),
                    loading: r.loading,
                    store: if e.card { r.section == "Top Hit" && r.subtitle.starts_with("Get · ") } else { m && r.section == "Software" && !r.loading },
                    tone: tone(r, dark, accent),
                    hero_kind,
                    key: glyph(key, m, false, false),
                    key_label: key_label.into(),
                }
            })
            .collect();
        self.row_keys = row_keys;
        v.set_rows(out, h);
        self.request_icons(st, want_icons);
        self.request_pictures(st, want_pics);
        for u in want_remote {
            self.remote.insert(u.clone(), None);
            let to = self.send(st);
            std::thread::spawn(move || {
                let p = providers::fetch_icon(&u);
                to(Box::new(Msg::Fetched(u, p)));
            });
        }
        self.push(st);
    }

    fn request_icons(&mut self, st: &State, want: Vec<IconKey>) {
        let missing: Vec<IconKey> = want.into_iter().filter(|k| !self.pending.contains(k)).collect::<HashSet<_>>().into_iter().collect();
        if missing.is_empty() {
            return;
        }
        self.pending.extend(missing.iter().cloned());
        let to = self.send(st);
        std::thread::spawn(move || {
            let out = missing.into_iter().map(|k| {
                let img = appicon::render(&k.path, k.px, &k.style, None);
                (k, img)
            });
            to(Box::new(Msg::Icons(out.collect())));
        });
    }

    fn request_pictures(&mut self, st: &State, want: Vec<String>) {
        for p in want {
            if self.pictures.contains_key(&p) {
                continue;
            }
            self.pictures.insert(p.clone(), None);
            let to = self.send(st);
            std::thread::spawn(move || {
                let px = image::open(&p).ok().map(|i| {
                    let t = i.resize_to_fill(96, 96, image::imageops::FilterType::Triangle).to_rgba8();
                    let mut b = Px::new(t.width(), t.height());
                    b.make_mut_bytes().copy_from_slice(t.as_raw());
                    b
                });
                to(Box::new(Msg::Picture(p, px)));
            });
        }
    }

    /// Icons and pictures that arrived: into the rows already shown
    fn update_images(&mut self) {
        let Some(v) = &self.view else { return };
        for (i, r) in self.rows.iter().enumerate() {
            let icon = self.row_keys.get(i).and_then(|k| k.as_ref()).and_then(|k| self.icons.get(k));
            let picture = if r.picture.is_empty() { None } else { self.pictures.get(&r.picture).and_then(|p| p.as_ref()) };
            if icon.is_some() || picture.is_some() {
                v.update_row(i, icon, picture);
            }
        }
    }

    /// Selection, the answer and the field's state into the window
    fn push(&mut self, st: &State) {
        let m = Self::material(st);
        let has_path = self.web.sel.is_none() && self.rows.get(self.sel).is_some_and(|r| !r.path.is_empty());
        let reading = self.web.sel.is_none() && self.rows.get(self.sel).is_some_and(|r| reading::url_of(r).is_some());
        self.key_ctx.set(KeyCtx { material: m, has_path, focus: if m { 0 } else { self.focus }, reading });
        let Some(v) = &self.view else { return };
        let shown = !self.quick.answer.is_empty() || !self.quick.error.is_empty();
        let short = rank::ai_row("", Some(&self.ai)).title;
        // The live web results
        let wv = &self.web;
        let web_shown = wv.loading || wv.shot.is_some() || wv.blocked || wv.plain;
        let name = tessera_core::weblink::search::name(wv.engine);
        let title = if wv.blocked { format!("{name} wants to check this is you") } else { format!("Search {name} for “{}”", tessera_core::weblink::search::short(&wv.query, 40)) };
        let web_hl = match (wv.sel.and_then(|i| wv.results.get(i)).and_then(|i| wv.links.get(*i)), &wv.shot) {
            (Some(l), Some((_, w, h))) if *w > 0 && *h > 0 => Some((l.x / *w as f32, l.y / *h as f32, l.w / *w as f32, l.h / *h as f32)),
            _ => None,
        };
        v.push(Pushed {
            // (the arrow keys in the web results: no row looks selected)
            sel: if wv.sel.is_some() { -1 } else { self.sel as i32 },
            answering: self.answering,
            button_focus: if m { 0 } else { self.focus },
            quick_busy: self.quick.busy,
            quick_shown: shown,
            quick_error: self.quick.error.as_str().into(),
            quick_answer: slint::StyledText::from_markdown(&self.quick.answer).unwrap_or_else(|_| slint::StyledText::from_plain_text(&self.quick.answer)),
            quick_done: !self.quick.busy && !self.quick.answer.is_empty() && self.quick.error.is_empty(),
            model_name: self.ai.name.as_str().into(),
            ask_label: if self.ai.configured { short.into() } else { "Set up AI".into() },
            web_shown,
            web_loading: wv.loading,
            web_name: name.into(),
            web_mode: if wv.blocked { 2 } else if wv.plain { 1 } else { 0 },
            web_title: title.as_str().into(),
            web_shot: wv.shot.as_ref().map(|(img, w, h)| (img.clone(), if *w > 0 { *h as f32 / *w as f32 } else { 0.0 })),
            web_hl,
            web_notice: web_shown && wv.notice,
        });
        if web_shown && self.web.notice {
            self.web.noticed = true;
        }
    }

    // ── The live web results (src/web.rs) ──

    /// Stop the render under way (its process killed)
    fn web_stop(&mut self) {
        self.timers[T_WEB].stop();
        self.web.renderer.stop();
        self.web.gen += 1;
        self.web.loading = false;
        self.web_clear();
        self.web.query.clear();
        self.web.url.clear();
    }

    fn web_clear(&mut self) {
        self.web.blocked = false;
        self.web.plain = false;
        self.web.shot = None;
        self.web.links.clear();
        self.web.results.clear();
        self.web.sel = None;
    }

    /// The text changed: the web results for it once typing pauses (a
    /// panel already there shimmers meanwhile); none for a path, a command
    /// or a sum, or with the setting off
    fn web_typed(&mut self, st: &State) {
        let q = self.text.trim().to_string();
        let want = self.web.on && self.active && !self.answering && rank::wants_web(&q) && web::long_enough(&q);
        if !want {
            if self.web.loading || self.web.shot.is_some() || !self.web.query.is_empty() {
                self.web_stop();
            }
            return;
        }
        if q == self.web.query {
            return;
        }
        let was_shown = self.web.loading || self.web.shot.is_some();
        self.timers[T_WEB].stop();
        self.web.renderer.stop();
        self.web.gen += 1;
        self.web.query = q;
        self.web.url.clear();
        self.web_clear();
        self.web.loading = was_shown;
        let gen = self.web.gen;
        self.after(st, T_WEB, web::DEBOUNCE_MS, move || Msg::WebGo(gen));
    }

    /// Typing paused: draw the results page (one render at a time)
    fn web_go(&mut self, st: &State, gen: u64) {
        if gen != self.web.gen || !self.active || self.web.query.is_empty() {
            return;
        }
        // (its turn: no more than one every few seconds, a few a minute)
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let wait = web::wait_ms(&self.web.starts, now_ms);
        if wait > 0 {
            self.after(st, T_WEB, wait, move || Msg::WebGo(gen));
            return;
        }
        let url = tessera_core::weblink::search::url_for(self.web.engine, &self.web.query);
        let out = web::out_dir();
        let _ = std::fs::create_dir_all(&out);
        let scale = self.view.as_ref().map_or(1.0, |v| v.scale());
        let args = web::args(self.web.engine, &url, &out, scale);
        self.web.url = url;
        // (after Google's robot check: the plain row a while, no render)
        if self.web.engine == "google" {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let ok_at = std::fs::read_to_string(web::google_ok_path()).ok().and_then(|t| web::google_ok_at(&t));
            if web::backed_off(self.web.blocked_at, ok_at, now) {
                self.web.loading = false;
                self.web_clear();
                self.web.plain = true;
                self.push(st);
                return;
            }
        }
        let to = self.send(st);
        let to2 = to.clone();
        let started = self.web.renderer.start(
            args,
            move |v| {
                // (read here, off the UI thread: the picture and its links)
                match v["event"].as_str() {
                    Some("rendered") => {
                        let png = v["png"].as_str().unwrap_or("").to_string();
                        let px = image::open(&png).ok().map(|i| {
                            let t = i.to_rgba8();
                            let mut b = Px::new(t.width(), t.height());
                            b.make_mut_bytes().copy_from_slice(t.as_raw());
                            b
                        });
                        let links = std::fs::read_to_string(v["links"].as_str().unwrap_or("")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).map(|j| web::parse_links(&j)).unwrap_or_default();
                        to(Box::new(Msg::WebShot(gen, px, links)));
                    }
                    Some("error") => to(Box::new(Msg::WebShot(gen, None, Vec::new()))),
                    Some("blocked") => to(Box::new(Msg::WebBlocked(gen))),
                    _ => {}
                }
            },
            move || to2(Box::new(Msg::WebEnded(gen))),
        );
        self.web.loading = started;
        if started {
            self.web.starts.retain(|&t| t + 60_000 > now_ms);
            self.web.starts.push(now_ms);
        } else {
            self.web_clear();
        }
        self.push(st);
    }

    /// Expand: the results page in the quick-look browser
    fn web_expand(&mut self) {
        if self.web.url.is_empty() {
            return;
        }
        let u = self.web.url.clone();
        self.close_self();
        tessera_core::weblink::open_quick(&u);
    }

    /// The arrow keys in the web results: Down from the last row goes in,
    /// Up from their first comes back out. True: it moved there
    fn web_step(&mut self, st: &State, down: bool) -> bool {
        let n = self.web.results.len();
        if n == 0 || self.web.loading {
            return false;
        }
        match (self.web.sel, down) {
            (None, true) if self.sel + 1 >= self.rows.len() => self.web.sel = Some(0),
            (Some(i), true) => self.web.sel = Some((i + 1).min(n - 1)),
            (Some(0), false) => self.web.sel = None,
            (Some(i), false) => self.web.sel = Some(i - 1),
            _ => return false,
        }
        self.push(st);
        true
    }

    fn reveal(&mut self, st: &State) {
        // (the list's height on this screen: less than its most on a short one, #1078)
        let most = if Self::material(st) { layout::MATERIAL_MAX } else { layout::GLASS_MAX };
        let view = self.view.as_ref().map_or(most, |v| layout::view_height(v.list_max(), most));
        let s = layout::reveal(&self.entries, self.sel, self.scroll, view);
        // (the Flickable may have been scrolled by hand)
        if let Some(v) = &self.view {
            let cur = -v.list_y();
            let s2 = layout::reveal(&self.entries, self.sel, cur, view);
            if s2 != cur {
                v.set_list_y(-s2);
            }
            self.scroll = s2;
            return;
        }
        self.scroll = s;
    }

    fn select(&mut self, st: &State, i: usize) {
        self.sel = i;
        self.reveal(st);
        self.push(st);
    }

    /// SearchModel.run: an AI row answers in place; the rest close and run
    fn run(&mut self, st: &mut State, i: usize) {
        let Some(r) = self.rows.get(i).cloned() else { return };
        if r.ai {
            if let Run::Ask(q) = &r.run {
                self.ask_ai(q.clone());
            }
            self.push(st);
            return;
        }
        self.close_self();
        self.run_row(st, &r.run);
    }

    fn run_row(&mut self, st: &State, run: &Run) {
        use serde_json::json;
        match run {
            Run::Nothing | Run::Ask(_) => {}
            Run::Copy(t) => tessera_core::util::pipe_to(&["wl-copy"], t.clone().into_bytes()),
            Run::App(id) => {
                if let Some(e) = st.shell.get::<Apps>().and_then(|a| a.by_id(id)) {
                    tessera_core::services::apps::launch(e);
                }
            }
            // (a web result: where Settings says, tessera-web or your browser)
            Run::Url(u) => tessera_core::shell::ask("launch.openLink", json!([u])),
            Run::Terminal(c) => tessera_core::shell::ask("search.command", json!([c])),
            Run::File(p) => tessera_core::shell::ask("files.open", json!([p])),
            Run::Action(t) => tessera_core::shell::ask("search.action", json!([t])),
            Run::Shell(k) => tessera_core::shell::ask("search.run", json!([k])),
            Run::SetupAi => tessera_core::shell::ask("panels.openSettings", json!(["ai"])),
            Run::Read(u) => tessera_core::weblink::reader(u),
        }
    }

    fn ask_ai(&mut self, q: String) {
        self.answering = true;
        self.quick = Quick { busy: true, ..Default::default() };
        // (its progress comes back to this host)
        tessera_core::search_route::asked_by(self.part);
        tessera_core::shell::ask("ai.quickAsk", serde_json::json!([q]));
    }

    /// The sparkle: ask about what's typed (or open the chat)
    fn ask_button(&mut self, st: &mut State) {
        let t = self.text.trim();
        let q = t.strip_prefix('?').unwrap_or(t).trim().to_string();
        if q.is_empty() {
            tessera_core::shell::ask("panels.show", serde_json::json!(["sidebar"]));
            return;
        }
        let r = rank::ai_row(&q, Some(&self.ai));
        if r.ai {
            self.ask_ai(q);
            self.push(st);
        } else {
            self.close_self();
            self.run_row(st, &r.run);
        }
        if let Some(v) = &self.view {
            v.focus();
        }
    }

    fn back(&mut self, st: &State) {
        self.answering = false;
        tessera_core::shell::ask("ai.stopQuick", serde_json::json!([]));
        self.refresh(st, true);
        if let Some(v) = &self.view {
            v.focus();
        }
    }

    /// A key the field gave Search (`handles`)
    pub fn key(&mut self, st: &mut State, text: &str, shift: bool, ctrl: bool) {
        // (what the key acts on: the rows for what's typed)
        self.rank_now(st);
        let is = |k: Key| text == SharedString::from(k).as_str();
        let m = Self::material(st);
        let has_text = !self.text.is_empty();
        if is(Key::Escape) {
            self.close_self();
        } else if is(Key::Return) || (self.focus != 0 && text == " ") {
            match self.focus {
                1 => {
                    self.focus = 0;
                    if let Some(v) = &self.view {
                        v.set_text("");
                    }
                    self.edited(st, String::new());
                }
                2 => self.ask_button(st),
                _ if self.answering => tessera_core::shell::ask("ai.continueQuick", serde_json::json!([])),
                // (a web result picked with the arrow keys: the quick-look browser there)
                _ if self.web.sel.is_some() => {
                    let u = self.web.sel.and_then(|i| self.web.results.get(i)).and_then(|i| self.web.links.get(*i)).map(|l| l.target().to_string());
                    if let Some(u) = u {
                        self.close_self();
                        tessera_core::weblink::open_quick(&u);
                    }
                }
                _ => self.run(st, self.sel),
            }
        } else if is(Key::DownArrow) {
            self.focus = 0;
            if self.web_step(st, true) {
                return;
            }
            let i = (self.sel + 1).min(self.rows.len().saturating_sub(1));
            self.select(st, i);
        } else if is(Key::UpArrow) {
            self.focus = 0;
            if self.web_step(st, false) {
                return;
            }
            let i = self.sel.saturating_sub(1);
            self.select(st, i);
        } else if is(Key::Tab) || is(Key::Backtab) {
            let back = is(Key::Backtab) || shift;
            if m {
                let n = self.rows.len().max(1);
                let i = if back { (self.sel + n - 1) % n } else { (self.sel + 1) % n };
                self.select(st, i);
                return;
            }
            let (f, next) = glass_tab(self.focus, back, self.rows.len(), has_text);
            self.focus = f;
            if next {
                let i = (self.sel + 1) % self.rows.len().max(1);
                self.select(st, i);
            } else {
                self.push(st);
            }
        } else if ctrl && (text.eq_ignore_ascii_case("d") || is(Key::Delete)) {
            // (a reading-list link: marked read, or taken off; reading.rs)
            if let Some(u) = self.rows.get(self.sel).and_then(reading::url_of).map(String::from) {
                if is(Key::Delete) {
                    tessera_core::readlist::remove(&u);
                } else {
                    tessera_core::readlist::mark_read(&u);
                }
                self.reading.retain(|i| i.url != u);
                self.refresh(st, false);
            }
        } else if (text == " " && shift) || (ctrl && text.eq_ignore_ascii_case("y")) {
            if let Some(p) = self.rows.get(self.sel).map(|r| r.path.clone()).filter(|p| !p.is_empty()) {
                tessera_core::shell::ask("panels.quickLook", serde_json::json!([p]));
            }
        }
    }

    /// The field's text changed
    pub fn edited(&mut self, st: &State, t: String) {
        self.focus = 0;
        if self.answering {
            self.answering = false;
            tessera_core::shell::ask("ai.stopQuick", serde_json::json!([]));
        }
        self.text = t;
        self.web.sel = None;
        self.refresh(st, true);
        self.web_typed(st);
    }

    fn find_files(&mut self, st: &State) {
        let fq = self.file_query.clone();
        self.contents.clear();
        let (to, plocate) = (self.send(st), self.plocate);
        let q = fq.clone();
        std::thread::spawn(move || {
            let out = providers::run_lines(&providers::names_command(&q, plocate));
            to(Box::new(Msg::Files(q, providers::parse_names(&out))));
        });
        if self.rg && rank::js_len(&fq) >= 3 {
            let to = self.send(st);
            std::thread::spawn(move || {
                let out = providers::run_lines(&providers::contents_command(&fq));
                to(Box::new(Msg::Contents(fq, out.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect())));
            });
        }
    }

    fn apps_changed(&mut self, st: &State) {
        let Some(a) = st.shell.get::<Apps>() else { return };
        self.apps = a
            .entries
            .iter()
            .filter(|e| !e.no_display)
            .map(|e| rank::App { id: e.id.clone(), name: e.name.clone(), generic: e.generic_name.clone(), comment: e.comment.clone(), keywords: e.keywords.clone(), icon: e.icon.clone() })
            .collect::<Vec<_>>()
            .into();
    }

    // ── The host's part, passed on ──

    /// Settings or the look changed
    pub fn config_changed(&mut self, st: &mut State) {
        self.look(st);
        if self.active {
            self.rebuild(st);
        }
    }

    /// The glass plugin came or went
    pub fn plugin_changed(&mut self, st: &State) {
        if let Some(v) = &self.view {
            v.set_glass_fill(glass_fill(st.shell.plugin, st.shell.look.dark));
        }
    }

    /// A reading changed (the apps, the icon theme)
    pub fn data(&mut self, st: &mut State, what: std::any::TypeId) {
        if what == std::any::TypeId::of::<Apps>() {
            self.apps_changed(st);
            if self.active {
                self.refresh(st, false);
            }
        } else if what == std::any::TypeId::of::<Icons>() && self.active {
            self.rebuild(st);
        }
    }

    /// A line from the shell for Search: its rows for a query, the AI's progress
    pub fn shell_line(&mut self, st: &mut State, v: &serde_json::Value) {
        if v["extras"].is_object() {
            let x = &v["extras"];
            if x["ai"].is_object() {
                self.ai = AiMeta { configured: x["ai"]["configured"].as_bool().unwrap_or(false), name: x["ai"]["name"].as_str().unwrap_or("").into() };
            }
            if let Some(c) = x["cast"].as_array() {
                self.cast = c
                    .iter()
                    .map(|a| rank::Action { title: a["title"].as_str().unwrap_or("").into(), glyph: a["glyph"].as_str().unwrap_or("").into(), keywords: a["keywords"].as_str().unwrap_or("").into() })
                    .collect();
                self.all_actions = Arc::new(self.actions.iter().chain(&self.cast).cloned().collect());
            }
            self.extras = Some(parse_extras(x));
            if self.active {
                self.refresh(st, false);
            }
        }
        if v["quick"].is_object() {
            let q = &v["quick"];
            self.quick = Quick { busy: q["busy"].as_bool().unwrap_or(false), answer: q["answer"].as_str().unwrap_or("").into(), error: q["error"].as_str().unwrap_or("").into() };
            // (hidden: what it shows stays as it was while it goes)
            if self.active {
                self.push(st);
            }
        }
    }

    /// One of its messages, back from the host's part
    pub fn message(&mut self, st: &mut State, m: Msg) {
        match m {
            Msg::Ranked(gen, r) => self.ranked(st, gen, *r),
            Msg::Key(t, shift, ctrl) => self.key(st, &t, shift, ctrl),
            Msg::Edited(t) => self.edited(st, t),
            Msg::Activate(i) => self.run(st, i.max(0) as usize),
            Msg::Look(i) => {
                if let Some(p) = self.rows.get(i.max(0) as usize).map(|r| r.path.clone()) {
                    tessera_core::shell::ask("panels.quickLook", serde_json::json!([p]));
                }
            }
            Msg::Cleared => self.edited(st, String::new()),
            Msg::Ask => self.ask_button(st),
            Msg::Back => self.back(st),
            Msg::Continue => tessera_core::shell::ask("ai.continueQuick", serde_json::json!([])),
            Msg::CopyAnswer => {
                tessera_core::util::pipe_to(&["wl-copy"], self.quick.answer.clone().into_bytes());
                self.close_self();
            }
            // (a link in the answer: where Settings says, tessera-web or your browser)
            Msg::Link(l) => tessera_core::shell::ask("launch.openLink", serde_json::json!([l])),
            Msg::AskAnswer => {
                let to = self.send(st);
                let q = self.ans_query.clone();
                self.answerer.ask(&q, move |id, a| to(Box::new(Msg::Answered(id, a))));
                self.after(st, T_IDLE, 120_000, || Msg::Idle);
            }
            Msg::Answered(id, a) => {
                if id == self.answerer.id {
                    self.answer = Some((self.ans_query.clone(), a));
                    if self.active && !self.answering {
                        self.refresh(st, false);
                    }
                }
            }
            Msg::Idle => self.answerer.stop(),
            Msg::Converted(q, r) => {
                if q == self.conv_query {
                    self.conv = Some((q, r));
                    if self.active && !self.answering {
                        self.refresh(st, false);
                    }
                }
            }
            Msg::Defined(w, d) => {
                if w == self.def_word {
                    self.def = Some((w, d));
                    if self.active && !self.answering {
                        self.refresh(st, false);
                    }
                }
            }
            Msg::FindFiles => self.find_files(st),
            Msg::Files(q, f) => {
                if q == self.file_query {
                    self.files = f;
                    if self.active && !self.answering {
                        self.refresh(st, false);
                    }
                }
            }
            Msg::Contents(q, c) => {
                if q == self.file_query {
                    self.contents = c;
                    if self.active && !self.answering {
                        self.refresh(st, false);
                    }
                }
            }
            Msg::Hold => {
                if self.hold_until.is_some() && self.active {
                    self.hold_until = Some(Instant::now());
                    self.refresh(st, false);
                }
            }
            Msg::IntroEnd => {
                if let Some(v) = &self.view {
                    v.set_introducing(false);
                }
            }
            Msg::Icons(list) => {
                for (k, px) in list {
                    self.pending.remove(&k);
                    if let Some(px) = px {
                        self.icons.insert(k, slint::Image::from_rgba8(px));
                    }
                }
                if self.icons.len() > 400 {
                    self.icons.clear();
                }
                self.update_images();
            }
            Msg::Fetched(u, p) => {
                let got = p.is_some();
                self.remote.insert(u, p);
                if got && self.active {
                    self.rebuild(st);
                }
            }
            Msg::WebGo(gen) => self.web_go(st, gen),
            Msg::WebShot(gen, px, links) => {
                if gen == self.web.gen && self.web.loading {
                    self.web.loading = false;
                    match px {
                        Some(px) => {
                            let (w, h) = (px.width(), px.height());
                            self.web.shot = Some((slint::Image::from_rgba8(px), w, h));
                            self.web.results = web::results(&links);
                            self.web.links = links;
                            self.web.sel = None;
                        }
                        // (it couldn't be drawn: no panel, no noise)
                        None => self.web_clear(),
                    }
                    self.push(st);
                }
            }
            Msg::WebBlocked(gen) => {
                if gen == self.web.gen && self.web.loading {
                    self.web.loading = false;
                    self.web_clear();
                    self.web.blocked = true;
                    self.web.blocked_at = Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
                    self.push(st);
                }
            }
            Msg::WebEnded(gen) => {
                // (ended without a picture: the shimmer goes)
                if gen == self.web.gen && self.web.loading {
                    self.web.loading = false;
                    if self.web.shot.is_none() {
                        self.web_clear();
                    }
                    self.push(st);
                }
            }
            Msg::WebClick(fx, fy) => {
                let link = self.web.shot.as_ref().and_then(|(_, w, h)| web::link_at(&self.web.links, fx * *w as f32, fy * *h as f32)).map(|l| l.target().to_string());
                if let Some(u) = link {
                    self.close_self();
                    tessera_core::weblink::open_quick(&u);
                }
            }
            Msg::WebExpand => self.web_expand(),
            Msg::WebBrowser => {
                if !self.web.url.is_empty() {
                    let u = self.web.url.clone();
                    self.close_self();
                    tessera_core::weblink::xdg_open(&u);
                }
            }
            Msg::WebNoticeOff => {
                self.close_self();
                tessera_core::shell::ask("panels.openSettings", serde_json::json!(["defaultapps"]));
            }
            Msg::Picture(p, px) => {
                self.pictures.insert(p, px.map(slint::Image::from_rgba8));
                if self.pictures.len() > 200 {
                    self.pictures.clear();
                }
                self.update_images();
            }
        }
    }

    /// The process is ending
    pub fn quit(&mut self) {
        self.answerer.stop();
        self.web_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn the_lane_ranks_as_the_ui_thread_did() {
        let apps = vec![
            rank::App { id: "firefox".into(), name: "Firefox".into(), generic: "Web Browser".into(), ..Default::default() },
            rank::App { id: "foot".into(), name: "Foot".into(), generic: "Terminal".into(), ..Default::default() },
        ];
        let actions = rank::actions();
        let inputs = Inputs {
            text: "fir".into(),
            apps: Arc::new(apps.clone()),
            actions: Arc::new(actions.clone()),
            answer: None,
            qalc: false,
            conversion: None,
            definition: None,
            extras: None,
            ai: AiMeta::default(),
            files: Vec::new(),
            contents: Vec::new(),
            home: "/home/x".into(),
            material: false,
            engine: "",
            reading: Vec::new(),
        };
        let r = std::thread::spawn(move || inputs.rank()).join().unwrap();
        let k = Known { apps: &apps, actions: &actions, ai: Some(&AiMeta::default()), home: "/home/x", ..Default::default() };
        let (rows, wants) = rank::query("fir", &k);
        assert_eq!(r.rows, rows);
        assert_eq!(r.wants, wants);
        assert_eq!(r.rows[0].title, "Firefox");
        assert_eq!((r.entries, r.content_h), layout::glass(&rows));
    }

    fn k(key: Key) -> String {
        SharedString::from(key).to_string()
    }

    #[test]
    fn keys_it_takes_from_the_field() {
        let glass = KeyCtx::default();
        let material = KeyCtx { material: true, ..Default::default() };
        for key in [Key::Escape, Key::Return, Key::UpArrow, Key::DownArrow, Key::Tab] {
            assert!(handles(glass, &k(key), false, false) && handles(material, &k(key), false, false));
        }
        assert!(handles(glass, &k(Key::Backtab), true, false));
        // (Material: Shift+Tab steps back through the results)
        assert!(handles(material, &k(Key::Backtab), true, false));
        assert!(!handles(glass, "a", false, false) && !handles(glass, &k(Key::Backspace), false, false));
        // Quick Look only for a file
        assert!(!handles(glass, " ", true, false));
        let file = KeyCtx { has_path: true, ..Default::default() };
        assert!(handles(file, " ", true, false) && handles(file, "y", false, true));
        assert!(!handles(file, " ", false, false));
        // a focused button takes Space
        assert!(handles(KeyCtx { focus: 2, ..Default::default() }, " ", false, false));
    }

    #[test]
    fn glass_tab_order() {
        // Tab moves through the results; with one or none, to the buttons
        assert_eq!(glass_tab(0, false, 3, true), (0, true));
        assert_eq!(glass_tab(0, false, 1, true), (1, false));
        assert_eq!(glass_tab(0, false, 0, false), (2, false));
        assert_eq!(glass_tab(1, false, 3, true), (2, false));
        assert_eq!(glass_tab(2, false, 3, true), (0, false));
        // Shift+Tab: Ask, then Clear (while there's text), then the field
        assert_eq!(glass_tab(0, true, 3, true), (2, false));
        assert_eq!(glass_tab(2, true, 3, true), (1, false));
        assert_eq!(glass_tab(2, true, 3, false), (0, false));
        assert_eq!(glass_tab(1, true, 3, true), (0, false));
    }

    #[test]
    fn lines_under_titles() {
        let r = |section: &str, sub: &str| Row { section: section.into(), subtitle: sub.into(), ..Default::default() };
        assert_eq!(sub_line(&r("Calculator", "2+2 ="), false, true), "2+2 = · Enter copies it");
        assert_eq!(sub_line(&r("Do", ""), true, true), "Enter to do it");
        assert_eq!(sub_line(&r("Dictionary", "Luck"), true, true), "Luck · Enter opens it");
        assert_eq!(sub_line(&r("Top Hit", "Get · Flathub"), true, true), "Flathub");
        assert_eq!(sub_line(&r("Top Hit", "Get · Flathub"), false, false), "Get · Flathub");
        assert_eq!(sub_line(&r("Software", "Get · AUR · 12 votes"), true, false), "AUR · 12 votes");
        assert_eq!(sub_line(&r("Software", "Get · AUR"), false, false), "Get · AUR");
    }

    #[test]
    fn answer_card_tones() {
        let a = slint::Color::from_rgb_u8(1, 2, 3);
        let r = |section: &str, kind: &str| Row { section: section.into(), kind: kind.into(), ..Default::default() };
        assert_eq!(tone(&r("Answer", "date"), true, a), hexc("#FF453A"));
        assert_eq!(tone(&r("Calculator", ""), false, a), hexc("#FF9500"));
        assert_eq!(tone(&r("Dictionary", ""), true, a), hexc("#BF5AF2"));
        assert_eq!(tone(&r("Do", "math"), true, a), a);
        assert_eq!(tone(&r("Answer", ""), true, a), a);
    }

    #[test]
    fn the_shells_rows() {
        let x = parse_extras(&json!({"q": " gimp ", "store": [{"key": "r3", "section": "Software", "title": "GIMP", "subtitle": "Get · Flathub", "icon": "/x/gimp.png", "loading": false}], "intents": "nonsense"}));
        assert_eq!(x.q, "gimp");
        assert_eq!((x.store[0].title.as_str(), &x.store[0].run, x.store[0].icon.as_str()), ("GIMP", &Run::Shell("r3".into()), "/x/gimp.png"));
        assert!(x.intents.is_empty() && x.notes.is_empty());
    }

    #[test]
    fn glass_is_clear_over_the_plugin() {
        assert_eq!(glass_fill(true, true).alpha(), 0);
        assert_eq!(glass_fill(false, true).alpha(), (0.62f32 * 255.0).round() as u8);
        assert!(glass_fill(false, false).red() > 240);
    }

    /// A view that keeps what it's told (no window)
    #[derive(Default)]
    struct Fake {
        geo: GlassGeo,
        texts: RefCell<Vec<String>>,
        lists_y: RefCell<Vec<f32>>,
    }

    impl View for Fake {
        fn wire(&self, _: Rc<dyn Fn(Box<dyn Any + Send>)>, _: Rc<dyn Fn(&str, bool, bool) -> bool>) {}
        fn theme(&self) -> ThemeColors {
            let k = Color::default();
            ThemeColors { accent: k, m3_scrim: k, m3_primary_container: k, m3_on_primary_container: k }
        }
        fn scale(&self) -> f32 {
            1.0
        }
        fn set_look(&self, _: LookView) {}
        fn set_placeholder(&self, _: &str) {}
        fn set_glass_fill(&self, _: Color) {}
        fn set_rows(&self, _: Vec<RowView>, _: f32) {}
        fn update_row(&self, _: usize, _: Option<&Image>, _: Option<&Image>) {}
        fn push(&self, _: Pushed) {}
        fn list_y(&self) -> f32 {
            0.0
        }
        fn set_list_y(&self, y: f32) {
            self.lists_y.borrow_mut().push(y);
        }
        fn list_max(&self) -> f32 {
            0.0
        }
        fn set_introducing(&self, _: bool) {}
        fn start_intro(&self) {}
        fn open(&self) {}
        fn set_text(&self, t: &str) {
            self.texts.borrow_mut().push(t.into());
        }
        fn set_text_end(&self, t: &str) {
            self.texts.borrow_mut().push(t.into());
        }
        fn focus(&self) {}
        fn glass(&self) -> GlassGeo {
            self.geo
        }
    }

    #[test]
    fn glass_follows_the_field_and_its_panels() {
        let look = theme::look_from(&json!({}), None, |_| None);
        // Search's own place: the field, the results 64 px under it, the web panel
        let v = Fake { geo: GlassGeo { alpha: 1.0, body_alpha: 1.0, x: 620.0, top: 216.0, w: 680.0, body_h: 300.0, web_y: 380.0, web_h: 200.0 }, ..Default::default() };
        let s = glass_shapes(&v, &look);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0][0], s[0][1], s[0][2], s[0][3], s[0][4]), (620.0, 216.0, 680.0, 56.0, 28.0));
        assert_eq!((s[1][1], s[1][2], s[1][3]), (280.0, 680.0, 300.0));
        assert_eq!((s[2][1], s[2][3]), (596.0, 200.0));
        assert_eq!(s, vec![
            theme::glass_shape(&look, 620.0, 216.0, 680.0, 56.0, 28.0, 1.0, 1.0),
            theme::glass_shape(&look, 620.0, 280.0, 680.0, 300.0, 24.0, 1.0, 1.0),
            theme::glass_shape(&look, 620.0, 596.0, 680.0, 200.0, 24.0, 1.0, 1.0),
        ]);
        // (the Overview's field on its way up, the results still shrunk: the
        // field alone, as wide as it is now)
        let v = Fake { geo: GlassGeo { alpha: 1.0, body_alpha: 0.0, x: 700.0, top: 120.0, w: 600.0, body_h: 0.0, web_y: 64.0, web_h: 0.0 }, ..Default::default() };
        let s = glass_shapes(&v, &look);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0][0], s[0][1], s[0][2]), (700.0, 120.0, 600.0));
    }

    #[test]
    fn one_search_for_any_view() {
        // the same controller, attached to one window then another (Search's
        // window; the Overview's on each display), keeps its state and draws
        // into the one attached
        let mut s = Search::new("overview");
        assert!(!s.is_active() && !s.take_done());
        let a: Rc<dyn View> = Rc::new(Fake::default());
        assert!(s.view.is_none());
        s.view = Some(a.clone());
        s.key_ctx.set(KeyCtx { has_path: true, ..Default::default() });
        let b: Rc<dyn View> = Rc::new(Fake::default());
        s.view = Some(b.clone());
        assert!(Rc::ptr_eq(s.view.as_ref().unwrap(), &b));
        // (the key context is shared with whichever window's field asks)
        let ctx = s.key_ctx();
        assert!(ctx.get().has_path);
        // closing itself is told once
        s.active = true;
        s.close_self();
        assert!(!s.is_active());
        assert!(s.take_done());
        assert!(!s.take_done());
        // deactivate drops a ranking still on its way
        let before = s.rank_gen;
        s.active = true;
        s.deactivate();
        assert!(s.rank_gen > before && s.rank_applied == s.rank_gen);
    }
}
