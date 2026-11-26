//! Icons by Phosphor name, as shell/common/Icon.qml draws them: the Phosphor
//! glyph (shell/common/phosphor.js), and in the Material look the matching
//! Material Symbols Rounded one where there is one (material-symbols.js).
//! The names come from the shell at run time (Focus modes, devices); the
//! maps are compiled in (an install without the QML tree still has icons),
//! and a name they don't know draws a question mark, not nothing.
//!
//! (Slint can't set the Symbols font's FILL axis: Material's filled icons
//! draw outlined.)

use std::collections::HashMap;
use std::sync::OnceLock;

/// `"name": "\uXXXX",` lines of a generated map
pub fn parse_map(src: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('"') else { continue };
        let Some((name, rest)) = rest.split_once('"') else { continue };
        let Some(i) = rest.find("\\u") else { continue };
        let hex: String = rest[i + 2..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            out.insert(name.to_string(), c.to_string());
        }
    }
    out
}

const PHOSPHOR: &str = include_str!("../../../../../shell/common/phosphor.js");
const SYMBOLS: &str = include_str!("../../../../../shell/common/material-symbols.js");

fn phosphor() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(|| parse_map(PHOSPHOR))
}

fn symbols() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(|| parse_map(SYMBOLS))
}

/// (glyph, drawn with the Symbols font)
pub fn glyph(name: &str, material: bool) -> (String, bool) {
    if material {
        if let Some(g) = symbols().get(name) {
            return (g.clone(), true);
        }
    }
    if let Some(g) = phosphor().get(name) {
        return (g.clone(), false);
    }
    if name.is_empty() {
        return (String::new(), false);
    }
    // (a new or misspelt name: a neutral glyph, said once in the log)
    static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut s) = SAID.lock() {
        if !s.iter().any(|x| x == name) {
            eprintln!("tessera-ui: no icon called {name:?}");
            s.push(name.to_string());
        }
    }
    (phosphor().get("question").cloned().unwrap_or_default(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_read_from_the_generated_files() {
        let m = parse_map("var glyphs = {\n    \"wifi-high\": \"\\ue4ea\",\n    \"x\": \"\\uE000\"\n};");
        assert_eq!(m["wifi-high"], "\u{e4ea}");
        assert_eq!(m["x"], "\u{e000}");
        // the repo's own
        assert_eq!(glyph("caret-left", false), ("\u{e138}".to_string(), false));
        let (g, sym) = glyph("arrow-clockwise", true);
        assert!(sym && !g.is_empty());
        assert_eq!(glyph("nothing-called-this", true), glyph("question", false));
        assert!(!glyph("nothing-called-this", false).0.is_empty());
        assert_eq!(glyph("", false), (String::new(), false));
    }
}
