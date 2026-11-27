//! The bar's look, read as the shell reads it: tessera_core's theme::read()
//! (config.json's appearance.*: Glass or Material, light or dark, the
//! accent, Material's colours from its seed, contrast) and theme::tokens,
//! the same values every Rust surface draws with, turned into GTK CSS.
//! Only the bar is glass (see-through, Hyprland's blur behind it: the window
//! rule in hypr/tessera/rules.lua); pages are never transparent.

use tessera_core::theme::Tokens;

pub type Rgba = [u8; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub material: bool,
    pub dark: bool,
    pub font: String,
    /// the bar (Glass: see-through, for the blur behind it)
    pub bar: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    /// the address pill
    pub field: Rgba,
    pub focus: Rgba,
    /// a button under the pointer
    pub hover: Rgba,
    pub accent: Rgba,
    /// suggestions' and menus' sheet
    pub menu: Rgba,
    pub selected: Rgba,
    pub on_selected: Rgba,
    pub separator: Rgba,
    /// behind a page until it paints (opaque)
    pub page: Rgba,
}

fn c(o: &Option<slint::Color>, fallback: Rgba) -> Rgba {
    o.map_or(fallback, |c| [c.red(), c.green(), c.blue(), c.alpha()])
}

fn alpha(mut c: Rgba, a: f32) -> Rgba {
    c[3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    c
}

/// The bar's colours for a look's tokens (theme::tokens)
pub fn palette(t: &Tokens) -> Palette {
    let material = t.material.unwrap_or(false);
    let dark = t.dark.unwrap_or(true);
    let (ink, paper): (Rgba, Rgba) = if dark { ([255, 255, 255, 235], [30, 30, 33, 255]) } else { ([0, 0, 0, 217], [245, 245, 247, 255]) };
    let window = alpha(c(&t.window_bg, paper), 1.0);
    let text = c(&t.label, ink);
    Palette {
        material,
        dark,
        font: t.font.as_ref().map_or_else(|| "Inter".to_string(), |f| f.to_string()),
        // (Material: its surface container, see-through only with Material's
        // transparency; Glass: the window's colour over Hyprland's blur)
        bar: if material { c(&t.panel, window) } else { alpha(window, 0.62) },
        text,
        dim: c(&t.secondary, alpha(ink, 0.6)),
        field: c(&t.field_bg, alpha(ink, 0.08)),
        focus: c(&t.field_focus, c(&t.accent, [10, 132, 255, 255])),
        hover: c(&t.fill_hover, alpha(ink, 0.12)),
        accent: c(&t.accent, [10, 132, 255, 255]),
        menu: alpha(c(&t.menu_bg, window), if material { 1.0 } else { 0.96 }),
        selected: c(&t.selection, c(&t.accent, [10, 132, 255, 255])),
        on_selected: c(&t.on_selection, [255, 255, 255, 255]),
        separator: c(&t.separator, alpha(ink, 0.12)),
        page: if material { alpha(c(&t.m3_surface, window), 1.0) } else { window },
    }
}

/// The look now, as the shell has it
pub fn current() -> Palette {
    let look = tessera_core::theme::read();
    palette(&tessera_core::theme::tokens(&look, false))
}

pub fn css_color(c: Rgba) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("rgba({}, {}, {}, {:.3})", c[0], c[1], c[2], c[3] as f32 / 255.0)
    }
}

