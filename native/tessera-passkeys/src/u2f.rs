//! U2F (CTAP1, raw messages over CTAPHID_MSG) for sites' older two-step
//! keys: REGISTER, AUTHENTICATE, VERSION. U2F has no keepalive: a host
//! asks again and again, and a key answers "conditions not satisfied"
//! (0x6985) until it's touched. So the first ask starts Tessera's prompt in
//! the background (`Latch`), the asks after it are told to wait, and once
//! the prompt is approved the next ask for the same thing is answered.

use crate::ctap2::{Ask, Authenticator, Purpose, Txn, Verdict};
use crate::keys;
use crate::store::{Cred, ES256};
use crate::b64;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub const SW_OK: u16 = 0x9000;
pub const SW_CONDITIONS: u16 = 0x6985;
pub const SW_WRONG_DATA: u16 = 0x6a80;
pub const SW_WRONG_LENGTH: u16 = 0x6700;
pub const SW_INS: u16 = 0x6d00;
pub const SW_CLA: u16 = 0x6e00;

const REGISTER: u8 = 0x01;
const AUTHENTICATE: u8 = 0x02;
const VERSION: u8 = 0x03;
const CHECK_ONLY: u8 = 0x07;
const ENFORCE: u8 = 0x03;
const DONT_ENFORCE: u8 = 0x08;

/// An approval lasts this long for the ask it was for
pub const APPROVED_FOR: i64 = 10;
/// after a refusal, the same ask isn't put to the user again for this long
pub const REFUSED_FOR: i64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Asking([u8; 32]),
    Approved([u8; 32], i64),
    Refused([u8; 32], i64),
}

/// The prompt's answer, held for the ask it was for
#[derive(Clone)]
pub struct Latch(Arc<Mutex<State>>);

impl Default for Latch {
    fn default() -> Self {
        Latch(Arc::new(Mutex::new(State::Idle)))
    }
}

/// What the latch says about an ask
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// approved: go ahead (the approval is used up)
    Go,
    /// asked already, or refused just now: wait
    Wait,
    /// put it to the user now
    Start,
}

impl Latch {
    pub fn check(&self, key: [u8; 32], now: i64) -> Gate {
        let mut s = self.0.lock().unwrap();
        match &*s {
            State::Approved(k, t) if *k == key && now - t <= APPROVED_FOR => {
                *s = State::Idle;
                Gate::Go
            }
            State::Asking(k) if *k == key => Gate::Wait,
            // (another ask is being put to the user: this one waits its turn)
            State::Asking(_) => Gate::Wait,
            State::Refused(k, t) if *k == key && now - t <= REFUSED_FOR => Gate::Wait,
            _ => {
                *s = State::Asking(key);
                Gate::Start
            }
        }
    }

    pub fn answered(&self, key: [u8; 32], approved: bool, now: i64) {
        let mut s = self.0.lock().unwrap();
        if *s == State::Asking(key) {
            *s = if approved { State::Approved(key, now) } else { State::Refused(key, now) };
        }
    }
}

fn sw(mut data: Vec<u8>, code: u16) -> Vec<u8> {
    data.extend_from_slice(&code.to_be_bytes());
    data
}

/// An APDU's instruction, P1 and data (short or extended lengths)
pub fn parse_apdu(a: &[u8]) -> Result<(u8, u8, u8, &[u8]), u16> {
    if a.len() < 4 {
        return Err(SW_WRONG_LENGTH);
    }
    let (cla, ins, p1) = (a[0], a[1], a[2]);
    if cla != 0 {
        return Err(SW_CLA);
    }
    if a.len() <= 5 {
        return Ok((ins, p1, a[3], &a[0..0]));
    }
    if a[4] == 0 && a.len() >= 7 {
        let n = u16::from_be_bytes([a[5], a[6]]) as usize;
        // (data, then perhaps a two-byte Le)
        if a.len() < 7 + n || a.len() > 7 + n + 2 {
            return Err(SW_WRONG_LENGTH);
        }
        return Ok((ins, p1, a[3], &a[7..7 + n]));
    }
    let n = a[4] as usize;
    if a.len() < 5 + n || a.len() > 5 + n + 1 {
        return Err(SW_WRONG_LENGTH);
    }
    Ok((ins, p1, a[3], &a[5..5 + n]))
}

