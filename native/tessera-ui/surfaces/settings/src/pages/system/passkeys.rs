//! Privacy & Security → Passkeys (Rust only): Tessera's virtual security
//! key (native/tessera-passkeys, docs/PASSKEYS.md). Off by default.
//!
//! What it reads: the daemon's socket (STATUS and LIST, off the UI thread),
//! whether its user service is enabled (systemctl --user is-enabled), and
//! /etc/tessera/passkeys-u2f (pam_u2f's key list, world-readable: public
//! keys) for "Use for sudo and login". What it does:
//!   on   the root helper's `passkeys on` (tpm2-tools and pam-u2f, the udev
//!        rule that opens /dev/uhid and the TPM to you, and the key's hidraw
//!        node to browsers: asks for your password), then the user service
//!        (written from native/tessera-passkeys/tessera-passkeys.service)
//!        enabled and started
//!   off  the service stopped and disabled, then `passkeys off` (the rule
//!        and any pam_u2f lines go). Saved passkeys stay, sealed, for when
//!        it's on again.
//!   Delete  the daemon's DELETE: it asks for your finger or password first
//!   sudo and login  pamu2fcfg makes a key for pam://tessera (the prompt
//!        asks), then the root helper's `passkeys-pam on <file>` adds it
//!        and the marked pam_u2f lines; `passkeys-pam off` takes them out.

use super::super::super::model::{btn, Ctx, Page, Row};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

pub const UNIT: &str = "tessera-passkeys.service";
const UNIT_TEMPLATE: &str = include_str!("../../../../../../tessera-passkeys/tessera-passkeys.service");
pub const U2F_KEYS: &str = "/etc/tessera/passkeys-u2f";
pub const ORIGIN: &str = "pam://tessera";
const ROOT: &str = "/usr/lib/tessera/tessera-root";
const POLICY: &str = "/usr/share/polkit-1/actions/org.tessera.root.policy";

pub fn socket_path() -> PathBuf {
    tessera_core::util::xdg("XDG_RUNTIME_DIR", ".cache").join("tessera/passkeys.sock")
}

/// One command to the daemon: its answer line ("" when it isn't running)
pub fn daemon(cmd: &str, wait: Duration) -> String {
    let Ok(s) = std::os::unix::net::UnixStream::connect(socket_path()) else { return String::new() };
    let _ = s.set_read_timeout(Some(wait));
    if (&s).write_all(format!("{cmd}\n").as_bytes()).is_err() {
        return String::new();
    }
    let mut l = String::new();
    let _ = BufReader::new(&s).read_line(&mut l);
    l.trim().to_string()
}

fn bin_dir() -> PathBuf {
    tessera_core::util::xdg("XDG_DATA_HOME", ".local/share").join("tessera/bin")
}

/// The user service, for this install's folders
pub fn unit_text(bin: &str, docs: &str) -> String {
    UNIT_TEMPLATE.replace("@BIN@", bin).replace("@DOCS@", docs)
}

fn root_cmd(args: &[&str]) -> Vec<String> {
    let mut v = vec!["sh".to_string(), "-c".into(), format!("{}exec pkexec \"$h\" \"$@\"", tessera_core::old_names::root_pick(ROOT, POLICY)), "sh".into()];
    v.extend(args.iter().map(|s| s.to_string()));
    v
}

/// Run to the end: success, and the last thing it said on stderr
fn run(cmd: &[String]) -> (bool, String) {
    match std::process::Command::new(&cmd[0]).args(&cmd[1..]).stdin(std::process::Stdio::null()).output() {
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
            (o.status.success(), last.strip_prefix("tessera-root: ").unwrap_or(last).to_string())
        }
        Err(e) => (false, e.to_string()),
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Turn it on (a thread): {"ok", "error"}
fn turn_on() -> Value {
    let bin = bin_dir();
    if !bin.join("tessera-passkeys").exists() {
        return json!({"ok": false, "error": "tessera-passkeys isn't installed yet. The next update installs it; then try again."});
    }
    let (ok, err) = run(&root_cmd(&["passkeys", "on"]));
    if !ok {
        return json!({"ok": false, "error": if err.is_empty() { "Not changed: the password wasn't given.".to_string() } else { err }});
    }
    let units = tessera_core::util::xdg("XDG_CONFIG_HOME", ".config").join("systemd/user");
    let docs = tessera_core::util::asset("docs").map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "/usr/share/doc/tessera".into());
    if let Err(e) = std::fs::create_dir_all(&units).and_then(|_| std::fs::write(units.join(UNIT), unit_text(&bin.to_string_lossy(), &docs))) {
        return json!({"ok": false, "error": format!("Couldn't write its service: {e}")});
    }
    let _ = run(&strs(&["systemctl", "--user", "daemon-reload"]));
    let (ok, err) = run(&strs(&["systemctl", "--user", "enable", "--now", UNIT]));
    // (a moment for it to make the key)
    std::thread::sleep(Duration::from_millis(1500));
    json!({"ok": ok, "error": if ok { String::new() } else { format!("It didn't start: {err}") }})
}

