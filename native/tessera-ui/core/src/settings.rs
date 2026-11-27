//! Settings more than one surface reads, from config.json with Config.qml's
//! defaults (each surface's own are in its crate's settings.rs)

use serde_json::Value;

/// The island shows the HUD instead (Glass only; either menu bar's island:
/// Osd.qml's Island.hud reaches the Rust one over the bridge)
pub fn island_hud(cfg: &Value, material: bool) -> bool {
    let i = &cfg["island"];
    !material && i["enabled"].as_bool().unwrap_or(true) && i["hud"].as_bool().unwrap_or(true)
}

/// Where the on-screen keyboard's button is offered beside a password
/// field (the lock, the login screen, the admin password dialog), from
/// Settings → Accessibility (#763): `keyboard.oskButton`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OskButton {
    Always,
    /// only on a computer with a touch screen (the default)
    Touch,
    Off,
}

impl OskButton {
    /// The saved value; anything else (an older config, a typo) is the default
    pub fn from_cfg(cfg: &Value) -> Self {
        match cfg["keyboard"]["oskButton"].as_str().unwrap_or("touch") {
            "always" => Self::Always,
            "off" => Self::Off,
            _ => Self::Touch,
        }
    }
    /// Shown here? `touch_screen`: this computer has one (touch::has_touch)
    pub fn shown(self, touch_screen: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Touch => touch_screen,
            Self::Off => false,
        }
    }
}

/// The emoji font for Settings' style (EmojiFont.qml), Noto when that one
/// isn't installed
pub fn emoji_font(cfg: &Value, installed: &[String]) -> String {
    let want = match cfg["emoji"]["style"].as_str().unwrap_or("android") {
        "apple" => "Apple Color Emoji",
        "windows" => "Fluent Emoji Color",
        _ => "Noto Color Emoji",
    };
    let has = |f: &str| installed.iter().any(|i| i.eq_ignore_ascii_case(f) || (f == "Fluent Emoji Color" && i.eq_ignore_ascii_case("FluentEmojiColor")));
    if installed.is_empty() || has(want) {
        want.into()
    } else {
        "Noto Color Emoji".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_keyboard_button_defaults_to_touch_screens() {
        assert_eq!(OskButton::from_cfg(&json!({})), OskButton::Touch);
        assert_eq!(OskButton::from_cfg(&json!({"keyboard": {"superTap": "overview"}})), OskButton::Touch);
        assert_eq!(OskButton::from_cfg(&json!({"keyboard": {"oskButton": "nonsense"}})), OskButton::Touch);
        assert_eq!(OskButton::from_cfg(&json!({"keyboard": {"oskButton": "always"}})), OskButton::Always);
        assert_eq!(OskButton::from_cfg(&json!({"keyboard": {"oskButton": "off"}})), OskButton::Off);
        assert!(OskButton::Touch.shown(true) && !OskButton::Touch.shown(false));
        assert!(OskButton::Always.shown(false) && !OskButton::Off.shown(true));
    }
}
