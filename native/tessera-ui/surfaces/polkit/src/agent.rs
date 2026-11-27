//! A polkit authentication agent (org.freedesktop.PolicyKit1.AuthenticationAgent)
//! and the conversation for one request (polkit-agent-helper-1's protocol),
//! as libpolkit-agent-1 does for Quickshell's PolkitAgent.
//!
//! The agent serves BeginAuthentication and CancelAuthentication at
//! OBJECT on a bus, and registers with the Authority there
//! (RegisterAuthenticationAgent, for this session), trying on until it's
//! taken, and again each time polkitd comes back (NameOwnerChanged for
//! org.freedesktop.PolicyKit1: a restart forgets every agent). BeginAuthentication
//! returns when the part answers: Ok once the helper said SUCCESS, the
//! Cancelled error when you cancel. Which bus is the part's choice (its top
//! comment): the system bus only for the real agent.
//!
//! The conversation: the helper (polkit's own, run as root by polkit: the
//! socket /run/polkit/agent-helper.socket, else the setuid
//! polkit-agent-helper-1; or a fake one for a test) is given the user and
//! the request's cookie, then speaks lines:
//!   PAM_PROMPT_ECHO_OFF <prompt> | PAM_PROMPT_ECHO_ON <prompt>  (answer a line)
//!   PAM_ERROR_MSG <text> | PAM_TEXT_INFO <text> | SUCCESS | FAILURE
//! On SUCCESS the helper has told polkit itself; the agent only answers.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::io::{BufRead, Write};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use zbus::zvariant::{OwnedValue, Value};

pub const OBJECT: &str = "/org/tessera/PolicyKit1/AuthenticationAgent";
pub const SOCKET: &str = "/run/polkit/agent-helper.socket";
/// polkit's setuid helper: /usr/lib/polkit-1 (Arch, Debian, Fedora), or
/// /usr/libexec (openSUSE, some builds); the first that exists
pub const HELPERS: [&str; 4] = ["/usr/lib/polkit-1/polkit-agent-helper-1", "/usr/libexec/polkit-1/polkit-agent-helper-1", "/usr/libexec/polkit-agent-helper-1", "/usr/lib64/polkit-1/polkit-agent-helper-1"];

pub fn helper_path() -> &'static str {
    HELPERS.iter().copied().find(|p| std::path::Path::new(p).exists()).unwrap_or(HELPERS[0])
}

type To = Arc<Mutex<Box<dyn Fn(Box<dyn Any + Send>) + Send>>>;

// ── A one-shot answer, without an async runtime of our own ──────────────────

#[derive(Default)]
struct Slot {
    value: Option<bool>,
    waker: Option<Waker>,
}

/// The part's answer to one BeginAuthentication: true authenticated, false cancelled
#[derive(Clone)]
pub struct Reply(Arc<Mutex<Slot>>);

