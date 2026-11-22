//! The password and fingerprint check, as the lock screen's PAM helper
//! (tessera-ui's surfaces/lock/src/auth.rs) does it: a child process,
//! `tessera-passkeys --pam-helper SERVICE CONFDIR`, so PAM runs single-
//! threaded away from the HID loop, and stopping a check is a kill (fprintd
//! lets the reader go when its bus connection closes).
//!
//! The service is Tessera's own, written by the daemon to its runtime
//! folder (`write_service`): a finger first (pam_fprintd, when installed),
//! then the password (pam_unix). Never the system's login or sudo files:
//! those may hold pam_u2f, which would ask this very key (a loop).
//!
//! The helper's lines (stdout), and its answers (stdin, one per prompt):
//!   ECHO_OFF <prompt> | ECHO_ON <prompt>  (reads one line)
//!   INFO <text> | ERROR <text> | RESULT <code> <text>

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{BufRead, Write};
use std::path::Path;

pub const SERVICE: &str = "tessera-passkeys";
pub const FPRINTD: [&str; 2] = ["/usr/lib/security/pam_fprintd.so", "/lib/security/pam_fprintd.so"];

/// The PAM file: a finger (when pam_fprintd is there), else or then the password
pub fn service_text(fprintd: bool) -> String {
    let mut s = String::from(
        "#%PAM-1.0\n# Tessera Passkeys (tessera-passkeys, written each time it starts): a finger,\n# then your password, before a passkey is used. Only this daemon reads it.\n",
    );
    if fprintd {
        s.push_str("auth       [success=done default=ignore]  pam_fprintd.so max-tries=3 timeout=15\n");
    }
    s.push_str("auth       required     pam_unix.so\naccount    required     pam_permit.so\n");
    s
}

/// The folder PAM reads: the service, and "other" refusing anything else
pub fn write_service(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let fp = FPRINTD.iter().any(|p| Path::new(p).exists());
    crate::store::write_private(&dir.join(SERVICE), service_text(fp).as_bytes())?;
    crate::store::write_private(&dir.join("other"), b"#%PAM-1.0\nauth required pam_deny.so\naccount required pam_deny.so\npassword required pam_deny.so\nsession required pam_deny.so\n")?;
    Ok(())
}

/// A helper line, as the polkit agent's protocol says it (the shell's
/// prompt speaks that one: surfaces/polkit/src/agent.rs). RESULT is the
/// caller's to turn into SUCCESS or FAILURE.
pub fn to_agent_line(l: &str) -> Option<String> {
    let (word, rest) = l.split_once(' ').unwrap_or((l, ""));
    Some(match word {
        "ECHO_OFF" => format!("PAM_PROMPT_ECHO_OFF {rest}"),
        "ECHO_ON" => format!("PAM_PROMPT_ECHO_ON {rest}"),
        "INFO" => format!("PAM_TEXT_INFO {rest}"),
        "ERROR" => format!("PAM_ERROR_MSG {rest}"),
        _ => return None,
    })
}

/// "RESULT <code> <text>": did it succeed
pub fn result_ok(l: &str) -> Option<bool> {
    let rest = l.strip_prefix("RESULT ")?;
    let code = rest.split(' ').next()?;
    Some(code == "0")
}

// ── The helper itself ───────────────────────────────────────────────────────

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: ConvFn,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
extern "C" {
    fn pam_start_confdir(service: *const c_char, user: *const c_char, conv: *const PamConv, confdir: *const c_char, pamh: *mut *mut c_void) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut c_void, errnum: c_int) -> *const c_char;
}

fn say(word: &str, text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{word} {}", text.replace('\n', " "));
    let _ = out.flush();
}

/// One answer from stdin (None: the caller went away)
fn answer() -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let n = std::io::stdin().lock().read_until(b'\n', &mut line).ok()?;
    if n == 0 {
        return None;
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    line.truncate(4096);
    Some(line)
}

