//! Sound outputs that need their card switched over first (replaces
//! services/AudioProfiles.qml, assets/scripts/audio-profiles.py): a plugged-in
//! monitor's speakers while the laptop's are active, or the laptop's while
//! the monitor's are. Control Center's Sound page lists them with the other
//! outputs; picking one switches the card and makes its output the default.
//! Read again when asked (the Sound page opens) and whenever a card changes
//! (pactl subscribe's "… on card #n": a monitor plugged in or out while the
//! page or menu is open).
//!
//! Calls (prefix "audioprofiles"): refresh, pick({card, profile}). State on
//! the bus as "audioprofiles": {alternatives:[{card,profile,name,kind}], switching}.
//! The pages pick one by its id (`alt_id`: its card and profile's names,
//! never its place in a list that may have changed since it was drawn).

use super::common::*;
use serde_json::{json, Value};
use std::any::Any;
use tessera_core::host::{Host, Service};

const NAME: &str = "AudioProfiles";

enum M {
    Listed(String),
    /// a card came, went or changed
    CardChanged,
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

/// An alternative's id for the pages: its card and profile names
pub fn alt_id(alt: &Value) -> String {
    format!("{}\u{1f}{}", alt["card"].as_str().unwrap_or(""), alt["profile"].as_str().unwrap_or(""))
}

/// The alternative with that id in the list as it is now (None: it's gone)
pub fn find(ap: &Value, id: &str) -> Option<Value> {
    let (card, profile) = id.split_once('\u{1f}')?;
    ap["alternatives"].as_array()?.iter().find(|x| x["card"].as_str() == Some(card) && x["profile"].as_str() == Some(profile)).cloned()
}

/// pactl subscribe's line for a card's change ("Event 'change' on card #3")
fn card_event(line: &str) -> bool {
    line.contains(" on card #")
}

#[derive(Default)]
pub struct AudioProfiles {
    alternatives: Value,
    switching: String,
    listing: bool,
    switching_now: bool,
    /// a card changed while a listing ran: list again after it
    again: bool,
}

impl AudioProfiles {
    fn publish(&self) {
        crate::bus::put(NAME, json!({"alternatives": self.alternatives, "switching": self.switching}));
    }
    fn refresh(&mut self, h: &mut Host) {
        if !has_tool("pactl") {
            return;
        }
        if self.listing {
            self.again = true;
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
        let send = h.sender(NAME);
        super::pulse_events::subscribe(Box::new(move |l: &str| {
            if card_event(l) {
                send(Box::new(M::CardChanged));
            }
        }));
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
                if std::mem::take(&mut self.again) {
                    self.refresh(h);
                }
            }
            // (while a switch runs its own changes come; it lists again when done)
            M::CardChanged if !self.switching_now => self.refresh(h),
            M::CardChanged => {}
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
    fn alternatives_are_picked_by_name() {
        let ap = json!({"alternatives": [{"card": "alsa_card.pci-0000_00_1f.3", "profile": "output:hdmi-stereo", "name": "Monitor"}, {"card": "alsa_card.usb", "profile": "output:analog-stereo", "name": "Headset"}]});
        let id = alt_id(&ap["alternatives"][1]);
        assert_eq!(find(&ap, &id).unwrap()["name"], "Headset");
        // (gone since it was drawn: nothing is switched)
        let later = json!({"alternatives": [{"card": "alsa_card.pci-0000_00_1f.3", "profile": "output:hdmi-stereo", "name": "Monitor"}]});
        assert_eq!(find(&later, &id), None);
        assert_eq!(find(&ap, "1"), None);
        assert!(card_event("Event 'change' on card #47"));
        assert!(!card_event("Event 'change' on sink #2"));
    }

    #[test]
    fn a_failed_switch_says_why() {
        assert_eq!(switch_error(0, "{\"ok\": true, \"sink\": \"alsa_output.x.hdmi-stereo\"}\n"), None);
        assert_eq!(switch_error(1, "{\"ok\": false, \"error\": \"The sound card didn't switch.\"}\n").as_deref(), Some("The sound card didn't switch."));
        // (killed at the deadline: -1 and nothing said)
        assert!(switch_error(-1, "").unwrap().contains("didn't switch"));
    }
}
