//! Sealing the wrapping key with the laptop's TPM 2.0, through tpm2-tools
//! (tpm2_createprimary, tpm2_create, tpm2_load, tpm2_unseal) on
//! /dev/tpmrm0, the kernel's resource manager.
//!
//! Why the tools and not tss-esapi: tss-esapi links the TSS C libraries at
//! build time, so every machine that builds Tessera (the release builder,
//! an install that builds here) would need tpm2-tss's headers, and the
//! daemon would fail to start without them. The tools are one package, used
//! only when the TPM is, and their command lines are stable since 5.0.
//!
//! The primary key: the owner hierarchy's ECC P-256 storage key, made the
//! same from the TPM's seed each time (never stored). The wrapping key is a
//! sealed data object under it, with no PCR policy: bound to this TPM (the
//! sealed blob opens nowhere else), but not to the firmware and boot chain,
//! so a BIOS, firmware or bootloader update, or Secure Boot changing, never
//! locks the passkeys away for good (there is no backup to come back from).
//! What that gives up: another system booted on this same laptop, with your
//! home folder readable, could unseal it; encrypt the disk against that.

use crate::store::Sealer;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const DEVICE: &str = "/dev/tpmrm0";
const MAGIC: &[u8; 4] = b"TPS1";

pub struct Tpm {
    pub work: PathBuf,
}

/// The blob saved: magic, then the sealed object's public and private parts, each length-prefixed
pub fn pack(public: &[u8], private: &[u8]) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend_from_slice(&(public.len() as u32).to_be_bytes());
    b.extend_from_slice(public);
    b.extend_from_slice(&(private.len() as u32).to_be_bytes());
    b.extend_from_slice(private);
    b
}

pub fn unpack(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = b.strip_prefix(MAGIC.as_slice())?;
    let part = |r: &[u8]| -> Option<(usize, usize)> {
        let n = u32::from_be_bytes(r.get(..4)?.try_into().ok()?) as usize;
        (r.len() >= 4 + n && n <= 64 * 1024).then_some((4, 4 + n))
    };
    let (a, z) = part(rest)?;
    let public = &rest[a..z];
    let rest = &rest[z..];
    let (a, z2) = part(rest)?;
    (z2 == rest.len()).then_some((public, &rest[a..z2]))
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

impl Tpm {
    pub fn new() -> Tpm {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Tpm { work: base.join(format!("tessera-passkeys-tpm-{}", std::process::id())) }
    }

    fn tool(&self, args: &[&str], stdin: Option<&[u8]>) -> io::Result<Vec<u8>> {
        let mut c = Command::new(args[0]);
        c.args(&args[1..])
            .env("TPM2TOOLS_TCTI", format!("device:{DEVICE}"))
            .current_dir(&self.work)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| io::Error::other(format!("{}: {e}", args[0])))?;
        if let (Some(data), Some(mut si)) = (stdin, child.stdin.take()) {
            si.write_all(data)?;
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!("{} failed: {}", args[0], err.lines().last().unwrap_or("").trim())));
        }
        Ok(out.stdout)
    }

    /// A private folder for the context files, gone afterwards
    fn with_work<T>(&self, f: impl FnOnce(&Path) -> io::Result<T>) -> io::Result<T> {
        use std::os::unix::fs::DirBuilderExt;
        let _ = std::fs::remove_dir_all(&self.work);
        std::fs::DirBuilder::new().mode(0o700).create(&self.work)?;
        let r = f(&self.work);
        let _ = std::fs::remove_dir_all(&self.work);
        r
    }

    fn primary(&self) -> io::Result<()> {
        self.tool(&["tpm2_createprimary", "-Q", "-C", "o", "-g", "sha256", "-G", "ecc", "-c", "primary.ctx"], None).map(|_| ())
    }
}

impl Default for Tpm {
    fn default() -> Self {
        Tpm::new()
    }
}

impl Sealer for Tpm {
    fn available(&self) -> Result<(), String> {
        if !Path::new(DEVICE).exists() {
            return Err("This computer has no TPM 2.0 (or it's turned off in the firmware settings), so passkeys can't be kept safely here.".into());
        }
        let c = std::ffi::CString::new(DEVICE).unwrap();
        if unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) } != 0 {
            return Err("The TPM isn't open to you yet. Turning Passkeys on again sets that up (it asks for your password).".into());
        }
        if !on_path("tpm2_createprimary") {
            return Err("tpm2-tools isn't installed (Tessera installs it when you turn Passkeys on).".into());
        }
        Ok(())
    }

    fn seal(&self, secret: &[u8]) -> io::Result<Vec<u8>> {
        self.with_work(|w| {
            self.primary()?;
            self.tool(&["tpm2_create", "-Q", "-C", "primary.ctx", "-g", "sha256", "-i", "-", "-u", "seal.pub", "-r", "seal.priv"], Some(secret))?;
            Ok(pack(&std::fs::read(w.join("seal.pub"))?, &std::fs::read(w.join("seal.priv"))?))
        })
    }

    fn unseal(&self, blob: &[u8]) -> io::Result<Vec<u8>> {
        let (public, private) = unpack(blob).ok_or_else(|| io::Error::other("the sealed key is damaged"))?;
        self.with_work(|w| {
            std::fs::write(w.join("seal.pub"), public)?;
            std::fs::write(w.join("seal.priv"), private)?;
            self.primary()?;
            self.tool(&["tpm2_load", "-Q", "-C", "primary.ctx", "-u", "seal.pub", "-r", "seal.priv", "-c", "seal.ctx"], None)?;
            self.tool(&["tpm2_unseal", "-Q", "-c", "seal.ctx"], None)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_pack_and_unpack() {
        let b = pack(b"pub", b"private");
        assert_eq!(unpack(&b), Some((b"pub".as_slice(), b"private".as_slice())));
        assert_eq!(unpack(b"XXXX"), None);
        assert_eq!(unpack(&b[..b.len() - 1]), None);
        let mut longer = b.clone();
        longer.push(0);
        assert_eq!(unpack(&longer), None);
    }
}
