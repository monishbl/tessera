//! Window controls' sizes and place (WindowControls.qml): the pill at the
//! top centre of the focused window, its expanded and tile-grid sizes, and
//! where the plugin's tile grid goes. Everything in layout px.

use tessera_core::services::clients::Client;

/// Room for the shadow round the pill
pub const M: f32 = 12.0;
/// The tile grid's zones, three rows of four (WindowControls.tileRows)
pub const TILE_ROWS: [[&str; 4]; 3] = [["left", "right", "top", "bottom"], ["topLeft", "topRight", "bottomLeft", "bottomRight"], ["leftThird", "centerThird", "rightThird", "fill"]];

/// Above the tile grid: the traffic lights' row (or just padding, plugin)
pub fn head(plugin: bool, material: bool) -> f32 {
    if plugin { 8.0 } else { (if material { 28.0 } else { 24.0 }) + 8.0 }
}

/// The window's size, the shadow's room included (implicitWidth/Height)
pub fn size(tiling: bool, expanded: bool, plugin: bool, material: bool) -> (f32, f32) {
    let w = if tiling { 196.0 } else if expanded { if material { 84.0 } else { 76.0 } } else { 44.0 };
    let h = if tiling { head(plugin, material) + 3.0 * 32.0 + 2.0 * 4.0 + 8.0 } else if expanded { if material { 28.0 } else { 24.0 } } else { 10.0 };
    (w + 2.0 * M, h + 2.0 * M)
}

/// The surface holds the largest it gets (the tile grid); the window grows
/// inside it, centred, from the top
pub fn surface(plugin: bool, material: bool) -> (u32, u32) {
    let (w, h) = size(true, true, plugin, material);
    (w.ceil() as u32, h.ceil() as u32)
}

/// The surface's top-left on its monitor: under the window's top centre
/// (`at`, `size`: the window's, global; `mon`: the monitor's origin)
pub fn place(at: (i32, i32), win: (i32, i32), mon: (i32, i32), surface_w: f32) -> (i32, i32) {
    let cx = (at.0 - mon.0) as f32 + win.0 as f32 / 2.0;
    ((cx - surface_w / 2.0).round() as i32, (at.1 - mon.1) as f32 as i32 + 4 - M as i32)
}

/// Where the pill itself is drawn (global x, y, w, h; expanded, as when
/// hovered), under the window's top centre
pub fn pill_rect(at: (i32, i32), win: (i32, i32), material: bool) -> (f32, f32, f32, f32) {
    let (w, h) = size(false, true, false, material);
    let (w, h) = (w - 2.0 * M, h - 2.0 * M);
    (at.0 as f32 + win.0 as f32 / 2.0 - w / 2.0, at.1 as f32 + 4.0, w, h)
}

/// How high a window is stacked (Hyprland's order; the overview's
/// layout::stack, with pinned windows over every floating one)
fn stack(c: &Client) -> i32 {
    if c.fullscreen > 0 {
        4000
    } else if c.pinned {
        3000 - c.focus_history_id.clamp(0, 999)
    } else if c.floating {
        2000 - c.focus_history_id.clamp(0, 999)
    } else {
        1
    }
}

/// Is the pill (`pill`: pill_rect) under another window? One shown on the
/// focused window's space (or pinned, so on every space of its display),
/// stacked above it (floating over tiled, a pinned or fullscreen window),
/// that overlaps the pill: the pill is a Top layer, above every window, so
/// it would be drawn over that window and take its clicks there (#494)
pub fn covered(focused: &Client, clients: &[Client], pill: (f32, f32, f32, f32)) -> bool {
    let mine = stack(focused);
    clients.iter().any(|c| {
        c.address != focused.address
            && c.mapped
            && !c.hidden
            && !c.aside()
            && (c.workspace.id == focused.workspace.id || c.pinned && c.monitor == focused.monitor)
            && stack(c) > mine
            && (c.at.0 as f32) < pill.0 + pill.2
            && (c.at.0 + c.size.0) as f32 > pill.0
            && (c.at.1 as f32) < pill.1 + pill.3
            && (c.at.1 + c.size.1) as f32 > pill.1
    })
}

/// Plugin mode: the grid under the green button's bottom centre (global)
pub fn place_at(point: (f32, f32), mon: (i32, i32), surface_w: f32) -> (i32, i32) {
    ((point.0 - mon.0 as f32 - surface_w / 2.0).round() as i32, (point.1 - mon.1 as f32 + 6.0 - M).round() as i32)
}

/// The plugin's line: `tessera-controls>>tile,ADDR,x,y`
pub fn controls_event(line: &str) -> Option<(String, f32, f32)> {
    let data = line.strip_prefix("tessera-controls>>")?;
    let f: Vec<&str> = data.trim_end().split(',').collect();
    if f.len() < 4 || f[0] != "tile" {
        return None;
    }
    let a = if f[1].starts_with("0x") { f[1].to_string() } else { format!("0x{}", f[1]) };
    Some((a, f[2].parse().ok()?, f[3].parse().ok()?))
}

