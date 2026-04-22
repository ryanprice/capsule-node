use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::keyring::{LockedKeyring, UnlockedKeyring};
use crate::registry::Registry;

pub mod mgmt;
pub mod public;

/// Shared read-only state handed to every request handler.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    start_time: Instant,
    vault_path: PathBuf,
    capsule_dir: PathBuf,
    version: &'static str,
    registry: Registry,
    keyring: Arc<RwLock<KeyringSlot>>,
}

/// Current state of the node identity keyring.
///
/// Transitions are one-way per session: None → Locked (via init); Locked →
/// Unlocked (via unlock); Unlocked → Locked (via lock or auto-lock). There
/// is no path from Unlocked or Locked back to None short of deleting the
/// keyring file on disk and restarting — that is intentional.
pub enum KeyringSlot {
    /// No keyring file on disk. Daemon can serve manifests but cannot
    /// perform any operation requiring the node identity.
    None,
    /// Keyring file loaded, ciphertext in memory, master secret still
    /// sealed. `unlock(passphrase)` transitions to Unlocked.
    Locked(LockedKeyring),
    /// Master secret decrypted and held in mlocked memory.
    Unlocked(UnlockedKeyring),
}

impl KeyringSlot {
    pub fn status_label(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Locked(_) => "locked",
            Self::Unlocked(_) => "unlocked",
        }
    }
}

impl AppState {
    pub fn new(
        vault_path: PathBuf,
        capsule_dir: PathBuf,
        registry: Registry,
        keyring: KeyringSlot,
    ) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                start_time: Instant::now(),
                vault_path,
                capsule_dir,
                version: env!("CARGO_PKG_VERSION"),
                registry,
                keyring: Arc::new(RwLock::new(keyring)),
            }),
        }
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.inner.start_time.elapsed().as_secs()
    }

    pub fn vault_path(&self) -> &PathBuf {
        &self.inner.vault_path
    }

    pub fn capsule_dir(&self) -> &PathBuf {
        &self.inner.capsule_dir
    }

    pub fn version(&self) -> &'static str {
        self.inner.version
    }

    pub fn registry(&self) -> &Registry {
        &self.inner.registry
    }

    pub fn keyring(&self) -> &Arc<RwLock<KeyringSlot>> {
        &self.inner.keyring
    }

    /// Current node payout address, as an EIP-55 Ethereum address.
    /// Returns `None` when the keyring is not Unlocked — public
    /// endpoints reflect this by omitting or nulling the `recipient`
    /// field in responses.
    pub fn wallet_address(&self) -> Option<String> {
        let slot = self.inner.keyring.read().ok()?;
        match &*slot {
            KeyringSlot::Unlocked(unlocked) => Some(unlocked.wallet_address().to_string()),
            _ => None,
        }
    }
}