fn turn_off() -> Value {
    let _ = run(&strs(&["systemctl", "--user", "disable", "--now", UNIT]));
    let (ok, err) = run(&root_cmd(&["passkeys", "off"]));
    json!({"ok": ok, "error": if ok { String::new() } else { format!("The service stopped, but the system part stays: {err}") }})
}

pub fn user_name() -> String {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_name.is_null() {
            return std::env::var("USER").unwrap_or_default();
        }
        std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
    }
}

/// Is this user's key in pam_u2f's list
pub fn pam_on_in(text: &str, user: &str) -> bool {
    text.lines().any(|l| l.split_once(':').is_some_and(|(u, rest)| u == user && !rest.trim().is_empty()))
}

/// A line pamu2fcfg printed, checked: "user:keyhandle,key,type,options"
pub fn mapping_ok(line: &str, user: &str) -> bool {
    let Some((u, rest)) = line.trim().split_once(':') else { return false };
    let parts: Vec<&str> = rest.split(',').collect();
    u == user && parts.len() >= 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=-_".contains(&b)))
}

fn pam_set(on: bool) -> Value {
    if !on {
        let (ok, err) = run(&root_cmd(&["passkeys-pam", "off"]));
        return json!({"ok": ok, "error": err});
    }
    let user = user_name();
    // (the key for sudo and login: made by this daemon, so the prompt asks)
    let out = std::process::Command::new("pamu2fcfg").args(["-o", ORIGIN, "-i", ORIGIN, "-u", &user, "--verification"]).stdin(std::process::Stdio::null()).output();
    let line = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => return json!({"ok": false, "error": format!("Couldn't make the key: {}", String::from_utf8_lossy(&o.stderr).trim())}),
        Err(_) => return json!({"ok": false, "error": "pamu2fcfg isn't installed (pam-u2f). Turn Passkeys off and on again to install it."}),
    };
    if !mapping_ok(&line, &user) {
        return json!({"ok": false, "error": "pamu2fcfg said something unexpected; nothing was changed."});
    }
    let file = tessera_core::util::xdg("XDG_RUNTIME_DIR", ".cache").join("tessera/passkeys-u2f.line");
    if let Err(e) = tessera_core::util::write_atomic(&file, format!("{line}\n").as_bytes()) {
        return json!({"ok": false, "error": e.to_string()});
    }
    let (ok, err) = run(&root_cmd(&["passkeys-pam", "on", &file.to_string_lossy()]));
    let _ = std::fs::remove_file(&file);
    json!({"ok": ok, "error": if ok || !err.is_empty() { err } else { "Not changed: the password wasn't given.".to_string() }})
}

/// The switch row's words: (detail, tone 0 plain | 2 error)
pub fn status_detail(on: bool, tpm: bool, status: &Value, error: &str) -> (String, i32) {
    if !error.is_empty() {
        return (error.to_string(), 2);
    }
    if !tpm {
        return ("Needs a TPM 2.0 security chip, which this computer doesn't have or has turned off in its firmware settings.".into(), 0);
    }
    if !on {
        return ("Sign in to websites and apps with your fingerprint, use it for SSH keys, and, if you like, for sudo. Turning it on asks for your password once.".into(), 0);
    }
    if status.is_null() || status["running"] == json!(false) {
        return ("Starting…".into(), 0);
    }
    if status["tpm"] == json!(false) {
        return (status["tpmError"].as_str().unwrap_or("The TPM can't be used.").to_string(), 2);
    }
    if status["device"] != json!(true) {
        return ("The security key isn't there yet (/dev/uhid). Turning Passkeys off and on again sets that up.".into(), 2);
    }
    if status["prompt"] != json!(true) {
        return ("On, but Tessera's prompt isn't listening, so requests are refused. Restarting the shell fixes this.".into(), 2);
    }
    ("On. Sites and apps see a security key called Tessera Passkeys; you approve each use with your finger or password.".into(), 0)
}

/// A saved credential's row: its title
pub fn cred_title(c: &Value) -> String {
    let site = c["site"].as_str().unwrap_or("");
    if c["u2f"] == json!(true) || site.is_empty() {
        "A site's two-step key (older kind)".into()
    } else if c["ssh"] == json!(true) {
        format!("SSH key ({})", c["alg"].as_str().unwrap_or("Ed25519"))
    } else if site.starts_with("pam://") {
        "sudo and login".into()
    } else {
        site.to_string()
    }
}

