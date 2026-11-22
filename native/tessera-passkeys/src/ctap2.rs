//! The authenticator (CTAP 2.0 commands, CBOR over CTAPHID_CBOR):
//! authenticatorMakeCredential, GetAssertion, GetNextAssertion, GetInfo;
//! ClientPIN, Reset and the rest answered with an error (getInfo says no
//! PIN: user verification is the authenticator's own, a fingerprint or the
//! login password in Tessera's prompt, so every credential reports uv).
//! Attestation is "packed" self-attestation (the credential's own key signs;
//! no certificate that could tell one laptop from another).

use crate::cbor::{self, int_map, text_map, Value};
use crate::keys;
use crate::store::{Cred, Store, EDDSA, ES256};
use crate::b64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const OK: u8 = 0x00;
pub const ERR_INVALID_COMMAND: u8 = 0x01;
pub const ERR_INVALID_PARAMETER: u8 = 0x02;
pub const ERR_INVALID_LENGTH: u8 = 0x03;
pub const ERR_CBOR_UNEXPECTED_TYPE: u8 = 0x11;
pub const ERR_INVALID_CBOR: u8 = 0x12;
pub const ERR_MISSING_PARAMETER: u8 = 0x14;
pub const ERR_CREDENTIAL_EXCLUDED: u8 = 0x19;
pub const ERR_UNSUPPORTED_ALGORITHM: u8 = 0x26;
pub const ERR_OPERATION_DENIED: u8 = 0x27;
pub const ERR_KEY_STORE_FULL: u8 = 0x28;
pub const ERR_UNSUPPORTED_OPTION: u8 = 0x2b;
pub const ERR_INVALID_OPTION: u8 = 0x2c;
pub const ERR_KEEPALIVE_CANCEL: u8 = 0x2d;
pub const ERR_NO_CREDENTIALS: u8 = 0x2e;
pub const ERR_USER_ACTION_TIMEOUT: u8 = 0x2f;
pub const ERR_NOT_ALLOWED: u8 = 0x30;
pub const ERR_PIN_NOT_SET: u8 = 0x35;
pub const ERR_OTHER: u8 = 0x7f;

pub const MAKE_CREDENTIAL: u8 = 0x01;
pub const GET_ASSERTION: u8 = 0x02;
pub const GET_INFO: u8 = 0x04;
pub const CLIENT_PIN: u8 = 0x06;
pub const RESET: u8 = 0x07;
pub const GET_NEXT_ASSERTION: u8 = 0x08;

/// Tessera's AAGUID: the same on every install (it names the kind of
/// authenticator, not this one)
pub const AAGUID: [u8; 16] = [0x7e, 0x55, 0xe7, 0xa0, 0x5a, 0x55, 0x4b, 0x3c, 0x9d, 0x1e, 0x2f, 0x6a, 0x8c, 0x0b, 0x4d, 0x13];
pub const MAX_MSG: i64 = 7609;
pub const MAX_CREDS: usize = 500;

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

// ── Asking the user ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// a new passkey or key (makeCredential, U2F register)
    Create,
    /// signing in (getAssertion, U2F authenticate)
    SignIn,
    /// Settings deleting one
    Delete,
}

impl Purpose {
    pub fn word(self) -> &'static str {
        match self {
            Purpose::Create => "create",
            Purpose::SignIn => "sign-in",
            Purpose::Delete => "delete",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub purpose: Purpose,
    /// the site (rpId), or how to name one we only know the hash of
    pub rp: String,
    /// the account, when one is known
    pub user: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// a finger, or the password, in Tessera's prompt
    Approved,
    /// Cancel in the prompt
    Denied,
    /// the browser (or app) gave up
    Cancelled,
    TimedOut,
    /// no prompt to ask in (the shell isn't running)
    NoPrompt,
}

/// One request's link to its transport: did the host cancel it, and the
/// keepalive to send while the user is asked
pub struct Txn<'a> {
    pub cancelled: &'a AtomicBool,
    pub keepalive: &'a (dyn Fn() + Sync),
}

impl Txn<'_> {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub trait Verifier: Send + Sync {
    fn verify(&self, ask: &Ask, txn: &Txn) -> Verdict;
}

