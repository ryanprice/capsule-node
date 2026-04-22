//! Encrypted keyring at rest (spec §9.4).
//!
//! The master secret is a 32-byte random key that higher-level slices derive
//! wallet keys, signing keys, and anything else node-identity-shaped from.
//! This module is the trust root: if an attacker can read the plaintext
//! master secret off disk or out of swap, every other security property the
//! daemon claims is void.
//!
//! File format (`.capsule/identity/keyring.enc`, mode 0600):
//!
//! ```text
//! offset  size   field
//! 0       8      magic "CAPSULE\0"
//! 8       1      version (0x01)
//! 9       1      kdf_id (0x01 = Argon2id)
//! 10      2      reserved (0x0000)
//! 12      4      argon2_m_cost  (KiB, little-endian u32)
//! 16      4      argon2_t_cost  (iterations, LE u32)
//! 20      4      argon2_p_cost  (parallelism, LE u32)
//! 24      16     salt (random)
//! 40      12     nonce (random, one-shot per file → ChaCha20Poly1305 is safe)
//! 52      48     ciphertext = Seal(master_secret || auth_tag)
//! ```
//!
//! Security invariants this module must preserve:
//!
//! * Decrypted master secret is held in a Zeroizing<Box<[u8; 32]>> whose
//!   heap pages are mlock()'d so the kernel cannot page it to swap.
//! * UnlockedKeyring has no Debug or Display impl; callers can only read
//!   the secret through `with_secret(|&[u8; 32]| ...)`, which keeps the
//!   reference scoped.
//! * Wrong passphrase → authentication failure, not a silent "unlocked with
//!   a junk key". The AEAD's integrity tag is what enforces this.
//! * Keyring file has mode 0600 on Unix; its parent `.capsule/identity/`
//!   directory is 0700.
//!
//! Slice 5a delivers create / load / unlock. Auto-lock timer and wallet
//! derivation land in 5b/5c.

use std::fmt;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand::{rngs::OsRng, RngCore};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const MASTER_SECRET_LEN: usize = 32;
const MAGIC: &[u8; 8] = b"CAPSULE\0";
const VERSION: u8 = 1;
const KDF_ID_ARGON2ID: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = 8 + 1 + 1 + 2 + 4 + 4 + 4 + SALT_LEN + NONCE_LEN;
const FILE_LEN: usize = HEADER_LEN + MASTER_SECRET_LEN + TAG_LEN;

/// Argon2id cost parameters. These match OWASP's recommended minimums for
/// interactive password authentication as of the time this slice shipped;
/// raising them is safe (readers re-parse from the file header), lowering
/// them weakens every keyring already on disk.
const ARGON2_M_COST_KIB: u32 = 65536; // 64 MB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;

pub fn keyring_path(capsule_dir: &Path) -> PathBuf {
    capsule_dir.join("identity").join("keyring.enc")
}

pub fn identity_dir(capsule_dir: &Path) -> PathBuf {
    capsule_dir.join("identity")
}

/// State of a keyring file on disk, parsed but not yet decrypted.
///
/// Holds the KDF parameters + ciphertext in memory so `unlock()` can derive
/// the KEK and attempt decryption without re-reading from disk.
pub struct LockedKeyring {
    path: PathBuf,
    kdf_params: Argon2Params,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

/// Decrypted master secret, held in mlocked + zeroized memory.
///
/// Intentionally has no Debug or Display impl — printing this struct would
/// leak key material. Callers access the secret through `with_secret`,
/// which scopes the reference.
///
/// The 32 bytes are held in a `Zeroizing<Vec<u8>>` (heap-allocated so we
/// can mlock a stable address; auto-zeroized on drop). Length is pinned
/// at MASTER_SECRET_LEN at construction and never mutated, so the
/// slice→[u8; 32] conversion in `with_secret` is infallible.
pub struct UnlockedKeyring {
    secret: Zeroizing<Vec<u8>>,
    /// True if mlock succeeded at construction. Drop only calls munlock
    /// when locking succeeded; otherwise we'd pass a non-locked pointer
    /// and the kernel would return an error we'd swallow anyway.
    mlocked: bool,
}

#[derive(Debug, Clone, Copy)]
struct Argon2Params {
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
}

impl Argon2Params {
    fn default() -> Self {
        Self {
            m_cost: ARGON2_M_COST_KIB,
            t_cost: ARGON2_T_COST,
            p_cost: ARGON2_P_COST,
        }
    }

