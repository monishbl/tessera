//! Plain names for the keys Hyprland reports by their XKB names, shared by
//! the shortcuts sheet, Settings → Keyboard and the shell's bind probes:
//! media keys ("XF86AudioRaiseVolume" → "Volume Up"), symbol keys
//! ("Equal" → "=") and the keypad keys bound by keycode (86 → "Keypad +").

/// An XF86 media key's short name ("XF86AudioMute" → "Mute"); None when `k`
/// isn't an XF86 key. Unknown ones are split into words ("XF86Foo" → "Foo").
pub fn xf86(k: &str) -> Option<String> {
    let rest = k.strip_prefix("XF86")?;
    let known = match rest {
        "AudioRaiseVolume" => "Volume Up",
        "AudioLowerVolume" => "Volume Down",
        "AudioMute" => "Mute",
        "AudioMicMute" => "Mic Mute",
        "AudioPlay" => "Play",
        "AudioPause" => "Pause",
        "AudioStop" => "Stop",
        "AudioNext" => "Next",
        "AudioPrev" => "Previous",
        "AudioRewind" => "Rewind",
        "AudioForward" => "Fast Forward",
        "AudioMedia" => "Media",
        "MonBrightnessUp" => "Brightness Up",
        "MonBrightnessDown" => "Brightness Down",
        "KbdBrightnessUp" => "Keyboard Light Up",
        "KbdBrightnessDown" => "Keyboard Light Down",
        "KbdLightOnOff" => "Keyboard Light",
        "TouchpadToggle" => "Touchpad",
        "WLAN" => "Wi-Fi",
        "PowerOff" => "Power",
        "Display" => "Display",
        "Calculator" => "Calculator",
        "Search" => "Search",
        "Mail" => "Mail",
        "Explorer" => "Files",
        "HomePage" => "Home Page",
        _ => "",
    };
    if !known.is_empty() {
        return Some(known.to_string());
    }
    // "SomeNewKey" -> "Some New Key"
    let mut out = String::new();
    let mut prev_lower = false;
    for c in rest.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    Some(out)
}

/// A symbol key's sign ("Equal" → "=", "Minus" → "−", "KP_Add" → "Keypad +")
pub fn symbol(k: &str) -> Option<&'static str> {
    Some(match k {
        "Period" | "period" => ".",
        "Comma" | "comma" => ",",
        "Slash" | "slash" => "/",
        "Semicolon" | "semicolon" => ";",
        "Apostrophe" | "apostrophe" => "'",
        "Minus" | "minus" => "−",
        "Equal" | "equal" => "=",
        "Plus" | "plus" => "+",
        "bracketleft" | "BracketLeft" => "[",
        "bracketright" | "BracketRight" => "]",
        "grave" | "Grave" => "`",
        "backslash" | "Backslash" => "\\",
        "KP_Add" => "Keypad +",
        "KP_Subtract" => "Keypad −",
        "KP_Multiply" => "Keypad ×",
        "KP_Divide" => "Keypad ÷",
        "KP_Enter" => "Keypad Enter",
        _ => return None,
    })
}

/// A key bound by its keycode (`code:86`), as Hyprland lists it with an
/// empty key name: the keypad's signs by name, others "Key 86"
pub fn keycode(code: i64) -> String {
    match code {
        86 => "Keypad +".into(),
        82 => "Keypad −".into(),
        63 => "Keypad ×".into(),
        106 => "Keypad ÷".into(),
        104 => "Keypad Enter".into(),
        n => format!("Key {n}"),
    }
}

/// The modifier a modifier key's own keysym stands for ("Alt_L" → "Alt");
/// Super's keysyms are left out (a Super tap is its own shortcut)
pub fn modifier_of(k: &str) -> Option<&'static str> {
    match k.to_ascii_lowercase().as_str() {
        "alt_l" | "alt_r" => Some("Alt"),
        "control_l" | "control_r" => Some("Ctrl"),
        "shift_l" | "shift_r" => Some("Shift"),
        _ => None,
    }
}

