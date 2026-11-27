//! Mouse and trackpad (Settings → Mouse, Trackpad), replacing
//! shell/services/Mouse.qml (and FocusDecision.js, the pure part of it).
//!
//! Mice: applied to each mouse as a Hyprland device config (`hl.device`);
//! trackpads keep their own speed, acceleration and scrolling. Window focus
//! (click or hover) is Hyprland's input.follow_mouse. Re-applied after a
//! config reload and when the device list changes (udev says so: one
//! `udevadm monitor`; without it, every 60 s). Also the tessera-glass plugin's
//! pointer boundaries and pointer-follow, focus that follows the pointer after
//! scene changes, and apps asking to come forward ("urgent").
//!
//! Prefix "mouse": bringForward [address]. State for others: bus "mouse"
//! { mice, touchpads, boundaryAvailable } (RustBridgeInput's feed).
//!
//! Differences: the idle monitor that tells "the user just clicked or typed"
//! without the plugin (ext-idle-notify) isn't ported: without plugin v4 an
//! urgent window is announced, not brought forward (the plugin is the normal
//! case). Island's "Show" button comes back as `mouse.bringForward`.

use super::common::*;
use serde_json::{json, Value};
use std::any::Any;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tessera_core::host::{Host, Service};

// ── FocusDecision.js ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Warp(String),
    Refocus,
    None,
}

const SCENE_CHANGES: [&str; 7] = ["workspace", "workspacev2", "activespecial", "activespecialv2", "closewindow", "movewindowv2", "changefloatingmode"];

fn is_hex_addr(a: &str) -> bool {
    let b = a.strip_prefix("0x").unwrap_or(a);
    !b.is_empty() && b.chars().all(|c| c.is_ascii_hexdigit())
}

/// After a burst of Hyprland events: what to do with the pointer
pub fn after_events(events: &[Raw]) -> Act {
    let mut opened = String::new();
    let mut changed = false;
    for e in events {
        if e.name == "openwindow" {
            let a = e.data.split(',').next().unwrap_or("");
            if is_hex_addr(a) {
                opened = if a.starts_with("0x") { a.to_string() } else { format!("0x{a}") };
            }
        } else if SCENE_CHANGES.contains(&e.name.as_str()) {
            changed = true;
        }
    }
    if !opened.is_empty() {
        Act::Warp(opened)
    } else if changed {
        Act::Refocus
    } else {
        Act::None
    }
}

/// A window asking for attention: bring it forward right after the user
/// clicked or typed, else just show that it wants attention
pub fn on_urgent(input_age_ms: Option<i64>, already_active: bool) -> &'static str {
    if already_active {
        return "none";
    }
    match input_age_ms {
        Some(a) if (0..=3000).contains(&a) => "bring",
        _ => "notify",
    }
}

// ── Hyprland device config ────────────────────────────────────────────────

/// A device name inside a Lua "…" string: backslash and quote escaped, and
/// control characters as \ddd (a name with a line break would leave the string
/// unfinished, and Hyprland would refuse every setting sent with it)
pub fn lua_str(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn is_mouse(name: &str) -> bool {
    let n = name.to_lowercase();
    !["touchpad", "trackpad", "ydotool", "virtual", "wacom", "stylus", "pen"].iter().any(|k| n.contains(k))
}

fn is_pad(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("touchpad") || n.contains("trackpad")
}

/// What the system says each input device is (udev's ID_INPUT_TOUCHPAD,
/// as libinput decides it), by the name Hyprland gives the device: a
/// trackpad whose name doesn't say so (bcm5974 on older MacBooks, some
/// "…-mouse" or "glidepoint" ones) is still a trackpad (#597)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kinds {
    /// the trackpads
    pub touchpads: std::collections::HashSet<String>,
    /// every device udev said something about (a name not in it: the name decides)
    pub known: std::collections::HashSet<String>,
}