/// A Hyprland window selector (Apps.windowActionFor's)
pub fn sel(address: &str) -> String {
    format!("address:{}", if address.starts_with("0x") { address.to_string() } else { format!("0x{address}") })
}

/// The dispatches behind the buttons (close, minimize, maximize), as the
/// shell's Apps.windowActionFor writes them
pub fn action(what: &str, address: &str) -> Option<String> {
    let s = sel(address);
    Some(match what {
        "close" => format!("hl.dsp.window.close({{ window = \"{s}\" }})"),
        "minimize" => format!("hl.dsp.window.move({{ workspace = \"special:minimized\", follow = false, window = \"{s}\" }})"),
        "maximize" => format!("hl.dsp.window.fullscreen({{ mode = \"maximized\", action = \"toggle\", window = \"{s}\" }})"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_as_the_qml() {
        assert_eq!(size(false, false, false, false), (68.0, 34.0));
        assert_eq!(size(false, true, false, false), (100.0, 48.0));
        assert_eq!(size(false, true, false, true), (108.0, 52.0));
        assert_eq!(size(true, true, false, false), (220.0, 32.0 + 96.0 + 8.0 + 8.0 + 24.0));
        assert_eq!(size(true, true, true, false).1, 8.0 + 112.0 + 24.0);
        assert_eq!(surface(false, true), (220, 36 + 112 + 24));
    }

    #[test]
    fn placed_under_the_windows_top_centre() {
        // a window at (100, 200) 800 wide on a monitor at (1920, 0)
        assert_eq!(place((2020, 200), (800, 600), (1920, 0), 220.0), (390, 192));
        assert_eq!(place_at((2500.0, 300.0), (1920, 0), 220.0), (470, 294));
    }

    #[test]
    fn the_pill_hides_under_a_window_above_it() {
        use tessera_core::services::clients::WsRef;
        let win = |a: &str, at: (i32, i32), size: (i32, i32)| Client { address: a.into(), at, size, mapped: true, workspace: WsRef { id: 1, name: "1".into() }, ..Default::default() };
        // the focused, tiled window at (0, 30) 1000 wide: its pill at x 462..538 (Glass), y 34..58
        let focused = Client { focus_history_id: 0, ..win("f", (0, 30), (1000, 700)) };
        let pill = pill_rect(focused.at, focused.size, false);
        assert_eq!(pill, (462.0, 34.0, 76.0, 24.0));
        assert_eq!(pill_rect((0, 30), (1000, 700), true).2, 84.0);
        let over = Client { floating: true, focus_history_id: 1, ..win("p", (400, 0), (300, 200)) };
        // a floating window (a picture-in-picture) over the pill
        assert!(covered(&focused, &[focused.clone(), over.clone()], pill));
        // one beside it, or on another space, or minimized, or hidden: not
        assert!(!covered(&focused, &[focused.clone(), Client { at: (600, 0), ..over.clone() }], pill));
        assert!(!covered(&focused, &[focused.clone(), Client { workspace: WsRef { id: 2, name: "2".into() }, ..over.clone() }], pill));
        assert!(!covered(&focused, &[focused.clone(), Client { workspace: WsRef { id: -99, name: "special:minimized".into() }, ..over.clone() }], pill));
        assert!(!covered(&focused, &[focused.clone(), Client { hidden: true, ..over.clone() }], pill));
        // a tiled neighbour never covers it; a pinned window from another space does
        assert!(!covered(&focused, &[focused.clone(), Client { floating: false, ..over.clone() }], pill));
        let pinned = Client { pinned: true, workspace: WsRef { id: 3, name: "3".into() }, ..over.clone() };
        assert!(covered(&focused, &[focused.clone(), pinned.clone()], pill));
        assert!(!covered(&focused, &[focused.clone(), Client { monitor: 1, ..pinned }], pill));
        // the focused window floating: an older floating one is under it,
        // but a pinned one stays above
        let front = Client { floating: true, ..focused.clone() };
        assert!(!covered(&front, &[front.clone(), over.clone()], pill));
        assert!(covered(&front, &[front.clone(), Client { pinned: true, ..over }], pill));
    }

    #[test]
    fn the_plugins_line_and_the_buttons() {
        assert_eq!(controls_event("tessera-controls>>tile,55aa,640,32"), Some(("0x55aa".into(), 640.0, 32.0)));
        assert_eq!(controls_event("tessera-controls>>hover,55aa,640,32"), None);
        assert_eq!(controls_event("activewindow>>foot,~"), None);
        assert_eq!(action("close", "55aa").unwrap(), "hl.dsp.window.close({ window = \"address:0x55aa\" })");
        assert!(action("minimize", "0x1").unwrap().contains("\"special:minimized\", follow = false"));
        assert!(action("maximize", "0x1").unwrap().contains("mode = \"maximized\", action = \"toggle\""));
        assert_eq!(action("explode", "0x1"), None);
    }
}
