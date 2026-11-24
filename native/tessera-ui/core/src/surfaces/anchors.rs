//! Where each menu bar item is, for the menus that open under it (the
//! status menus, the Wi-Fi menu). The bar says where its items sit as it
//! lays them out (`remember`); a menu opened by a shortcut or a command,
//! with no item's x of its own (Panels.anchorX 0), opens under its item
//! all the same (`place`), or, when the bar has no such item, right-aligned
//! under the status area. Before, it opened at the screen's top-left corner.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// An anchor past any screen's right edge: the menu's own clamp
/// (`min(anchor - off, width - menu - margin)`) right-aligns it
pub const RIGHT: f32 = 1.0e6;

fn table() -> &'static Mutex<HashMap<(String, String), f32>> {
    static T: OnceLock<Mutex<HashMap<(String, String), f32>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The bar's item for `panel` on `output` has its left edge at `x`
pub fn remember(output: &str, panel: &str, x: f32) {
    if let Ok(mut t) = table().lock() {
        t.insert((output.to_string(), panel.to_string()), x);
    }
}

/// The item gone (the bar no longer shows it)
pub fn forget(output: &str, panel: &str) {
    if let Ok(mut t) = table().lock() {
        t.remove(&(output.to_string(), panel.to_string()));
    }
}

pub fn recall(output: &str, panel: &str) -> Option<f32> {
    table().lock().ok().and_then(|t| t.get(&(output.to_string(), panel.to_string())).copied())
}

/// The anchor a menu uses: the one it was given (a click on its item), else
/// where its item is, else right-aligned (RIGHT)
pub fn resolve(given: f32, remembered: Option<f32>) -> f32 {
    if given.is_finite() && given > 0.5 {
        given
    } else {
        remembered.filter(|x| x.is_finite() && *x > 0.5).unwrap_or(RIGHT)
    }
}

/// `resolve` with the remembered place of `panel`'s item on `output`
pub fn place(given: f32, output: &str, panel: &str) -> f32 {
    resolve(given, recall(output, panel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_without_an_anchor_goes_under_its_item_or_right() {
        // clicked: where the click said
        assert_eq!(resolve(812.0, Some(700.0)), 812.0);
        // a shortcut (no x): under the item the bar placed
        assert_eq!(resolve(0.0, Some(700.0)), 700.0);
        // no such item on the bar: right-aligned, never the top-left corner
        assert_eq!(resolve(0.0, None), RIGHT);
        assert_eq!(resolve(f32::NAN, None), RIGHT);
        remember("T-1", "soundMenu", 1500.0);
        assert_eq!(place(0.0, "T-1", "soundMenu"), 1500.0);
        assert_eq!(place(0.0, "T-2", "soundMenu"), RIGHT);
        forget("T-1", "soundMenu");
        assert_eq!(place(0.0, "T-1", "soundMenu"), RIGHT);
    }
}