    fn build(&self) -> Result<Argon2<'_>, KeyringError> {
        let params = Params::new(self.m_cost, self.t_cost, self.p_cost, Some(32))
            .map_err(|e| KeyringError::KdfParams(e.to_string()))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("keyring file already exists at {0}")]
    AlreadyExists(PathBuf),
    #[error("keyring file not found at {0}")]
    NotFound(PathBuf),
    #[error("keyring file is shorter than expected (need at least {expected} bytes, got {got})")]
    TooShort { expected: usize, got: usize },
    #[error("keyring file has bad magic — not a capsule keyring")]
    BadMagic,
    #[error("keyring file version {0} is not supported by this build")]
    UnsupportedVersion(u8),
    #[error("keyring file uses KDF id {0}, expected Argon2id ({1})")]
    UnsupportedKdf(u8, u8),
    #[error("argon2 parameters invalid: {0}")]
    KdfParams(String),
    #[error("argon2 key derivation failed: {0}")]
    KdfRun(String),
    /// Covers wrong passphrase AND tampering: ChaCha20Poly1305 can't tell us
    /// which, and we don't want to leak that distinction to callers.
    #[error("decryption failed: bad passphrase or corrupted keyring")]
    BadPassphrase,
    #[error("mlock failed ({0}); decrypted key would be pageable to swap — refusing to unlock")]
    Mlock(std::io::Error),
    #[error("passphrase must not be empty")]
    EmptyPassphrase,
}

/// Create a fresh keyring at `path`. Fails if the file already exists.
///
/// Returns the UnlockedKeyring so the caller doesn't have to round-trip
/// through `load` + `unlock` immediately after `create`.
pub fn create(path: &Path, passphrase: &[u8]) -> Result<UnlockedKeyring, KeyringError> {
    if passphrase.is_empty() {
        return Err(KeyringError::EmptyPassphrase);
    }
    if path.exists() {
        return Err(KeyringError::AlreadyExists(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        create_identity_dir(parent)?;
    }

    let kdf_params = Argon2Params::default();
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);

    // Random master secret. After encryption we build the UnlockedKeyring
    // from the SAME bytes — no second OsRng call, no re-read from disk.
    let mut master = [0u8; MASTER_SECRET_LEN];
    OsRng.fill_bytes(&mut master);
    let master_zeroizing = Zeroizing::new(master);

    let kek = derive_kek(passphrase, &salt, kdf_params)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(kek.as_slice()));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), master_zeroizing.as_slice())
        .map_err(|_| KeyringError::BadPassphrase)?;

    let mut bytes = Vec::with_capacity(FILE_LEN);
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.push(KDF_ID_ARGON2ID);
    bytes.extend_from_slice(&[0, 0]); // reserved
    bytes.extend_from_slice(&kdf_params.m_cost.to_le_bytes());
    bytes.extend_from_slice(&kdf_params.t_cost.to_le_bytes());
    bytes.extend_from_slice(&kdf_params.p_cost.to_le_bytes());
    bytes.extend_from_slice(&salt);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&ciphertext);

    write_keyring_file(path, &bytes)?;

    let mut master_for_handle = [0u8; MASTER_SECRET_LEN];
    master_for_handle.copy_from_slice(&master_zeroizing[..]);
    UnlockedKeyring::from_bytes(master_for_handle)
}

/// Read and parse the keyring file. Does not decrypt — that's `unlock`.
pub fn load(path: &Path) -> Result<LockedKeyring, KeyringError> {
    if !path.exists() {
        return Err(KeyringError::NotFound(path.to_path_buf()));
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() < HEADER_LEN {
        return Err(KeyringError::TooShort {
            expected: HEADER_LEN,
            got: bytes.len(),
        });
    }
    if &bytes[0..8] != MAGIC {
        return Err(KeyringError::BadMagic);
    }
    let version = bytes[8];
    if version != VERSION {
        return Err(KeyringError::UnsupportedVersion(version));
    }
    let kdf_id = bytes[9];
    if kdf_id != KDF_ID_ARGON2ID {
        return Err(KeyringError::UnsupportedKdf(kdf_id, KDF_ID_ARGON2ID));
    }

    let kdf_params = Argon2Params {
        m_cost: u32_from_le(&bytes[12..16]),
        t_cost: u32_from_le(&bytes[16..20]),
        p_cost: u32_from_le(&bytes[20..24]),
    };
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes[24..24 + SALT_LEN]);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes[40..40 + NONCE_LEN]);
    let ciphertext = bytes[HEADER_LEN..].to_vec();

    Ok(LockedKeyring {
        path: path.to_path_buf(),
        kdf_params,
        salt,
        nonce,
        ciphertext,
    })
}

