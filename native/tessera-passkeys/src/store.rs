//! Saved credentials, in $XDG_DATA_HOME/tessera/passkeys (0700):
//!   wrap.sealed     a random 32-byte wrapping key, sealed by the TPM (tpm.rs)
//!   creds.json      the metadata: site, user, dates, sign counter (no secrets)
//!   keys/<id>.key   each private key, AES-256-GCM under the wrapping key
//!                   (a version byte, a 12-byte nonce, the ciphertext; the
//!                   credential id is the associated data, so files can't
//!                   be swapped between credentials)
//! The wrapping key is unsealed once and kept in memory while the daemon
//! runs. A copied folder is useless elsewhere: only this TPM unseals it.

use crate::b64;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const ES256: i64 = -7;
pub const EDDSA: i64 = -8;
const KEY_VERSION: u8 = 1;
const AAD_PREFIX: &[u8] = b"tessera-passkeys v1 ";

/// What seals the wrapping key: the TPM, or a test's fake
pub trait Sealer: Send {
    /// can it be used here (why not, said plainly)
    fn available(&self) -> Result<(), String>;
    fn seal(&self, secret: &[u8]) -> io::Result<Vec<u8>>;
    fn unseal(&self, blob: &[u8]) -> io::Result<Vec<u8>>;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cred {
    /// the credential id (base64url): 32 random bytes
    pub id: String,
    /// the site (rpId), or a U2F site's app id when it's known
    pub rp_id: String,
    /// SHA-256 of the rpId (hex): what requests are matched on
    pub rp_hash: String,
    pub rp_name: String,
    /// the site's id for the account (base64url)
    pub user_id: String,
    pub user_name: String,
    pub display_name: String,
    /// COSE: -7 ES256, -8 EdDSA
    pub alg: i64,
    /// Unix seconds
    pub created: i64,
    pub last_used: i64,
    pub counter: u32,
    /// discoverable (a passkey); false: a site's second factor or an SSH key
    pub resident: bool,
    /// made through U2F (CTAP1)
    pub u2f: bool,
}

impl Cred {
    pub fn id_bytes(&self) -> Vec<u8> {
        b64::decode(&self.id).unwrap_or_default()
    }
    pub fn user_id_bytes(&self) -> Vec<u8> {
        b64::decode(&self.user_id).unwrap_or_default()
    }
}

pub struct Store {
    dir: PathBuf,
    sealer: Box<dyn Sealer>,
    wrap: Option<[u8; 32]>,
    creds: Vec<Cred>,
}

pub fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(b)
}

/// Replace a file whole (a temporary file beside it, synced, renamed), 0600
pub fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(d) = path.parent() {
        if let Ok(f) = std::fs::File::open(d) {
            let _ = f.sync_all();
        }
    }
    Ok(())
}

fn private_dir(d: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d)?;
    std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))
}