/// Hyprland's name for a device (deviceNameToInternalString: spaces and
/// line breaks as '-', lower case)
pub fn hypr_name(name: &str) -> String {
    name.trim_end_matches('\n').chars().map(|c| if c == ' ' || c == '\n' { '-' } else { c }).collect::<String>().to_lowercase()
}

/// One input device's udev record (`/run/udev/data/c<major>:<minor>`):
/// Some(true) a trackpad, Some(false) another pointer or key device, None
/// when it says neither
pub fn udev_kind(record: &str) -> Option<bool> {
    let has = |k: &str| record.lines().any(|l| l.trim() == format!("E:{k}=1"));
    if has("ID_INPUT_TOUCHPAD") {
        Some(true)
    } else if has("ID_INPUT_MOUSE") || has("ID_INPUT_POINTINGSTICK") || has("ID_INPUT_TABLET") || has("ID_INPUT_KEYBOARD") || has("ID_INPUT_KEY") {
        Some(false)
    } else {
        None
    }
}

/// Every event device's kind, from /sys/class/input and udev's database
/// (read, nothing run); None when there's no database to read (a container)
pub fn read_kinds(sys_input: &std::path::Path, udev_data: &std::path::Path) -> Option<Kinds> {
    if !udev_data.is_dir() {
        return None;
    }
    let mut k = Kinds::default();
    for e in std::fs::read_dir(sys_input).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !n.starts_with("event") {
            continue;
        }
        let dir = e.path();
        let (Ok(name), Ok(dev)) = (std::fs::read_to_string(dir.join("device/name")), std::fs::read_to_string(dir.join("dev"))) else { continue };
        let Ok(record) = std::fs::read_to_string(udev_data.join(format!("c{}", dev.trim()))) else { continue };
        let Some(pad) = udev_kind(&record) else { continue };
        let name = hypr_name(name.trim_end());
        if pad {
            k.touchpads.insert(name.clone());
        }
        k.known.insert(name);
    }
    Some(k)
}

/// A trackpad: as the system says, else by its name
pub fn pad_by(name: &str, kinds: Option<&Kinds>) -> bool {
    match kinds {
        Some(k) if k.touchpads.contains(name) => true,
        Some(k) if k.known.contains(name) => false,
        _ => is_pad(name),
    }
}

