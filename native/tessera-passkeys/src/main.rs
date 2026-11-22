//! tessera-passkeys: Tessera's passkeys, as a virtual FIDO2 security key.
//!
//! Browsers (Chrome, Firefox, Zen, Electron apps) and libfido2 (ssh-keygen
//! -t ed25519-sk, pam_u2f) all speak to USB security keys over hidraw; Linux
//! has no platform authenticator for them. This daemon makes one through
//! /dev/uhid (uhid.rs) that speaks CTAPHID (ctaphid.rs), CTAP2 (ctap2.rs)
//! and U2F (u2f.rs), keeps each credential's key sealed by the laptop's TPM
//! (store.rs, tpm.rs), and answers only once you've approved in Tessera's
//! prompt with your fingerprint or password (prompt.rs, pam.rs: the shell's
//! surfaces/polkit/src/passkeys.rs shows it). A user service
//! (tessera-passkeys.service), so it outlives a shell restart. See
//! docs/PASSKEYS.md.
//!
//!   tessera-passkeys                 run (the service)
//!   tessera-passkeys --status        the running daemon's STATUS (JSON), else {"running":false}
//!   tessera-passkeys --list          its saved credentials (JSON)
//!   tessera-passkeys --version
//!   tessera-passkeys --pam-helper SERVICE CONFDIR   (its own check; pam.rs)

mod b64;
mod cbor;
mod ctap2;
mod ctaphid;
mod keys;
mod pam;
mod prompt;
mod store;
mod tpm;
mod u2f;
mod uhid;

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") => println!("{}", env!("CARGO_PKG_VERSION")),
        Some("--pam-helper") if args.len() == 4 => std::process::exit(pam::helper_main(&args[2], &args[3])),
        Some("--status") => println!("{}", ask("STATUS").unwrap_or_else(|| r#"{"running":false}"#.to_string())),
        Some("--list") => println!("{}", ask("LIST").unwrap_or_else(|| "[]".to_string())),
        None => std::process::exit(run()),
        Some(_) => {
            eprintln!("usage: tessera-passkeys [--status | --list | --version]");
            std::process::exit(2);
        }
    }
}

/// One command to the running daemon: its one-line answer
fn ask(cmd: &str) -> Option<String> {
    let s = std::os::unix::net::UnixStream::connect(prompt::socket_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    (&s).write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).ok()?;
    let l = line.trim();
    (!l.is_empty()).then(|| l.to_string())
}

fn now_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// The transaction being worked on: its channel and its cancel flag
type Busy = Arc<Mutex<Option<(u32, Arc<AtomicBool>)>>>;

fn send(dev: &uhid::Device, packets: Vec<[u8; ctaphid::PACKET]>) {
    for p in packets {
        if let Err(e) = dev.send(&p) {
            eprintln!("tessera-passkeys: couldn't write to the key: {e}");
            return;
        }
    }
}

fn new_cid() -> u32 {
    loop {
        let c = u32::from_be_bytes(store::random::<4>().unwrap_or([1, 2, 3, 4]));
        if c != 0 && c != ctaphid::BROADCAST {
            return c;
        }
    }
}