pub fn default_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
    });
    data.join("tessera/passkeys")
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Store {
    pub fn open(dir: PathBuf, sealer: Box<dyn Sealer>) -> io::Result<Store> {
        private_dir(&dir)?;
        private_dir(&dir.join("keys"))?;
        let creds = match std::fs::read(dir.join("creds.json")) {
            Ok(b) => serde_json::from_slice::<Vec<Cred>>(&b).map_err(|e| io::Error::other(format!("creds.json: {e}")))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Store { dir, sealer, wrap: None, creds: creds.into_iter().filter(|c| is_id(&c.id)).collect() })
    }

    pub fn sealer_available(&self) -> Result<(), String> {
        self.sealer.available()
    }

    pub fn creds(&self) -> &[Cred] {
        &self.creds
    }

    /// The wrapping key: unsealed (once), or made and sealed the first time
    fn wrapping_key(&mut self) -> io::Result<[u8; 32]> {
        if let Some(k) = self.wrap {
            return Ok(k);
        }
        let path = self.dir.join("wrap.sealed");
        let key = match std::fs::read(&path) {
            Ok(blob) => {
                let k = self.sealer.unseal(&blob)?;
                <[u8; 32]>::try_from(k.as_slice()).map_err(|_| io::Error::other("the sealed key has the wrong length"))?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // (keys sealed under a lost wrapping key could never open: refuse rather than orphan them)
                if self.creds.iter().any(|c| self.dir.join("keys").join(format!("{}.key", c.id)).exists()) {
                    return Err(io::Error::other("wrap.sealed is missing, so the saved passkeys can't be opened"));
                }
                let k = random::<32>()?;
                let blob = self.sealer.seal(&k)?;
                write_private(&path, &blob)?;
                k
            }
            Err(e) => return Err(e),
        };
        self.wrap = Some(key);
        Ok(key)
    }

    fn save(&self) -> io::Result<()> {
        let j = serde_json::to_vec_pretty(&self.creds).map_err(io::Error::other)?;
        write_private(&self.dir.join("creds.json"), &j)
    }

    fn key_path(&self, id: &str) -> PathBuf {
        self.dir.join("keys").join(format!("{id}.key"))
    }

    fn aad(id: &str) -> Vec<u8> {
        [AAD_PREFIX, id.as_bytes()].concat()
    }

    /// A new credential and its private key
    pub fn add(&mut self, cred: Cred, secret: &[u8]) -> io::Result<()> {
        if !is_id(&cred.id) {
            return Err(io::Error::other("bad credential id"));
        }
        let wrap = self.wrapping_key()?;
        let nonce = random::<12>()?;
        let c = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap));
        let aad = Self::aad(&cred.id);
        let ct = c.encrypt(Nonce::from_slice(&nonce), Payload { msg: secret, aad: &aad }).map_err(|_| io::Error::other("encryption failed"))?;
        let mut blob = vec![KEY_VERSION];
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        write_private(&self.key_path(&cred.id), &blob)?;
        // (a resident key replaces the one for the same site and account)
        if cred.resident {
            let old: Vec<String> = self.creds.iter().filter(|o| o.resident && o.rp_hash == cred.rp_hash && o.user_id == cred.user_id && !o.user_id.is_empty()).map(|o| o.id.clone()).collect();
            for id in old {
                self.creds.retain(|o| o.id != id);
                let _ = std::fs::remove_file(self.key_path(&id));
            }
        }
        self.creds.insert(0, cred);
        self.save()
    }

    /// A credential's private key (the caller wipes it after use)
    pub fn secret(&mut self, id: &str) -> io::Result<Vec<u8>> {
        if !self.creds.iter().any(|c| c.id == id) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no such credential"));
        }
        let wrap = self.wrapping_key()?;
        let blob = std::fs::read(self.key_path(id))?;
        if blob.len() < 1 + 12 + 16 || blob[0] != KEY_VERSION {
            return Err(io::Error::other("the key file is damaged"));
        }
        let c = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap));
        let aad = Self::aad(id);
        c.decrypt(Nonce::from_slice(&blob[1..13]), Payload { msg: &blob[13..], aad: &aad }).map_err(|_| io::Error::other("the key file doesn't open with this computer's key"))
    }

    /// One more use: the counter goes up (saved before the signature leaves), and when
    pub fn bump(&mut self, id: &str, now: i64) -> io::Result<u32> {
        let c = self.creds.iter_mut().find(|c| c.id == id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such credential"))?;
        c.counter = c.counter.checked_add(1).ok_or_else(|| io::Error::other("the counter is spent"))?;
        c.last_used = now;
        let n = c.counter;
        self.save()?;
        Ok(n)
    }

    pub fn delete(&mut self, id: &str) -> io::Result<bool> {
        let before = self.creds.len();
        self.creds.retain(|c| c.id != id);
        if self.creds.len() == before {
            return Ok(false);
        }
        self.save()?;
        if is_id(id) {
            let p = self.key_path(id);
            // (overwritten first: the ciphertext is useless without the TPM, but still)
            if let Ok(m) = std::fs::metadata(&p) {
                let _ = write_private(&p, &vec![0u8; m.len() as usize]);
            }
            let _ = std::fs::remove_file(p);
        }
        Ok(true)
    }

    pub fn find(&self, id: &str) -> Option<&Cred> {
        self.creds.iter().find(|c| c.id == id)
    }

    /// The credentials for a site among these ids (an allow list)
    pub fn allowed(&self, rp_hash: &str, ids: &[Vec<u8>]) -> Vec<Cred> {
        let ids: Vec<String> = ids.iter().map(|i| b64::encode(i)).collect();
        self.creds.iter().filter(|c| c.rp_hash == rp_hash && ids.contains(&c.id)).cloned().collect()
    }

    /// A site's passkeys, newest first
    pub fn resident(&self, rp_hash: &str) -> Vec<Cred> {
        let mut v: Vec<Cred> = self.creds.iter().filter(|c| c.resident && c.rp_hash == rp_hash).cloned().collect();
        v.sort_by(|a, b| b.created.cmp(&a.created));
        v
    }
}

/// A sealer for tests: no TPM, a fixed transform (never used outside tests)
#[cfg(test)]
pub struct FakeSealer;

#[cfg(test)]
impl Sealer for FakeSealer {
    fn available(&self) -> Result<(), String> {
        Ok(())
    }
    fn seal(&self, s: &[u8]) -> io::Result<Vec<u8>> {
        Ok([b"FAKE".as_slice(), &s.iter().map(|b| b ^ 0x5a).collect::<Vec<u8>>()].concat())
    }
    fn unseal(&self, b: &[u8]) -> io::Result<Vec<u8>> {
        b.strip_prefix(b"FAKE").map(|s| s.iter().map(|b| b ^ 0x5a).collect()).ok_or_else(|| io::Error::other("not sealed here"))
    }
}