impl LockedKeyring {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unlock(&self, passphrase: &[u8]) -> Result<UnlockedKeyring, KeyringError> {
        if passphrase.is_empty() {
            return Err(KeyringError::EmptyPassphrase);
        }
        let kek = derive_kek(passphrase, &self.salt, self.kdf_params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(kek.as_slice()));
        let mut plaintext = Zeroizing::new(
            cipher
                .decrypt(Nonce::from_slice(&self.nonce), self.ciphertext.as_ref())
                .map_err(|_| KeyringError::BadPassphrase)?,
        );
        if plaintext.len() != MASTER_SECRET_LEN {
            return Err(KeyringError::BadPassphrase);
        }
        let mut master = [0u8; MASTER_SECRET_LEN];
        master.copy_from_slice(&plaintext[..]);
        plaintext.zeroize();
        UnlockedKeyring::from_bytes(master)
    }
}

impl UnlockedKeyring {
    fn from_bytes(mut secret: [u8; MASTER_SECRET_LEN]) -> Result<Self, KeyringError> {
        // Copy the bytes into a heap Vec so we have a stable address
        // for mlock, and can zeroize the input array afterwards.
        let mut vec = vec![0u8; MASTER_SECRET_LEN];
        vec.copy_from_slice(&secret);
        secret.zeroize();
        let secret_vec = Zeroizing::new(vec);
        let mlocked = lock_memory(secret_vec.as_ptr(), MASTER_SECRET_LEN)?;
        Ok(Self {
            secret: secret_vec,
            mlocked,
        })
    }

    /// Expose the master secret to a closure. The reference is scoped —
    /// callers can't smuggle it out by design.
    pub fn with_secret<R>(&self, f: impl FnOnce(&[u8; MASTER_SECRET_LEN]) -> R) -> R {
        let arr: &[u8; MASTER_SECRET_LEN] = self
            .secret
            .as_slice()
            .try_into()
            .expect("master secret length pinned to MASTER_SECRET_LEN at construction");
        f(arr)
    }

    #[cfg(test)]
    fn secret_bytes_for_tests(&self) -> [u8; MASTER_SECRET_LEN] {
        let mut out = [0u8; MASTER_SECRET_LEN];
        out.copy_from_slice(&self.secret[..]);
        out
    }
}

impl Drop for UnlockedKeyring {
    fn drop(&mut self) {
        if self.mlocked {
            // Unlock the pages before the Vec's allocator returns them to
            // the system. Zeroization of the bytes themselves is handled
            // by Zeroizing<Vec<u8>> when it drops after this.
            let _ = unlock_memory(self.secret.as_ptr(), MASTER_SECRET_LEN);
        }
    }
}

// Deliberately non-derived Debug impls that refuse to print key material
// or any bytes that were the output of decryption. `Result::unwrap_err` and
// similar patterns require Debug on the success type; without these impls
// call sites would leak the secret into panic messages.
impl fmt::Debug for UnlockedKeyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnlockedKeyring")
            .field("secret", &"<redacted>")
            .field("mlocked", &self.mlocked)
            .finish()
    }
}

impl fmt::Debug for LockedKeyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LockedKeyring")
            .field("path", &self.path)
            .field("kdf_params", &self.kdf_params)
            .field("ciphertext_len", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

fn derive_kek(
    passphrase: &[u8],
    salt: &[u8; SALT_LEN],
    params: Argon2Params,
) -> Result<Zeroizing<[u8; 32]>, KeyringError> {
    let argon2 = params.build()?;
    let mut kek = [0u8; 32];
    argon2
        .hash_password_into(passphrase, salt, &mut kek)
        .map_err(|e| KeyringError::KdfRun(e.to_string()))?;
    Ok(Zeroizing::new(kek))
}

fn u32_from_le(bytes: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes);
    u32::from_le_bytes(buf)
}

#[cfg(unix)]
fn create_identity_dir(path: &Path) -> Result<(), KeyringError> {
    use std::os::unix::fs::PermissionsExt;
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn create_identity_dir(path: &Path) -> Result<(), KeyringError> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    // TODO: Windows ACL to restrict to current user (spec §9.4).
    Ok(())
}

#[cfg(unix)]
fn write_keyring_file(path: &Path, bytes: &[u8]) -> Result<(), KeyringError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_keyring_file(path: &Path, bytes: &[u8]) -> Result<(), KeyringError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn lock_memory(ptr: *const u8, len: usize) -> Result<bool, KeyringError> {
    // SAFETY: `ptr` comes from a live Box<[u8; N]> whose lifetime exceeds
    // this call; `len` matches that allocation.
    let result = unsafe { libc::mlock(ptr as *const libc::c_void, len) };
    if result != 0 {
        return Err(KeyringError::Mlock(std::io::Error::last_os_error()));
    }
    Ok(true)
}

