//! The passkeys daemon's prompt (native/tessera-passkeys, its prompt.rs):
//! when a site, ssh or sudo wants a passkey, the daemon asks here, and the
//! same dialog as a password prompt shows, naming the site and the account.
//! The check itself (a finger first, then the password) is the daemon's own
//! PAM conversation, spoken over its socket in polkit's helper protocol, so
//! the dialog drives it exactly as it drives polkit's (agent::converse_stream).
//! What the shell says never approves anything by itself: only the
//! daemon's check succeeding does.
//!
//! The socket: $XDG_RUNTIME_DIR/tessera/passkeys.sock. `watch` keeps a WATCH
//! connection open (BEGIN / END lines), again every few seconds while the
//! daemon isn't running; one CONVERSE connection per check; REFUSE for Cancel.

use crate::agent;
use std::any::Any;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// What the watcher hears, for the part
#[derive(Debug, Clone, PartialEq)]
pub enum PasskeyRequest {
    Begin { id: u64, purpose: String, rp: String, user: String },
    End { id: u64 },
    /// the daemon went away: every request of its is over
    Lost,
}

pub fn socket_path() -> PathBuf {
    let run = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    run.join("tessera/passkeys.sock")
}

/// A WATCH line, parsed
pub fn parse(l: &str) -> Option<PasskeyRequest> {
    let mut it = l.trim_end().splitn(3, ' ');
    let word = it.next()?;
    let id = it.next()?.parse::<u64>().ok()?;
    match word {
        "BEGIN" => {
            let j: serde_json::Value = serde_json::from_str(it.next()?).ok()?;
            let s = |k: &str| j[k].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(200).collect::<String>();
            Some(PasskeyRequest::Begin { id, purpose: s("purpose"), rp: s("rp"), user: s("user") })
        }
        "END" => Some(PasskeyRequest::End { id }),
        _ => None,
    }
}

/// What the dialog's title says
pub fn message(purpose: &str, rp: &str, user: &str) -> String {
    let who = if user.is_empty() { String::new() } else { format!(" as {user}") };
    if rp.starts_with("ssh:") {
        return match purpose {
            "create" => "Make a new SSH key on this computer".into(),
            "delete" => "Delete this SSH key".into(),
            _ => "Use your SSH key".into(),
        };
    }
    if rp.starts_with("pam://") {
        return match purpose {
            "create" => "Set up your passkey for sudo and login".into(),
            "delete" => "Delete your passkey for sudo and login".into(),
            _ => "Approve sudo or a login with your passkey".into(),
        };
    }
    match purpose {
        "create" => format!("Save a passkey for {rp}{who}"),
        "delete" => format!("Delete your passkey for {rp}{}", if user.is_empty() { String::new() } else { format!(" ({user})") }),
        _ => format!("Sign in to {rp}{who} with your passkey"),
    }
}

/// Follow the daemon (a thread, for good): its requests to `to`
pub fn watch(to: impl Fn(Box<dyn Any + Send>) + Send + 'static) {
    std::thread::spawn(move || {
        let mut said = false;
        loop {
            if let Ok(mut s) = UnixStream::connect(socket_path()) {
                if s.write_all(b"WATCH\n").is_ok() {
                    if !said {
                        eprintln!("tessera-ui: polkit: passkeys' prompt is watching");
                        said = true;
                    }
                    for l in BufReader::new(&s).lines().map_while(Result::ok) {
                        if let Some(r) = parse(&l) {
                            to(Box::new(r));
                        }
                    }
                    to(Box::new(PasskeyRequest::Lost));
                }
            }
            // (no daemon, or it stopped: looked for again)
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

/// One check for request `id` (the dialog's conversation `conv`)
pub fn converse(conv: u64, id: u64, to: impl Fn(Box<dyn Any + Send>) + Send + 'static) -> std::io::Result<agent::Session> {
    let mut s = UnixStream::connect(socket_path())?;
    s.write_all(format!("CONVERSE {id}\n").as_bytes())?;
    agent::converse_stream(conv, s, to)
}

/// Cancel in the dialog
pub fn refuse(id: u64) {
    std::thread::spawn(move || {
        if let Ok(mut s) = UnixStream::connect(socket_path()) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = s.write_all(format!("REFUSE {id}\n").as_bytes());
            let mut l = String::new();
            let _ = BufReader::new(&s).read_line(&mut l);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_lines() {
        assert_eq!(
            parse(r#"BEGIN 3 {"purpose":"sign-in","rp":"github.com","user":"sam"}"#),
            Some(PasskeyRequest::Begin { id: 3, purpose: "sign-in".into(), rp: "github.com".into(), user: "sam".into() })
        );
        assert_eq!(parse("END 3\n"), Some(PasskeyRequest::End { id: 3 }));
        assert_eq!(parse("BEGIN x {}"), None);
        assert_eq!(parse("BEGIN 4 not json"), None);
        assert_eq!(parse("HELLO 1"), None);
        // (no control characters reach the dialog)
        let Some(PasskeyRequest::Begin { rp, .. }) = parse("BEGIN 5 {\"purpose\":\"create\",\"rp\":\"a\\nb.com\",\"user\":\"\"}") else { panic!() };
        assert_eq!(rp, "ab.com");
    }

    #[test]
    fn titles_name_the_site_and_account() {
        assert_eq!(message("sign-in", "github.com", "sam"), "Sign in to github.com as sam with your passkey");
        assert_eq!(message("sign-in", "github.com", ""), "Sign in to github.com with your passkey");
        assert_eq!(message("create", "webauthn.io", "sam"), "Save a passkey for webauthn.io as sam");
        assert_eq!(message("delete", "github.com", "sam"), "Delete your passkey for github.com (sam)");
        assert_eq!(message("create", "ssh:", ""), "Make a new SSH key on this computer");
        assert_eq!(message("sign-in", "ssh:", ""), "Use your SSH key");
        assert!(message("sign-in", "pam://tessera", "").contains("sudo"));
    }
}