fn gate_key(ins: u8, app: &[u8], kh: &[u8]) -> [u8; 32] {
    keys::sha256(&[&[ins][..], app, kh].concat())
}

impl Authenticator {
    /// One CTAPHID_MSG: an APDU in, response data and status word out
    pub fn handle_u2f(&mut self, apdu: &[u8]) -> Vec<u8> {
        let (ins, p1, _p2, data) = match parse_apdu(apdu) {
            Ok(x) => x,
            Err(code) => return sw(Vec::new(), code),
        };
        match ins {
            VERSION => sw(b"U2F_V2".to_vec(), SW_OK),
            REGISTER => self.u2f_register(data),
            AUTHENTICATE => self.u2f_authenticate(p1, data),
            _ => sw(Vec::new(), SW_INS),
        }
    }

    /// Put the ask to the user on a thread of its own; its answer goes to the latch
    fn u2f_start(&self, key: [u8; 32], ask: Ask) {
        let (latch, verifier, now) = (self.u2f.clone(), self.verifier.clone(), self.now);
        std::thread::spawn(move || {
            static NEVER: AtomicBool = AtomicBool::new(false);
            let k = || {};
            let v = verifier.verify(&ask, &Txn { cancelled: &NEVER, keepalive: &k });
            latch.answered(key, v == Verdict::Approved, now());
        });
    }

    fn u2f_gate(&self, key: [u8; 32], ask: Ask) -> bool {
        match self.u2f.check(key, (self.now)()) {
            Gate::Go => true,
            Gate::Wait => false,
            Gate::Start => {
                self.u2f_start(key, ask);
                false
            }
        }
    }

    fn u2f_register(&mut self, data: &[u8]) -> Vec<u8> {
        if data.len() != 64 {
            return sw(Vec::new(), SW_WRONG_LENGTH);
        }
        let (chal, app) = (&data[..32], &data[32..]);
        if !self.u2f_gate(gate_key(REGISTER, app, &[]), Ask { purpose: Purpose::Create, rp: "a site's two-step sign-in (U2F)".into(), user: String::new() }) {
            return sw(Vec::new(), SW_CONDITIONS);
        }
        let made = (|| -> std::io::Result<Vec<u8>> {
            let mut secret = keys::generate(ES256)?;
            let kh = crate::store::random::<32>()?;
            let point = keys::es256_point(&secret)?;
            let cred = Cred {
                id: b64::encode(&kh),
                rp_hash: b64::hex(app),
                alg: ES256,
                created: (self.now)(),
                u2f: true,
                ..Default::default()
            };
            self.store.lock().unwrap().add(cred, &secret)?;
            secret.iter_mut().for_each(|b| *b = 0);
            // (a key of its own signs this one registration, so none can be linked to another)
            let att = keys::generate(ES256)?;
            let cert = keys::self_signed_cert(&att, "Tessera Passkeys")?;
            let signed = [&[0u8][..], app, chal, &kh[..], &point[..]].concat();
            let sig = keys::sign(ES256, &att, &signed)?;
            let mut r = vec![0x05];
            r.extend_from_slice(&point);
            r.push(kh.len() as u8);
            r.extend_from_slice(&kh);
            r.extend_from_slice(&cert);
            r.extend_from_slice(&sig);
            Ok(r)
        })();
        match made {
            Ok(r) => sw(r, SW_OK),
            Err(e) => {
                eprintln!("tessera-passkeys: U2F registration failed: {e}");
                sw(Vec::new(), SW_WRONG_DATA)
            }
        }
    }

