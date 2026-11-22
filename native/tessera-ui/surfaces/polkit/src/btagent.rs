//! A Bluetooth pairing agent (org.bluez.Agent1), BlueZ's default one: with
//! no agent at all (bluetoothctl in command mode registers none) BlueZ
//! refused every pairing that needs a code: phones (numeric comparison),
//! keyboards (type a passkey), older devices (a PIN code).
//!
//! The agent serves OBJECT on the system bus, registers with BlueZ's
//! AgentManager1 as KeyboardDisplay and asks to be the default agent; it
//! registers again whenever bluetoothd comes back (NameOwnerChanged for
//! org.bluez). Each request that needs you goes to the part (lib.rs: the
//! polkit dialog, in its Bluetooth form) and the method returns when you
//! answer: Pair, the code typed, or Cancel (org.bluez.Error.Rejected).
//! BlueZ waits 60 s for an agent's answer (src/agent.c, REQUEST_TIMEOUT);
//! TIMEOUT before that the request is given up (org.bluez.Error.Canceled)
//! and the dialog closes. BlueZ's own Cancel closes it too.
//!
//! The adapter's pairable and discoverable settings aren't touched here:
//! bluez::pair makes it pairable only for a pairing you start, so a device
//! that asks to pair by itself meanwhile is still refused by BlueZ.
//!
//! What it decides is in pure functions (the texts, the answers, whether a
//! service is allowed, when a shown code goes), tested below.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

pub const OBJECT: &str = "/org/tessera/bluez/agent";
pub const CAPABILITY: &str = "KeyboardDisplay";
/// BlueZ gives up on an agent's answer after 60 s; given up a little before
pub const TIMEOUT: Duration = Duration::from_secs(55);

type To = Arc<Mutex<Box<dyn Fn(Box<dyn Any + Send>) + Send>>>;

fn tell(to: &To, r: BtRequest) {
    if let Ok(t) = to.lock() {
        t(Box::new(r));
    }
}

// ── What is asked, and answered ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// type a PIN for it (older devices)
    PinCode,
    /// type the number it shows (up to 6 digits)
    Passkey,
    /// type this PIN on it
    DisplayPinCode(String),
    /// type this number on it (how many digits it has typed so far)
    DisplayPasskey(u32, u16),
    /// check it shows the same number (numeric comparison: phones)
    Confirm(u32),
    /// it asks to pair, no code (just works)
    Authorize,
}

