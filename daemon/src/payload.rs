//! Encrypted payload at rest (`.capsule/payloads/{cid}.enc`).
//!
//! Each capsule has its own payload file, encrypted with a key that's
//! derived deterministically from the node's master secret + the
//! capsule_id. That means:
//!
//! * No per-capsule key storage — the daemon can always re-derive from
//!   the unlocked keyring. No extra state to back up.
//! * Leaking one capsule's key doesn't expose any other capsule's payload;
//!   the HKDF info binds each derivation to its capsule_id.
//! * Payload integrity is the AEAD tag — an attacker can't swap payloads
//!   between capsules without breaking authentication.
//!
//! File format:
//!
//! ```text
//! offset  size  field
//! 0       8     magic "CPAYLOAD"
//! 8       1     version (0x01)
//! 9       3     reserved (all zero)
//! 12      12    nonce (random per write)
//! 24      N     ciphertext (plaintext + 16-byte auth tag)
//! ```
//!
//! `payload_cid` is `sha256(ciphertext)` hex — a pure content identifier
//! over the encrypted bytes, so two nodes with the same master secret +
//! same plaintext will NOT match (different nonces → different
//! ciphertext → different CID). That's deliberate: CIDs identify "this
//! exact published version on this exact node," not the underlying data.
//! When we wire a real IPFS/IPLD CIDv1 later, the shape upgrades; the
//! meaning stays the same.

use std::path::{Path, PathBuf};

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

/// Domain separator for payload keys. The capsule_id gets appended at
/// derivation time so two capsules on the same node never share a key.
const DOMAIN_PAYLOAD_V1: &[u8] = b"capsule-node/payload/v1/";

const MAGIC: &[u8; 8] = b"CPAYLOAD";
const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + 1 + 3 + NONCE_LEN;

pub fn payloads_dir(capsule_dir: &Path) -> PathBuf {
    capsule_dir.join("payloads")
}

pub fn payload_path(capsule_dir: &Path, capsule_id: &str) -> PathBuf {
    payloads_dir(capsule_dir).join(format!("{capsule_id}.enc"))
}

#[derive(Debug, Error)]
pub enum PayloadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("HKDF expand failed: {0}")]
    Hkdf(String),
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed: bad key or corrupted payload")]
    Decrypt,
    #[error("payload file is too short (expected at least {expected} bytes, got {got})")]
    TooShort { expected: usize, got: usize },
    #[error("payload file has bad magic — not a capsule payload")]
    BadMagic,
    #[error("payload file version {0} is not supported by this build")]
    UnsupportedVersion(u8),
}

/// Outcome of a successful write.
pub struct PayloadWritten {
    pub payload_cid: String,
    pub size: u64,
}

/// Encrypt `plaintext` with the derived payload key for `capsule_id` and
/// write the sealed blob to `.capsule/payloads/{capsule_id}.enc`.
///
/// Overwrites any existing payload at that path. File is created with
/// mode 0600 on Unix; the parent directory gets 0700.
pub fn write(
    capsule_dir: &Path,
    capsule_id: &str,
    master_secret: &[u8; 32],
    plaintext: &[u8],
) -> Result<PayloadWritten, PayloadError> {
    let dir = payloads_dir(capsule_dir);
    ensure_payloads_dir(&dir)?;
    let path = payload_path(capsule_dir, capsule_id);

    let key = derive_payload_key(master_secret, capsule_id)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| PayloadError::Encrypt)?;

    let mut bytes = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.extend_from_slice(&[0u8; 3]);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&ciphertext);

    write_payload_file(&path, &bytes)?;

    let payload_cid = sha256_hex(&ciphertext);
    Ok(PayloadWritten {
        payload_cid,
        size: bytes.len() as u64,
    })
}

/// Read + decrypt the payload for `capsule_id`. Returned plaintext is a
/// plain Vec — caller is responsible for wrapping in Zeroizing if it
/// contains sensitive bytes. Not used by this slice (no serving path
/// consumes payloads yet); ships here so the write side is testable
/// via round-trip without a second module.
pub fn read(
    capsule_dir: &Path,
    capsule_id: &str,
    master_secret: &[u8; 32],
) -> Result<Vec<u8>, PayloadError> {
    let path = payload_path(capsule_dir, capsule_id);
    let bytes = std::fs::read(&path)?;
    if bytes.len() < HEADER_LEN + 16 {
        return Err(PayloadError::TooShort {
            expected: HEADER_LEN + 16,
            got: bytes.len(),
        });
    }
    if &bytes[0..8] != MAGIC {
        return Err(PayloadError::BadMagic);
    }
    let version = bytes[8];
    if version != VERSION {
        return Err(PayloadError::UnsupportedVersion(version));
    }
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[12..12 + NONCE_LEN]);
    let ciphertext = &bytes[HEADER_LEN..];

    let key = derive_payload_key(master_secret, capsule_id)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext)
        .map_err(|_| PayloadError::Decrypt)
}