pub fn verdict_error(v: Verdict) -> u8 {
    match v {
        Verdict::Approved => OK,
        Verdict::Denied | Verdict::NoPrompt => ERR_OPERATION_DENIED,
        Verdict::Cancelled => ERR_KEEPALIVE_CANCEL,
        Verdict::TimedOut => ERR_USER_ACTION_TIMEOUT,
    }
}

/// What a request needs from the user, from its options
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// show the prompt (user verification: a finger or the password)
    pub prompt: bool,
    /// the flags signed: user present, user verified
    pub up: bool,
    pub uv: bool,
}

fn option(options: Option<&Value>, key: &str) -> Result<Option<bool>, u8> {
    let Some(o) = options else { return Ok(None) };
    if o.as_map().is_none() {
        return Err(ERR_CBOR_UNEXPECTED_TYPE);
    }
    match o.get_text(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ERR_CBOR_UNEXPECTED_TYPE),
    }
}

/// makeCredential: always verified (a new credential is never made silently); up=false is invalid
pub fn plan_make(options: Option<&Value>) -> Result<Plan, u8> {
    if option(options, "up")? == Some(false) {
        return Err(ERR_INVALID_OPTION);
    }
    option(options, "uv")?;
    option(options, "rk")?;
    Ok(Plan { prompt: true, up: true, uv: true })
}

/// getAssertion: up (default true) or uv asks; neither: a silent check,
/// signed with no flags (the site sees nobody was asked); rk isn't an option here
pub fn plan_get(options: Option<&Value>) -> Result<Plan, u8> {
    if option(options, "rk")?.is_some() {
        return Err(ERR_UNSUPPORTED_OPTION);
    }
    let up = option(options, "up")?.unwrap_or(true);
    let uv = option(options, "uv")?.unwrap_or(false);
    let prompt = up || uv;
    Ok(Plan { prompt, up, uv: prompt })
}

// ── The authenticator ───────────────────────────────────────────────────────

struct Next {
    creds: Vec<Cred>,
    cdh: Vec<u8>,
    rp_hash: [u8; 32],
    plan: Plan,
    until: i64,
}

pub struct Authenticator {
    pub store: Arc<Mutex<Store>>,
    pub verifier: Arc<dyn Verifier>,
    next: Option<Next>,
    pub now: fn() -> i64,
    pub u2f: crate::u2f::Latch,
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn reply(status: u8, body: Option<Value>) -> Vec<u8> {
    let mut r = vec![status];
    if let Some(b) = body {
        r.extend(b.encode());
    }
    r
}

fn err(code: u8) -> Vec<u8> {
    vec![code]
}

pub fn auth_data(rp_hash: &[u8; 32], flags: u8, counter: u32, attested: Option<&[u8]>) -> Vec<u8> {
    let mut a = rp_hash.to_vec();
    a.push(flags | if attested.is_some() { FLAG_AT } else { 0 });
    a.extend_from_slice(&counter.to_be_bytes());
    if let Some(at) = attested {
        a.extend_from_slice(at);
    }
    a
}

fn flags(plan: &Plan) -> u8 {
    (if plan.up { FLAG_UP } else { 0 }) | (if plan.uv { FLAG_UV } else { 0 })
}

pub fn get_info() -> Value {
    int_map(vec![
        (1, Value::Array(vec![Value::text("U2F_V2"), Value::text("FIDO_2_0")])),
        (3, Value::Bytes(AAGUID.to_vec())),
        (4, text_map(vec![("rk", Value::Bool(true)), ("up", Value::Bool(true)), ("uv", Value::Bool(true)), ("plat", Value::Bool(false))])),
        (5, Value::Int(MAX_MSG)),
        (7, Value::Int(16)),
        (8, Value::Int(128)),
        (9, Value::Array(vec![Value::text("usb")])),
        (
            10,
            Value::Array(vec![
                text_map(vec![("alg", Value::Int(ES256)), ("type", Value::text("public-key"))]),
                text_map(vec![("alg", Value::Int(EDDSA)), ("type", Value::text("public-key"))]),
            ]),
        ),
    ])
}

/// The ids in an allow or exclude list (public-key ones)
fn list_ids(v: Option<&Value>) -> Result<Vec<Vec<u8>>, u8> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let a = v.as_array().ok_or(ERR_CBOR_UNEXPECTED_TYPE)?;
    let mut ids = Vec::new();
    for d in a {
        if d.as_map().is_none() {
            return Err(ERR_CBOR_UNEXPECTED_TYPE);
        }
        let id = d.get_text("id").and_then(Value::as_bytes).ok_or(ERR_MISSING_PARAMETER)?;
        if d.get_text("type").and_then(Value::as_text) == Some("public-key") {
            ids.push(id.to_vec());
        }
    }
    Ok(ids)
}

