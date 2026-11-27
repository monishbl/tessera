//! Icons by freedesktop name: the tray's IconName, an app's Icon= from its
//! desktop entry (the spaces' app icons). A small lookup, not the whole spec:
//! the user's GTK icon theme, then Papirus, breeze, Adwaita and hicolor,
//! trying the sizes a 16-32 px icon looks best at (scalable first), then
//! /usr/share/pixmaps. Answers are remembered until the shell's index is
//! built again (an icon installed while it runs: services/icons.rs watches),
//! a miss for MISS_KEPT at most (#846: a miss was kept all session).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let user = crate::util::xdg("XDG_DATA_HOME", ".local/share");
    let sys = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut v = vec![home.join(".icons"), user.join("icons")];
    v.extend(sys.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("icons")));
    v
}

/// The GTK icon theme the user chose (gtk-3.0/settings.ini), if any
fn user_theme() -> Option<String> {
    let cfg = crate::util::xdg("XDG_CONFIG_HOME", ".config");
    let text = std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("gtk-icon-theme-name").map(|v| v.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string()))
}

// (the large ones last: an app that ships only 512, 192, 96 or 72 px icons,
// as many Electron and Flatpak apps do, was never found; #846)
const SIZES: &[&str] = &["scalable", "32x32", "32", "24x24", "24", "22x22", "22", "48x48", "48", "16x16", "16", "64x64", "64", "128x128", "256x256", "96x96", "72x72", "192x192", "512x512", "symbolic"];
const KINDS: &[&str] = &["apps", "status", "devices", "panel", "actions", "categories", "places", "mimetypes", "emblems", "legacy"];

