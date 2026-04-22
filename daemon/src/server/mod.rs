use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

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
    /// Last time the daemon did something that consumed the unlocked
    /// master secret. Updated at unlock/init time and on every endpoint
    /// that produces a signed or keyring-gated response. The auto-lock
    /// task compares `last_activity.elapsed()` against `auto_lock` and
    /// transitions the slot back to Locked when idle too long.
    last_activity: Arc<RwLock<Instant>>,
    /// Idle timeout after which the keyring auto-locks. `None` disables
    /// auto-lock (spec §9.4 allows this for always-on dev setups).
    auto_lock: Option<Duration>,
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
        auto_lock: Option<Duration>,
    ) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                start_time: Instant::now(),
                vault_path,
                capsule_dir,
                version: env!("CARGO_PKG_VERSION"),
                registry,
                keyring: Arc::new(RwLock::new(keyring)),
                last_activity: Arc::new(RwLock::new(Instant::now())),
                auto_lock,
            }),
        }
    }

    /// Bump the activity timestamp. Call after any endpoint that consumed
    /// the unlocked master secret (e.g. after building a 402 response that
    /// commits to the node's payout address). Call on unlock/init so a
    /// just-unlocked keyring doesn't auto-lock immediately.
    pub fn record_activity(&self) {
        if let Ok(mut ts) = self.inner.last_activity.write() {
            *ts = Instant::now();
        }
    }

    /// Configured idle timeout. None means auto-lock is disabled.
    pub fn auto_lock(&self) -> Option<Duration> {
        self.inner.auto_lock
    }

    /// Seconds remaining until auto-lock fires, given current activity.
    /// None if the keyring isn't Unlocked or auto-lock is disabled.
    pub fn auto_lock_seconds_remaining(&self) -> Option<u64> {
        let slot = self.inner.keyring.read().ok()?;
        if !matches!(*slot, KeyringSlot::Unlocked(_)) {
            return None;
        }
        let timeout = self.inner.auto_lock?;
        let last = self.inner.last_activity.read().ok()?;
        let elapsed = last.elapsed();
        Some(timeout.saturating_sub(elapsed).as_secs())
    }

    pub(crate) fn last_activity(&self) -> Arc<RwLock<Instant>> {
        Arc::clone(&self.inner.last_activity)
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
