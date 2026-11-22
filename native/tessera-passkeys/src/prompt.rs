//! Asking the user, through the shell: the daemon's socket,
//! $XDG_RUNTIME_DIR/tessera/passkeys.sock (0600, and only connections from
//! this user's processes are served: SO_PEERCRED). One command per
//! connection, its first line:
//!   WATCH            the shell's prompt (surfaces/polkit/src/passkeys.rs):
//!                    "BEGIN <id> <json>" when a request needs the user
//!                    ({"purpose","rp","user"}), "END <id>" when it's over
//!                    (answered, cancelled by the browser, timed out). Pending
//!                    ones are sent at once, so a restarted shell asks again.
//!   CONVERSE <id>    one check for that request: the polkit agent helper's
//!                    lines (PAM_PROMPT_ECHO_OFF …, PAM_TEXT_INFO …, SUCCESS,
//!                    FAILURE), the client's lines answering the prompts.
//!                    SUCCESS approves the request. Closing it stops the check.
//!   REFUSE <id>      Cancel in the prompt
//!   LIST             the saved credentials as one JSON line (no secrets)
//!   STATUS           {"version","tpm","tpmError","device","count","prompt"}
//!   DELETE <id>      delete one, after the same check ("OK", "NO <why>")
//! A request no prompt is watching for is refused at once, so a browser
//! isn't left waiting with nothing on screen.

use crate::ctap2::{Ask, Purpose, Txn, Verdict, Verifier};
use crate::pam;
use crate::store::Store;
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long the user has to answer
pub const TIMEOUT: Duration = Duration::from_secs(120);

struct Pending {
    id: u64,
    ask: Ask,
    verdict: Option<Verdict>,
    conv: Option<Arc<Mutex<Option<Child>>>>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    pending: Vec<Pending>,
    watchers: Vec<(u64, UnixStream)>,
    next_watcher: u64,
}

pub struct Prompts {
    inner: Mutex<Inner>,
    cv: Condvar,
    /// the check's command: this binary's --pam-helper, or a test's fake
    pub helper: Vec<String>,
    pub timeout: Duration,
}

pub fn begin_line(id: u64, ask: &Ask) -> String {
    format!("BEGIN {id} {}\n", json!({"purpose": ask.purpose.word(), "rp": ask.rp, "user": ask.user}))
}

fn send_all(inner: &mut Inner, line: &str) {
    inner.watchers.retain_mut(|(_, s)| s.write_all(line.as_bytes()).is_ok());
}

fn kill(conv: &Option<Arc<Mutex<Option<Child>>>>) {
    if let Some(c) = conv {
        if let Ok(mut c) = c.lock() {
            if let Some(ch) = c.as_mut() {
                let _ = ch.kill();
            }
        }
    }
}

impl Prompts {
    pub fn new(helper: Vec<String>) -> Prompts {
        Prompts { inner: Mutex::new(Inner::default()), cv: Condvar::new(), helper, timeout: TIMEOUT }
    }

    pub fn watching(&self) -> usize {
        self.inner.lock().unwrap().watchers.len()
    }

    /// The prompt is watching: it hears of every request from now on
    pub fn add_watcher(&self, mut s: UnixStream) -> u64 {
        let mut i = self.inner.lock().unwrap();
        for p in i.pending.iter().filter(|p| p.verdict.is_none()) {
            let _ = s.write_all(begin_line(p.id, &p.ask).as_bytes());
        }
        i.next_watcher += 1;
        let n = i.next_watcher;
        i.watchers.push((n, s));
        n
    }

    pub fn remove_watcher(&self, n: u64) {
        self.inner.lock().unwrap().watchers.retain(|(w, _)| *w != n);
        self.cv.notify_all();
    }

    /// The answer for a request (the first one counts)
    pub fn answer(&self, id: u64, v: Verdict) -> bool {
        let mut i = self.inner.lock().unwrap();
        let Some(p) = i.pending.iter_mut().find(|p| p.id == id) else { return false };
        if p.verdict.is_none() {
            p.verdict = Some(v);
        }
        drop(i);
        self.cv.notify_all();
        true
    }

