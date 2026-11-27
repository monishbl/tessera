//! Credentials for the AI models, where shell/services/AiKeys.qml keeps them:
//! an environment variable, else the system keyring (secret-tool, the
//! attributes `application tessera provider <slot>`, or the old name's (old_names)
//! from before the rename), else the private 0600 file
//! $XDG_STATE_HOME/tessera/ai-keys.json when no keyring runs. Keys stay in
//! memory and only ever go into request headers: never printed, logged or
//! written anywhere but those two places. A key goes to secret-tool on its
//! stdin, never on a command line.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

const ENV: &[(&str, &str)] = &[
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("gemini", "GEMINI_API_KEY"),
    ("aistudio", "GEMINI_API_KEY"),
    ("vertex", "VERTEX_API_KEY"),
    ("vertex-oauth", "GOOGLE_OAUTH_ACCESS_TOKEN"),
    ("openai", "OPENAI_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
];
/// The emoji picker's KLIPY app key: in the keyring with the AI credentials
/// (the private file without one), no longer in config.json (#965)
pub const KLIPY: &str = "klipy";
/// config.json's marker that the KLIPY key changed (never the key: a count,
/// so the picker knows to read the keyring again)
pub const KLIPY_SAVED: &str = "emoji.klipyKeySaved";

/// The slots looked up besides each model's own key
pub const SLOTS: &[&str] = &["anthropic", "gemini", "aistudio", "vertex", "vertex-oauth", "openai", "openrouter"];

#[derive(Clone, Default)]
pub struct Keys {
    keys: HashMap<String, String>,
    /// slots this process changed (a late keyring answer doesn't undo them)
    edited: std::collections::HashSet<String>,
}

/// (never the keys themselves)
impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keys({} slots)", self.keys.len())
    }
}

fn file_path() -> std::path::PathBuf {
    super::chats::state_dir().join("ai-keys.json")
}

impl Keys {
    /// The credential for a model: `provider` is its credential provider
    /// (models::credential_provider), `slot` its key; `env` reads a variable
    pub fn get_with(&self, provider: &str, slot: &str, env: impl Fn(&str) -> Option<String>) -> String {
        let var = ENV.iter().find(|e| e.0 == provider).and_then(|e| env(e.1)).filter(|v| !v.is_empty());
        let google = if provider == "aistudio" || provider == "vertex" { env("GOOGLE_API_KEY").filter(|v| !v.is_empty()) } else { None };
        let k = |s: &str| self.keys.get(s).filter(|v| !v.is_empty()).cloned();
        // (a key typed for this very model wins over the environment's: an
        // old exported key must not silently override a new one)
        k(slot).or(var).or(google).or_else(|| k(provider)).or_else(|| if provider == "aistudio" { k("gemini") } else { None }).unwrap_or_default()
    }
    pub fn get(&self, provider: &str, slot: &str) -> String {
        self.get_with(provider, slot, |v| std::env::var(v).ok())
    }
    pub fn has(&self, provider: &str, slot: &str) -> bool {
        provider == "ollama" || !self.get(provider, slot).is_empty()
    }

    /// Everything there is: the file first, the keyring's entries over it
    /// (blocking: run it on a thread)
    pub fn load(slots: &[String]) -> Keys {
        let mut k = Keys::default();
        if let Ok(text) = std::fs::read_to_string(file_path()) {
            if let Ok(serde_json::Value::Object(o)) = serde_json::from_str(&text) {
                for (s, v) in o {
                    if let Some(v) = v.as_str() {
                        k.keys.insert(s, v.to_string());
                    }
                }
            }
        }
        for slot in slots {
            for app in ["tessera", crate::old_names::KEYRING_APP] {
                let out = Command::new("secret-tool").args(["lookup", "application", app, "provider", slot]).stdin(Stdio::null()).stderr(Stdio::null()).output();
                let Ok(out) = out else { return k }; // (no secret-tool at all)
                let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if out.status.success() && !v.is_empty() {
                    k.keys.insert(slot.clone(), v);
                    break;
                }
            }
        }
        k
    }

    /// Keep one (the composer's key field, /key): in memory now, in the
    /// keyring on a thread, the private file if the keyring can't
    pub fn set(&mut self, slot: &str, key: &str, label: &str) {
        self.set_then(slot, key, label, |_| {});
    }

    /// set, then `done(kept)` on its thread: whether the keyring (or the
    /// private file) really has it
    pub fn set_then(&mut self, slot: &str, key: &str, label: &str, done: impl FnOnce(bool) + Send + 'static) {
        let key = key.trim().to_string();
        self.keys.insert(slot.to_string(), key.clone());
        self.edited.insert(slot.to_string());
        let (slot, label) = (slot.to_string(), label.to_string());
        std::thread::spawn(move || {
            done(store(&slot, &key, &label));
        });
    }

    /// One slot's saved credential, "" when none (blocking: run it on a thread)
    pub fn lookup(slot: &str) -> String {
        Keys::load(&[slot.to_string()]).keys.remove(slot).unwrap_or_default().trim().to_string()
    }

    /// A late reading (from load) over what's here, keeping what this process set
    pub fn merge(&mut self, other: Keys) {
        for (s, v) in other.keys {
            if !self.edited.contains(&s) {
                self.keys.insert(s, v);
            }
        }
    }