/// The window's GTK style sheet
pub fn css(p: &Palette) -> String {
    // (the bar's numbers: tessera_core::weblink::bar, which Circle to Search
    // lands the picture with)
    use tessera_core::weblink::bar;
    let k = css_color;
    let font = p.font.replace(['"', '\\', ';', '{', '}'], "");
    let pill_radius = if p.material { 28 } else { 999 };
    format!(
        r#"window.tessera-web {{ background: transparent; }}
window.tessera-web .root {{ transition: opacity 260ms ease-out; }}
window.tessera-web .root.entering {{ opacity: 0; }}
.bar {{ background-color: {bar}; color: {text}; padding: {pad_y}px {pad_x}px; font-family: "{font}", "Inter", sans-serif; font-size: 13px; }}
.bar button {{ background: none; border: none; box-shadow: none; outline: none; min-width: {button}px; min-height: {button}px; padding: 0; border-radius: 999px; color: {text}; }}
.bar button:hover {{ background-color: {hover}; }}
.bar button:checked {{ color: {accent}; }}
.bar button:disabled {{ color: {dim}; opacity: 0.45; }}
.pill {{ background-color: {field}; border-radius: {pill_radius}px; min-height: {pill_h}px; padding: 0 {pill_pad}px; }}
.pill:focus-within {{ box-shadow: inset 0 0 0 2px {focus}; }}
.pill entry, .pill text {{ background: none; border: none; box-shadow: none; outline: none; color: {text}; caret-color: {accent}; min-height: 30px; padding: 0; }}
.pill .where {{ color: {text}; }}
.pill .glyph {{ color: {dim}; }}
.pill .note {{ color: {dim}; }}
.pill .thumb {{ border-radius: {thumb_radius}px; min-width: {thumb}px; min-height: {thumb}px; }}
.progress {{ min-height: 2px; background: none; }}
.progress trough {{ min-height: 2px; background: none; border: none; }}
.progress progress {{ min-height: 2px; background-color: {accent}; border: none; border-radius: 0; }}
.suggest {{ background-color: {menu}; color: {text}; border-radius: 14px; padding: 6px; box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28); font-family: "{font}", "Inter", sans-serif; font-size: 13px; }}
.suggest list {{ background: none; }}
.suggest row {{ border-radius: 9px; padding: 6px 10px; color: {text}; background: none; }}
.suggest row:selected, .suggest row:hover {{ background-color: {selected}; color: {on_selected}; }}
.suggest row .url {{ font-size: 11px; opacity: 0.75; }}
popover.menu contents, popover contents {{ background-color: {menu}; color: {text}; }}
popover modelbutton:hover {{ background-color: {selected}; color: {on_selected}; }}
.page {{ background-color: {page}; }}
"#,
        bar = k(p.bar),
        text = k(p.text),
        dim = k(p.dim),
        field = k(p.field),
        focus = k(p.focus),
        hover = k(p.hover),
        accent = k(p.accent),
        menu = k(p.menu),
        selected = k(p.selected),
        on_selected = k(p.on_selected),
        page = k(p.page),
        pad_x = bar::PAD_X,
        pad_y = bar::PAD_Y,
        button = bar::BUTTON,
        pill_h = bar::PILL_H,
        pill_pad = bar::PILL_PAD,
        thumb = bar::THUMB,
        thumb_radius = bar::THUMB_RADIUS,
    )
}

