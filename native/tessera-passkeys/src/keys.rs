//! The key pairs: ES256 (P-256 ECDSA with SHA-256, every site) and EdDSA
//! (Ed25519, `ssh-keygen -t ed25519-sk`), their COSE public keys, and the
//! self-signed X.509 certificate a U2F registration must carry (a fresh
//! key each time, so registrations can't be linked across sites).

use crate::cbor::{int_map, Value};
use crate::store::{random, EDDSA, ES256};
use sha2::{Digest, Sha256};
use std::io;

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// A new private key for this algorithm (its bytes, as stored)
pub fn generate(alg: i64) -> io::Result<Vec<u8>> {
    match alg {
        ES256 => loop {
            let b = random::<32>()?;
            // (a scalar of 0 or past the group's order: vanishingly rare; again)
            if p256::ecdsa::SigningKey::from_slice(&b).is_ok() {
                return Ok(b.to_vec());
            }
        },
        EDDSA => Ok(random::<32>()?.to_vec()),
        _ => Err(io::Error::other("unsupported algorithm")),
    }
}

fn es256(secret: &[u8]) -> io::Result<p256::ecdsa::SigningKey> {
    p256::ecdsa::SigningKey::from_slice(secret).map_err(|_| io::Error::other("bad P-256 key"))
}

fn ed25519(secret: &[u8]) -> io::Result<ed25519_dalek::SigningKey> {
    let b: [u8; 32] = secret.try_into().map_err(|_| io::Error::other("bad Ed25519 key"))?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&b))
}

/// The P-256 public point, uncompressed (0x04 | x | y)
pub fn es256_point(secret: &[u8]) -> io::Result<Vec<u8>> {
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    let sk = es256(secret)?;
    Ok(p256::PublicKey::from(sk.verifying_key()).to_encoded_point(false).as_bytes().to_vec())
}

/// The public key as COSE_Key (CTAP's attested credential data)
pub fn cose_public(alg: i64, secret: &[u8]) -> io::Result<Value> {
    match alg {
        ES256 => {
            let p = es256_point(secret)?;
            Ok(int_map(vec![(1, Value::Int(2)), (3, Value::Int(ES256)), (-1, Value::Int(1)), (-2, Value::Bytes(p[1..33].to_vec())), (-3, Value::Bytes(p[33..65].to_vec()))]))
        }
        EDDSA => {
            let pk = ed25519(secret)?.verifying_key().to_bytes();
            Ok(int_map(vec![(1, Value::Int(1)), (3, Value::Int(EDDSA)), (-1, Value::Int(6)), (-2, Value::Bytes(pk.to_vec()))]))
        }
        _ => Err(io::Error::other("unsupported algorithm")),
    }
}

/// A signature over `msg`: ES256 as DER (SHA-256 inside), EdDSA as 64 bytes
pub fn sign(alg: i64, secret: &[u8], msg: &[u8]) -> io::Result<Vec<u8>> {
    match alg {
        ES256 => {
            use p256::ecdsa::signature::Signer;
            let sig: p256::ecdsa::Signature = es256(secret)?.sign(msg);
            Ok(sig.to_der().as_bytes().to_vec())
        }
        EDDSA => {
            use ed25519_dalek::Signer;
            Ok(ed25519(secret)?.sign(msg).to_bytes().to_vec())
        }
        _ => Err(io::Error::other("unsupported algorithm")),
    }
}

// ── DER, for the U2F certificate ───────────────────────────────────────────

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 {
        out.push(n as u8);
    } else if n <= 0xff {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    out.extend_from_slice(content);
    out
}

fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

const ECDSA_SHA256: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const EC_PUBLIC_KEY: &[u8] = &[0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const PRIME256V1: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const COMMON_NAME: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];

fn name(cn: &str) -> Vec<u8> {
    seq(&[tlv(0x31, &seq(&[COMMON_NAME.to_vec(), tlv(0x0c, cn.as_bytes())]))])
}

/// A self-signed X.509 v1 certificate for this P-256 key
pub fn self_signed_cert(secret: &[u8], cn: &str) -> io::Result<Vec<u8>> {
    let point = es256_point(secret)?;
    let mut bits = vec![0u8];
    bits.extend_from_slice(&point);
    let tbs = seq(&[
        tlv(0x02, &[0x01]),
        seq(&[ECDSA_SHA256.to_vec()]),
        name(cn),
        seq(&[tlv(0x17, b"200101000000Z"), tlv(0x18, b"20991231235959Z")]),
        name(cn),
        seq(&[seq(&[EC_PUBLIC_KEY.to_vec(), PRIME256V1.to_vec()]), tlv(0x03, &bits)]),
    ]);
    let sig = sign(ES256, secret, &tbs)?;
    let mut sig_bits = vec![0u8];
    sig_bits.extend_from_slice(&sig);
    Ok(seq(&[tbs, seq(&[ECDSA_SHA256.to_vec()]), tlv(0x03, &sig_bits)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn es256_signs_and_verifies() {
        use p256::ecdsa::signature::Verifier;
        let k = generate(ES256).unwrap();
        let sig = sign(ES256, &k, b"hello").unwrap();
        let vk = es256(&k).unwrap();
        let vk = vk.verifying_key();
        let s = p256::ecdsa::Signature::from_der(&sig).unwrap();
        assert!(vk.verify(b"hello", &s).is_ok());
        assert!(vk.verify(b"other", &s).is_err());
        let cose = cose_public(ES256, &k).unwrap();
        assert_eq!(cose.get(3), Some(&Value::Int(-7)));
        assert_eq!(cose.get(-2).and_then(Value::as_bytes).map(<[u8]>::len), Some(32));
    }

    #[test]
    fn eddsa_signs_and_verifies() {
        let k = generate(EDDSA).unwrap();
        let sig = sign(EDDSA, &k, b"hello").unwrap();
        assert_eq!(sig.len(), 64);
        let vk = ed25519(&k).unwrap().verifying_key();
        let s = ed25519_dalek::Signature::from_slice(&sig).unwrap();
        assert!(vk.verify_strict(b"hello", &s).is_ok());
        let cose = cose_public(EDDSA, &k).unwrap();
        assert_eq!(cose.get(1), Some(&Value::Int(1)));
        assert_eq!(cose.get(-1), Some(&Value::Int(6)));
        assert!(generate(-257).is_err());
    }

    #[test]
    fn the_certificate_is_der() {
        let k = generate(ES256).unwrap();
        let c = self_signed_cert(&k, "Tessera Passkeys").unwrap();
        assert_eq!(c[0], 0x30);
        // (a long form length: the whole thing is past 255 bytes)
        assert_eq!(c[1], 0x82);
        assert_eq!(((c[2] as usize) << 8 | c[3] as usize) + 4, c.len());
        assert!(c.windows(16).any(|w| w == b"Tessera Passkeys"));
        assert_eq!(tlv(0x04, &[0u8; 300])[..4], [0x04, 0x82, 0x01, 0x2c]);
    }
}