    #[cfg(test)]
    pub fn with(pairs: &[(&str, &str)]) -> Keys {
        Keys { keys: pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), ..Default::default() }
    }
}

/// Keep one credential (blocking: run it on a thread): the keyring, on
/// secret-tool's stdin, else the private 0600 file; whether it was kept
pub fn store(slot: &str, key: &str, label: &str) -> bool {
    let stored = Command::new("secret-tool")
        .args(["store", &format!("--label=Tessera · {label}"), "application", "tessera", "provider", slot])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut c| {
            c.stdin.take().unwrap().write_all(key.as_bytes())?;
            c.wait()
        })
        .is_ok_and(|s| s.success());
    if stored {
        return true;
    }
    // (read again under a lock and merged: each save wrote its own stale
    // copy, so a second key in one session dropped the first; audit #26)
    static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut file: HashMap<String, String> = std::fs::read_to_string(file_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    file.insert(slot.to_string(), key.to_string());
    super::chats::write_private(&file_path(), &serde_json::to_string(&file).unwrap()).is_ok()
}

/// Settings' KLIPY key field (blocking: on a thread): into the keyring (or
/// the private file) and out of config.json, whose marker counts up so the
/// picker reads it again. Only when neither can take it does it stay in
/// config.json (private to you since #965). Whether the keyring or file kept it.
pub fn save_klipy(key: &str) -> bool {
    let key = key.trim().to_string();
    if key.is_empty() {
        return false;
    }
    let kept = store(KLIPY, &key, "KLIPY app key");
    let _ = crate::cfgfile::edit_config(|m| klipy_moved(m, if kept { None } else { Some(&key) }));
    kept
}

/// An emoji.klipyKey still in config.json (from before #965) moved into the
/// keyring, once (blocking: on a thread, at the start)
pub fn migrate_klipy() {
    let old = crate::cfgfile::config_value()["emoji"]["klipyKey"].as_str().unwrap_or("").trim().to_string();
    if !old.is_empty() && store(KLIPY, &old, "KLIPY app key") {
        // (only if it's still that key: one typed meanwhile is left to its own save)
        let _ = crate::cfgfile::edit_config(|m| m.get("emoji").and_then(|e| e["klipyKey"].as_str()).is_some_and(|k| k.trim() == old) && klipy_moved(m, None));
    }
}

/// config.json once a KLIPY key moved: no key in it (`keep` when nothing
/// else could take it), the marker counted up
fn klipy_moved(m: &mut serde_json::Map<String, serde_json::Value>, keep: Option<&str>) -> bool {
    use serde_json::{json, Value};
    let e = m.entry("emoji").or_insert_with(|| json!({}));
    if !e.is_object() {
        *e = json!({});
    }
    let Some(e) = e.as_object_mut() else { return false };
    match keep {
        Some(k) => {
            e.insert("klipyKey".into(), json!(k));
        }
        None => {
            e.remove("klipyKey");
        }
    }
    let n = e.get("klipyKeySaved").and_then(Value::as_u64).unwrap_or(0) + 1;
    e.insert("klipyKeySaved".into(), json!(n));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_order_as_the_shell() {
        let k = Keys::with(&[("custom|m", "slot-key"), ("openai", "provider-key"), ("gemini", "old-gemini")]);
        let none = |_: &str| None;
        assert_eq!(k.get_with("custom", "custom|m", none), "slot-key");
        assert_eq!(k.get_with("openai", "openai|x", none), "provider-key");
        assert_eq!(k.get_with("aistudio", "aistudio|x", none), "old-gemini");
        assert_eq!(k.get_with("openai", "openai|x", |v| (v == "OPENAI_API_KEY").then(|| "env".into())), "env");
        assert_eq!(k.get_with("vertex", "v", |v| (v == "GOOGLE_API_KEY").then(|| "g".into())), "g");
        assert_eq!(k.get_with("anthropic", "a", none), "");
        // the model's own key beats an exported variable
        assert_eq!(k.get_with("custom", "custom|m", |_| Some("env".into())), "slot-key");
        assert!(k.has("ollama", "anything"));
    }

    #[test]
    fn a_moved_klipy_key_leaves_config_json() {
        use serde_json::json;
        let mut m = json!({"emoji": {"klipyKey": "secret", "gifFilter": "low"}}).as_object().unwrap().clone();
        assert!(klipy_moved(&mut m, None));
        assert_eq!(serde_json::Value::Object(m.clone()), json!({"emoji": {"gifFilter": "low", "klipyKeySaved": 1}}));
        assert!(klipy_moved(&mut m, None));
        assert_eq!(m["emoji"]["klipyKeySaved"], json!(2));
        // nowhere else to keep it: it stays, and the picker still hears of it
        assert!(klipy_moved(&mut m, Some("k2")));
        assert_eq!((m["emoji"]["klipyKey"].clone(), m["emoji"]["klipyKeySaved"].clone()), (json!("k2"), json!(3)));
        let mut odd = json!({"emoji": "x"}).as_object().unwrap().clone();
        assert!(klipy_moved(&mut odd, None));
        assert_eq!(odd["emoji"], json!({"klipyKeySaved": 1}));
    }

    #[test]
    fn debug_hides_keys() {
        assert!(!format!("{:?}", Keys::with(&[("openai", "sk-secret")])).contains("sk-secret"));
    }
}