/// Reading mode's page style (src/reader.js puts it in the page): the
/// look's colours and font, opaque, one calm column
pub fn reader_css(p: &Palette) -> String {
    let k = |c: Rgba| css_color([c[0], c[1], c[2], 255]);
    let font = p.font.replace(['"', '\\', ';', '{', '}'], "");
    let scheme = if p.dark { "dark" } else { "light" };
    format!(
        r#":root {{ color-scheme: {scheme}; }}
html, body {{ margin: 0; background: {page}; color: {text}; }}
body {{ font: 19px/1.65 "{font}", "Inter", system-ui, sans-serif; -webkit-font-smoothing: antialiased; }}
main {{ max-width: 40em; margin: 0 auto; padding: 48px 24px 120px; }}
h1 {{ font-size: 2em; line-height: 1.2; margin: 0.2em 0 0.8em; }}
h2, h3, h4 {{ line-height: 1.3; margin: 1.6em 0 0.5em; }}
p, li {{ margin: 0 0 1em; }}
a {{ color: {accent}; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
img, video, figure, picture {{ max-width: 100%; height: auto; border-radius: 10px; margin: 0.6em 0; }}
figcaption {{ color: {dim}; font-size: 0.8em; }}
pre, code {{ font-family: "JetBrains Mono", monospace; font-size: 0.85em; }}
pre {{ background: {field}; padding: 12px 14px; border-radius: 10px; overflow: auto; }}
blockquote {{ margin: 1em 0; padding-left: 1em; border-left: 3px solid {accent}; color: {dim}; }}
table {{ border-collapse: collapse; max-width: 100%; display: block; overflow: auto; }}
td, th {{ border: 1px solid {separator}; padding: 4px 8px; }}
.tessera-site {{ color: {dim}; font-size: 0.8em; margin: 0; }}
"#,
        page = k(p.page),
        text = k(p.text),
        accent = k(p.accent),
        dim = k(p.dim),
        field = css_color(p.field),
        separator = css_color(p.separator),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tessera_core::theme::{look_from, tokens};

    fn pal(cfg: serde_json::Value) -> Palette {
        palette(&tokens(&look_from(&cfg, None, |_| None), false))
    }

    #[test]
    fn glass_dark_and_light() {
        let d = pal(json!({"appearance": {"look": "glass", "mode": "dark", "accent": "blue"}}));
        assert!(!d.material && d.dark);
        assert_eq!(d.font, "Inter");
        // (the bar see-through over the blur; the page behind it opaque)
        assert!(d.bar[3] < 255 && d.bar[3] > 100);
        assert_eq!(d.page[3], 255);
        assert_eq!(d.accent, [0x0a, 0x84, 0xff, 255]);
        assert!(d.text[0] > 200);
        let l = pal(json!({"appearance": {"look": "glass", "mode": "light", "accent": "orange"}}));
        assert!(!l.dark);
        assert_eq!(l.accent, [0xff, 0x95, 0x00, 255]);
        assert!(l.text[0] < 60);
        assert!(l.page[0] > 200);
    }

    #[test]
    fn material_from_its_seed() {
        let m = pal(json!({"appearance": {"look": "material", "mode": "dark", "materialSeed": "#6750A4"}}));
        assert!(m.material && m.dark);
        assert_eq!(m.font, "Tessera Text Flex");
        assert_eq!(m.bar[3], 255);
        assert_eq!(m.page[3], 255);
        assert!(m.page[0] < 60);
        let l = pal(json!({"appearance": {"look": "material", "mode": "light", "materialSeed": "#6750A4"}}));
        assert!(l.page[0] > 200);
        assert_ne!(l.accent, m.accent);
    }

    #[test]
    fn the_style_sheet() {
        let d = pal(json!({"appearance": {"look": "glass", "mode": "dark"}}));
        let s = css(&d);
        assert!(s.contains(".bar { background-color: rgba("));
        assert!(s.contains("font-family: \"Inter\""));
        assert!(s.contains(".pill { background-color:"));
        assert!(s.contains("border-radius: 999px; min-height: 34px"));
        let m = css(&pal(json!({"appearance": {"look": "material", "mode": "light"}})));
        assert!(m.contains("border-radius: 28px; min-height: 34px"));
        // (the bar's numbers, tessera_core::weblink::bar: Circle to Search's
        // picture lands at x 130, y 14, 22 × 22, its corners 6 px)
        assert!(s.contains("padding: 8px 10px;"));
        assert!(s.contains("min-width: 30px; min-height: 30px;"));
        assert!(s.contains("padding: 0 12px; }"));
        assert!(s.contains(".pill .thumb { border-radius: 6px; min-width: 22px; min-height: 22px; }"));
        assert_eq!(css_color([1, 2, 3, 255]), "#010203");
        assert_eq!(css_color([255, 0, 0, 128]), "rgba(255, 0, 0, 0.502)");
        // (a font name can't break out of its quotes)
        let mut odd = d.clone();
        odd.font = "x\"; } * { color: red".into();
        assert!(!css(&odd).contains("x\";"));
        // (reading mode: opaque, the look's scheme)
        let r = reader_css(&d);
        assert!(r.contains("color-scheme: dark"));
        assert!(r.contains(&format!("background: {}", css_color(d.page))));
        assert!(reader_css(&pal(json!({"appearance": {"look": "glass", "mode": "light"}}))).contains("color-scheme: light"));
    }
}