fn run() -> i32 {
    let runtime = prompt::socket_path().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir);
    let pam_dir = runtime.join("passkeys-pam");
    if let Err(e) = pam::write_service(&pam_dir) {
        eprintln!("tessera-passkeys: couldn't write its PAM file in {}: {e}", pam_dir.display());
        return 1;
    }
    let sealer = tpm::Tpm::new();
    let tpm_ok = crate::store::Sealer::available(&sealer);
    let store = match store::Store::open(store::default_dir(), Box::new(sealer)) {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            eprintln!("tessera-passkeys: can't open the saved passkeys: {e}");
            return 1;
        }
    };
    let health = Arc::new(prompt::Health { device: AtomicBool::new(false), tpm_error: Mutex::new(String::new()) });
    let helper = vec!["/proc/self/exe".to_string(), "--pam-helper".into(), pam::SERVICE.into(), pam_dir.to_string_lossy().into_owned()];
    let prompts = Arc::new(prompt::Prompts::new(helper));
    let listener = match prompt::listen(&prompt::socket_path()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tessera-passkeys: {e}");
            return 1;
        }
    };
    {
        let (p, s, h) = (prompts.clone(), store.clone(), health.clone());
        std::thread::spawn(move || prompt::serve(listener, p, s, h));
    }
    // Without the TPM: no key at all (nothing could be kept safely), but
    // the socket answers STATUS, so Settings can say why
    if let Err(why) = tpm_ok {
        eprintln!("tessera-passkeys: no key: {why}");
        *health.tpm_error.lock().unwrap() = why;
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    let auth = Arc::new(Mutex::new(ctap2::Authenticator::new(store, prompts.clone())));
    loop {
        match uhid::Device::create() {
            Ok(dev) => {
                eprintln!("tessera-passkeys: the key is ready ({:04x}:{:04x})", uhid::VENDOR, uhid::PRODUCT);
                health.device.store(true, Ordering::SeqCst);
                let e = hid_loop(Arc::new(dev), auth.clone());
                health.device.store(false, Ordering::SeqCst);
                eprintln!("tessera-passkeys: the key stopped ({e}); making it again");
            }
            Err(e) => eprintln!("tessera-passkeys: can't make the key through {} ({e}); trying again in 10 s", uhid::Device::PATH),
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

/// Reads the host's packets until the device fails
fn hid_loop(dev: Arc<uhid::Device>, auth: Arc<Mutex<ctap2::Authenticator>>) -> std::io::Error {
    let start = Instant::now();
    let mut asm = ctaphid::Assembler::default();
    let busy: Busy = Arc::new(Mutex::new(None));
    loop {
        let ev = match dev.read() {
            Ok(ev) => ev,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return e,
        };
        let pkt = match ev {
            uhid::Event::Output(p) => p,
            // (the browser let the device go mid-request: that request is over)
            uhid::Event::Close => {
                if let Some((_, c)) = busy.lock().unwrap().as_ref() {
                    c.store(true, Ordering::SeqCst);
                }
                continue;
            }
            _ => continue,
        };
        match asm.push(&pkt, now_ms(start)) {
            ctaphid::Event::Nothing => {}
            ctaphid::Event::Error { cid, code } => send(&dev, vec![ctaphid::error(cid, code)]),
            ctaphid::Event::Message { cid, cmd, data } => message(&dev, &auth, &busy, cid, cmd, data),
        }
    }
}

fn message(dev: &Arc<uhid::Device>, auth: &Arc<Mutex<ctap2::Authenticator>>, busy: &Busy, cid: u32, cmd: u8, data: Vec<u8>) {
    match cmd {
        ctaphid::INIT => {
            if data.len() != 8 {
                return send(dev, vec![ctaphid::error(cid, ctaphid::ERR_INVALID_LEN)]);
            }
            // (INIT on a channel mid-request: that request is abandoned)
            if let Some((c, flag)) = busy.lock().unwrap().as_ref() {
                if *c == cid {
                    flag.store(true, Ordering::SeqCst);
                }
            }
            let new = if cid == ctaphid::BROADCAST { new_cid() } else { cid };
            send(dev, ctaphid::frames(cid, ctaphid::INIT, &ctaphid::init_reply(&data, new)));
        }
        ctaphid::PING => send(dev, ctaphid::frames(cid, ctaphid::PING, &data)),
        ctaphid::WINK | ctaphid::LOCK => send(dev, ctaphid::frames(cid, cmd & 0x7f, &[])),
        ctaphid::CANCEL => {
            if let Some((c, flag)) = busy.lock().unwrap().as_ref() {
                if *c == cid {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        }
        ctaphid::CBOR | ctaphid::MSG => {
            let flag = Arc::new(AtomicBool::new(false));
            {
                let mut b = busy.lock().unwrap();
                if b.is_some() {
                    return send(dev, vec![ctaphid::error(cid, ctaphid::ERR_CHANNEL_BUSY)]);
                }
                *b = Some((cid, flag.clone()));
            }
            let (dev, auth, busy) = (dev.clone(), auth.clone(), busy.clone());
            std::thread::spawn(move || {
                let d2 = dev.clone();
                let keepalive = move || {
                    let _ = d2.send(&ctaphid::keepalive(cid, ctaphid::STATUS_UPNEEDED));
                };
                let txn = ctap2::Txn { cancelled: &flag, keepalive: &keepalive };
                // (a panic answers this request with an error; the next one goes on)
                let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut a = auth.lock().unwrap_or_else(|e| e.into_inner());
                    if cmd == ctaphid::CBOR { a.handle(&data, &txn) } else { a.handle_u2f(&data) }
                }))
                .unwrap_or_else(|_| if cmd == ctaphid::CBOR { vec![ctap2::ERR_OTHER] } else { vec![0x6f, 0x00] });
                *busy.lock().unwrap() = None;
                send(&dev, ctaphid::frames(cid, cmd & 0x7f, &reply));
            });
        }
        _ => send(dev, vec![ctaphid::error(cid, ctaphid::ERR_INVALID_CMD)]),
    }
}