impl Reply {
    pub fn send(&self, ok: bool) {
        if let Ok(mut s) = self.0.lock() {
            if s.value.is_none() {
                s.value = Some(ok);
            }
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        }
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

struct Wait(Arc<Mutex<Slot>>);

impl Future for Wait {
    type Output = bool;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let mut s = self.0.lock().unwrap();
        match s.value {
            Some(v) => Poll::Ready(v),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

pub fn oneshot() -> (Reply, impl Future<Output = bool>) {
    let s = Arc::new(Mutex::new(Slot::default()));
    (Reply(s.clone()), Wait(s))
}

// ── The agent ───────────────────────────────────────────────────────────────

/// What the agent hears, for the part
#[derive(Debug)]
pub enum Request {
    Begin { action: String, message: String, cookie: String, user: String, reply: Reply },
    Cancel { cookie: String },
    /// registered with the Authority (or why not)
    Registered(Result<(), String>),
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
pub enum PkError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Cancelled(String),
    Failed(String),
}

struct Agent {
    to: To,
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        _icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), PkError> {
        let (reply, wait) = oneshot();
        let user = pick_user(&identities);
        if let Ok(to) = self.to.lock() {
            to(Box::new(Request::Begin { action: action_id, message, cookie, user, reply }));
        }
        if wait.await {
            Ok(())
        } else {
            Err(PkError::Cancelled("The authentication was cancelled".into()))
        }
    }

    async fn cancel_authentication(&self, cookie: String) {
        if let Ok(to) = self.to.lock() {
            to(Box::new(Request::Cancel { cookie }));
        }
    }
}

/// Who to ask about: you, when you're among the identities polkit offers;
/// else the first user it names (an admin)
pub fn pick_user(ids: &[(String, HashMap<String, OwnedValue>)]) -> String {
    let me = unsafe { libc::getuid() };
    let uids: Vec<u32> = ids
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, d)| d.get("uid").and_then(|v| match &**v {
            Value::U32(u) => Some(*u),
            Value::I32(i) => Some(*i as u32),
            _ => None,
        }))
        .collect();
    let uid = if uids.contains(&me) || uids.is_empty() { me } else { uids[0] };
    user_of(uid)
}

pub fn user_of(uid: u32) -> String {
    unsafe {
        let pw = libc::getpwuid(uid);
        if pw.is_null() || (*pw).pw_name.is_null() {
            return std::env::var("USER").unwrap_or_default();
        }
        std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
    }
}

/// logind's display session for this user (User.Display on user/self)
fn display_session(conn: &zbus::blocking::Connection) -> Option<String> {
    let reply = conn
        .call_method(Some("org.freedesktop.login1"), "/org/freedesktop/login1/user/self", Some("org.freedesktop.DBus.Properties"), "Get", &("org.freedesktop.login1.User", "Display"))
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    let mut v = Value::from(v);
    // (a variant holding the (so) pair, unwrapped however deep it came)
    while let Value::Value(inner) = v {
        v = *inner;
    }
    match v {
        Value::Structure(s) => match s.fields().first() {
            Some(Value::Str(id)) => Some(id.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// The session the agent registers for: the display session, else the
/// environment's, else none (then the process itself)
pub fn choose_session(display: Option<String>, env: Option<String>) -> Option<String> {
    display.filter(|s| !s.is_empty()).or(env.filter(|s| !s.is_empty()))
}

/// Serve the agent on `system` (else the session bus) and register it;
/// tries again a few times while another agent still holds this session
pub fn spawn(system: bool, to: impl Fn(Box<dyn Any + Send>) + Send + 'static) {
    let to: To = Arc::new(Mutex::new(Box::new(to)));
    std::thread::spawn(move || {
        let say = |r: Result<(), String>| {
            if let Ok(t) = to.lock() {
                t(Box::new(Request::Registered(r)));
            }
        };
        let conn = match if system { zbus::blocking::Connection::system() } else { zbus::blocking::Connection::session() } {
            Ok(c) => c,
            Err(e) => return say(Err(e.to_string())),
        };
        if let Err(e) = conn.object_server().at(OBJECT, Agent { to: to.clone() }) {
            return say(Err(e.to_string()));
        }
        // The session: logind's graphical one for this user first (where the
        // desktop's asks come from), then XDG_SESSION_ID. 20 Nov: a shell
        // restarted over ssh kept that ssh login's XDG_SESSION_ID, so the
        // agent served it and the desktop's own asks found no agent.
        let session = if system { display_session(&conn) } else { None };
        let session = choose_session(session, std::env::var("XDG_SESSION_ID").ok());
        let subject: (String, HashMap<String, Value>) = match session {
            Some(id) => ("unix-session".into(), HashMap::from([("session-id".to_string(), Value::from(id))])),
            None => {
                let pid = std::process::id();
                let start: u64 = std::fs::read_to_string("/proc/self/stat").ok().and_then(|s| s.rsplit(')').next().and_then(|r| r.split_whitespace().nth(19)).and_then(|v| v.parse().ok())).unwrap_or(0);
                ("unix-process".into(), HashMap::from([("pid".to_string(), Value::U32(pid)), ("start-time".to_string(), Value::U64(start))]))
            }
        };
        let locale = std::env::var("LANG").unwrap_or_else(|_| "C".into());
        let register = || {
            conn.call_method(
                Some("org.freedesktop.PolicyKit1"),
                "/org/freedesktop/PolicyKit1/Authority",
                Some("org.freedesktop.PolicyKit1.Authority"),
                "RegisterAuthenticationAgent",
                &(&subject, locale.as_str(), OBJECT),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        };
        // polkitd forgets its agents when it restarts (an update, a crash,
        // `systemctl restart polkit`): every admin action then said "Not
        // authorized" with no dialog until the shell restarted (audit #769).
        // So its name is watched (first, so a restart in between isn't
        // missed) and the agent registers again with each new polkitd.
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg(0, AUTHORITY))
            .map(|b| b.build());
        let signals = rule.and_then(|r| zbus::blocking::MessageIterator::for_match_rule(r, &conn, Some(16)));
        register_until_done(&register, &say);
        let mut owner = name_owner(&conn);
        let Ok(signals) = signals else {
            eprintln!("tessera-ui: polkit: can't watch for polkit restarts");
            // (served for as long as this process lives)
            loop {
                std::thread::park();
            }
        };
        for m in signals.flatten() {
            let Ok((name, _old, new)) = m.body().deserialize::<(String, String, String)>() else { continue };
            if name != AUTHORITY {
                continue;
            }
            if new.is_empty() {
                // (gone: Settings → Health says so until it's back)
                say(Err("polkit stopped".into()));
                continue;
            }
            if !register_again(&name, &new, &owner) {
                continue;
            }
            eprintln!("tessera-ui: polkit came back: registering the agent again");
            register_until_done(&register, &say);
            owner = name_owner(&conn).or(Some(new));
        }
        // (the bus went: the object goes with the process)
        loop {
            std::thread::park();
        }
    });
}

/// polkit's well-known name on the bus
pub const AUTHORITY: &str = "org.freedesktop.PolicyKit1";

/// The bus name polkitd has now (its unique name), if it's there
fn name_owner(conn: &zbus::blocking::Connection) -> Option<String> {
    let m = conn.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "GetNameOwner", &(AUTHORITY,)).ok()?;
    m.body().deserialize::<String>().ok()
}

/// A NameOwnerChanged for `name` with `new_owner` means registering again:
/// polkit's name, a new owner, not the one the agent registered with (a
/// signal queued while registering mustn't register twice: polkit refuses a
/// second agent for the session, and that would retry for ever)
pub fn register_again(name: &str, new_owner: &str, registered_with: &Option<String>) -> bool {
    name == AUTHORITY && !new_owner.is_empty() && registered_with.as_deref() != Some(new_owner)
}

/// How long before try `attempt` (from 0) again: half a second more each
/// time (the shell's own agent letting go of this session takes a moment),
/// at most 30 s; it never gives up (it stopped after ten tries, 27 s)
pub fn retry_after(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis((500 * (attempt as u64 + 1)).min(30_000))
}

/// The first answer that's not yet a registration is said after ten tries
/// (Settings → Health: "not registered"), as before; the tries go on
pub fn say_failure_at(attempt: u32) -> bool {
    attempt == 9
}

/// Register until it's done, saying so (and saying once that it isn't yet)
fn register_until_done(register: &impl Fn() -> Result<(), String>, say: &impl Fn(Result<(), String>)) {
    let mut attempt = 0u32;
    loop {
        match register() {
            Ok(()) => return say(Ok(())),
            Err(e) => {
                if say_failure_at(attempt) {
                    say(Err(e));
                }
            }
        }
        std::thread::sleep(retry_after(attempt));
        attempt = attempt.saturating_add(1);
    }
}

// ── One conversation with the helper ────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Prompt { echo: bool, text: String },
    Info(String),
    Error(String),
    Done(bool),
}

/// A helper's line, parsed
pub fn parse_line(l: &str) -> Option<Line> {
    let l = l.trim_end_matches(['\n', '\r']);
    if l == "SUCCESS" {
        return Some(Line::Done(true));
    }
    if l == "FAILURE" {
        return Some(Line::Done(false));
    }
    let (word, rest) = l.split_once(' ').unwrap_or((l, ""));
    Some(match word {
        "PAM_PROMPT_ECHO_OFF" => Line::Prompt { echo: false, text: rest.to_string() },
        "PAM_PROMPT_ECHO_ON" => Line::Prompt { echo: true, text: rest.to_string() },
        "PAM_ERROR_MSG" => Line::Error(rest.to_string()),
        "PAM_TEXT_INFO" => Line::Info(rest.to_string()),
        _ => return None,
    })
}

/// A conversation's news for the part (`id` tells it from an older one's)
#[derive(Debug)]
pub struct SessionLine(pub u64, pub Line);

/// A running conversation: answer prompts with `respond`, end it with `stop`
pub struct Session {
    pub id: u64,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    child: Option<std::process::Child>,
    stream: Option<std::os::unix::net::UnixStream>,
}

impl Session {
    pub fn respond(&self, text: &str) {
        if let Ok(mut w) = self.writer.lock() {
            if let Some(w) = w.as_mut() {
                // (the answer with its newline, overwritten once it's sent)
                let mut line = Vec::with_capacity(text.len() + 1);
                line.extend_from_slice(text.as_bytes());
                line.push(b'\n');
                let _ = w.write_all(&line);
                let _ = w.flush();
                line.iter_mut().for_each(|b| *b = 0);
                std::hint::black_box(&line);
            }
        }
    }
    pub fn stop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
        if let Some(s) = &self.stream {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        if let Ok(mut w) = self.writer.lock() {
            *w = None;
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
        if let Some(mut c) = self.child.take() {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
    }
}

/// Start the helper for `user` and `cookie`. `helper`: Some(fake) for a
/// test (never the real one then); None for polkit's own (socket, else the
/// setuid helper)
pub fn converse(id: u64, user: &str, cookie: &str, helper: Option<&str>, to: impl Fn(Box<dyn Any + Send>) + Send + 'static) -> std::io::Result<Session> {
    let mut child = None;
    let mut stream = None;
    let (reader, writer): (Box<dyn std::io::Read + Send>, Box<dyn Write + Send>) = match helper {
        None if std::path::Path::new(SOCKET).exists() => match std::os::unix::net::UnixStream::connect(SOCKET) {
            Ok(mut s) => {
                s.write_all(format!("{user}\n{cookie}\n").as_bytes())?;
                let r = s.try_clone()?;
                let w = s.try_clone()?;
                stream = Some(s);
                (Box::new(r), Box::new(w))
            }
            Err(_) => spawn_helper(helper_path(), user, cookie, &mut child)?,
        },
        None => spawn_helper(helper_path(), user, cookie, &mut child)?,
        Some(fake) => spawn_helper(fake, user, cookie, &mut child)?,
    };
    read_lines(id, reader, to);
    Ok(Session { id, writer: Arc::new(Mutex::new(Some(writer))), child, stream })
}

/// The helper's lines, to the part, until it's done (a Done(false) if it ends without one)
fn read_lines(id: u64, reader: Box<dyn std::io::Read + Send>, to: impl Fn(Box<dyn Any + Send>) + Send + 'static) {
    std::thread::spawn(move || {
        let mut done = false;
        for l in std::io::BufReader::new(reader).lines().map_while(Result::ok) {
            if let Some(line) = parse_line(&l) {
                done |= matches!(line, Line::Done(_));
                to(Box::new(SessionLine(id, line)));
                if done {
                    break;
                }
            }
        }
        if !done {
            to(Box::new(SessionLine(id, Line::Done(false))));
        }
    });
}

type Pipes = (Box<dyn std::io::Read + Send>, Box<dyn Write + Send>);

fn spawn_helper(path: &str, user: &str, cookie: &str, child: &mut Option<std::process::Child>) -> std::io::Result<Pipes> {
    use std::process::{Command, Stdio};
    let mut c = Command::new(path).arg(user).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut stdin = c.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
    let stdout = c.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;
    stdin.write_all(format!("{cookie}\n").as_bytes())?;
    *child = Some(c);
    Ok((Box::new(stdout), Box::new(stdin)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_session_before_the_environments() {
        assert_eq!(choose_session(Some("3".into()), Some("52".into())), Some("3".into()));
        assert_eq!(choose_session(None, Some("52".into())), Some("52".into()));
        assert_eq!(choose_session(Some(String::new()), Some(String::new())), None);
        assert_eq!(choose_session(None, None), None);
    }

    #[test]
    fn polkit_coming_back_registers_again_once() {
        let had = Some(":1.10".to_string());
        // a new polkitd: again
        assert!(register_again(AUTHORITY, ":1.42", &had));
        assert!(register_again(AUTHORITY, ":1.42", &None));
        // the one it registered with (a signal queued meanwhile): not twice
        assert!(!register_again(AUTHORITY, ":1.10", &had));
        // gone, or another name
        assert!(!register_again(AUTHORITY, "", &had));
        assert!(!register_again("org.bluez", ":1.42", &had));
    }

    #[test]
    fn registering_tries_on_with_a_capped_wait() {
        use std::time::Duration;
        assert_eq!(retry_after(0), Duration::from_millis(500));
        assert_eq!(retry_after(9), Duration::from_secs(5));
        assert_eq!(retry_after(59), Duration::from_secs(30));
        assert_eq!(retry_after(u32::MAX), Duration::from_secs(30));
        // (Health hears "not registered" once, after the ten tries it used to give up at)
        let said: Vec<u32> = (0..100).filter(|a| say_failure_at(*a)).collect();
        assert_eq!(said, vec![9]);
        let waited: u64 = (0..9).map(|a| retry_after(a).as_millis() as u64).sum();
        assert!(waited < 30_000, "{waited} ms before Health hears it");
    }

    #[test]
    fn helper_lines() {
        assert_eq!(parse_line("PAM_PROMPT_ECHO_OFF Password: "), Some(Line::Prompt { echo: false, text: "Password: ".into() }));
        assert_eq!(parse_line("PAM_PROMPT_ECHO_ON Name:"), Some(Line::Prompt { echo: true, text: "Name:".into() }));
        assert_eq!(parse_line("PAM_TEXT_INFO Place your finger on the reader\n"), Some(Line::Info("Place your finger on the reader".into())));
        assert_eq!(parse_line("PAM_ERROR_MSG Sorry"), Some(Line::Error("Sorry".into())));
        assert_eq!(parse_line("SUCCESS"), Some(Line::Done(true)));
        assert_eq!(parse_line("FAILURE"), Some(Line::Done(false)));
        assert_eq!(parse_line("nonsense"), None);
    }

    #[test]
    fn answers_wake_the_waiter() {
        let (reply, wait) = oneshot();
        reply.send(true);
        reply.send(false); // (only the first answer counts)
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut f = Box::pin(wait);
        assert_eq!(f.as_mut().poll(&mut cx), Poll::Ready(true));
    }

    #[test]
    fn a_fake_helper_converses() {
        // (a fake of polkit's helper: never the real one in a test)
        let dir = std::env::temp_dir().join(format!("tessera-pk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("helper.sh");
        std::fs::write(&fake, "#!/bin/sh\nread cookie\necho \"PAM_TEXT_INFO hello $1\"\necho 'PAM_PROMPT_ECHO_OFF Password: '\nread pw\n[ \"$pw\" = secret ] && [ \"$cookie\" = c1 ] && echo SUCCESS || echo FAILURE\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<Line>();
        let s = converse(7, "sam", "c1", Some(fake.to_str().unwrap()), move |b| {
            let l = b.downcast::<SessionLine>().unwrap();
            assert_eq!(l.0, 7);
            let _ = tx.send(l.1);
        })
        .unwrap();
        let t = std::time::Duration::from_secs(5);
        assert_eq!(rx.recv_timeout(t).unwrap(), Line::Info("hello sam".into()));
        assert!(matches!(rx.recv_timeout(t).unwrap(), Line::Prompt { echo: false, .. }));
        s.respond("secret");
        assert_eq!(rx.recv_timeout(t).unwrap(), Line::Done(true));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