fn text_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or("").chars().take(256).collect()
}

/// How the prompt names a credential's account
pub fn user_label(c: &Cred) -> String {
    if !c.user_name.is_empty() {
        c.user_name.clone()
    } else {
        c.display_name.clone()
    }
}

impl Authenticator {
    pub fn new(store: Arc<Mutex<Store>>, verifier: Arc<dyn Verifier>) -> Authenticator {
        Authenticator { store, verifier, next: None, now: unix_now, u2f: Default::default() }
    }

    /// One CTAPHID_CBOR message: the command byte and its CBOR → status and CBOR
    pub fn handle(&mut self, msg: &[u8], txn: &Txn) -> Vec<u8> {
        let Some((&cmd, body)) = msg.split_first() else { return err(ERR_INVALID_LENGTH) };
        if cmd != GET_NEXT_ASSERTION {
            self.next = None;
        }
        let params = if body.is_empty() {
            None
        } else {
            match cbor::decode(body) {
                Ok(v) if v.as_map().is_some() => Some(v),
                Ok(_) => return err(ERR_CBOR_UNEXPECTED_TYPE),
                Err(_) => return err(ERR_INVALID_CBOR),
            }
        };
        match cmd {
            GET_INFO => reply(OK, Some(get_info())),
            MAKE_CREDENTIAL => match params {
                Some(p) => self.make_credential(&p, txn),
                None => err(ERR_MISSING_PARAMETER),
            },
            GET_ASSERTION => match params {
                Some(p) => self.get_assertion(&p, txn),
                None => err(ERR_MISSING_PARAMETER),
            },
            GET_NEXT_ASSERTION => self.get_next(),
            // (no PIN; removing everything is Settings' to do, one at a time)
            CLIENT_PIN => err(ERR_PIN_NOT_SET),
            RESET => err(ERR_OPERATION_DENIED),
            _ => err(ERR_INVALID_COMMAND),
        }
    }