    fn u2f_authenticate(&mut self, p1: u8, data: &[u8]) -> Vec<u8> {
        if data.len() < 65 || data.len() != 65 + data[64] as usize {
            return sw(Vec::new(), SW_WRONG_LENGTH);
        }
        let (chal, app, kh) = (&data[..32], &data[32..64], &data[65..]);
        let cred = { self.store.lock().unwrap().allowed(&b64::hex(app), &[kh.to_vec()]).into_iter().next() };
        let Some(cred) = cred else { return sw(Vec::new(), SW_WRONG_DATA) };
        if cred.alg != ES256 {
            return sw(Vec::new(), SW_WRONG_DATA);
        }
        let present = match p1 {
            CHECK_ONLY => return sw(Vec::new(), SW_CONDITIONS),
            DONT_ENFORCE => false,
            ENFORCE => {
                let rp = if cred.rp_id.is_empty() { "a site's two-step sign-in (U2F)".to_string() } else { cred.rp_id.clone() };
                if !self.u2f_gate(gate_key(AUTHENTICATE, app, kh), Ask { purpose: Purpose::SignIn, rp, user: String::new() }) {
                    return sw(Vec::new(), SW_CONDITIONS);
                }
                true
            }
            _ => return sw(Vec::new(), SW_WRONG_DATA),
        };
        let signed = (|| -> std::io::Result<Vec<u8>> {
            let (counter, mut secret) = {
                let mut s = self.store.lock().unwrap();
                let secret = s.secret(&cred.id)?;
                (s.bump(&cred.id, (self.now)())?, secret)
            };
            let flag = if present { 0x01u8 } else { 0x00 };
            let msg = [app, &[flag][..], &counter.to_be_bytes()[..], chal].concat();
            let sig = keys::sign(ES256, &secret, &msg);
            secret.iter_mut().for_each(|b| *b = 0);
            let mut r = vec![flag];
            r.extend_from_slice(&counter.to_be_bytes());
            r.extend_from_slice(&sig?);
            Ok(r)
        })();
        match signed {
            Ok(r) => sw(r, SW_OK),
            Err(e) => {
                eprintln!("tessera-passkeys: U2F sign-in failed: {e}");
                sw(Vec::new(), SW_WRONG_DATA)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctap2::tests::{authenticator, Fake};

    fn apdu(ins: u8, p1: u8, data: &[u8]) -> Vec<u8> {
        let mut a = vec![0, ins, p1, 0, 0];
        a.extend_from_slice(&(data.len() as u16).to_be_bytes());
        a.extend_from_slice(data);
        a.extend_from_slice(&[0, 0]);
        a
    }

    fn status(r: &[u8]) -> u16 {
        u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]])
    }

