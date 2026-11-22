//! Sound outputs that need their card switched over first (replaces
//! services/AudioProfiles.qml, assets/scripts/audio-profiles.py): a plugged-in
//! monitor's speakers while the laptop's are active, or the laptop's while
//! the monitor's are. Control Center's Sound page lists them with the other
//! outputs; picking one switches the card and makes its output the default.
//! Read again when asked (the Sound page opens, a card changes).
//!
//! Calls (prefix "audioprofiles"): refresh, pick({card, profile}). State on
//! the bus as "audioprofiles": {alternatives:[{card,profile,name,kind}], switching}.

use super::common::*;
use serde_json::{json, Value};
use std::any::Any;
use tessera_core::host::{Host, Service};

const NAME: &str = "AudioProfiles";

enum M {
    Listed(String),
    /// the switch's exit code and its last line ({"ok", "error"})
    Switched(i32, String),
}

/// Why a switch failed, from audio-profiles.py's answer (None: it worked).
/// 22 Nov: its "The sound card didn't switch." was read by nobody, so a
/// failed pick only stopped spinning
pub fn switch_error(code: i32, out: &str) -> Option<String> {
    let last = out.trim().lines().next_back().unwrap_or("");
    let v: Value = serde_json::from_str(last).unwrap_or(Value::Null);
    if code == 0 && v["ok"] != false {
        return None;
    }
    Some(v["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("The sound output didn't switch. Try again, or pick it in Settings → Sound.").to_string())
}

#[derive(Default)]
pub struct AudioProfiles {
    alternatives: Value,
    switching: String,
    listing: bool,
    switching_now: bool,
}

impl AudioProfiles {
    fn publish(&self) {
        crate::bus::put(NAME, json!({"alternatives": self.alternatives, "switching": self.switching}));
    }
    fn refresh(&mut self, h: &mut Host) {
        if !has_tool("pactl") || self.listing {
            return;
        }
        self.listing = true;
        let send = h.sender(NAME);
        capture_then_within(script_cmd("audio-profiles.py", &["list"]), None, Some(std::time::Duration::from_secs(15)), move |_, t| send(Box::new(M::Listed(t))));
    }
}

impl Service for AudioProfiles {
    fn name(&self) -> &'static str {
        NAME
    }
    fn calls(&self) -> &'static [&'static str] {
        &["audioprofiles"]
    }
    fn start(&mut self, h: &mut Host) {
        self.alternatives = json!([]);
        self.refresh(h);
        self.publish();
    }
    fn call(&mut self, h: &mut Host, call: &str, args: &Value) {
        let a = args_list(args);
        match call.strip_prefix("audioprofiles.").unwrap_or("") {
            "refresh" => self.refresh(h),
            "pick" => {
                if self.switching_now {
                    return;
                }
                let alt = a.first().cloned().unwrap_or(Value::Null);
                let (card, profile) = (alt["card"].as_str().unwrap_or("").to_string(), alt["profile"].as_str().unwrap_or("").to_string());
                if card.is_empty() || profile.is_empty() {
                    return;
                }
                self.switching = profile.clone();
                self.switching_now = true;
                let send = h.sender(NAME);
                capture_then_within(script_cmd("audio-profiles.py", &["switch", &card, &profile]), None, Some(std::time::Duration::from_secs(20)), move |code, out| send(Box::new(M::Switched(code, out))));
            }
            _ => {}
        }
        self.publish();
    }
    fn message(&mut self, h: &mut Host, msg: Box<dyn Any + Send>) {
        let Ok(m) = msg.downcast::<M>() else { return };
        match *m {
            M::Listed(t) => {
                self.listing = false;
                if let Ok(next) = serde_json::from_str::<Value>(&t) {
                    if next != self.alternatives {
                        self.alternatives = next;
                    }
                }
            }
            M::Switched(code, out) => {
                self.switching.clear();
                self.switching_now = false;
                // (said, not only the spinner stopping)
                if let Some(why) = switch_error(code, &out) {
                    crate::input::common_island::post(h, json!({"kind": "custom", "key": "audioprofile", "icon": "speaker-x", "title": "Couldn't switch the sound output", "subtitle": why}), 6000);
                }
                self.refresh(h);
            }
        }
        self.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_switch_says_why() {
        assert_eq!(switch_error(0, "{\"ok\": true, \"sink\": \"alsa_output.x.hdmi-stereo\"}\n"), None);
        assert_eq!(switch_error(1, "{\"ok\": false, \"error\": \"The sound card didn't switch.\"}\n").as_deref(), Some("The sound card didn't switch."));
        // (killed at the deadline: -1 and nothing said)
        assert!(switch_error(-1, "").unwrap().contains("didn't switch"));
    }
}
