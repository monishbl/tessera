//! Small helpers the smaller surfaces share (overlay, welcome, resources,
//! cheatsheet, osk, hotcorners, tiling, decorations): an output's size, an
//! icon in both looks, colours from the shell's hex strings, the short
//! clock, memory in GB.

use crate::wayland::State;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;

/// An output's size in logical px (the screen a surface goes on)
pub fn screen_size(st: &State, output: &WlOutput) -> (f32, f32) {
    let Some(info) = st.outputs.info(output) else { return (1920.0, 1080.0) };
    if let Some((w, h)) = info.logical_size {
        return (w.max(1) as f32, h.max(1) as f32);
    }
    let (w, h) = info.modes.iter().find(|m| m.current).map_or((1920, 1080), |m| m.dimensions);
    let s = info.scale_factor.max(1);
    ((w / s).max(1) as f32, (h / s).max(1) as f32)
}

/// A screen's size in logical px by its name, the focused one for "" (where
/// Hyprland opens a new window); None before any output is known
pub fn screen_named(st: &State, name: &str) -> Option<(f32, f32)> {
    let focused = || st.shell.get::<crate::services::hypr::Snapshot>().map(|s| s.focused.clone()).unwrap_or_default();
    let name = if name.is_empty() { focused() } else { name.to_string() };
    let o = st.output_named(&name).or_else(|| st.outputs_now().into_iter().next().map(|(o, _)| o))?;
    Some(screen_size(st, &o))
}

/// The on-screen keyboard (surfaces/osk, Keyboard.qml): its key unit on a
/// screen this wide (logical px)
pub fn osk_unit(screen_w: f32) -> f32 {
    58f32.min((screen_w - 120.0) / 15.6)
}

/// The on-screen keyboard's window: its width and height with the shadow's
/// room `m` round it
pub fn osk_window(screen_w: f32, m: f32) -> (u32, u32) {
    let u = osk_unit(screen_w);
    ((u * 15.6 + 24.0 + 2.0 * m).round() as u32, (u * 5.0 + 4.0 * 6.0 + 24.0 + 2.0 * m).round() as u32)
}

/// The room round the on-screen keyboard for its shadow
pub const OSK_SHADOW: f32 = 20.0;

/// How much of the screen's bottom the on-screen keyboard takes (its
/// exclusive zone): the lock and the login screen lift their field above
/// it while it's up (#763)
pub fn osk_covers(screen_w: f32) -> f32 {
    osk_window(screen_w, OSK_SHADOW).1 as f32 - OSK_SHADOW
}

/// A regular window's size on a screen of `screen` logical px: as asked,
/// made smaller to stay whole on a small or portrait screen (below the menu
/// bar, clear of the edges), never under 480×360
pub fn fit_window(size: (u32, u32), screen: (f32, f32)) -> (u32, u32) {
    let w = (size.0 as f32).min(screen.0 - 48.0).max(480.0);
    let h = (size.1 as f32).min(screen.1 - 96.0).max(360.0);
    (w.round() as u32, h.round() as u32)
}

/// An output's size in its own pixels, as the screen shows them (the mode,
/// turned for a rotated output): what a full-screen picture is decoded at
pub fn pixel_size(st: &State, output: &WlOutput) -> Option<(u32, u32)> {
    use smithay_client_toolkit::reexports::client::protocol::wl_output::Transform;
    let info = st.outputs.info(output)?;
    let (w, h) = info.modes.iter().find(|m| m.current)?.dimensions;
    let turned = matches!(info.transform, Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270);
    let (w, h) = if turned { (h, w) } else { (w, h) };
    Some((w.max(1) as u32, h.max(1) as u32))
}

/// A Phosphor icon by the shell's name, and the Material Symbol the Material
/// look draws in its place (Icon.qml): (phosphor, symbol). A surface turns
/// it into its own Slint `Gl` (@tessera/small-icons.slint) with `gl_fn!`
pub fn glyphs(name: &str) -> (&'static str, &'static str) {
    let p = crate::icons::glyph(name);
    (p, crate::icons::symbol(p))
}

/// `fn gl(name) -> ui::Gl` for a surface crate whose Slint exports `Gl`
#[macro_export]
macro_rules! gl_fn {
    ($ui:ident) => {
        /// An icon in both looks (@tessera/small-icons.slint's Gl)
        pub fn gl(name: &str) -> $ui::Gl {
            let (p, m) = tessera_core::surfaces::kit::glyphs(name);
            $ui::Gl { p: p.into(), m: m.into() }
        }
    };
}

/// "#RRGGBB" / "#AARRGGBB": the shell's hex colours
pub fn hex(s: &str) -> slint::Color {
    let h = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    match h.len() {
        6 => slint::Color::from_argb_u8(255, (v >> 16) as u8, (v >> 8) as u8, v as u8),
        8 => slint::Color::from_argb_u8((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8),
        _ => slint::Color::from_argb_u8(255, 0, 0, 0),
    }
}

/// The wall clock as the shell writes hours and minutes, short: "HH:mm"
/// with the 24-hour setting, else the 12-hour hour unpadded, no AM/PM
/// ("9:05", "4:41"; TimeFormat.hmShort's "h:mm" gave the 24-hour hour,
/// "16:41" beside a menu bar saying 4:41 PM: audit #862)
pub fn hm_short(cfg: &serde_json::Value) -> String {
    let h24 = crate::locale::clock24(&cfg);
    hm_short_of(&crate::util::now("%H:%M"), h24)
}

/// hm_short for a "HH:MM" time
pub fn hm_short_of(t: &str, h24: bool) -> String {
    if h24 {
        return t.to_string();
    }
    let (h, m) = t.split_once(':').unwrap_or(("0", "00"));
    let h: u32 = h.parse().unwrap_or(0);
    format!("{}:{m}", (h + 11) % 12 + 1)
}

/// /proc/meminfo: "6.1 of 15.4 GB" (Resources' memory detail)
pub fn mem_detail(info: &str) -> String {
    let kb = |k: &str| -> f64 { info.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix(':'))).and_then(|r| r.split_whitespace().next()?.parse().ok()).unwrap_or(0.0) };
    let (total, avail) = (kb("MemTotal"), kb("MemAvailable"));
    format!("{:.1} of {:.1} GB", (total - avail) / 1_048_576.0, total / 1_048_576.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_on_screen_keyboard_covers_the_bottom_of_the_screen() {
        assert_eq!(osk_window(1920.0, 20.0), ((58.0f32 * 15.6 + 64.0).round() as u32, 58 * 5 + 24 + 24 + 40));
        // (its exclusive zone: the window less the shadow's room at its top)
        assert_eq!(osk_covers(1920.0), (58 * 5 + 24 + 24 + 20) as f32);
        assert!(osk_covers(800.0) < osk_covers(1920.0));
    }

    #[test]
    fn colours_from_hex() {
        assert_eq!(hex("#30D158"), slint::Color::from_argb_u8(255, 0x30, 0xd1, 0x58));
        assert_eq!(hex("80FFFFFF"), slint::Color::from_argb_u8(0x80, 255, 255, 255));
        assert_eq!(hex("bad"), slint::Color::from_argb_u8(255, 0, 0, 0));
    }

    #[test]
    fn memory_and_icons() {
        assert_eq!(mem_detail("MemTotal: 16148844 kB\nMemAvailable: 9759008 kB\n"), "6.1 of 15.4 GB");
        assert_eq!(glyphs("x").0, crate::icons::glyph("x"));
    }
}