impl Kind {
    /// Shown only: nothing to answer (BlueZ has its reply already)
    pub fn display(&self) -> bool {
        matches!(self, Kind::DisplayPinCode(_) | Kind::DisplayPasskey(..))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Pin(String),
    Passkey(u32),
    Yes,
    /// you said no
    No,
    /// BlueZ's Cancel, the time ran out, or the shell is going
    Cancelled,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum BtError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

fn refused(a: &Answer) -> BtError {
    match a {
        Answer::Cancelled => BtError::Canceled("The pairing was cancelled".into()),
        _ => BtError::Rejected("The pairing was turned down".into()),
    }
}

/// RequestPinCode's reply
pub fn pin_reply(a: Answer) -> Result<String, BtError> {
    match a {
        Answer::Pin(p) if valid_pin(&p) => Ok(p),
        a => Err(refused(&a)),
    }
}

/// RequestPasskey's reply
pub fn passkey_reply(a: Answer) -> Result<u32, BtError> {
    match a {
        Answer::Passkey(n) if n <= 999_999 => Ok(n),
        a => Err(refused(&a)),
    }
}

/// RequestConfirmation's and RequestAuthorization's reply
pub fn yes_reply(a: Answer) -> Result<(), BtError> {
    match a {
        Answer::Yes => Ok(()),
        a => Err(refused(&a)),
    }
}

/// A PIN BlueZ takes: 1 to 16 characters, letters and digits
pub fn valid_pin(p: &str) -> bool {
    (1..=16).contains(&p.chars().count()) && p.chars().all(|c| c.is_ascii_alphanumeric())
}

/// A passkey as typed: 1 to 6 digits (spaces allowed between)
pub fn parse_passkey(t: &str) -> Option<u32> {
    let digits: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.is_empty() || digits.len() > 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A passkey as shown: always six digits, zero-padded, in two threes
pub fn passkey_text(n: u32) -> String {
    let s = format!("{:06}", n % 1_000_000);
    format!("{} {}", &s[..3], &s[3..])
}

/// AuthorizeService: a service of a device that is paired and trusted (and
/// not blocked) is allowed; any other is refused
pub fn authorize_service(paired: bool, trusted: bool, blocked: bool) -> bool {
    paired && trusted && !blocked
}

/// What the dialog calls the device: its alias, else its name, else its address
pub fn name_of(alias: &str, name: &str, address: &str) -> String {
    [alias, name, address].into_iter().map(str::trim).find(|s| !s.is_empty()).unwrap_or("A Bluetooth device").to_string()
}

/// A code shown on the screen can go: the pairing is done, the pairing
/// this shell started is over (`was_busy`: one ran when it was shown), or
/// the time ran out
pub fn display_over(paired: bool, was_busy: bool, busy_now: bool, elapsed: Duration) -> bool {
    paired || (was_busy && !busy_now) || elapsed >= TIMEOUT
}

/// bluetoothd (org.bluez) has a new owner: register again
pub fn bluez_came_back(name: &str, new_owner: &str) -> bool {
    name == "org.bluez" && !new_owner.is_empty()
}

/// The field the dialog shows
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    None,
    Pin,
    Passkey,
}

/// What the dialog says for a request
#[derive(Clone, Debug, PartialEq)]
pub struct Texts {
    pub title: String,
    pub detail: String,
    /// the code, large ("" none)
    pub code: String,
    pub field: Field,
    pub prompt: &'static str,
    /// the main button ("" only Cancel)
    pub ok: &'static str,
}

pub fn texts(kind: &Kind, name: &str) -> Texts {
    let title = format!("Pair with {name}");
    let t = |detail: String, code: String, field: Field, prompt: &'static str, ok: &'static str| Texts { title: title.clone(), detail, code, field, prompt, ok };
    match kind {
        Kind::PinCode => t(format!("Type a PIN for {name}. A keyboard: type the same PIN on it, then press Enter on it. Others often use 0000 or 1234 (see its manual)."), String::new(), Field::Pin, "PIN", "Pair"),
        Kind::Passkey => t(format!("Type the code {name} shows."), String::new(), Field::Passkey, "Code", "Pair"),
        Kind::DisplayPinCode(pin) => t(format!("Type this PIN on {name}, then press Enter on it."), pin.clone(), Field::None, "", ""),
        Kind::DisplayPasskey(n, entered) => {
            let typed = if *entered > 0 { format!(" ({} of 6 typed)", (*entered).min(6)) } else { String::new() };
            t(format!("Type this code on {name}, then press Enter on it.{typed}"), passkey_text(*n), Field::None, "", "")
        }
        Kind::Confirm(n) => t(format!("Check that {name} shows this code, then press Pair here and on {name}."), passkey_text(*n), Field::None, "", "Pair"),
        Kind::Authorize => Texts { title: format!("Pair with {name}?"), ..t(format!("{name} asks to pair with this computer."), String::new(), Field::None, "", "Pair") },
    }
}

/// The answer to the dialog's main button for a request (`text`: the field)
/// Err: what to say instead (the field's content doesn't do)
pub fn answer_for(kind: &Kind, text: &str) -> Result<Answer, &'static str> {
    match kind {
        Kind::PinCode => {
            let t = text.trim();
            if valid_pin(t) { Ok(Answer::Pin(t.to_string())) } else { Err("A PIN is 1 to 16 letters or digits.") }
        }
        Kind::Passkey => parse_passkey(text).map(Answer::Passkey).ok_or("Type the code it shows: up to 6 digits."),
        Kind::Confirm(_) | Kind::Authorize => Ok(Answer::Yes),
        Kind::DisplayPinCode(_) | Kind::DisplayPasskey(..) => Err(""),
    }
}

// ── A one-shot answer, without an async runtime of our own ──────────────────

#[derive(Default)]
struct Slot {
    value: Option<Answer>,
    waker: Option<Waker>,
}

/// The part's answer to one request
#[derive(Clone)]
pub struct BtReply(Arc<Mutex<Slot>>);

impl BtReply {
    /// Answer it; false when it was answered already (only the first counts)
    pub fn send(&self, a: Answer) -> bool {
        let Ok(mut s) = self.0.lock() else { return false };
        let first = s.value.is_none();
        if first {
            s.value = Some(a);
        }
        if let Some(w) = s.waker.take() {
            w.wake();
        }
        first
    }
}

impl std::fmt::Debug for BtReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BtReply")
    }
}