pub fn cred_detail(c: &Value) -> String {
    let date = |k: &str| c[k].as_i64().filter(|t| *t > 0).map(|t| tessera_core::util::strftime("%e %b %Y", t).trim().to_string());
    let mut parts = Vec::new();
    if let Some(u) = c["user"].as_str().filter(|u| !u.is_empty()) {
        parts.push(u.to_string());
    }
    if let Some(d) = date("created") {
        parts.push(format!("Created {d}"));
    }
    parts.push(match date("lastUsed") {
        Some(d) => format!("Last used {d}"),
        None => "Not used yet".into(),
    });
    parts.join(" · ")
}

pub const FOOTER: &str = "Passkeys stay on this laptop: its security chip (TPM) seals them, so they can't be copied, backed up or moved, and a copied file is useless anywhere else. Keep another way to sign in to each site (a password, your phone, another key). For SSH, `ssh-keygen -t ed25519-sk` makes a key here.";

/// The Passkeys group, on Privacy & Security
pub fn passkeys(cx: &mut Ctx, p: &mut Page) {
    let tpm = std::path::Path::new("/dev/tpmrm0").exists();
    let on = cx.once("pk.enabled", || Value::Bool(Ctx::run(&["systemctl", "--user", "is-enabled", UNIT]) == "enabled")).as_bool().unwrap_or(false);
    let status = if on {
        cx.once("pk.status", || serde_json::from_str(&daemon("STATUS", Duration::from_secs(3))).unwrap_or(json!({"running": false})))
    } else {
        Value::Null
    };
    let list = if on { cx.once("pk.list", || serde_json::from_str(&daemon("LIST", Duration::from_secs(3))).unwrap_or(json!([]))) } else { json!([]) };
    // (starting: asked again until it answers)
    if on && status["running"] == json!(false) && !cx.local_b("pk.status.busy") && !cx.local_b("pk.retried") {
        cx.local_set("pk.retried", true);
        cx.spawn("pk.status", || {
            std::thread::sleep(Duration::from_secs(2));
            serde_json::from_str(&daemon("STATUS", Duration::from_secs(3))).unwrap_or(json!({"running": false}))
        });
    }
    // The switch's work finished: read everything again
    if let Some(r) = cx.local.remove("pk.switch") {
        cx.local_set("pk.error", r["error"].as_str().unwrap_or(""));
        for k in ["pk.enabled", "pk.status", "pk.list", "pk.retried", "pk.pam"] {
            cx.local.remove(k);
        }
        return passkeys(cx, p);
    }
    if let Some(r) = cx.local.remove("pk.deleted") {
        let ok = r.as_str() == Some("OK");
        cx.local_set("pk.error", if ok { String::new() } else { "Not deleted.".to_string() });
        cx.local.remove("pk.list");
        return passkeys(cx, p);
    }
    let error = cx.local_s("pk.error");
    let busy = cx.local_b("pk.switch.busy");
    let (detail, tone) = status_detail(on, tpm, &status, &error);
    let id = cx.on(move |cx, _| {
        cx.local_set("pk.error", "");
        if on {
            cx.spawn("pk.switch", turn_off);
        } else {
            cx.spawn("pk.switch", turn_on);
        }
    });
    let mut rows = vec![Row::new("Passkeys").icon("key").detail(if busy { "Changing…".to_string() } else { detail }).tone(tone).switch(on, id).enabled(tpm && !busy)];

    if on {
        // The saved ones, newest first, each with Delete (it asks for your finger or password)
        let creds = list.as_array().cloned().unwrap_or_default();
        let confirming = cx.local_s("pk.confirming");
        if creds.is_empty() {
            rows.push(Row::new("No passkeys yet").detail("On a site's security settings, add a passkey or a security key; Tessera's prompt asks for your finger."));
        }
        for c in &creds {
            let cid = c["id"].as_str().unwrap_or("").to_string();
            let asking = confirming == cid;
            let c2 = cid.clone();
            let act = cx.on(move |cx, a| {
                if asking && a.i == 0 {
                    cx.local_set("pk.confirming", "");
                } else if !asking {
                    cx.local_set("pk.confirming", c2.clone());
                } else {
                    cx.local_set("pk.confirming", "");
                    let id = c2.clone();
                    cx.spawn("pk.deleted", move || Value::String(daemon(&format!("DELETE {id}"), Duration::from_secs(150))));
                }
            });
            let mut b = Vec::new();
            if asking {
                b.push(btn("Keep"));
            }
            b.push(btn(if asking { "Delete, asking for my finger".to_string() } else { "Delete".into() }).danger().off(cx.local_b("pk.deleted.busy")));
            let detail = if asking { "The site can't be signed in to with it again. Make sure you have another way in.".to_string() } else { cred_detail(c) };
            rows.push(Row::new(cred_title(c)).icon("key").detail(detail).buttons(b, act));
        }

        // sudo and login, through pam_u2f
        let user = user_name();
        let pam_on = cx.once("pk.pam", move || Value::Bool(pam_on_in(&std::fs::read_to_string(U2F_KEYS).unwrap_or_default(), &user))).as_bool().unwrap_or(false);
        if let Some(r) = cx.local.remove("pk.pamset") {
            let e = r["error"].as_str().unwrap_or("").to_string();
            cx.local_set("pk.pamError", if r["ok"] == json!(true) { String::new() } else { e });
            cx.local.remove("pk.pam");
            cx.local.remove("pk.list");
            return passkeys(cx, p);
        }
        let pam_busy = cx.local_b("pk.pamset.busy");
        let pam_error = cx.local_s("pk.pamError");
        let pid = cx.on(move |cx, _| cx.spawn("pk.pamset", move || pam_set(!pam_on)));
        rows.push(
            Row::new("Use for sudo and login")
                .detail(if pam_busy {
                    "Changing… Tessera's prompt asks for your finger, then the password once.".to_string()
                } else if !pam_error.is_empty() {
                    pam_error
                } else {
                    "sudo, and a login screen while you're signed in, ask this key first; your password works as before. Needs your password once.".to_string()
                })
                .tone(if cx.local_s("pk.pamError").is_empty() { 0 } else { 2 })
                .switch(pam_on, pid)
                .enabled(!pam_busy && status["device"] == json!(true)),
        );
    }
    let g = p.group("Passkeys");
    g.footer(FOOTER);
    for r in rows {
        g.row(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_says_where_it_stands() {
        assert!(status_detail(false, false, &Value::Null, "").0.contains("TPM 2.0"));
        assert!(status_detail(false, true, &Value::Null, "").0.contains("asks for your password once"));
        assert_eq!(status_detail(true, true, &json!({"running": false}), "").0, "Starting…");
        assert_eq!(status_detail(true, true, &json!({"tpm": false, "tpmError": "No TPM"}), ""), ("No TPM".to_string(), 2));
        assert_eq!(status_detail(true, true, &json!({"tpm": true, "device": false}), "").1, 2);
        assert!(status_detail(true, true, &json!({"tpm": true, "device": true, "prompt": false}), "").0.contains("prompt isn't listening"));
        assert!(status_detail(true, true, &json!({"tpm": true, "device": true, "prompt": true}), "").0.starts_with("On."));
        assert_eq!(status_detail(true, true, &Value::Null, "Broke").1, 2);
    }

    #[test]
    fn saved_ones_are_named_plainly() {
        assert_eq!(cred_title(&json!({"site": "github.com"})), "github.com");
        assert_eq!(cred_title(&json!({"site": "ssh:", "ssh": true, "alg": "Ed25519"})), "SSH key (Ed25519)");
        assert_eq!(cred_title(&json!({"site": "", "u2f": true})), "A site's two-step key (older kind)");
        assert_eq!(cred_title(&json!({"site": "pam://tessera"})), "sudo and login");
        let d = cred_detail(&json!({"user": "sam", "created": 1_700_000_000, "lastUsed": 0}));
        assert!(d.starts_with("sam · Created "));
        assert!(d.ends_with("Not used yet"));
    }

    #[test]
    fn pam_u2f_lines_are_checked() {
        assert!(pam_on_in("sam:abc,def,es256,+presence\nlee:x,y,z\n", "sam"));
        assert!(!pam_on_in("samuel:abc,def,es256\n", "sam"));
        assert!(!pam_on_in("", "sam"));
        assert!(mapping_ok("sam:AbC+/=,BBBB-_,es256,+presence+verification", "sam"));
        assert!(!mapping_ok("lee:a,b,c", "sam"));
        assert!(!mapping_ok("sam:a,b", "sam"));
        assert!(!mapping_ok("sam:a;rm -rf,b,c", "sam"));
    }

    #[test]
    fn the_service_points_at_this_install() {
        let u = unit_text("/home/sam/.local/share/tessera/bin", "/opt/tessera/docs");
        assert!(u.contains("ExecStart=/home/sam/.local/share/tessera/bin/tessera-passkeys"));
        assert!(u.contains("WantedBy=default.target"));
        assert!(!u.contains('@'));
        // (the password check needs unix_chkpwd's setgid: never NoNewPrivileges)
        assert!(!u.lines().any(|l| l.starts_with("NoNewPrivileges")));
        assert!(FOOTER.contains("can't be copied, backed up or moved"));
    }
}
