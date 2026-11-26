//! Desktop.qml and Windows.qml. Windows.qml groups Hyprland's windows by
//! space for the views: that is `Windows` in core (services::clients, with
//! `covered`, fullscreen and minimized helpers), so nothing is kept here.
//! Desktop.qml's Show Desktop is: the focused space's windows step aside to a
//! hidden special workspace, and come back on the next call, or when the
//! space changes or a window opens.

use super::hx;
use serde_json::{json, Value};
use tessera_core::host::Host;
use tessera_core::services::clients::Windows;

#[derive(Default)]
pub struct Desk {
    /// [(address, space)] of the windows stepped aside
    stashed: Vec<(String, i32)>,
}

/// state.json's record of the windows stepped aside, so a shell that
/// restarts while the desktop shows (an update, a crash) can bring them back
const STASH_KEY: &str = "desktopStash";

fn stash_of(v: &Value) -> Vec<(String, i32)> {
    v.as_array()
        .map(|a| a.iter().filter_map(|e| Some((e.get(0)?.as_str()?.to_string(), e.get(1)?.as_i64()? as i32))).filter(|(a, ws)| !a.is_empty() && *ws > 0).collect())
        .unwrap_or_default()
}

impl Desk {
    /// At start: windows a shell before this one stepped aside come back
    pub fn recover(&mut self) {
        let left = stash_of(&hx::state_get(STASH_KEY));
        if !left.is_empty() {
            self.stashed = left;
            self.restore_desktop();
        }
    }

    pub fn showing(&self) -> bool {
        !self.stashed.is_empty()
    }

    pub fn show_desktop(&mut self, h: &Host) {
        if self.showing() {
            return self.restore_desktop();
        }
        let Some(w) = h.st.shell.get::<Windows>() else { return };
        let ws = w.focused_monitor().map(|m| m.active.id).unwrap_or(0);
        if ws <= 0 {
            return;
        }
        self.stashed = w.clients.iter().filter(|c| c.workspace.id == ws).map(|c| (hx::addr(&c.address), ws)).collect();
        if self.stashed.is_empty() {
            return;
        }
        hx::state_set(STASH_KEY, json!(self.stashed.iter().map(|(a, ws)| json!([a, ws])).collect::<Vec<_>>()));
        for (a, _) in &self.stashed {
            hx::dispatch(format!("hl.dsp.window.move({{ workspace = \"special:tessera-desktop\", follow = false, window = \"address:{a}\" }})"));
        }
    }

    pub fn restore_desktop(&mut self) {
        if self.stashed.is_empty() {
            return;
        }
        hx::state_set(STASH_KEY, json!([]));
        for (a, ws) in std::mem::take(&mut self.stashed) {
            hx::dispatch(format!("hl.dsp.window.move({{ workspace = {ws}, follow = false, window = \"address:{a}\" }})"));
        }
    }

    /// Leaving the space while the desktop shows brings the windows back
    pub fn event(&mut self, name: &str) {
        if self.showing() && (name == "workspace" || name == "openwindow") {
            self.restore_desktop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stash_reads_back_and_skips_junk() {
        let v = json!([["0xabc", 3], ["", 2], ["0xdef", 0], "x", ["0x1", 5]]);
        assert_eq!(stash_of(&v), vec![("0xabc".to_string(), 3), ("0x1".to_string(), 5)]);
        assert!(stash_of(&Value::Null).is_empty());
    }
}