    /// One check for request `id`, over `client` (CONVERSE)
    pub fn converse(self: &Arc<Self>, id: u64, client: UnixStream) {
        let slot = Arc::new(Mutex::new(None::<Child>));
        {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.pending.iter_mut().find(|p| p.id == id && p.verdict.is_none()) else {
                drop(i);
                let _ = (&client).write_all(b"FAILURE\n");
                return;
            };
            // (one check at a time: a new one stops the one before)
            kill(&p.conv);
            p.conv = Some(slot.clone());
        }
        let Some((cmd, args)) = self.helper.split_first() else { return };
        let child = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                eprintln!("tessera-passkeys: the check didn't start: {e}");
                let _ = (&client).write_all(b"PAM_ERROR_MSG Couldn't start the check\nFAILURE\n");
                return;
            }
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
        *slot.lock().unwrap() = Some(child);
        // the client's answers to the helper; its end stops the check
        let reader = match client.try_clone() {
            Ok(r) => r,
            Err(_) => return,
        };
        let slot2 = slot.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(reader).lines() {
                let Ok(mut l) = l else { break };
                let ok = stdin.write_all(l.as_bytes()).and_then(|_| stdin.write_all(b"\n")).and_then(|_| stdin.flush()).is_ok();
                // (the password, overwritten once it's passed on)
                unsafe { l.as_bytes_mut().iter_mut().for_each(|b| *b = 0) };
                if !ok {
                    break;
                }
            }
            kill(&Some(slot2));
        });
        // the helper's lines to the client
        let mut done = None;
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(ok) = pam::result_ok(&l) {
                done = Some(ok);
                break;
            }
            if let Some(a) = pam::to_agent_line(&l) {
                if (&client).write_all(format!("{a}\n").as_bytes()).is_err() {
                    break;
                }
            }
        }
        if done == Some(true) {
            self.answer(id, Verdict::Approved);
        }
        let _ = (&client).write_all(if done == Some(true) { b"SUCCESS\n" } else { b"FAILURE\n" });
        let _ = client.shutdown(std::net::Shutdown::Both);
        if let Ok(mut c) = slot.lock() {
            if let Some(mut ch) = c.take() {
                let _ = ch.kill();
                let _ = ch.wait();
            }
        }
    }
}

impl Verifier for Prompts {
    fn verify(&self, ask: &Ask, txn: &Txn) -> Verdict {
        let start = Instant::now();
        let mut i = self.inner.lock().unwrap();
        if i.watchers.is_empty() {
            eprintln!("tessera-passkeys: {} for {} refused: no prompt is watching (is the shell running?)", ask.purpose.word(), ask.rp);
            return Verdict::NoPrompt;
        }
        i.next += 1;
        let id = i.next;
        i.pending.push(Pending { id, ask: ask.clone(), verdict: None, conv: None });
        send_all(&mut i, &begin_line(id, ask));
        let verdict = loop {
            if let Some(v) = i.pending.iter().find(|p| p.id == id).and_then(|p| p.verdict) {
                break v;
            }
            if txn.is_cancelled() {
                break Verdict::Cancelled;
            }
            if start.elapsed() > self.timeout {
                break Verdict::TimedOut;
            }
            if i.watchers.is_empty() {
                break Verdict::NoPrompt;
            }
            drop(i);
            (txn.keepalive)();
            i = self.inner.lock().unwrap();
            i = self.cv.wait_timeout(i, Duration::from_millis(100)).unwrap().0;
        };
        if let Some(at) = i.pending.iter().position(|p| p.id == id) {
            let p = i.pending.remove(at);
            kill(&p.conv);
        }
        send_all(&mut i, &format!("END {id}\n"));
        verdict
    }
}

// ── The socket ──────────────────────────────────────────────────────────────

pub fn socket_path() -> PathBuf {
    let run = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    run.join("tessera/passkeys.sock")
}

fn peer_uid(s: &UnixStream) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) };
    (r == 0).then_some(cred.uid)
}

/// What STATUS says beyond the store
pub struct Health {
    pub device: AtomicBool,
    pub tpm_error: Mutex<String>,
}

pub fn listen(path: &std::path::Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if let Some(d) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d)?;
    }
    // (one left by a daemon that didn't end cleanly; another running one keeps its own)
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another tessera-passkeys is running"));
        }
        let _ = std::fs::remove_file(path);
    }
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

/// The saved credentials, for Settings (no secrets)
pub fn list_json(store: &Store) -> String {
    let v: Vec<serde_json::Value> = store
        .creds()
        .iter()
        .map(|c| {
            json!({
                "id": c.id, "site": if c.rp_id.is_empty() { "" } else { c.rp_id.as_str() }, "name": c.rp_name,
                "user": crate::ctap2::user_label(c), "created": c.created, "lastUsed": c.last_used,
                "resident": c.resident, "u2f": c.u2f, "ssh": c.rp_id.starts_with("ssh:"),
                "alg": if c.alg == crate::store::EDDSA { "Ed25519" } else { "P-256" },
            })
        })
        .collect();
    serde_json::Value::Array(v).to_string()
}