    fn make_credential(&mut self, p: &Value, txn: &Txn) -> Vec<u8> {
        let (Some(cdh), Some(rp), Some(user), Some(params)) = (p.get(1), p.get(2), p.get(3), p.get(4)) else { return err(ERR_MISSING_PARAMETER) };
        let Some(cdh) = cdh.as_bytes() else { return err(ERR_CBOR_UNEXPECTED_TYPE) };
        let Some(rp_id) = rp.get_text("id").and_then(Value::as_text) else { return err(ERR_MISSING_PARAMETER) };
        let Some(user_id) = user.get_text("id").and_then(Value::as_bytes) else { return err(ERR_MISSING_PARAMETER) };
        if user_id.len() > 64 || rp_id.is_empty() || rp_id.len() > 256 {
            return err(ERR_INVALID_PARAMETER);
        }
        if p.get(8).is_some() {
            return err(ERR_PIN_NOT_SET);
        }
        let Some(params) = params.as_array() else { return err(ERR_CBOR_UNEXPECTED_TYPE) };
        let alg = params
            .iter()
            .filter(|c| c.get_text("type").and_then(Value::as_text) == Some("public-key"))
            .filter_map(|c| c.get_text("alg").and_then(Value::as_int))
            .find(|a| *a == ES256 || *a == EDDSA);
        let Some(alg) = alg else { return err(ERR_UNSUPPORTED_ALGORITHM) };
        let plan = match plan_make(p.get(7)) {
            Ok(pl) => pl,
            Err(e) => return err(e),
        };
        let resident = option(p.get(7), "rk").ok().flatten().unwrap_or(false);
        let rp_hash = keys::sha256(rp_id.as_bytes());
        let exclude = match list_ids(p.get(5)) {
            Ok(x) => x,
            Err(e) => return err(e),
        };
        {
            let s = self.store.lock().unwrap();
            if !exclude.is_empty() && !s.allowed(&b64::hex(&rp_hash), &exclude).is_empty() {
                return err(ERR_CREDENTIAL_EXCLUDED);
            }
            if s.creds().len() >= MAX_CREDS {
                return err(ERR_KEY_STORE_FULL);
            }
        }
        let user_name = text_of(user.get_text("name"));
        let display_name = text_of(user.get_text("displayName"));
        let ask = Ask { purpose: Purpose::Create, rp: rp_id.to_string(), user: if user_name.is_empty() { display_name.clone() } else { user_name.clone() } };
        let v = self.verifier.verify(&ask, txn);
        if v != Verdict::Approved {
            return err(verdict_error(v));
        }
        let made = (|| -> std::io::Result<Vec<u8>> {
            let mut secret = keys::generate(alg)?;
            let id = crate::store::random::<32>()?;
            let cose = keys::cose_public(alg, &secret)?.encode();
            let cred = Cred {
                id: b64::encode(&id),
                rp_id: rp_id.to_string(),
                rp_hash: b64::hex(&rp_hash),
                rp_name: text_of(rp.get_text("name")),
                user_id: b64::encode(user_id),
                user_name,
                display_name,
                alg,
                created: (self.now)(),
                last_used: 0,
                counter: 0,
                resident,
                u2f: false,
            };
            self.store.lock().unwrap().add(cred, &secret)?;
            let mut attested = AAGUID.to_vec();
            attested.extend_from_slice(&(id.len() as u16).to_be_bytes());
            attested.extend_from_slice(&id);
            attested.extend_from_slice(&cose);
            let ad = auth_data(&rp_hash, flags(&plan), 0, Some(&attested));
            let sig = keys::sign(alg, &secret, &[ad.as_slice(), cdh].concat())?;
            secret.iter_mut().for_each(|b| *b = 0);
            let att = text_map(vec![("alg", Value::Int(alg)), ("sig", Value::Bytes(sig))]);
            Ok(reply(OK, Some(int_map(vec![(1, Value::text("packed")), (2, Value::Bytes(ad)), (3, att)]))))
        })();
        made.unwrap_or_else(|e| {
            eprintln!("tessera-passkeys: couldn't save the new credential: {e}");
            err(ERR_OTHER)
        })
    }

    fn get_assertion(&mut self, p: &Value, txn: &Txn) -> Vec<u8> {
        let (Some(rp_id), Some(cdh)) = (p.get(1).and_then(Value::as_text), p.get(2).and_then(Value::as_bytes)) else { return err(ERR_MISSING_PARAMETER) };
        if p.get(6).is_some() {
            return err(ERR_PIN_NOT_SET);
        }
        let plan = match plan_get(p.get(5)) {
            Ok(pl) => pl,
            Err(e) => return err(e),
        };
        let allow = match list_ids(p.get(3)) {
            Ok(x) => x,
            Err(e) => return err(e),
        };
        let rp_hash = keys::sha256(rp_id.as_bytes());
        let hex = b64::hex(&rp_hash);
        let creds = {
            let s = self.store.lock().unwrap();
            if allow.is_empty() { s.resident(&hex) } else { s.allowed(&hex, &allow) }
        };
        if creds.is_empty() {
            return err(ERR_NO_CREDENTIALS);
        }
        if plan.prompt {
            let user = if creds.len() == 1 { user_label(&creds[0]) } else { String::new() };
            let v = self.verifier.verify(&Ask { purpose: Purpose::SignIn, rp: rp_id.to_string(), user }, txn);
            if v != Verdict::Approved {
                return err(verdict_error(v));
            }
        }
        let many = allow.is_empty() && creds.len() > 1;
        let mut rest = creds;
        let first = rest.remove(0);
        let r = self.assertion(&first, cdh, &rp_hash, &plan, if many { Some(rest.len() + 1) } else { None }, allow.is_empty());
        if many && r.first() == Some(&OK) {
            self.next = Some(Next { creds: rest, cdh: cdh.to_vec(), rp_hash, plan, until: (self.now)() + 30 });
        }
        r
    }