extern "C" fn conversation(n: c_int, msgs: *mut *const PamMessage, resp: *mut *mut PamResponse, _data: *mut c_void) -> c_int {
    if n <= 0 || msgs.is_null() || resp.is_null() {
        return PAM_CONV_ERR;
    }
    let out = unsafe { libc::calloc(n as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse };
    if out.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..n as usize {
        let m = unsafe { &**msgs.add(i) };
        let text = if m.msg.is_null() { String::new() } else { unsafe { CStr::from_ptr(m.msg) }.to_string_lossy().into_owned() };
        match m.msg_style {
            PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                say(if m.msg_style == PAM_PROMPT_ECHO_OFF { "ECHO_OFF" } else { "ECHO_ON" }, &text);
                let Some(mut a) = answer() else {
                    // (free what was answered so far, then give up)
                    for j in 0..i {
                        unsafe { libc::free((*out.add(j)).resp as *mut c_void) };
                    }
                    unsafe { libc::free(out as *mut c_void) };
                    return PAM_CONV_ERR;
                };
                let c = CString::new(a.clone()).unwrap_or_default();
                a.iter_mut().for_each(|b| *b = 0);
                unsafe { (*out.add(i)).resp = libc::strdup(c.as_ptr()) };
                let mut bytes = c.into_bytes();
                bytes.iter_mut().for_each(|b| *b = 0);
            }
            PAM_ERROR_MSG => say("ERROR", &text),
            PAM_TEXT_INFO => say("INFO", &text),
            _ => {}
        }
    }
    unsafe { *resp = out };
    PAM_SUCCESS
}

pub fn user_name() -> String {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_name.is_null() {
            return std::env::var("USER").unwrap_or_default();
        }
        CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
    }
}

/// `--pam-helper SERVICE CONFDIR`: one check of the current user
pub fn helper_main(service: &str, dir: &str) -> i32 {
    let (Ok(svc), Ok(user), Ok(d)) = (CString::new(service), CString::new(user_name()), CString::new(dir)) else { return 2 };
    let conv = PamConv { conv: conversation, appdata_ptr: std::ptr::null_mut() };
    let mut h: *mut c_void = std::ptr::null_mut();
    let r = unsafe { pam_start_confdir(svc.as_ptr(), user.as_ptr(), &conv, d.as_ptr(), &mut h) };
    if r != PAM_SUCCESS || h.is_null() {
        say("RESULT", &format!("{r} pam_start failed"));
        return 1;
    }
    let mut r = unsafe { pam_authenticate(h, 0) };
    if r == PAM_SUCCESS {
        r = unsafe { pam_acct_mgmt(h, 0) };
    }
    let text = unsafe {
        let p = pam_strerror(h, r);
        if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
    };
    unsafe { pam_end(h, r) };
    say("RESULT", &format!("{r} {text}"));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_asks_for_a_finger_only_when_it_can() {
        let with = service_text(true);
        assert!(with.lines().any(|l| l.starts_with("auth") && l.contains("pam_fprintd.so") && l.contains("success=done")));
        let without = service_text(false);
        assert!(!without.contains("pam_fprintd"));
        for t in [with, without] {
            // (the password always; never pam_u2f, which would ask this key)
            assert!(t.contains("pam_unix.so"));
            assert!(!t.contains("pam_u2f"));
            assert!(!t.contains("pam_permit.so\nauth"));
            assert!(t.lines().filter(|l| l.starts_with("auth")).all(|l| !l.contains("pam_permit")));
        }
    }

    #[test]
    fn helper_lines_become_agent_lines() {
        assert_eq!(to_agent_line("ECHO_OFF Password: ").as_deref(), Some("PAM_PROMPT_ECHO_OFF Password: "));
        assert_eq!(to_agent_line("ECHO_ON Name").as_deref(), Some("PAM_PROMPT_ECHO_ON Name"));
        assert_eq!(to_agent_line("INFO Place your finger").as_deref(), Some("PAM_TEXT_INFO Place your finger"));
        assert_eq!(to_agent_line("ERROR Failed to match").as_deref(), Some("PAM_ERROR_MSG Failed to match"));
        assert_eq!(to_agent_line("RESULT 0 Success"), None);
        assert_eq!(result_ok("RESULT 0 Success"), Some(true));
        assert_eq!(result_ok("RESULT 7 Authentication failure"), Some(false));
        assert_eq!(result_ok("INFO x"), None);
    }
}