pub fn serve(listener: UnixListener, prompts: Arc<Prompts>, store: Arc<Mutex<Store>>, health: Arc<Health>) {
    let me = unsafe { libc::getuid() };
    for conn in listener.incoming() {
        let Ok(s) = conn else { continue };
        if peer_uid(&s) != Some(me) {
            continue;
        }
        let (prompts, store, health) = (prompts.clone(), store.clone(), health.clone());
        std::thread::spawn(move || one(s, prompts, store, health));
    }
}

fn one(s: UnixStream, prompts: Arc<Prompts>, store: Arc<Mutex<Store>>, health: Arc<Health>) {
    let Ok(read) = s.try_clone() else { return };
    let mut r = BufReader::new(read);
    let mut first = String::new();
    if r.by_ref().take(512).read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let first = first.trim_end().to_string();
    let (cmd, arg) = first.split_once(' ').unwrap_or((first.as_str(), ""));
    let mut out = &s;
    match cmd {
        "WATCH" => {
            let Ok(w) = s.try_clone() else { return };
            let n = prompts.add_watcher(w);
            // (until the shell closes it)
            let mut sink = String::new();
            while r.read_line(&mut sink).unwrap_or(0) > 0 {
                sink.clear();
            }
            prompts.remove_watcher(n);
        }
        "CONVERSE" => {
            if let Ok(id) = arg.parse::<u64>() {
                prompts.converse(id, s);
            }
        }
        "REFUSE" => {
            let ok = arg.parse::<u64>().is_ok_and(|id| prompts.answer(id, Verdict::Denied));
            let _ = out.write_all(if ok { b"OK\n" } else { b"NO\n" });
        }
        "LIST" => {
            let j = list_json(&store.lock().unwrap());
            let _ = out.write_all(format!("{j}\n").as_bytes());
        }
        "STATUS" => {
            let (count, sealer) = {
                let st = store.lock().unwrap();
                (st.creds().len(), st.sealer_available())
            };
            let err = health.tpm_error.lock().unwrap().clone();
            let tpm_error = if !err.is_empty() { err } else { sealer.err().unwrap_or_default() };
            let j = json!({
                "version": env!("CARGO_PKG_VERSION"), "tpm": tpm_error.is_empty(), "tpmError": tpm_error,
                "device": health.device.load(Ordering::SeqCst), "count": count, "prompt": prompts.watching() > 0,
            });
            let _ = out.write_all(format!("{j}\n").as_bytes());
        }
        "DELETE" => {
            let found = store.lock().unwrap().find(arg).cloned();
            let Some(c) = found else {
                let _ = out.write_all(b"NO not found\n");
                return;
            };
            let never = AtomicBool::new(false);
            let k = || {};
            let rp = if c.rp_id.is_empty() { "a site's two-step sign-in (U2F)".to_string() } else { c.rp_id.clone() };
            let v = prompts.verify(&Ask { purpose: Purpose::Delete, rp, user: crate::ctap2::user_label(&c) }, &Txn { cancelled: &never, keepalive: &k });
            let line = if v == Verdict::Approved {
                match store.lock().unwrap().delete(arg) {
                    Ok(true) => "OK\n".to_string(),
                    Ok(false) => "NO not found\n".to_string(),
                    Err(e) => format!("NO {e}\n"),
                }
            } else {
                format!("NO {v:?}\n")
            };
            let _ = out.write_all(line.as_bytes());
        }
        _ => {
            let _ = out.write_all(b"NO unknown command\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn ask() -> Ask {
        Ask { purpose: Purpose::SignIn, rp: "example.com".into(), user: "sam".into() }
    }

    fn read_line(s: &UnixStream) -> String {
        let mut out = Vec::new();
        let mut b = [0u8; 1];
        while (&*s).read(&mut b).unwrap_or(0) == 1 && b[0] != b'\n' {
            out.push(b[0]);
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn no_prompt_no_waiting() {
        let p = Prompts::new(vec!["true".into()]);
        let never = AtomicBool::new(false);
        let k = || {};
        assert_eq!(p.verify(&ask(), &Txn { cancelled: &never, keepalive: &k }), Verdict::NoPrompt);
    }

    #[test]
    fn the_prompt_hears_of_requests_and_refuses_them() {
        let p = Arc::new(Prompts::new(vec!["true".into()]));
        let (ours, theirs) = UnixStream::pair().unwrap();
        p.add_watcher(ours);
        let p2 = p.clone();
        let t = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            let k = || {};
            p2.verify(&ask(), &Txn { cancelled: &never, keepalive: &k })
        });
        let begin = read_line(&theirs);
        assert!(begin.starts_with("BEGIN 1 "), "{begin}");
        let j: serde_json::Value = serde_json::from_str(begin.splitn(3, ' ').nth(2).unwrap()).unwrap();
        assert_eq!(j["rp"], "example.com");
        assert_eq!(j["purpose"], "sign-in");
        assert!(p.answer(1, Verdict::Denied));
        assert_eq!(t.join().unwrap(), Verdict::Denied);
        assert_eq!(read_line(&theirs), "END 1");
        assert!(!p.answer(1, Verdict::Approved));
    }

    #[test]
    fn the_browser_cancelling_ends_it() {
        let p = Arc::new(Prompts::new(vec!["true".into()]));
        let (ours, theirs) = UnixStream::pair().unwrap();
        p.add_watcher(ours);
        let cancel = Arc::new(AtomicBool::new(false));
        let (p2, c2) = (p.clone(), cancel.clone());
        let alive = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let a2 = alive.clone();
        let t = std::thread::spawn(move || {
            let k = move || {
                a2.fetch_add(1, Ordering::SeqCst);
            };
            p2.verify(&ask(), &Txn { cancelled: &c2, keepalive: &k })
        });
        read_line(&theirs);
        std::thread::sleep(Duration::from_millis(350));
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(t.join().unwrap(), Verdict::Cancelled);
        // (keepalives went out while it waited)
        assert!(alive.load(Ordering::SeqCst) >= 2);
        assert_eq!(read_line(&theirs), "END 1");
    }

    #[test]
    fn time_runs_out_and_a_closed_prompt_refuses() {
        let mut p = Prompts::new(vec!["true".into()]);
        p.timeout = Duration::from_millis(200);
        let p = Arc::new(p);
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let n = p.add_watcher(ours);
        let never = AtomicBool::new(false);
        let k = || {};
        assert_eq!(p.verify(&ask(), &Txn { cancelled: &never, keepalive: &k }), Verdict::TimedOut);
        // the shell going away mid-request
        let mut p2 = Prompts::new(vec!["true".into()]);
        p2.timeout = Duration::from_secs(30);
        let p2 = Arc::new(p2);
        let (ours, theirs) = UnixStream::pair().unwrap();
        let n2 = p2.add_watcher(ours);
        let p3 = p2.clone();
        let t = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            let k = || {};
            p3.verify(&ask(), &Txn { cancelled: &never, keepalive: &k })
        });
        read_line(&theirs);
        p2.remove_watcher(n2);
        assert_eq!(t.join().unwrap(), Verdict::NoPrompt);
        p.remove_watcher(n);
    }

    #[test]
    fn a_check_that_succeeds_approves() {
        // (a fake helper: asks for the password, says yes to "secret")
        let dir = std::env::temp_dir().join(format!("tessera-passkeys-conv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("helper.sh");
        std::fs::write(&fake, "#!/bin/sh\necho 'INFO Place your finger on the reader'\necho 'ECHO_OFF Password: '\nread pw\n[ \"$pw\" = secret ] && echo 'RESULT 0 Success' || echo 'RESULT 7 Authentication failure'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = Arc::new(Prompts::new(vec![fake.to_string_lossy().into_owned()]));
        let (ours, theirs) = UnixStream::pair().unwrap();
        p.add_watcher(ours);
        let p2 = p.clone();
        let t = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            let k = || {};
            p2.verify(&ask(), &Txn { cancelled: &never, keepalive: &k })
        });
        read_line(&theirs);
        for (pw, want) in [("wrong", "FAILURE"), ("secret", "SUCCESS")] {
            let (client, server) = UnixStream::pair().unwrap();
            let p3 = p.clone();
            let c = std::thread::spawn(move || p3.converse(1, server));
            assert_eq!(read_line(&client), "PAM_TEXT_INFO Place your finger on the reader");
            assert_eq!(read_line(&client), "PAM_PROMPT_ECHO_OFF Password: ");
            (&client).write_all(format!("{pw}\n").as_bytes()).unwrap();
            assert_eq!(read_line(&client), want);
            c.join().unwrap();
        }
        assert_eq!(t.join().unwrap(), Verdict::Approved);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