    fn get_next(&mut self) -> Vec<u8> {
        let now = (self.now)();
        if !self.next.as_ref().is_some_and(|n| now <= n.until && !n.creds.is_empty()) {
            self.next = None;
            return err(ERR_NOT_ALLOWED);
        }
        let Some(n) = self.next.as_mut() else { return err(ERR_NOT_ALLOWED) };
        let c = n.creds.remove(0);
        let (cdh, rp_hash, plan) = (n.cdh.clone(), n.rp_hash, n.plan);
        let r = self.assertion(&c, &cdh, &rp_hash, &plan, None, true);
        if self.next.as_ref().is_some_and(|n| n.creds.is_empty()) {
            self.next = None;
        }
        r
    }

    /// One credential's assertion: the counter up and saved first, then signed
    fn assertion(&mut self, c: &Cred, cdh: &[u8], rp_hash: &[u8; 32], plan: &Plan, count: Option<usize>, with_user: bool) -> Vec<u8> {
        let signed = (|| -> std::io::Result<Vec<u8>> {
            let (counter, mut secret) = {
                let mut s = self.store.lock().unwrap();
                let secret = s.secret(&c.id)?;
                (s.bump(&c.id, (self.now)())?, secret)
            };
            let ad = auth_data(rp_hash, flags(plan), counter, None);
            let sig = keys::sign(c.alg, &secret, &[ad.as_slice(), cdh].concat());
            secret.iter_mut().for_each(|b| *b = 0);
            let mut m = vec![
                (1, text_map(vec![("id", Value::Bytes(c.id_bytes())), ("type", Value::text("public-key"))])),
                (2, Value::Bytes(ad)),
                (3, Value::Bytes(sig?)),
            ];
            if with_user && c.resident {
                let mut u = vec![("id", Value::Bytes(c.user_id_bytes()))];
                // (names only once the user is verified)
                if plan.uv {
                    if !c.user_name.is_empty() {
                        u.push(("name", Value::Text(c.user_name.clone())));
                    }
                    if !c.display_name.is_empty() {
                        u.push(("displayName", Value::Text(c.display_name.clone())));
                    }
                }
                m.push((4, text_map(u)));
            }
            if let Some(n) = count {
                m.push((5, Value::Int(n as i64)));
            }
            Ok(reply(OK, Some(int_map(m))))
        })();
        signed.unwrap_or_else(|e| {
            eprintln!("tessera-passkeys: couldn't sign: {e}");
            err(ERR_OTHER)
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::store::{test_dir, FakeSealer};

    /// A verifier that answers as told, and remembers what it was asked
    pub struct Fake {
        pub answer: Mutex<Verdict>,
        pub asked: Mutex<Vec<Ask>>,
    }

    impl Fake {
        pub fn new(v: Verdict) -> Arc<Fake> {
            Arc::new(Fake { answer: Mutex::new(v), asked: Mutex::new(Vec::new()) })
        }
    }

    impl Verifier for Fake {
        fn verify(&self, ask: &Ask, txn: &Txn) -> Verdict {
            (txn.keepalive)();
            self.asked.lock().unwrap().push(ask.clone());
            *self.answer.lock().unwrap()
        }
    }

    pub fn with_txn<T>(f: impl FnOnce(&Txn) -> T) -> T {
        let c = AtomicBool::new(false);
        let k = || {};
        f(&Txn { cancelled: &c, keepalive: &k })
    }

    pub fn authenticator(name: &str, v: Arc<Fake>) -> Authenticator {
        let store = Store::open(test_dir(name), Box::new(FakeSealer)).unwrap();
        let mut a = Authenticator::new(Arc::new(Mutex::new(store)), v);
        a.now = || 1_700_000_000;
        a
    }

    fn make_req(rp: &str, user: &[u8], alg: i64, rk: bool, exclude: Vec<Vec<u8>>) -> Vec<u8> {
        let mut m = vec![
            (1, Value::Bytes(vec![7u8; 32])),
            (2, text_map(vec![("id", Value::text(rp)), ("name", Value::text("Site"))])),
            (3, text_map(vec![("id", Value::Bytes(user.to_vec())), ("name", Value::text("sam@example.com")), ("displayName", Value::text("Sam"))])),
            (4, Value::Array(vec![text_map(vec![("alg", Value::Int(-257)), ("type", Value::text("public-key"))]), text_map(vec![("alg", Value::Int(alg)), ("type", Value::text("public-key"))])])),
            (7, text_map(vec![("rk", Value::Bool(rk))])),
        ];
        if !exclude.is_empty() {
            m.push((5, Value::Array(exclude.into_iter().map(|id| text_map(vec![("id", Value::Bytes(id)), ("type", Value::text("public-key"))])).collect())));
        }
        [vec![MAKE_CREDENTIAL], int_map(m).encode()].concat()
    }

    fn get_req(rp: &str, allow: Vec<Vec<u8>>, options: Option<Value>) -> Vec<u8> {
        let mut m = vec![(1, Value::text(rp)), (2, Value::Bytes(vec![9u8; 32]))];
        if !allow.is_empty() {
            m.push((3, Value::Array(allow.into_iter().map(|id| text_map(vec![("id", Value::Bytes(id)), ("type", Value::text("public-key"))])).collect())));
        }
        if let Some(o) = options {
            m.push((5, o));
        }
        [vec![GET_ASSERTION], int_map(m).encode()].concat()
    }

    fn body(r: &[u8]) -> Value {
        assert_eq!(r[0], OK, "status {:#x}", r[0]);
        cbor::decode(&r[1..]).unwrap()
    }

    /// The credential id out of makeCredential's authData
    pub fn cred_id(made: &Value) -> Vec<u8> {
        let ad = made.get(2).unwrap().as_bytes().unwrap();
        let n = u16::from_be_bytes([ad[53], ad[54]]) as usize;
        ad[55..55 + n].to_vec()
    }

    #[test]
    fn get_info_says_what_it_is() {
        let a = &mut authenticator("info", Fake::new(Verdict::Approved));
        let r = with_txn(|t| a.handle(&[GET_INFO], t));
        let v = body(&r);
        assert_eq!(v.get(1).unwrap().as_array().unwrap()[1], Value::text("FIDO_2_0"));
        assert_eq!(v.get(3).unwrap().as_bytes(), Some(AAGUID.as_slice()));
        let o = v.get(4).unwrap();
        assert_eq!(o.get_text("uv"), Some(&Value::Bool(true)));
        assert_eq!(o.get_text("rk"), Some(&Value::Bool(true)));
        // no PIN at all: not offered, so never asked for
        assert_eq!(o.get_text("clientPin"), None);
        assert_eq!(r[1] & 0xe0, 0xa0); // (a map)
    }

    #[test]
    fn options_decide_who_is_asked() {
        let m = |e: Vec<(&str, Value)>| text_map(e);
        assert_eq!(plan_make(None), Ok(Plan { prompt: true, up: true, uv: true }));
        assert_eq!(plan_make(Some(&m(vec![("uv", Value::Bool(false))]))), Ok(Plan { prompt: true, up: true, uv: true }));
        assert_eq!(plan_make(Some(&m(vec![("up", Value::Bool(false))]))), Err(ERR_INVALID_OPTION));
        assert_eq!(plan_get(None), Ok(Plan { prompt: true, up: true, uv: true }));
        // a silent check: nobody asked, nothing claimed
        assert_eq!(plan_get(Some(&m(vec![("up", Value::Bool(false))]))), Ok(Plan { prompt: false, up: false, uv: false }));
        assert_eq!(plan_get(Some(&m(vec![("up", Value::Bool(false)), ("uv", Value::Bool(true))]))), Ok(Plan { prompt: true, up: false, uv: true }));
        assert_eq!(plan_get(Some(&m(vec![("rk", Value::Bool(true))]))), Err(ERR_UNSUPPORTED_OPTION));
        assert_eq!(plan_get(Some(&m(vec![("up", Value::Int(1))]))), Err(ERR_CBOR_UNEXPECTED_TYPE));
        assert_eq!(plan_get(Some(&Value::Int(1))), Err(ERR_CBOR_UNEXPECTED_TYPE));
        assert_eq!(verdict_error(Verdict::Cancelled), ERR_KEEPALIVE_CANCEL);
        assert_eq!(verdict_error(Verdict::NoPrompt), ERR_OPERATION_DENIED);
        assert_eq!(verdict_error(Verdict::TimedOut), ERR_USER_ACTION_TIMEOUT);
    }

    #[test]
    fn a_passkey_is_made_and_signs_in() {
        use p256::ecdsa::signature::Verifier as _;
        let fake = Fake::new(Verdict::Approved);
        let a = &mut authenticator("make", fake.clone());
        let made = body(&with_txn(|t| a.handle(&make_req("example.com", b"u1", ES256, true, vec![]), t)));
        assert_eq!(made.get(1), Some(&Value::text("packed")));
        let ad = made.get(2).unwrap().as_bytes().unwrap().to_vec();
        assert_eq!(&ad[..32], &keys::sha256(b"example.com"));
        assert_eq!(ad[32], FLAG_UP | FLAG_UV | FLAG_AT);
        assert_eq!(&ad[37..53], &AAGUID);
        // the COSE key after the id, and the self-attestation verifies with it
        let id = cred_id(&made);
        assert_eq!(id.len(), 32);
        let cose = cbor::decode(&ad[55 + 32..]).unwrap();
        let point = [vec![4u8], cose.get(-2).unwrap().as_bytes().unwrap().to_vec(), cose.get(-3).unwrap().as_bytes().unwrap().to_vec()].concat();
        let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).unwrap();
        let att = made.get(3).unwrap();
        assert_eq!(att.get_text("alg"), Some(&Value::Int(ES256)));
        let sig = p256::ecdsa::Signature::from_der(att.get_text("sig").unwrap().as_bytes().unwrap()).unwrap();
        assert!(vk.verify(&[ad.as_slice(), &[7u8; 32][..]].concat(), &sig).is_ok());
        assert_eq!(fake.asked.lock().unwrap()[0], Ask { purpose: Purpose::Create, rp: "example.com".into(), user: "sam@example.com".into() });

        // sign in with no allow list (a passkey), twice: the counter climbs
        for want in 1..=2u32 {
            let got = body(&with_txn(|t| a.handle(&get_req("example.com", vec![], None), t)));
            assert_eq!(got.get(1).unwrap().get_text("id").unwrap().as_bytes(), Some(id.as_slice()));
            let ad = got.get(2).unwrap().as_bytes().unwrap();
            assert_eq!(ad.len(), 37);
            assert_eq!(ad[32], FLAG_UP | FLAG_UV);
            assert_eq!(u32::from_be_bytes(ad[33..37].try_into().unwrap()), want);
            let sig = p256::ecdsa::Signature::from_der(got.get(3).unwrap().as_bytes().unwrap()).unwrap();
            assert!(vk.verify(&[ad, &[9u8; 32][..]].concat(), &sig).is_ok());
            let user = got.get(4).unwrap();
            assert_eq!(user.get_text("id").unwrap().as_bytes(), Some(b"u1".as_slice()));
            assert_eq!(user.get_text("name"), Some(&Value::text("sam@example.com")));
            assert!(got.get(5).is_none());
        }
        // another site: nothing
        assert_eq!(with_txn(|t| a.handle(&get_req("other.com", vec![], None), t)), vec![ERR_NO_CREDENTIALS]);
        // made again with it excluded: refused before asking
        let n = fake.asked.lock().unwrap().len();
        assert_eq!(with_txn(|t| a.handle(&make_req("example.com", b"u2", ES256, true, vec![id.clone()]), t)), vec![ERR_CREDENTIAL_EXCLUDED]);
        assert_eq!(fake.asked.lock().unwrap().len(), n);
    }

    #[test]
    fn ssh_keys_are_eddsa_and_need_their_id() {
        let fake = Fake::new(Verdict::Approved);
        let a = &mut authenticator("ssh", fake.clone());
        let made = body(&with_txn(|t| a.handle(&make_req("ssh:", &[0u8; 32], EDDSA, false, vec![]), t)));
        assert_eq!(made.get(3).unwrap().get_text("alg"), Some(&Value::Int(EDDSA)));
        let id = cred_id(&made);
        // not discoverable: no allow list finds nothing
        assert_eq!(with_txn(|t| a.handle(&get_req("ssh:", vec![], None), t)), vec![ERR_NO_CREDENTIALS]);
        let got = body(&with_txn(|t| a.handle(&get_req("ssh:", vec![id], None), t)));
        assert_eq!(got.get(3).unwrap().as_bytes().unwrap().len(), 64);
        assert!(got.get(4).is_none());
    }

    #[test]
    fn refusals_and_silent_checks() {
        let fake = Fake::new(Verdict::Approved);
        let a = &mut authenticator("refuse", fake.clone());
        let made = body(&with_txn(|t| a.handle(&make_req("example.com", b"u1", ES256, true, vec![]), t)));
        let id = cred_id(&made);
        *fake.answer.lock().unwrap() = Verdict::Denied;
        assert_eq!(with_txn(|t| a.handle(&get_req("example.com", vec![], None), t)), vec![ERR_OPERATION_DENIED]);
        *fake.answer.lock().unwrap() = Verdict::Cancelled;
        assert_eq!(with_txn(|t| a.handle(&make_req("b.com", b"u", ES256, true, vec![]), t)), vec![ERR_KEEPALIVE_CANCEL]);
        // nothing saved for a refused one
        assert_eq!(a.store.lock().unwrap().creds().len(), 1);
        // up=false: answered without asking, and with no flags
        let n = fake.asked.lock().unwrap().len();
        let got = body(&with_txn(|t| a.handle(&get_req("example.com", vec![id], Some(text_map(vec![("up", Value::Bool(false))]))), t)));
        assert_eq!(got.get(2).unwrap().as_bytes().unwrap()[32], 0);
        assert_eq!(fake.asked.lock().unwrap().len(), n);
        // a PIN is never set; bad CBOR and unknown commands
        assert_eq!(with_txn(|t| a.handle(&[CLIENT_PIN, 0xa0], t)), vec![ERR_PIN_NOT_SET]);
        assert_eq!(with_txn(|t| a.handle(&[GET_ASSERTION, 0xff], t)), vec![ERR_INVALID_CBOR]);
        assert_eq!(with_txn(|t| a.handle(&[GET_ASSERTION, 0x01], t)), vec![ERR_CBOR_UNEXPECTED_TYPE]);
        assert_eq!(with_txn(|t| a.handle(&[0x55], t)), vec![ERR_INVALID_COMMAND]);
        assert_eq!(with_txn(|t| a.handle(&[RESET], t)), vec![ERR_OPERATION_DENIED]);
        assert_eq!(with_txn(|t| a.handle(&[], t)), vec![ERR_INVALID_LENGTH]);
        // only algorithms it has
        let req = [vec![MAKE_CREDENTIAL], int_map(vec![
            (1, Value::Bytes(vec![0; 32])),
            (2, text_map(vec![("id", Value::text("x.com"))])),
            (3, text_map(vec![("id", Value::Bytes(vec![1]))])),
            (4, Value::Array(vec![text_map(vec![("alg", Value::Int(-257)), ("type", Value::text("public-key"))])])),
        ]).encode()].concat();
        assert_eq!(with_txn(|t| a.handle(&req, t)), vec![ERR_UNSUPPORTED_ALGORITHM]);
    }

    #[test]
    fn several_passkeys_come_one_after_another() {
        let fake = Fake::new(Verdict::Approved);
        let a = &mut authenticator("next", fake.clone());
        a.now = || 100;
        body(&with_txn(|t| a.handle(&make_req("example.com", b"u1", ES256, true, vec![]), t)));
        a.now = || 200;
        body(&with_txn(|t| a.handle(&make_req("example.com", b"u2", EDDSA, true, vec![]), t)));
        let first = body(&with_txn(|t| a.handle(&get_req("example.com", vec![], None), t)));
        assert_eq!(first.get(5), Some(&Value::Int(2)));
        // newest first
        assert_eq!(first.get(4).unwrap().get_text("id").unwrap().as_bytes(), Some(b"u2".as_slice()));
        // the prompt named no one account
        assert_eq!(fake.asked.lock().unwrap().last().unwrap().user, "");
        let second = body(&with_txn(|t| a.handle(&[GET_NEXT_ASSERTION], t)));
        assert_eq!(second.get(4).unwrap().get_text("id").unwrap().as_bytes(), Some(b"u1".as_slice()));
        assert!(second.get(5).is_none());
        assert_eq!(with_txn(|t| a.handle(&[GET_NEXT_ASSERTION], t)), vec![ERR_NOT_ALLOWED]);
    }
}
