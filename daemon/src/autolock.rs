//! Background task that transitions the keyring from Unlocked back to
//! Locked after a configured period of inactivity (spec §9.4).
//!
//! "Activity" is anything that consumed the master secret to produce a
//! response — right now just the 402 compute response. Future slices
//! (signing, pod coordination) will call `AppState::record_activity()`
//! from their handlers too.
//!
//! The task polls at a coarse interval (`CHECK_INTERVAL`) rather than
//! firing a per-unlock timer. At 1800s default timeout, a 30s polling
//! cadence means the actual lock happens within 30s of the deadline —
//! tight enough for security, loose enough that we don't burn CPU on
//! clock checks.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tracing::{info, warn};

use crate::keyring;
use crate::server::{AppState, KeyringSlot};

const CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Spawn the auto-lock task. Returns a handle that, when dropped,
/// triggers shutdown of the task on the next poll.
///
/// When `auto_lock` is None the task never locks — but we still spawn
/// it so the shape of the daemon stays consistent. A disabled timer is
/// a cheap poll that always does nothing.
pub fn spawn(state: AppState) -> AutoLockHandle {
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_task = Arc::clone(&shutdown);
    let handle = tokio::spawn(async move {
        run(state, shutdown_for_task).await;
    });
    AutoLockHandle {
        shutdown,
        _task: handle,
    }
}

async fn run(state: AppState, shutdown: Arc<Notify>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(CHECK_INTERVAL) => {
                if let Err(e) = maybe_lock(&state) {
                    warn!(error = %e, "auto-lock check failed");
                }
            }
            _ = shutdown.notified() => {
                return;
            }
        }
    }
}

fn maybe_lock(state: &AppState) -> Result<(), String> {
    let Some(timeout) = state.auto_lock() else {
        return Ok(()); // disabled
    };

    // Snapshot the decision without holding the slot's write lock across
    // the disk read. We only take the write lock to flip the slot.
    let should_lock = {
        let slot = state
            .keyring()
            .read()
            .map_err(|e| format!("keyring lock poisoned: {e}"))?;
        if !matches!(*slot, KeyringSlot::Unlocked(_)) {
            return Ok(());
        }
        let activity = state.last_activity();
        let last = activity
            .read()
            .map_err(|e| format!("activity lock poisoned: {e}"))?;
        elapsed_since(*last) >= timeout
    };
    if !should_lock {
        return Ok(());
    }

    let path = keyring::keyring_path(state.capsule_dir());
    let locked = keyring::load(&path).map_err(|e| format!("load keyring: {e}"))?;

    // Re-check the slot under the write lock — a concurrent /lock call
    // could have already transitioned us, in which case we don't want
    // to clobber with a freshly-loaded LockedKeyring.
    let mut slot = state
        .keyring()
        .write()
        .map_err(|e| format!("keyring lock poisoned: {e}"))?;
    if matches!(*slot, KeyringSlot::Unlocked(_)) {
        info!(
            idle_secs = state.auto_lock().map(|d| d.as_secs()).unwrap_or(0),
            "auto-locking keyring after inactivity"
        );
        *slot = KeyringSlot::Locked(locked);
    }
    Ok(())
}

fn elapsed_since(t: Instant) -> Duration {
    Instant::now().saturating_duration_since(t)
}

pub struct AutoLockHandle {
    shutdown: Arc<Notify>,
    _task: tokio::task::JoinHandle<()>,
}

impl Drop for AutoLockHandle {
    fn drop(&mut self) {
        self.shutdown.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyring;
    use crate::registry::Registry;
    use crate::server::KeyringSlot;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tempdir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "capsuled-autolock-{}-{}",
            std::process::id(),
            suffix
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Idle for longer than the timeout → maybe_lock transitions slot
    /// to Locked. We exercise the sync function directly so we don't
    /// have to wait a full CHECK_INTERVAL tick in the test.
    #[test]
    fn transitions_unlocked_to_locked_when_idle() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        let keyring_path = keyring::keyring_path(&capsule_dir);
        std::fs::create_dir_all(keyring_path.parent().unwrap()).unwrap();

        let unlocked = keyring::create(&keyring_path, b"t").expect("create");
        let state = AppState::new(
            tmp.clone(),
            capsule_dir,
            Registry::new(),
            KeyringSlot::Unlocked(unlocked),
            Some(Duration::from_millis(1)),
        );

        // Immediately after construction, last_activity is now — so the
        // timer says we haven't been idle long enough to lock yet.
        assert!(matches!(
            *state.keyring().read().unwrap(),
            KeyringSlot::Unlocked(_)
        ));

        // Backdate last_activity past the timeout.
        {
            let activity = state.last_activity();
            let mut ts = activity.write().unwrap();
            *ts = Instant::now().checked_sub(Duration::from_secs(60)).unwrap();
        }

        maybe_lock(&state).expect("maybe_lock");
        assert!(matches!(
            *state.keyring().read().unwrap(),
            KeyringSlot::Locked(_)
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Fresh activity → slot stays Unlocked even past the timeout
    /// boundary, because record_activity pushed last_activity forward.
    #[test]
    fn record_activity_defers_the_lock() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        let keyring_path = keyring::keyring_path(&capsule_dir);
        std::fs::create_dir_all(keyring_path.parent().unwrap()).unwrap();

        let unlocked = keyring::create(&keyring_path, b"t").expect("create");
        let state = AppState::new(
            tmp.clone(),
            capsule_dir,
            Registry::new(),
            KeyringSlot::Unlocked(unlocked),
            Some(Duration::from_millis(1)),
        );

        // Backdate → would lock on next maybe_lock call.
        {
            let activity = state.last_activity();
            let mut ts = activity.write().unwrap();
            *ts = Instant::now().checked_sub(Duration::from_secs(60)).unwrap();
        }
        // But record_activity resets it.
        state.record_activity();

        maybe_lock(&state).expect("maybe_lock");
        assert!(matches!(
            *state.keyring().read().unwrap(),
            KeyringSlot::Unlocked(_)
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// auto_lock = None disables auto-lock entirely.
    #[test]
    fn disabled_never_locks() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        let keyring_path = keyring::keyring_path(&capsule_dir);
        std::fs::create_dir_all(keyring_path.parent().unwrap()).unwrap();

        let unlocked = keyring::create(&keyring_path, b"t").expect("create");
        let state = AppState::new(
            tmp.clone(),
            capsule_dir,
            Registry::new(),
            KeyringSlot::Unlocked(unlocked),
            None,
        );

        // Backdate — would lock if enabled.
        {
            let activity = state.last_activity();
            let mut ts = activity.write().unwrap();
            *ts = Instant::now()
                .checked_sub(Duration::from_secs(600))
                .unwrap();
        }

        maybe_lock(&state).expect("maybe_lock");
        assert!(matches!(
            *state.keyring().read().unwrap(),
            KeyringSlot::Unlocked(_)
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