#[cfg(test)]
pub fn test_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tessera-passkeys-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(id: &str, rp: &str, user: &str, resident: bool, created: i64) -> Cred {
        Cred { id: id.into(), rp_id: rp.into(), rp_hash: format!("h-{rp}"), user_id: user.into(), user_name: user.into(), alg: ES256, created, resident, ..Default::default() }
    }

    #[test]
    fn keys_are_sealed_and_open_again() {
        let d = test_dir("store");
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        s.add(cred("AAAA", "example.com", "dXNlcg", true, 1), b"secret key bytes").unwrap();
        // on disk: no plain key, a sealed wrapping key, private modes
        let blob = std::fs::read(d.join("keys/AAAA.key")).unwrap();
        assert!(!blob.windows(6).any(|w| w == b"secret"));
        assert!(std::fs::read(d.join("wrap.sealed")).unwrap().starts_with(b"FAKE"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(d.join("keys/AAAA.key")).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        // another run reads it back
        drop(s);
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        assert_eq!(s.secret("AAAA").unwrap(), b"secret key bytes");
        assert_eq!(s.creds()[0].rp_id, "example.com");
        // a key file moved to another credential doesn't open
        s.add(cred("BBBB", "example.org", "", false, 2), b"other").unwrap();
        std::fs::copy(d.join("keys/AAAA.key"), d.join("keys/BBBB.key")).unwrap();
        assert!(s.secret("BBBB").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_sealed_key_from_elsewhere_doesnt_open() {
        struct Other;
        impl Sealer for Other {
            fn available(&self) -> Result<(), String> {
                Ok(())
            }
            fn seal(&self, _: &[u8]) -> io::Result<Vec<u8>> {
                Ok(b"OTHER".to_vec())
            }
            fn unseal(&self, _: &[u8]) -> io::Result<Vec<u8>> {
                Err(io::Error::other("not this TPM"))
            }
        }
        let d = test_dir("elsewhere");
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        s.add(cred("AAAA", "a", "", false, 1), b"k").unwrap();
        drop(s);
        let mut s = Store::open(d.clone(), Box::new(Other)).unwrap();
        assert!(s.secret("AAAA").is_err());
        // and a lost wrapping key never makes a new one under old keys
        std::fs::remove_file(d.join("wrap.sealed")).unwrap();
        let mut s2 = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        assert!(s2.secret("AAAA").is_err());
        assert!(!d.join("wrap.sealed").exists());
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn counters_only_go_up_and_are_saved() {
        let d = test_dir("counter");
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        s.add(cred("AAAA", "a", "", false, 1), b"k").unwrap();
        assert_eq!(s.bump("AAAA", 100).unwrap(), 1);
        assert_eq!(s.bump("AAAA", 200).unwrap(), 2);
        drop(s);
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        assert_eq!(s.find("AAAA").unwrap().counter, 2);
        assert_eq!(s.find("AAAA").unwrap().last_used, 200);
        assert_eq!(s.bump("AAAA", 300).unwrap(), 3);
        assert!(s.bump("nope", 1).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lookups_and_deletes() {
        let d = test_dir("lookup");
        let mut s = Store::open(d.clone(), Box::new(FakeSealer)).unwrap();
        s.add(cred("AAAA", "a.com", "dTE", true, 1), b"1").unwrap();
        s.add(cred("BBBB", "a.com", "dTI", true, 2), b"2").unwrap();
        s.add(cred("CCCC", "b.com", "", false, 3), b"3").unwrap();
        let r = s.resident("h-a.com");
        assert_eq!(r.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["BBBB", "AAAA"]);
        assert!(s.resident("h-b.com").is_empty());
        let ids = vec![b64::decode("CCCC").unwrap(), b64::decode("AAAA").unwrap()];
        assert_eq!(s.allowed("h-b.com", &ids).len(), 1);
        assert_eq!(s.allowed("h-a.com", &ids)[0].id, "AAAA");
        // the same site and account again: the new one replaces the old
        s.add(cred("DDDD", "a.com", "dTE", true, 4), b"4").unwrap();
        assert!(s.find("AAAA").is_none());
        assert!(!d.join("keys/AAAA.key").exists());
        assert!(s.delete("BBBB").unwrap());
        assert!(!s.delete("BBBB").unwrap());
        assert!(!d.join("keys/BBBB.key").exists());
        assert_eq!(s.creds().len(), 2);
        // a bad id never names a path
        assert!(s.add(cred("../x", "a", "", false, 5), b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