    /// Ask until the background prompt has answered (as a browser polls)
    fn poll(a: &mut Authenticator, req: &[u8]) -> Vec<u8> {
        for _ in 0..200 {
            let r = a.handle_u2f(req);
            if status(&r) != SW_CONDITIONS {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("never answered");
    }

    #[test]
    fn apdus_parse_short_and_extended() {
        assert_eq!(parse_apdu(&[0, 3, 0, 0]), Ok((3, 0, 0, &[][..])));
        assert_eq!(parse_apdu(&[0, 2, 7, 0, 2, 9, 9]), Ok((2, 7, 0, &[9u8, 9][..])));
        assert_eq!(parse_apdu(&[0, 2, 7, 0, 2, 9, 9, 0]), Ok((2, 7, 0, &[9u8, 9][..])));
        assert_eq!(parse_apdu(&[0, 1, 0, 0, 0, 0, 2, 5, 6, 0, 0]), Ok((1, 0, 0, &[5u8, 6][..])));
        assert_eq!(parse_apdu(&[0, 1, 0, 0, 0, 0, 9, 5]), Err(SW_WRONG_LENGTH));
        assert_eq!(parse_apdu(&[0x80, 1, 0, 0]), Err(SW_CLA));
        assert_eq!(parse_apdu(&[0, 1]), Err(SW_WRONG_LENGTH));
    }

    #[test]
    fn the_latch_holds_an_answer_for_its_ask() {
        let l = Latch::default();
        let (a, b) = ([1u8; 32], [2u8; 32]);
        assert_eq!(l.check(a, 0), Gate::Start);
        assert_eq!(l.check(a, 1), Gate::Wait);
        assert_eq!(l.check(b, 1), Gate::Wait);
        l.answered(a, true, 2);
        assert_eq!(l.check(b, 3), Gate::Start); // (another ask: asked anew)
        l.answered(a, true, 3); // (no longer the one being asked: ignored)
        l.answered(b, false, 4);
        assert_eq!(l.check(b, 5), Gate::Wait);
        assert_eq!(l.check(b, 5 + REFUSED_FOR + 1), Gate::Start);
        l.answered(b, true, 40);
        assert_eq!(l.check(b, 40 + APPROVED_FOR + 1), Gate::Start); // (too old)
        l.answered(b, true, 60);
        assert_eq!(l.check(b, 61), Gate::Go);
        assert_eq!(l.check(b, 61), Gate::Start); // (used up)
    }

    #[test]
    fn register_then_sign_in() {
        use p256::ecdsa::signature::Verifier as _;
        let fake = Fake::new(Verdict::Approved);
        let mut a = authenticator("u2f", fake.clone());
        assert_eq!(a.handle_u2f(&apdu(VERSION, 0, &[])), [b"U2F_V2".as_slice(), &[0x90, 0]].concat());
        let (chal, app) = ([3u8; 32], keys::sha256(b"https://old.example"));
        let req = apdu(REGISTER, 0, &[chal.as_slice(), &app[..]].concat());
        // the first ask only starts the prompt
        assert_eq!(a.handle_u2f(&req), [0x69, 0x85]);
        let r = poll(&mut a, &req);
        assert_eq!(status(&r), SW_OK);
        assert_eq!(r[0], 0x05);
        let point = r[1..66].to_vec();
        let kh = r[67..67 + r[66] as usize].to_vec();
        assert_eq!(kh.len(), 32);
        assert_eq!(fake.asked.lock().unwrap()[0].purpose, Purpose::Create);
        assert!(a.store.lock().unwrap().creds()[0].u2f);

        let auth = |p1: u8, kh: &[u8]| apdu(AUTHENTICATE, p1, &[&chal[..], &app[..], &[kh.len() as u8][..], kh].concat());
        assert_eq!(a.handle_u2f(&auth(CHECK_ONLY, &kh)), [0x69, 0x85]);
        assert_eq!(a.handle_u2f(&auth(CHECK_ONLY, &[9u8; 32])), [0x6a, 0x80]);
        let r = poll(&mut a, &auth(ENFORCE, &kh));
        assert_eq!(status(&r), SW_OK);
        assert_eq!(r[0], 0x01);
        assert_eq!(u32::from_be_bytes(r[1..5].try_into().unwrap()), 1);
        let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).unwrap();
        let sig = p256::ecdsa::Signature::from_der(&r[5..r.len() - 2]).unwrap();
        assert!(vk.verify(&[&app[..], &[0x01u8][..], &1u32.to_be_bytes()[..], &chal[..]].concat(), &sig).is_ok());

        // refused in the prompt: the browser is told to wait, and isn't asked again at once
        *fake.answer.lock().unwrap() = Verdict::Denied;
        let n = fake.asked.lock().unwrap().len();
        assert_eq!(a.handle_u2f(&auth(ENFORCE, &kh)), [0x69, 0x85]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(a.handle_u2f(&auth(ENFORCE, &kh)), [0x69, 0x85]);
        assert_eq!(fake.asked.lock().unwrap().len(), n + 1);
    }
}