/// Where a name's file is in one theme folder (both layouts: size/kind and kind/size)
pub fn in_theme(dir: &Path, name: &str) -> Option<PathBuf> {
    // (a miss walks every theme in every data folder: only the folders that
    // exist are looked in, not all 640 paths of each)
    if !dir.is_dir() {
        return None;
    }
    let sizes: Vec<&str> = SIZES.iter().copied().filter(|s| dir.join(s).is_dir()).collect();
    let kinds: Vec<&str> = KINDS.iter().copied().filter(|k| dir.join(k).is_dir()).collect();
    for size in SIZES {
        for kind in KINDS {
            for (a, b) in [(size, kind), (kind, size)] {
                if !sizes.contains(a) && !kinds.contains(a) {
                    continue;
                }
                for ext in ["svg", "png"] {
                    let p = dir.join(a).join(b).join(format!("{name}.{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn lookup(name: &str, extra: Option<&Path>) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    // an app's own folder (the tray's IconThemePath): flat, or a theme
    if let Some(e) = extra {
        for ext in ["svg", "png"] {
            let p = e.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        if let Some(p) = in_theme(e, name).or_else(|| in_theme(&e.join("hicolor"), name)) {
            return Some(p);
        }
    }
    // The shell's own index (services/icons.rs: the theme Settings chose,
    // e.g. Colloid, the themes it inherits, hicolor, the pixmaps; the QML's
    // IconLookup), with its name variants (case, a reverse-DNS id's last part)
    if let Some(icons) = crate::services::icons::shared() {
        if let Some(p) = icons.exact(name) {
            return Some(p);
        }
    }
    // (before the index is built, or a name it hasn't: the themes walked
    // here, the shell's theme and what it inherits first, as the QML's
    // Quickshell.iconPath fallback does)
    // (read once: the shell's theme is chosen at start, as Quickshell's)
    static SHELL_CHAIN: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let mut themes: Vec<String> = SHELL_CHAIN
        .get_or_init(|| {
            let roots = crate::services::icons::roots();
            crate::services::icons::chain_of(&roots, &crate::services::icons::theme_name(&crate::theme::read_json()))
        })
        .clone();
    themes.extend(user_theme());
    for t in ["Papirus", "breeze", "Adwaita", "hicolor"] {
        if !themes.iter().any(|x| x == t) {
            themes.push(t.into());
        }
    }
    let dirs = data_dirs();
    for t in &themes {
        for d in &dirs {
            if let Some(p) = in_theme(&d.join(t), name) {
                return Some(p);
            }
        }
    }
    // (no .xpm: nothing here decodes it, and a path found skips the
    // callers' generic fallback icon)
    for ext in ["svg", "png"] {
        let p = PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Answers by name: the file (None: a miss) and when it was looked for
static CACHE: Mutex<Option<HashMap<String, (Option<PathBuf>, std::time::Instant)>>> = Mutex::new(None);

/// How long a miss is trusted (an app installed meanwhile brings its icon)
pub const MISS_KEPT: std::time::Duration = std::time::Duration::from_secs(60);

/// A remembered answer still good at `now`: a file always (until forget), a miss for MISS_KEPT
fn still_good(hit: &(Option<PathBuf>, std::time::Instant), now: std::time::Instant) -> bool {
    hit.0.is_some() || now.saturating_duration_since(hit.1) < MISS_KEPT
}

/// The file for an icon name (`extra`: a folder of the app's own icons)
pub fn find(name: &str, extra: Option<&Path>) -> Option<PathBuf> {
    let key = format!("{name}\n{}", extra.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    let now = std::time::Instant::now();
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&key).cloned())) {
        if still_good(&hit, now) {
            return hit.0;
        }
    }
    let found = lookup(name, extra);
    // (a miss before the shell's index is built isn't kept: asked again once it is)
    if found.is_some() || crate::services::icons::shared().is_some() {
        if let Ok(mut c) = CACHE.lock() {
            c.get_or_insert_with(HashMap::new).insert(key, (found.clone(), now));
        }
    }
    found
}

/// What icon lookups depend on, as one string: the shell's index (its
/// theme, and which build of it) and the user's GTK icon theme. When it
/// changes, remembered answers are stale (the tray's, audit #52)
pub fn theme_stamp() -> String {
    let index = crate::services::icons::shared().map(|i| format!("{}@{:p}", i.theme, std::sync::Arc::as_ptr(&i.index))).unwrap_or_default();
    format!("{index}\n{}", user_theme().unwrap_or_default())
}

/// The shell's index was (re)built: answers found without it are dropped
pub fn forget() {
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

/// An app's icon by its window class (as Apps.iconFor: its desktop entry's
/// Icon=, else the class itself as an icon name)
pub fn for_class(class: &str) -> Option<PathBuf> {
    let icon = crate::util::desktop_field(class, "Icon").unwrap_or_else(|| class.to_lowercase());
    find(&icon, None).or_else(|| find(class, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misses_are_kept_a_minute() {
        let t = std::time::Instant::now();
        assert!(still_good(&(Some(PathBuf::from("/a.png")), t), t + std::time::Duration::from_secs(3600)));
        assert!(still_good(&(None, t), t + std::time::Duration::from_secs(30)));
        assert!(!still_good(&(None, t), t + MISS_KEPT));
        // the large sizes Electron and Flatpak apps ship
        let d = std::env::temp_dir().join(format!("tessera-icons-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("hicolor/512x512/apps")).unwrap();
        std::fs::write(d.join("hicolor/512x512/apps/app.png"), b"x").unwrap();
        assert_eq!(in_theme(&d.join("hicolor"), "app"), Some(d.join("hicolor/512x512/apps/app.png")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn both_theme_layouts() {
        let d = std::env::temp_dir().join(format!("tessera-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("t1/32x32/apps")).unwrap();
        std::fs::write(d.join("t1/32x32/apps/foo.png"), b"x").unwrap();
        std::fs::create_dir_all(d.join("t2/apps/scalable")).unwrap();
        std::fs::write(d.join("t2/apps/scalable/bar.svg"), b"x").unwrap();
        assert_eq!(in_theme(&d.join("t1"), "foo"), Some(d.join("t1/32x32/apps/foo.png")));
        assert_eq!(in_theme(&d.join("t2"), "bar"), Some(d.join("t2/apps/scalable/bar.svg")));
        assert_eq!(in_theme(&d.join("t2"), "foo"), None);
        // an app's own flat folder wins
        std::fs::write(d.join("own.png"), b"x").unwrap();
        assert_eq!(lookup("own", Some(&d)), Some(d.join("own.png")));
        assert_eq!(lookup("", None), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
