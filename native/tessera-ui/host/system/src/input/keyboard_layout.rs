//! Active keyboard layout of the main keyboard, and switching between
//! layouts, replacing shell/services/KeyboardLayout.qml. Says the menu bar's
//! `keyboard` state ({multiple, short, name}) and publishes bus
//! "keyboardlayout" {name, layouts, variants, multiple, short} (Live Text
//! reads the variants). Call "keyboardlayout.next" (`hyprctl switchxkblayout
//! all next`).

use super::common::*;
use serde_json::{json, Value};
use std::any::Any;
use tessera_core::host::{Host, Service};

/// "English (US)" -> "EN"; else the first layout code, upper case
pub fn short(name: &str, layouts: &[String]) -> String {
    let n = name.to_lowercase();
    let b = n.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_lowercase() && b[1].is_ascii_lowercase() {
        n[..2].to_uppercase()
    } else {
        layouts.first().cloned().unwrap_or_default().to_uppercase()
    }
}

/// With several layouts, the active one's code ("US", "GB", "UA": English (US)
/// and English (UK) both read "EN" by name), with its variant when two share
/// a code; None when the index isn't known (then `short` by the name)
pub fn short_of(index: Option<usize>, layouts: &[String], variants: &[String]) -> Option<String> {
    let i = index?;
    let code = layouts.get(i)?;
    if layouts.len() < 2 || code.is_empty() {
        return None;
    }
    let var = variants.get(i).map(String::as_str).unwrap_or("");
    let shared = layouts.iter().filter(|l| *l == code).count() > 1;
    Some(if shared && !var.is_empty() { format!("{}-{}", code.to_uppercase(), var.chars().take(3).collect::<String>().to_uppercase()) } else { code.to_uppercase() })
}

/// The main keyboard's active_layout_index from `hyprctl -j devices`
pub fn active_index(devices: &Value) -> Option<usize> {
    let kbs = devices["keyboards"].as_array()?;
    let main = kbs.iter().find(|k| k["main"] == true).or_else(|| kbs.first())?;
    main["active_layout_index"].as_u64().map(|i| i as usize)
}

/// The main keyboard's (active keymap, layouts, variants) from `hyprctl -j devices`
pub fn parse(devices: &Value) -> Option<(String, Vec<String>, Vec<String>)> {
    let kbs = devices["keyboards"].as_array()?;
    let main = kbs.iter().find(|k| k["main"] == true).or_else(|| kbs.first())?;
    let name = main["active_keymap"].as_str().unwrap_or("").to_string();
    let names: Vec<&str> = main["layout"].as_str().unwrap_or("").split(',').collect();
    let vars: Vec<&str> = main["variant"].as_str().unwrap_or("").split(',').collect();
    let layouts = names.iter().filter(|x| !x.is_empty()).map(|x| x.to_string()).collect();
    let variants = names.iter().enumerate().filter(|(_, n)| !n.is_empty()).map(|(i, _)| vars.get(i).unwrap_or(&"").to_string()).collect();
    Some((name, layouts, variants))
}

enum M {
    Devices(Value),
}

#[derive(Default)]
pub struct KeyboardLayout {
    name: String,
    layouts: Vec<String>,
    variants: Vec<String>,
    active: Option<usize>,
}

impl KeyboardLayout {
    fn refresh(&self, h: &Host) {
        let send = h.sender("KeyboardLayout");
        std::thread::spawn(move || send(Box::new(M::Devices(hyprctl_json(&["devices"])))));
    }
    fn publish(&self, h: &mut Host) {
        let multiple = self.layouts.len() > 1;
        let short = short_of(self.active, &self.layouts, &self.variants).unwrap_or_else(|| short(&self.name, &self.layouts));
        h.set_state("keyboard", json!({"multiple": multiple, "short": short, "name": self.name}));
        crate::bus::put("keyboardlayout", json!({"name": self.name, "layouts": self.layouts, "variants": self.variants, "multiple": multiple, "short": short}));
    }
}

impl Service for KeyboardLayout {
    fn name(&self) -> &'static str {
        "KeyboardLayout"
    }
    fn calls(&self) -> &'static [&'static str] {
        &["keyboardlayout"]
    }
    fn start(&mut self, h: &mut Host) {
        spawn_hypr_events(&["activelayout", "configreloaded"], h.sender("KeyboardLayout"));
        self.publish(h);
        self.refresh(h);
    }
    fn call(&mut self, _h: &mut Host, call: &str, _args: &Value) {
        if call.ends_with(".next") {
            hyprctl(&["switchxkblayout", "all", "next"]);
        }
    }
    fn message(&mut self, h: &mut Host, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Raw>() {
            Ok(_) => {
                // (activelayout is for whichever keyboard changed, a virtual one
                // too: the main keyboard's is read again rather than taken from it)
                self.refresh(h);
                return;
            }
            Err(m) => m,
        };
        if let Ok(m) = msg.downcast::<M>() {
            let M::Devices(d) = *m;
            if let Some((name, layouts, variants)) = parse(&d) {
                self.active = active_index(&d);
                self.name = name;
                self.layouts = layouts;
                self.variants = variants;
                self.publish(h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts() {
        let d = json!({"keyboards": [{"main": false, "layout": "fr"}, {"main": true, "active_keymap": "English (US)", "layout": "us,de,", "variant": ",nodeadkeys"}]});
        let (n, l, v) = parse(&d).unwrap();
        assert_eq!((n.as_str(), l.len()), ("English (US)", 2));
        assert_eq!(v, vec!["".to_string(), "nodeadkeys".to_string()]);
        assert_eq!(short(&n, &l), "EN");
        assert_eq!(short("", &l), "US");
        assert_eq!(short("(none)", &["de".to_string()]), "DE");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(short_of(Some(1), &s(&["us", "gb"]), &s(&["", ""])).as_deref(), Some("GB"));
        assert_eq!(short_of(Some(0), &s(&["ua", "us"]), &s(&["", ""])).as_deref(), Some("UA"));
        assert_eq!(short_of(Some(1), &s(&["us", "us"]), &s(&["", "intl"])).as_deref(), Some("US-INT"));
        assert_eq!(short_of(Some(0), &s(&["us"]), &s(&[""])), None);
        assert_eq!(short_of(None, &s(&["us", "de"]), &s(&["", ""])), None);
        assert_eq!(active_index(&json!({"keyboards": [{"main": true, "active_layout_index": 1}]})), Some(1));
    }
}