/// Keybinds.prettyKey: one key of `hyprctl -j binds` as a chip's text
pub fn pretty_key(k: &str) -> String {
    let mapped = match k {
        "Return" => Some("Enter"),
        "space" | "Space" => Some("Space"),
        "Tab" => Some("Tab"),
        "Escape" => Some("Esc"),
        "Print" => Some("Print Screen"),
        "BackSpace" => Some("Backspace"),
        "Page_Up" => Some("Page Up"),
        "Page_Down" => Some("Page Down"),
        "mouse:272" => Some("Left Click"),
        "mouse:273" => Some("Right Click"),
        "mouse:274" => Some("Middle Click"),
        "SUPER_L" | "SUPER_R" | "Super_L" | "Super_R" => Some(""),
        "mouse_down" => Some("Scroll Down"),
        "mouse_up" => Some("Scroll Up"),
        _ => symbol(k),
    };
    if let Some(m) = mapped {
        return m.to_string();
    }
    if let Some(x) = xf86(k) {
        return x;
    }
    if k.chars().count() == 1 { k.to_uppercase() } else { k.to_string() }
}

/// One bind of `hyprctl -j binds` as its chips: the held modifiers, then the
/// key (by name, or by keycode when bound as `code:N`). A release bind on a
/// modifier alone ("ALT + Alt_L", let go of Alt) reads "Alt" "release".
pub fn bind_keys(modmask: u32, key: &str, keycode: i64, release: bool) -> Vec<String> {
    let mut keys: Vec<String> = [(64, "Super"), (4, "Ctrl"), (8, "Alt"), (1, "Shift")].iter().filter(|(bit, _)| modmask & bit != 0).map(|(_, n)| n.to_string()).collect();
    if let Some(m) = modifier_of(key) {
        if !keys.iter().any(|k| k == m) {
            keys.push(m.to_string());
        }
        if release {
            keys.push("release".into());
        }
        return keys;
    }
    let k = if key.is_empty() && keycode > 0 { self::keycode(keycode) } else { pretty_key(key) };
    if !k.is_empty() {
        keys.push(k);
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_keys_read_as_words() {
        assert_eq!(xf86("XF86AudioRaiseVolume").as_deref(), Some("Volume Up"));
        assert_eq!(xf86("XF86AudioLowerVolume").as_deref(), Some("Volume Down"));
        assert_eq!(xf86("XF86AudioMute").as_deref(), Some("Mute"));
        assert_eq!(xf86("XF86AudioMicMute").as_deref(), Some("Mic Mute"));
        assert_eq!(xf86("XF86MonBrightnessUp").as_deref(), Some("Brightness Up"));
        assert_eq!(xf86("XF86MonBrightnessDown").as_deref(), Some("Brightness Down"));
        assert_eq!(xf86("XF86AudioPlay").as_deref(), Some("Play"));
        assert_eq!(xf86("XF86AudioPause").as_deref(), Some("Pause"));
        assert_eq!(xf86("XF86AudioNext").as_deref(), Some("Next"));
        assert_eq!(xf86("XF86AudioPrev").as_deref(), Some("Previous"));
        assert_eq!(xf86("XF86SomeNewKey").as_deref(), Some("Some New Key"));
        assert_eq!(xf86("Return"), None);
    }

    #[test]
    fn symbols_and_keypad() {
        assert_eq!(pretty_key("Equal"), "=");
        assert_eq!(pretty_key("Minus"), "−");
        assert_eq!(pretty_key("period"), ".");
        assert_eq!(pretty_key("KP_Add"), "Keypad +");
        assert_eq!(pretty_key("Print"), "Print Screen");
        assert_eq!(pretty_key("q"), "Q");
        assert_eq!(pretty_key("SUPER_L"), "");
        assert_eq!(keycode(86), "Keypad +");
        assert_eq!(keycode(82), "Keypad −");
        assert_eq!(keycode(12), "Key 12");
    }

    #[test]
    fn binds_as_chips() {
        // "$MOD + code:86": Hyprland lists key "" and keycode 86
        assert_eq!(bind_keys(64, "", 86, false), ["Super", "Keypad +"]);
        assert_eq!(bind_keys(64, "", 82, false), ["Super", "Keypad −"]);
        // the switcher's "finish when Alt is let go"
        assert_eq!(bind_keys(8, "Alt_L", 0, true), ["Alt", "release"]);
        assert_eq!(bind_keys(0, "XF86AudioRaiseVolume", 0, false), ["Volume Up"]);
        assert_eq!(bind_keys(65, "S", 0, false), ["Super", "Shift", "S"]);
        // a Super tap stays "Super" alone
        assert_eq!(bind_keys(64, "SUPER_L", 0, true), ["Super"]);
        assert_eq!(bind_keys(64, "", 0, false), ["Super"]);
    }
}
