//! Where the window opens, through Hyprland (a Wayland window can't place
//! itself). Without --from/--at the window rules do it (hypr/tessera/rules.lua:
//! floating, centred, about 66% × 70% of the screen). --at: a rule of its own
//! while the window opens puts it exactly there, unanimated (the shell
//! animated to it already). --from: the window opens on the rectangle, then
//! is moved and sized to where the rules would put it, which Hyprland
//! animates: the window grows out of the circled selection. The rule is
//! turned off again as soon as the window is placed.

use crate::args::Rect;
use serde_json::Value;

/// A display, logical px, in the layout's coordinates
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The displays as Hyprland lists them (`j/monitors`)
pub fn monitors_from(v: &Value) -> Vec<Monitor> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|m| {
                    let (w, h) = tessera_core::services::clients::logical_of(m);
                    Monitor { x: m["x"].as_f64().unwrap_or(0.0).round() as i32, y: m["y"].as_f64().unwrap_or(0.0).round() as i32, w: w.round() as i32, h: h.round() as i32 }
                })
                .filter(|m| m.w > 0 && m.h > 0)
                .collect()
        })
        .unwrap_or_default()
}

/// The display a rectangle is on (its centre's; the first one otherwise)
pub fn monitor_for(r: &Rect, mons: &[Monitor]) -> Option<Monitor> {
    let (cx, cy) = r.centre();
    mons.iter().find(|m| cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h).or(mons.first()).copied()
}

/// Where the window rules put the window on a display: 66% × 70%, centred
pub fn usual(m: &Monitor) -> Rect {
    let (w, h) = ((m.w as f64 * 0.66).round() as i32, (m.h as f64 * 0.7).round() as i32);
    Rect { x: m.x + (m.w - w) / 2, y: m.y + (m.h - h) / 2, w, h }
}

/// The rule the window opens with: on `r` (a rule's position is the
/// display's own, so `m`'s corner is taken off), floating; `still`: not
/// animated. Any earlier one is turned off first.
pub fn rule_lua(r: &Rect, m: &Monitor, still: bool) -> String {
    format!(
        "if TESSERA_WEB_PLACE then pcall(function() TESSERA_WEB_PLACE:set_enabled(false) end) end\n\
         TESSERA_WEB_PLACE = hl.window_rule({{ match = {{ class = \"^tessera-web$\" }}, float = true, size = \"{} {}\", move = \"{} {}\"{} }})\n",
        r.w,
        r.h,
        r.x - m.x,
        r.y - m.y,
        if still { ", no_anim = true" } else { "" }
    )
}

/// The rule turned off (the window is placed; the next one opens as the rules say)
pub fn rule_off_lua() -> &'static str {
    "if TESSERA_WEB_PLACE then pcall(function() TESSERA_WEB_PLACE:set_enabled(false) end) TESSERA_WEB_PLACE = nil end\n"
}

/// This process's window sized and moved to `r` (layout coordinates)
pub fn move_lua(pid: u32, r: &Rect) -> String {
    let w = format!("pid:{pid}");
    format!(
        "hl.dispatch(hl.dsp.window.resize({{ x = {}, y = {}, window = \"{w}\" }}))\nhl.dispatch(hl.dsp.window.move({{ x = {}, y = {}, window = \"{w}\" }}))\n",
        r.w, r.h, r.x, r.y
    )
}

/// In a Hyprland session?
pub fn in_hyprland() -> bool {
    tessera_core::services::hypr::dir().is_some()
}

pub fn monitors() -> Vec<Monitor> {
    tessera_core::services::hypr::request("j/monitors").ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).map(|v| monitors_from(&v)).unwrap_or_default()
}

/// `hyprctl eval`, here and now (it's quick); false when refused
pub fn eval(lua: &str) -> bool {
    tessera_core::services::hypr::eval_logged(lua)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn two() -> Vec<Monitor> {
        monitors_from(&json!([
            {"x": 0, "y": 0, "width": 2880, "height": 1800, "scale": 2.0, "transform": 0},
            {"x": 1440, "y": 0, "width": 1920, "height": 1080, "scale": 1.0, "transform": 0}
        ]))
    }

    #[test]
    fn displays_in_logical_px() {
        let m = two();
        assert_eq!(m[0], Monitor { x: 0, y: 0, w: 1440, h: 900 });
        assert_eq!(m[1], Monitor { x: 1440, y: 0, w: 1920, h: 1080 });
        assert!(monitors_from(&json!("nonsense")).is_empty());
    }

    #[test]
    fn grows_to_where_the_rules_put_it_on_its_display() {
        let m = two();
        let sel = Rect { x: 1600, y: 100, w: 200, h: 150 };
        let on = monitor_for(&sel, &m).unwrap();
        assert_eq!(on.x, 1440);
        let to = usual(&on);
        assert_eq!((to.w, to.h), (1267, 756));
        assert_eq!((to.x, to.y), (1440 + (1920 - 1267) / 2, (1080 - 756) / 2));
        // (off every display: the first)
        assert_eq!(monitor_for(&Rect { x: -900, y: -900, w: 10, h: 10 }, &m).unwrap().x, 0);
        assert_eq!(monitor_for(&sel, &[]), None);
    }

    #[test]
    fn the_rule_and_the_move() {
        let m = Monitor { x: 1440, y: 0, w: 1920, h: 1080 };
        let lua = rule_lua(&Rect { x: 1600, y: 100, w: 200, h: 150 }, &m, true);
        assert!(lua.contains("class = \"^tessera-web$\""));
        assert!(lua.contains("size = \"200 150\", move = \"160 100\", no_anim = true"));
        assert!(lua.starts_with("if TESSERA_WEB_PLACE then"));
        assert!(!rule_lua(&Rect { x: 1600, y: 100, w: 200, h: 150 }, &m, false).contains("no_anim"));
        let mv = move_lua(42, &Rect { x: 10, y: 20, w: 300, h: 400 });
        assert!(mv.contains("hl.dsp.window.resize({ x = 300, y = 400, window = \"pid:42\" })"));
        assert!(mv.contains("hl.dsp.window.move({ x = 10, y = 20, window = \"pid:42\" })"));
        assert!(rule_off_lua().contains("set_enabled(false)"));
    }
}