#[cfg(unix)]
fn unlock_memory(ptr: *const u8, len: usize) -> Result<(), KeyringError> {
    // SAFETY: symmetric with `lock_memory`; called from Drop before the
    // Box's backing allocation is freed.
    let result = unsafe { libc::munlock(ptr as *const libc::c_void, len) };
    if result != 0 {
        return Err(KeyringError::Mlock(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn lock_memory(_ptr: *const u8, _len: usize) -> Result<bool, KeyringError> {
    // TODO: VirtualLock on Windows.
    tracing::warn!("mlock unavailable on this platform; master secret may page to swap");
    Ok(false)
}

#[cfg(not(unix))]
fn unlock_memory(_ptr: *const u8, _len: usize) -> Result<(), KeyringError> {
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
            "capsuled-keyring-{}-{}",
            std::process::id(),
            suffix
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// For tests: weaker Argon2 params so the suite runs in reasonable time.
    /// Production code uses Argon2Params::default (ARGON2_M_COST_KIB etc).
    fn test_params() -> Argon2Params {
        Argon2Params {
            m_cost: 1024, // 1 MB
            t_cost: 1,
            p_cost: 1,
        }
    }

    /// Private helper mirroring `create` but with reduced KDF cost so the
    /// test suite doesn't spend 100ms per keyring operation.
    fn create_with_params(
        path: &Path,
        passphrase: &[u8],
        params: Argon2Params,
    ) -> Result<UnlockedKeyring, KeyringError> {
        if passphrase.is_empty() {
            return Err(KeyringError::EmptyPassphrase);
        }
        if path.exists() {
            return Err(KeyringError::AlreadyExists(path.to_path_buf()));
        }
        if let Some(parent) = path.parent() {
            create_identity_dir(parent)?;
        }

        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let mut master = [0u8; MASTER_SECRET_LEN];
        OsRng.fill_bytes(&mut master);
        let master_zeroizing = Zeroizing::new(master);

        let kek = derive_kek(passphrase, &salt, params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(kek.as_slice()));
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), master_zeroizing.as_slice())
            .map_err(|_| KeyringError::BadPassphrase)?;

        let mut bytes = Vec::with_capacity(FILE_LEN);
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.push(KDF_ID_ARGON2ID);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&params.m_cost.to_le_bytes());
        bytes.extend_from_slice(&params.t_cost.to_le_bytes());
        bytes.extend_from_slice(&params.p_cost.to_le_bytes());
        bytes.extend_from_slice(&salt);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&ciphertext);

        write_keyring_file(path, &bytes)?;
        let mut master_for_handle = [0u8; MASTER_SECRET_LEN];
        master_for_handle.copy_from_slice(&master_zeroizing[..]);
        UnlockedKeyring::from_bytes(master_for_handle)
    }

    #[test]
    fn create_load_unlock_roundtrip() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");

        let created = create_with_params(&path, b"correct horse battery staple", test_params())
            .expect("create");
        let created_secret = created.secret_bytes_for_tests();
        drop(created);

        let locked = load(&path).expect("load");
        let unlocked = locked
            .unlock(b"correct horse battery staple")
            .expect("unlock");
        assert_eq!(unlocked.secret_bytes_for_tests(), created_secret);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn wrong_passphrase_fails_authentication() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        create_with_params(&path, b"correct", test_params()).expect("create");

        let locked = load(&path).expect("load");
        let err = locked.unlock(b"incorrect").unwrap_err();
        assert!(matches!(err, KeyringError::BadPassphrase));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        create_with_params(&path, b"correct", test_params()).expect("create");

        // Flip a byte inside the ciphertext region.
        let mut bytes = std::fs::read(&path).unwrap();
        let idx = HEADER_LEN + 1;
        bytes[idx] ^= 0x01;
        std::fs::write(&path, &bytes).unwrap();

        let locked = load(&path).expect("load");
        let err = locked.unlock(b"correct").unwrap_err();
        assert!(matches!(err, KeyringError::BadPassphrase));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_passphrase_is_rejected() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        let err = create_with_params(&path, b"", test_params()).unwrap_err();
        assert!(matches!(err, KeyringError::EmptyPassphrase));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        create_with_params(&path, b"a", test_params()).expect("first");
        let err = create_with_params(&path, b"b", test_params()).unwrap_err();
        assert!(matches!(err, KeyringError::AlreadyExists(_)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        create_identity_dir(&tmp.join("identity")).unwrap();
        // Full header length of garbage — TooShort would trigger before
        // BadMagic otherwise.
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[0..8].copy_from_slice(b"NOTCAPS\0");
        std::fs::write(&path, &bytes).unwrap();
        let err = load(&path).unwrap_err();
        assert!(matches!(err, KeyringError::BadMagic), "got {err:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn file_perms_are_0600_and_dir_is_0700() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir();
        let path = tmp.join("identity").join("keyring.enc");
        create_with_params(&path, b"correct", test_params()).expect("create");

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "keyring.enc must be 0600");
        let dir_mode = std::fs::metadata(tmp.join("identity"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "identity/ must be 0700");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