struct Wait(Arc<Mutex<Slot>>);

impl Future for Wait {
    type Output = Answer;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Answer> {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match s.value.clone() {
            Some(v) => Poll::Ready(v),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

fn oneshot() -> (BtReply, Wait) {
    let s = Arc::new(Mutex::new(Slot::default()));
    (BtReply(s.clone()), Wait(s))
}

// ── The agent ───────────────────────────────────────────────────────────────

/// What the agent hears, for the part
#[derive(Debug)]
pub enum BtRequest {
    /// show this (`reply`: None for a code that's only shown). The same `id`
    /// again: the same request, changed (a keyboard's typed digits)
    Ask { id: u64, path: String, name: String, kind: Kind, reply: Option<BtReply> },
    /// this request is over (answered elsewhere, timed out, paired)
    Gone { id: u64 },
    /// BlueZ cancelled whatever it asked, or let the agent go
    CancelAll,
    /// registered with BlueZ (or why not)
    Registered(Result<(), String>),
}

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Agent {
    to: To,
    /// requests waiting for an answer
    pending: Arc<Mutex<Vec<(u64, BtReply)>>>,
    /// codes shown, by device (DisplayPasskey comes again with each digit typed)
    shown: Arc<Mutex<HashMap<String, u64>>>,
}

/// The device's Device1 properties
async fn device_props(conn: &zbus::Connection, path: &str) -> HashMap<String, OwnedValue> {
    if ObjectPath::try_from(path).is_err() {
        return HashMap::new();
    }
    match conn.call_method(Some("org.bluez"), path, Some("org.freedesktop.DBus.Properties"), "GetAll", &("org.bluez.Device1",)).await {
        Ok(m) => m.body().deserialize::<HashMap<String, OwnedValue>>().unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn prop_str(p: &HashMap<String, OwnedValue>, k: &str) -> String {
    match p.get(k).map(|v| &**v) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn prop_bool(p: &HashMap<String, OwnedValue>, k: &str) -> bool {
    matches!(p.get(k).map(|v| &**v), Some(Value::Bool(true)))
}

impl Agent {
    async fn name(conn: &zbus::Connection, path: &str) -> String {
        let p = device_props(conn, path).await;
        name_of(&prop_str(&p, "Alias"), &prop_str(&p, "Name"), &prop_str(&p, "Address"))
    }

    /// Ask the part, and wait for its answer (or TIMEOUT: Cancelled)
    async fn ask(&self, conn: &zbus::Connection, device: &OwnedObjectPath, kind: Kind) -> Answer {
        // (a device answering a pairing: no discovery meanwhile)
        let _busy = tessera_core::services::bluez::busy_guard();
        let name = Self::name(conn, device.as_str()).await;
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let (reply, wait) = oneshot();
        if let Ok(mut p) = self.pending.lock() {
            p.push((id, reply.clone()));
        }
        tell(&self.to, BtRequest::Ask { id, path: device.to_string(), name, kind, reply: Some(reply.clone()) });
        {
            let (reply, to) = (reply.clone(), self.to.clone());
            std::thread::spawn(move || {
                std::thread::sleep(TIMEOUT);
                if reply.send(Answer::Cancelled) {
                    eprintln!("tessera-ui: bluetooth agent: no answer in {} s, cancelled", TIMEOUT.as_secs());
                    tell(&to, BtRequest::Gone { id });
                }
            });
        }
        let a = wait.await;
        if let Ok(mut p) = self.pending.lock() {
            p.retain(|(i, _)| *i != id);
        }
        a
    }

    /// Show a code (nothing to answer): the dialog closes when the pairing
    /// is over (its Paired, or the shell's own pairing ended) or BlueZ cancels
    async fn show(&self, conn: &zbus::Connection, device: &OwnedObjectPath, kind: Kind) {
        let path = device.to_string();
        let name = Self::name(conn, &path).await;
        let again = self.shown.lock().ok().and_then(|s| s.get(&path).copied());
        let id = again.unwrap_or_else(|| NEXT.fetch_add(1, Ordering::SeqCst));
        tell(&self.to, BtRequest::Ask { id, path: path.clone(), name, kind, reply: None });
        if again.is_some() {
            return;
        }
        if let Ok(mut s) = self.shown.lock() {
            s.insert(path.clone(), id);
        }
        let was_busy = tessera_core::services::bluez::busy();
        let (to, shown, conn) = (self.to.clone(), self.shown.clone(), zbus::blocking::Connection::from(conn.clone()));
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let paired = conn
                    .call_method(Some("org.bluez"), path.as_str(), Some("org.freedesktop.DBus.Properties"), "Get", &("org.bluez.Device1", "Paired"))
                    .ok()
                    .and_then(|m| m.body().deserialize::<OwnedValue>().ok())
                    .is_some_and(|v| matches!(&*v, Value::Bool(true)));
                let gone = shown.lock().map(|s| s.get(&path) != Some(&id)).unwrap_or(true);
                if gone || display_over(paired, was_busy, tessera_core::services::bluez::busy(), start.elapsed()) {
                    break;
                }
            }
            if let Ok(mut s) = shown.lock() {
                if s.get(&path) == Some(&id) {
                    s.remove(&path);
                }
            }
            tell(&to, BtRequest::Gone { id });
        });
    }

    fn cancel_all(&self) {
        if let Ok(mut p) = self.pending.lock() {
            for (_, r) in p.drain(..) {
                r.send(Answer::Cancelled);
            }
        }
        if let Ok(mut s) = self.shown.lock() {
            s.clear();
        }
        tell(&self.to, BtRequest::CancelAll);
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    /// BlueZ let the agent go (it unregistered it, or is stopping)
    fn release(&self) {
        eprintln!("tessera-ui: bluetooth agent: released by BlueZ");
        self.cancel_all();
    }

    async fn request_pin_code(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath) -> Result<String, BtError> {
        pin_reply(self.ask(conn, &device, Kind::PinCode).await)
    }

    async fn display_pin_code(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath, pincode: String) -> Result<(), BtError> {
        self.show(conn, &device, Kind::DisplayPinCode(pincode)).await;
        Ok(())
    }

    async fn request_passkey(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath) -> Result<u32, BtError> {
        passkey_reply(self.ask(conn, &device, Kind::Passkey).await)
    }

    async fn display_passkey(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath, passkey: u32, entered: u16) {
        self.show(conn, &device, Kind::DisplayPasskey(passkey, entered)).await;
    }

    async fn request_confirmation(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath, passkey: u32) -> Result<(), BtError> {
        yes_reply(self.ask(conn, &device, Kind::Confirm(passkey)).await)
    }

    async fn request_authorization(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath) -> Result<(), BtError> {
        yes_reply(self.ask(conn, &device, Kind::Authorize).await)
    }

    /// A service of a device connecting: only a paired, trusted one's
    async fn authorize_service(&self, #[zbus(connection)] conn: &zbus::Connection, device: OwnedObjectPath, uuid: String) -> Result<(), BtError> {
        let p = device_props(conn, device.as_str()).await;
        if authorize_service(prop_bool(&p, "Paired"), prop_bool(&p, "Trusted"), prop_bool(&p, "Blocked")) {
            Ok(())
        } else {
            eprintln!("tessera-ui: bluetooth agent: refused service {uuid} of a device that isn't paired and trusted");
            Err(BtError::Rejected("Only paired, trusted devices".into()))
        }
    }

    fn cancel(&self) {
        self.cancel_all();
    }
}

/// Register with BlueZ (AgentManager1) and ask to be the default agent
fn register(conn: &zbus::blocking::Connection) -> Result<(), String> {
    let path = ObjectPath::try_from(OBJECT).map_err(|e| e.to_string())?;
    let (dest, at, iface) = (Some("org.bluez"), "/org/bluez", Some("org.bluez.AgentManager1"));
    match conn.call_method(dest, at, iface, "RegisterAgent", &(&path, CAPABILITY)) {
        Ok(_) => {}
        // (still registered from before: that will do)
        Err(e) if e.to_string().contains("AlreadyExists") => {}
        Err(e) => return Err(e.to_string()),
    }
    conn.call_method(dest, at, iface, "RequestDefaultAgent", &(&path,)).map(|_| ()).map_err(|e| e.to_string())
}

/// Cancel a pairing whose code is shown (Cancel on the dialog): the
/// device's CancelPairing, on a thread
pub fn cancel_pairing(path: String) {
    std::thread::spawn(move || {
        let Ok(conn) = zbus::blocking::Connection::system() else { return };
        let _ = conn.call_method(Some("org.bluez"), path.as_str(), Some("org.bluez.Device1"), "CancelPairing", &());
    });
}

/// Serve the agent on the system bus, register it, and register again each
/// time bluetoothd comes back
pub fn serve(to: impl Fn(Box<dyn Any + Send>) + Send + 'static) {
    let to: To = Arc::new(Mutex::new(Box::new(to)));
    std::thread::spawn(move || {
        let conn = match zbus::blocking::Connection::system() {
            Ok(c) => c,
            Err(e) => return tell(&to, BtRequest::Registered(Err(e.to_string()))),
        };
        let agent = Agent { to: to.clone(), pending: Arc::new(Mutex::new(Vec::new())), shown: Arc::new(Mutex::new(HashMap::new())) };
        if let Err(e) = conn.object_server().at(OBJECT, agent) {
            return tell(&to, BtRequest::Registered(Err(e.to_string())));
        }
        // (watching first, then registering: a restart in between isn't missed)
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg(0, "org.bluez"))
            .map(|b| b.build());
        let signals = rule.and_then(|r| zbus::blocking::MessageIterator::for_match_rule(r, &conn, Some(16)));
        // (no BlueZ yet: it registers when bluetoothd starts)
        tell(&to, BtRequest::Registered(register(&conn)));
        let Ok(signals) = signals else {
            eprintln!("tessera-ui: bluetooth agent: can't watch for bluetoothd restarts");
            loop {
                std::thread::park();
            }
        };
        for m in signals.flatten() {
            let Ok((name, _old, new)) = m.body().deserialize::<(String, String, String)>() else { continue };
            if name != "org.bluez" {
                continue;
            }
            if !bluez_came_back(&name, &new) {
                // (bluetoothd went: whatever it asked is over)
                tell(&to, BtRequest::CancelAll);
                continue;
            }
            // (its AgentManager comes up with the name: a moment, a few tries)
            let mut r = Err(String::new());
            for attempt in 0..5u64 {
                std::thread::sleep(Duration::from_millis(400 * (attempt + 1)));
                r = register(&conn);
                if r.is_ok() {
                    break;
                }
            }
            tell(&to, BtRequest::Registered(r));
        }
        // (the bus went: the object goes with the process)
        loop {
            std::thread::park();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_become_bluez_replies() {
        assert_eq!(pin_reply(Answer::Pin("0000".into())).unwrap(), "0000");
        assert!(matches!(pin_reply(Answer::Pin(String::new())), Err(BtError::Rejected(_))));
        assert!(matches!(pin_reply(Answer::No), Err(BtError::Rejected(_))));
        assert!(matches!(pin_reply(Answer::Cancelled), Err(BtError::Canceled(_))));
        assert_eq!(passkey_reply(Answer::Passkey(123456)).unwrap(), 123456);
        assert!(matches!(passkey_reply(Answer::Passkey(1_000_000)), Err(BtError::Rejected(_))));
        assert!(matches!(passkey_reply(Answer::Cancelled), Err(BtError::Canceled(_))));
        assert!(yes_reply(Answer::Yes).is_ok());
        assert!(matches!(yes_reply(Answer::No), Err(BtError::Rejected(_))));
        assert!(matches!(yes_reply(Answer::Cancelled), Err(BtError::Canceled(_))));
    }

    #[test]
    fn codes_typed_and_shown() {
        assert!(valid_pin("0000") && valid_pin("abc123") && valid_pin("1234567890123456"));
        assert!(!valid_pin("") && !valid_pin("12345678901234567") && !valid_pin("12 34") && !valid_pin("pïn"));
        assert_eq!(parse_passkey("123456"), Some(123456));
        assert_eq!(parse_passkey("012 345"), Some(12345));
        assert_eq!(parse_passkey("42"), Some(42));
        assert_eq!(parse_passkey(""), None);
        assert_eq!(parse_passkey("1234567"), None);
        assert_eq!(parse_passkey("12a456"), None);
        assert_eq!(passkey_text(12345), "012 345");
        assert_eq!(passkey_text(999999), "999 999");
        assert_eq!(passkey_text(0), "000 000");
    }

    #[test]
    fn only_paired_trusted_devices_get_services() {
        assert!(authorize_service(true, true, false));
        assert!(!authorize_service(true, false, false));
        assert!(!authorize_service(false, true, false));
        assert!(!authorize_service(true, true, true));
    }

    #[test]
    fn the_dialog_says_what_to_do() {
        let t = texts(&Kind::Confirm(7), "Phone");
        assert_eq!((t.title.as_str(), t.code.as_str(), t.field, t.ok), ("Pair with Phone", "000 007", Field::None, "Pair"));
        let t = texts(&Kind::DisplayPasskey(123456, 0), "Keyboard");
        assert_eq!((t.code.as_str(), t.ok, t.field), ("123 456", "", Field::None));
        assert!(t.detail.starts_with("Type this code on Keyboard"));
        assert!(texts(&Kind::DisplayPasskey(123456, 3), "Keyboard").detail.ends_with("(3 of 6 typed)"));
        assert_eq!(texts(&Kind::PinCode, "Speaker").field, Field::Pin);
        assert_eq!(texts(&Kind::Passkey, "Speaker").field, Field::Passkey);
        assert_eq!(texts(&Kind::DisplayPinCode("4821".into()), "Speaker").code, "4821");
        let t = texts(&Kind::Authorize, "Speaker");
        assert_eq!((t.title.as_str(), t.ok), ("Pair with Speaker?", "Pair"));
        assert!(Kind::DisplayPinCode("1".into()).display() && !Kind::Confirm(1).display());
    }

    #[test]
    fn the_main_button_answers() {
        assert_eq!(answer_for(&Kind::PinCode, " 1234 "), Ok(Answer::Pin("1234".into())));
        assert!(answer_for(&Kind::PinCode, "").is_err());
        assert_eq!(answer_for(&Kind::Passkey, "654321"), Ok(Answer::Passkey(654321)));
        assert!(answer_for(&Kind::Passkey, "abc").is_err());
        assert_eq!(answer_for(&Kind::Confirm(1), ""), Ok(Answer::Yes));
        assert_eq!(answer_for(&Kind::Authorize, ""), Ok(Answer::Yes));
        assert!(answer_for(&Kind::DisplayPasskey(1, 0), "").is_err());
    }

    #[test]
    fn names_and_restarts() {
        assert_eq!(name_of("Desk keyboard", "KB-100", "AA:BB:CC:DD:EE:FF"), "Desk keyboard");
        assert_eq!(name_of("", "KB-100", "AA:BB:CC:DD:EE:FF"), "KB-100");
        assert_eq!(name_of(" ", "", "AA:BB:CC:DD:EE:FF"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(name_of("", "", ""), "A Bluetooth device");
        assert!(bluez_came_back("org.bluez", ":1.42"));
        assert!(!bluez_came_back("org.bluez", ""));
        assert!(!bluez_came_back("org.bluez.obex", ":1.43"));
    }

    #[test]
    fn a_shown_code_goes_when_the_pairing_is_over() {
        let s = Duration::from_secs(1);
        assert!(!display_over(false, true, true, s));
        assert!(display_over(true, true, true, s));
        // (the shell's own pairing ended, paired or not)
        assert!(display_over(false, true, false, s));
        // (one the shell didn't start: only paired, or the time)
        assert!(!display_over(false, false, false, s));
        assert!(display_over(false, false, false, TIMEOUT));
    }

    #[test]
    fn only_the_first_answer_counts() {
        let (reply, wait) = oneshot();
        assert!(reply.send(Answer::Yes));
        assert!(!reply.send(Answer::Cancelled));
        let mut cx = Context::from_waker(Waker::noop());
        let mut f = Box::pin(wait);
        assert_eq!(f.as_mut().poll(&mut cx), Poll::Ready(Answer::Yes));
    }
}