/// A mouse: a pointer that isn't a trackpad (nor a pen or a virtual device)
pub fn mouse_by(name: &str, kinds: Option<&Kinds>) -> bool {
    !pad_by(name, kinds) && is_mouse(name)
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v[k].as_f64().unwrap_or(d)
}
fn flag(v: &Value, k: &str, d: bool) -> bool {
    v[k].as_bool().unwrap_or(d)
}
fn text<'a>(v: &'a Value, k: &str, d: &'a str) -> &'a str {
    v[k].as_str().unwrap_or(d)
}
fn tf(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

pub fn device_spec(name: &str, m: &Value) -> String {
    let sens = num(m, "speed", 0.0).clamp(-1.0, 1.0);
    format!(
        "hl.device({{ name = \"{}\", accel_profile = \"{}\", sensitivity = {:.2}, natural_scroll = {}, scroll_factor = {:.2}, left_handed = {}, middle_button_emulation = {} }})",
        lua_str(name),
        if text(m, "accelProfile", "adaptive") == "flat" { "flat" } else { "adaptive" },
        sens,
        tf(flag(m, "naturalScroll", false)),
        num(m, "scrollSpeed", 1.0).clamp(0.2, 3.0),
        tf(flag(m, "leftHanded", false)),
        tf(flag(m, "middleEmulation", false)),
    )
}

pub fn touchpad_spec(tp: &Value) -> String {
    format!(
        "hl.config({{ input = {{ touchpad = {{ tap_to_click = {}, tap_and_drag = {}, drag_lock = {}, natural_scroll = {}, scroll_factor = {:.2}, disable_while_typing = {}, clickfinger_behavior = {}, middle_button_emulation = {} }} }} }})",
        tf(flag(tp, "tapToClick", true)),
        tf(flag(tp, "tapAndDrag", true)),
        if flag(tp, "dragLock", false) { 1 } else { 0 },
        tf(flag(tp, "naturalScroll", true)),
        num(tp, "scrollSpeed", 0.7).clamp(0.2, 3.0),
        tf(flag(tp, "disableWhileTyping", false)),
        tf(text(tp, "rightClick", "twoFingers") != "corner"),
        tf(flag(tp, "middleClick", false)),
    )
}

/// Whether a middle click (a three-finger click on a trackpad) pastes the
/// last selected text: off unless Settings turns it on, as on macOS
pub fn paste_spec(m: &Value) -> String {
    format!("hl.config({{ misc = {{ middle_click_paste = {} }} }})", tf(flag(m, "middlePaste", false)))
}

pub fn touchpad_device(name: &str, tp: &Value) -> String {
    format!("hl.device({{ name = \"{}\", sensitivity = {:.2} }})", lua_str(name), num(tp, "speed", 0.0).clamp(-1.0, 1.0))
}

/// Any setting change (Mouse.signature)
fn signature(cfg: &Value) -> String {
    let m = &cfg["mouse"];
    let t = &cfg["trackpad"];
    let g = |v: &Value, k: &str| v[k].to_string();
    [
        g(m, "accelProfile"), g(m, "speed"), g(m, "naturalScroll"), g(m, "scrollSpeed"), g(m, "leftHanded"), g(m, "middleEmulation"), g(m, "middlePaste"), g(m, "focus"),
        g(t, "speed"), g(t, "tapToClick"), g(t, "tapAndDrag"), g(t, "dragLock"), g(t, "naturalScroll"), g(t, "scrollSpeed"), g(t, "disableWhileTyping"), g(t, "rightClick"), g(t, "middleClick"),
    ]
    .join("|")
}

enum M {
    Raw(Raw),
    /// hyprctl's devices, and what udev says each one is
    Devices(Value, Option<Kinds>),
    /// the plugin's name, version, and whether its pointer hook is in place
    /// (a "nohook:<why>" in the ping: boundaries can't work, the rest can)
    Plugin(String, i64, bool),
    LookAgain(u64),
    UdevLine,
    UdevEnded,
    ApplyLater(u64),
    FocusLater(u64),
    UrgentSettle(u64),
    UrgentInfo { address: String, found: bool, active: bool, class: String, title: String, age: Option<i64> },
    Lease(u64),
    Poll,
}

pub struct Mouse {
    mice: Vec<String>,
    touchpads: Vec<String>,
    sig: String,
    apply_gen: u64,
    look_gen: u64,
    plugin_name: String,
    plugin_version: i64,
    plugin_hooked: bool,
    pending: Vec<Raw>,
    focus_gen: u64,
    urgent_window: String,
    urgent_gen: u64,
    opened_class: BTreeMap<String, (String, u64)>,
    boundary_sig: String,
    lease_gen: u64,
}

impl Default for Mouse {
    fn default() -> Self {
        Self {
            mice: vec![], touchpads: vec![], sig: String::new(), apply_gen: 0, look_gen: 0, plugin_name: "tessera-glass".into(), plugin_version: 0, plugin_hooked: true,
            pending: vec![], focus_gen: 0, urgent_window: String::new(), urgent_gen: 0, opened_class: BTreeMap::new(), boundary_sig: String::new(), lease_gen: 0,
        }
    }
}

impl Mouse {
    fn boundary_available(&self) -> bool {
        self.plugin_version >= 3 && self.plugin_hooked
    }
    fn follow_available(&self) -> bool {
        self.plugin_version >= 4
    }
    fn hover(h: &Host) -> bool {
        text(&h.cfg()["mouse"], "focus", "hover") == "hover"
    }
    fn locked(h: &Host) -> bool {
        h.state("locked").as_bool().unwrap_or(false)
    }
    /// ScreenTools.selecting: host-desktop's shell state "screentools" (17 Nov:
    /// read from this crate's bus, which nothing filled standalone, so a
    /// screenshot's selection never held the pointer to its screen and hover
    /// focus went on during it)
    fn selecting(h: &Host) -> String {
        h.state("screentools")["selecting"].as_str().unwrap_or("").to_string()
    }
    fn publish(&self) {
        crate::bus::put("mouse", json!({"mice": self.mice, "touchpads": self.touchpads, "boundaryAvailable": self.boundary_available()}));
    }

    fn apply(&mut self, h: &mut Host) {
        if h.cfg().is_null() {
            return;
        }
        let m = &h.cfg()["mouse"];
        let tp = &h.cfg()["trackpad"];
        let hover = Self::hover(h);
        let mut lua = format!("hl.config({{ input = {{ follow_mouse = {}, mouse_refocus = true, focus_on_close = {} }} }})\n", if hover { 1 } else { 2 }, if hover { 1 } else { 0 });
        for d in &self.mice {
            lua += &device_spec(d, m);
            lua.push('\n');
        }
        lua += &touchpad_spec(tp);
        lua.push('\n');
        lua += &paste_spec(m);
        lua.push('\n');
        for t in &self.touchpads {
            lua += &touchpad_device(t, tp);
            lua.push('\n');
        }
        hyprctl_eval(&lua);
        self.apply_boundaries(h);
    }

    fn plugin(&self, args: &[&str]) {
        let mut v = vec![self.plugin_name.as_str()];
        v.extend_from_slice(args);
        hyprctl(&v);
    }

    fn apply_boundaries(&mut self, h: &mut Host) {
        if h.cfg().is_null() {
            return;
        }
        if self.follow_available() {
            self.plugin(&["pointer-follow", if Self::hover(h) { "1" } else { "0" }]);
        }
        if !self.boundary_available() {
            return;
        }
        let m = &h.cfg()["mouse"];
        let mode = match text(m, "boundaryMode", "corners") {
            "corners" => "1",
            "resistance" => "2",
            _ => "0",
        };
        let size = (num(m, "cornerSize", 24.0).clamp(4.0, 100.0) as i64).to_string();
        self.plugin(&["pointer-policy", mode, &size]);
        self.confine_selection(h);
    }

    fn confine_selection(&mut self, h: &mut Host) {
        if !self.boundary_available() {
            return;
        }
        let off = text(&h.cfg()["mouse"], "boundaryMode", "corners") == "off";
        let sel = Self::selecting(h);
        let name = if !off && !Self::locked(h) && !sel.is_empty() { h.state("screentools")["selectingScreen"].as_str().unwrap_or("").to_string() } else { String::new() };
        self.plugin(&["pointer-confine", if name.is_empty() { "-" } else { &name }]);
        // A short lease releases confinement if the shell exits during selection
        if !off && !sel.is_empty() {
            self.lease_gen += 1;
            after(h.sender("Mouse"), 500, M::Lease(self.lease_gen));
        }
    }

    fn look(&mut self, h: &mut Host) {
        let send = h.sender("Mouse");
        std::thread::spawn(move || {
            let kinds = read_kinds(std::path::Path::new("/sys/class/input"), std::path::Path::new("/run/udev/data"));
            send(Box::new(M::Devices(hyprctl_json(&["devices"]), kinds)))
        });
    }

    fn bring_forward(&self, h: &Host, address: &str) {
        let mut lua = format!("hl.dispatch(hl.dsp.focus({{ window = {} }}))\n", serde_json::to_string(&format!("address:{address}")).unwrap_or_default());
        if Self::hover(h) {
            lua += &format!("dofile({})({})\n", serde_json::to_string(&crate::util::script("pointer-focus.lua")).unwrap_or_default(), serde_json::to_string(address).unwrap_or_default());
        }
        hyprctl_eval(&lua);
    }

    fn urgent_decided(&mut self, h: &mut Host, address: String, found: bool, active: bool, class: String, title: String, age: Option<i64>) {
        if !found {
            return;
        }
        let opened = self.opened_class.get(&address).cloned();
        let fresh = opened.as_ref().is_some_and(|(_, at)| now_ms().saturating_sub(*at) < 2000);
        let cls = opened.as_ref().map(|(c, _)| c.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| class.clone());
        // Tessera's own windows are only opened on purpose: they come forward,
        // never as a "wants your attention" alert
        if fresh || tessera_core::wm::own_window(&cls) {
            if !active {
                self.bring_forward(h, &address);
            }
            return;
        }
        match on_urgent(age, active) {
            "bring" => self.bring_forward(h, &address),
            "notify" => {
                let name = h
                    .st
                    .shell
                    .get::<tessera_core::services::apps::Apps>()
                    .and_then(|a| a.entry_for(&class))
                    .map(|e| e.name.clone())
                    .filter(|n| !n.is_empty())
                    .or_else(|| Some(title.clone()).filter(|t| !t.is_empty()))
                    .or_else(|| Some(class.clone()).filter(|c| !c.is_empty()))
                    .unwrap_or_else(|| "An app".into());
                super::common_island::post(h, json!({"kind": "custom", "icon": "bell-ringing", "title": name, "subtitle": "Wants your attention", "color": "accent", "action": "Show", "onAction": {"call": "mouse.bringForward", "args": [address]}}), 6000);
            }
            _ => {}
        }
    }
}

impl Mouse {
    /// The glass plugin: its name and version (a ping answers "<name> <version>");
    /// none answering: version 0 (its pointer features are off)
    fn ping_plugin(h: &Host) {
        let send = h.sender("Mouse");
        std::thread::spawn(move || {
            for n in ["tessera-glass"] {
                if let Some((0, o)) = crate::util::capture(&["hyprctl", n, "ping"], std::time::Duration::from_secs(3)) {
                    if o.trim().starts_with(n) {
                        let v = o.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(1);
                        let hooked = !o.split_whitespace().any(|w| w.starts_with("nohook"));
                        send(Box::new(M::Plugin(n.to_string(), v, hooked)));
                        return;
                    }
                }
            }
            send(Box::new(M::Plugin("tessera-glass".into(), 0, true)));
        });
    }
}

impl Service for Mouse {
    /// (a plugin loaded later, after Build glass or an update's rebuild, or gone)
    fn plugin_changed(&mut self, h: &mut Host) {
        Self::ping_plugin(h);
    }
    fn name(&self) -> &'static str {
        "Mouse"
    }
    fn calls(&self) -> &'static [&'static str] {
        &["mouse"]
    }
    fn start(&mut self, h: &mut Host) {
        self.sig = signature(h.cfg());
        // (only the events it acts on: every title change of a busy terminal woke it)
        spawn_hypr_events(&["configreloaded", "openwindow", "urgent", "workspace", "workspacev2", "activespecial", "activespecialv2", "closewindow", "movewindowv2", "changefloatingmode"], h.sender("Mouse"));
        Self::ping_plugin(h);
        // udev says when a mouse comes or goes; without udevadm, every 60 s
        // (asked on the thread: `udevadm --version` on the loop held up the start)
        let send = h.sender("Mouse");
        std::thread::spawn(move || {
            let udev = Command::new("udevadm").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if !udev {
                // (the first poll; each poll asks for the next: M::Poll)
                std::thread::sleep(std::time::Duration::from_secs(60));
                send(Box::new(M::Poll));
                return;
            }
            loop {
                let mut cmd = Command::new("udevadm");
                cmd.args(["monitor", "--udev", "--subsystem-match=input"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
                // (ends with this thread, so with the shell: one was left per restart)
                tessera_core::util::dies_with_us(&mut cmd);
                let child = cmd.spawn();
                if let Ok(mut c) = child {
                    if let Some(out) = c.stdout.take() {
                        for line in BufReader::new(out).lines().map_while(Result::ok) {
                            if line.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "add" || w == "remove") {
                                send(Box::new(M::UdevLine));
                            }
                        }
                    }
                    let _ = c.wait();
                }
                send(Box::new(M::UdevEnded));
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        });
        self.look(h);
        self.apply_gen += 1;
        after(h.sender("Mouse"), 150, M::ApplyLater(self.apply_gen));
    }
    fn config_changed(&mut self, h: &mut Host) {
        let sig = signature(h.cfg());
        if sig != self.sig {
            self.sig = sig;
            self.apply_gen += 1;
            after(h.sender("Mouse"), 150, M::ApplyLater(self.apply_gen));
        }
        let m = &h.cfg()["mouse"];
        let bs = format!("{}|{}", text(m, "boundaryMode", "corners"), num(m, "cornerSize", 24.0));
        if bs != self.boundary_sig {
            self.boundary_sig = bs;
            self.apply_boundaries(h);
        }
    }
    fn call(&mut self, h: &mut Host, call: &str, args: &Value) {
        match call.split_once('.').map(|x| x.1).unwrap_or("") {
            "bringForward" => {
                if let Some(a) = args.get(0).and_then(Value::as_str) {
                    self.bring_forward(h, a);
                }
            }
            // The lock screen or a screen selection changed (Mouse.confineSelection)
            "confine" => self.confine_selection(h),
            _ => {}
        }
    }
    fn message(&mut self, h: &mut Host, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Raw>() {
            Ok(e) => {
                if e.name == "configreloaded" {
                    self.apply_gen += 1;
                    after(h.sender("Mouse"), 150, M::ApplyLater(self.apply_gen));
                }
                if e.name == "openwindow" {
                    let f: Vec<&str> = e.data.split(',').collect();
                    let a = f[0].trim_start_matches("0x");
                    self.opened_class.insert(format!("0x{a}"), (f.get(2).unwrap_or(&"").to_string(), now_ms()));
                    if self.opened_class.len() > 64 {
                        if let Some(k) = self.opened_class.keys().next().cloned() {
                            self.opened_class.remove(&k);
                        }
                    }
                }
                if e.name == "urgent" {
                    let a = e.data.trim_start_matches("0x");
                    if !is_hex_addr(a) || Self::locked(h) {
                        return;
                    }
                    self.urgent_window = format!("0x{a}");
                    self.urgent_gen += 1;
                    after(h.sender("Mouse"), 250, M::UrgentSettle(self.urgent_gen));
                    return;
                }
                if !Self::hover(h) {
                    return;
                }
                if e.name == "openwindow" || SCENE_CHANGES.contains(&e.name.as_str()) {
                    self.pending.push((*e).clone());
                    self.focus_gen += 1;
                    after(h.sender("Mouse"), 180, M::FocusLater(self.focus_gen));
                }
                return;
            }
            Err(m) => m,
        };
        let Ok(m) = msg.downcast::<M>() else { return };
        match *m {
            M::Raw(_) => {}
            M::Devices(d, kinds) => {
                let mice = d["mice"].as_array().cloned().unwrap_or_default();
                let names: Vec<String> = mice.iter().filter_map(|m| m["name"].as_str()).filter(|n| mouse_by(n, kinds.as_ref())).map(String::from).collect();
                let pads: Vec<String> = mice.iter().filter_map(|m| m["name"].as_str()).filter(|n| pad_by(n, kinds.as_ref())).map(String::from).collect();
                if d.is_object() && (names != self.mice || pads != self.touchpads) {
                    self.mice = names;
                    self.touchpads = pads;
                    self.publish();
                    self.apply_gen += 1;
                    after(h.sender("Mouse"), 150, M::ApplyLater(self.apply_gen));
                }
            }
            M::Plugin(n, v, hooked) => {
                self.plugin_name = n;
                self.plugin_version = v;
                self.plugin_hooked = hooked;
                self.publish();
                self.apply_boundaries(h);
            }
            M::UdevLine => {
                self.look_gen += 1;
                let g = self.look_gen;
                // (one look for a burst of udev lines: only the last one's runs)
                after(h.sender("Mouse"), 800, M::LookAgain(g));
            }
            M::LookAgain(g) => {
                if g == self.look_gen {
                    self.look(h);
                }
            }
            M::UdevEnded => {}
            M::Poll => {
                self.look(h);
                after(h.sender("Mouse"), 60000, M::Poll);
            }
            M::ApplyLater(g) => {
                if g == self.apply_gen {
                    self.apply(h);
                }
            }
            M::FocusLater(g) => {
                if g != self.focus_gen {
                    return;
                }
                let act = after_events(&self.pending);
                self.pending.clear();
                if act == Act::None || h.cfg().is_null() || !Self::hover(h) || Self::locked(h) || !h.state("open").as_str().unwrap_or("").is_empty() || !Self::selecting(h).is_empty() {
                    return;
                }
                if act == Act::Refocus && self.boundary_available() {
                    self.plugin(&["pointer-refocus"]);
                    return;
                }
                let addr = if let Act::Warp(a) = &act { a.clone() } else { String::new() };
                let script = serde_json::to_string(&crate::util::script("pointer-focus.lua")).unwrap_or_default();
                hyprctl_eval(&format!("local focus = dofile({script}); focus({})", serde_json::to_string(&addr).unwrap_or_default()));
            }
            M::UrgentSettle(g) => {
                if g != self.urgent_gen || self.urgent_window.is_empty() {
                    return;
                }
                let address = std::mem::take(&mut self.urgent_window);
                let follow = self.follow_available();
                let name = self.plugin_name.clone();
                let send = h.sender("Mouse");
                std::thread::spawn(move || {
                    let age = if follow {
                        crate::util::capture(&["hyprctl", &name, "input-age"], std::time::Duration::from_secs(3)).and_then(|(_, o)| o.trim().parse::<i64>().ok())
                    } else {
                        None
                    };
                    let clients = hyprctl_json(&["clients"]);
                    let cl = clients.as_array().and_then(|a| a.iter().find(|c| c["address"] == address.as_str()).cloned());
                    let active = hyprctl_json(&["activewindow"])["address"] == address.as_str();
                    send(Box::new(M::UrgentInfo {
                        found: cl.is_some(),
                        active,
                        class: cl.as_ref().and_then(|c| c["class"].as_str()).unwrap_or("").into(),
                        title: cl.as_ref().and_then(|c| c["title"].as_str()).unwrap_or("").into(),
                        address,
                        age,
                    }));
                });
            }
            M::UrgentInfo { address, found, active, class, title, age } => self.urgent_decided(h, address, found, active, class, title, age),
            M::Lease(g) => {
                if g == self.lease_gen {
                    self.confine_selection(h);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_stay_inside_their_lua_string() {
        assert_eq!(lua_str("Logi \"MX\" \\ 3"), "Logi \\\"MX\\\" \\\\ 3");
        assert_eq!(lua_str("Buds\nPro"), "Buds\\010Pro");
        assert!(!device_spec("a\nb", &json!({})).contains('\n'));
    }

    fn ev(n: &str, d: &str) -> Raw {
        Raw { name: n.into(), data: d.into() }
    }

    #[test]
    fn focus_decisions() {
        assert_eq!(after_events(&[]), Act::None);
        assert_eq!(after_events(&[ev("workspacev2", "1,1")]), Act::Refocus);
        assert_eq!(after_events(&[ev("workspacev2", "1,1"), ev("openwindow", "55aa,1,kitty,kitty")]), Act::Warp("0x55aa".into()));
        assert_eq!(after_events(&[ev("openwindow", "zz,1,k,k")]), Act::None);
        assert_eq!(on_urgent(Some(500), false), "bring");
        assert_eq!(on_urgent(Some(5000), false), "notify");
        assert_eq!(on_urgent(None, false), "notify");
        assert_eq!(on_urgent(Some(1), true), "none");
    }

    #[test]
    fn trackpads_by_what_the_system_says() {
        // (#597) bcm5974: a MacBook's trackpad, its name doesn't say so
        assert_eq!(hypr_name("bcm5974"), "bcm5974");
        assert_eq!(hypr_name("SynPS/2 Synaptics TouchPad\n"), "synps/2-synaptics-touchpad");
        assert_eq!(udev_kind("E:ID_INPUT=1\nE:ID_INPUT_TOUCHPAD=1\nE:ID_INPUT_WIDTH_MM=105\n"), Some(true));
        assert_eq!(udev_kind("E:ID_INPUT=1\nE:ID_INPUT_MOUSE=1\n"), Some(false));
        assert_eq!(udev_kind("E:ID_INPUT=1\n"), None);
        let d = std::env::temp_dir().join(format!("tessera-mouse-kinds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (sys, udev) = (d.join("sys"), d.join("udev"));
        for (ev, name, dev, rec) in [("event5", "bcm5974", "13:69", "E:ID_INPUT_TOUCHPAD=1\n"), ("event6", "Logitech MX Master 3", "13:70", "E:ID_INPUT_MOUSE=1\n")] {
            std::fs::create_dir_all(sys.join(ev).join("device")).unwrap();
            std::fs::write(sys.join(ev).join("device/name"), format!("{name}\n")).unwrap();
            std::fs::write(sys.join(ev).join("dev"), format!("{dev}\n")).unwrap();
            std::fs::create_dir_all(&udev).unwrap();
            std::fs::write(udev.join(format!("c{dev}")), rec).unwrap();
        }
        let k = read_kinds(&sys, &udev).unwrap();
        assert!(pad_by("bcm5974", Some(&k)) && !mouse_by("bcm5974", Some(&k)));
        assert!(mouse_by("logitech-mx-master-3", Some(&k)) && !pad_by("logitech-mx-master-3", Some(&k)));
        // (a device udev didn't describe, or no database at all: by its name)
        assert!(pad_by("synaptics-touchpad", Some(&k)) && pad_by("synaptics-touchpad", None));
        assert!(!pad_by("bcm5974", None) && mouse_by("bcm5974", None));
        assert!(read_kinds(&sys, &d.join("none")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn device_lua() {
        assert!(is_mouse("logitech-mx") && !is_mouse("Synaptics Touchpad") && !is_mouse("ydotoold virtual device"));
        let s = device_spec("m\"x", &json!({"speed": 3, "accelProfile": "flat", "scrollSpeed": 9}));
        assert_eq!(s, "hl.device({ name = \"m\\\"x\", accel_profile = \"flat\", sensitivity = 1.00, natural_scroll = false, scroll_factor = 3.00, left_handed = false, middle_button_emulation = false })");
        assert!(touchpad_spec(&json!({"rightClick": "corner"})).contains("clickfinger_behavior = false"));
        assert_eq!(paste_spec(&json!({})), "hl.config({ misc = { middle_click_paste = false } })");
        assert_eq!(paste_spec(&json!({"middlePaste": true})), "hl.config({ misc = { middle_click_paste = true } })");
        assert!(touchpad_spec(&json!({})).contains("tap_to_click = true, tap_and_drag = true, drag_lock = 0, natural_scroll = true, scroll_factor = 0.70"));
    }
}