fn derive_payload_key(
    master_secret: &[u8; 32],
    capsule_id: &str,
) -> Result<Zeroizing<[u8; 32]>, PayloadError> {
    let mut info = Vec::with_capacity(DOMAIN_PAYLOAD_V1.len() + capsule_id.len());
    info.extend_from_slice(DOMAIN_PAYLOAD_V1);
    info.extend_from_slice(capsule_id.as_bytes());
    let hkdf: Hkdf<Sha256> = Hkdf::new(None, master_secret);
    let mut out = [0u8; 32];
    hkdf.expand(&info, &mut out)
        .map_err(|e| PayloadError::Hkdf(e.to_string()))?;
    Ok(Zeroizing::new(out))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(CHARS[(b >> 4) as usize] as char);
        out.push(CHARS[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(unix)]
fn ensure_payloads_dir(path: &Path) -> Result<(), PayloadError> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_payloads_dir(path: &Path) -> Result<(), PayloadError> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn write_payload_file(path: &Path, bytes: &[u8]) -> Result<(), PayloadError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // Truncate-overwrite: republishing a capsule replaces its payload.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_payload_file(path: &Path, bytes: &[u8]) -> Result<(), PayloadError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "capsuled-payload-{}-{}",
            std::process::id(),
            suffix
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn write_read_roundtrip() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master = [7u8; 32];
        let plaintext = br#"{"records":[{"ts":"06:00","mg_dl":95}]}"#;

        let res = write(&capsule_dir, "cap_abc", &master, plaintext).expect("write");
        assert!(res.size > plaintext.len() as u64);
        assert_eq!(res.payload_cid.len(), 64);

        let got = read(&capsule_dir, "cap_abc", &master).expect("read");
        assert_eq!(got, plaintext);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn different_capsule_ids_yield_different_ciphertexts() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master = [7u8; 32];
        let plaintext = b"same plaintext";

        write(&capsule_dir, "cap_a", &master, plaintext).unwrap();
        write(&capsule_dir, "cap_b", &master, plaintext).unwrap();

        let a = std::fs::read(payload_path(&capsule_dir, "cap_a")).unwrap();
        let b = std::fs::read(payload_path(&capsule_dir, "cap_b")).unwrap();
        assert_ne!(
            a, b,
            "different capsule_id must produce different ciphertext"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn wrong_master_fails_auth() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master_a = [1u8; 32];
        let master_b = [2u8; 32];
        let plaintext = b"secret records";

        write(&capsule_dir, "cap_x", &master_a, plaintext).unwrap();
        let err = read(&capsule_dir, "cap_x", &master_b).unwrap_err();
        assert!(matches!(err, PayloadError::Decrypt));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tampered_ciphertext_fails_auth() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master = [3u8; 32];
        let plaintext = b"tamper me";
        write(&capsule_dir, "cap_t", &master, plaintext).unwrap();

        let path = payload_path(&capsule_dir, "cap_t");
        let mut bytes = std::fs::read(&path).unwrap();
        let idx = HEADER_LEN + 1;
        bytes[idx] ^= 0x01;
        std::fs::write(&path, &bytes).unwrap();

        let err = read(&capsule_dir, "cap_t", &master).unwrap_err();
        assert!(matches!(err, PayloadError::Decrypt));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn file_perms_are_0600_and_dir_is_0700() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master = [5u8; 32];
        write(&capsule_dir, "cap_p", &master, b"x").unwrap();

        let file_mode = std::fs::metadata(payload_path(&capsule_dir, "cap_p"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);

        let dir_mode = std::fs::metadata(payloads_dir(&capsule_dir))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn republish_overwrites() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir_all(&capsule_dir).unwrap();
        let master = [11u8; 32];
        write(&capsule_dir, "cap_v", &master, b"v1").unwrap();
        write(&capsule_dir, "cap_v", &master, b"v2-a-bit-longer").unwrap();

        let got = read(&capsule_dir, "cap_v", &master).unwrap();
        assert_eq!(got, b"v2-a-bit-longer");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
