use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::{event::EventKind, Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::registry::{
    capsule_id_from_path, is_manifest_filename, load_manifest_file, ManifestLoadError, Registry,
};

/// How long a path must be quiet before we act on it. Coalesces the burst of
/// events that fires during a single logical write (Obsidian adapter close →
/// Syncthing propagation → macOS AppleDouble side-writes, etc.) into one
/// read. 150ms is long enough to absorb normal filesystem chatter and short
/// enough that the daemon feels instant to a user editing a note.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Fallback sleep when no paths are pending. Any new event cancels it via
/// `select!`, so the magnitude doesn't matter — just keep it bounded so
/// the task isn't blocked indefinitely if rx is dropped cleanly.
const IDLE_SLEEP: Duration = Duration::from_secs(3600);

/// Spawn a filesystem watcher on `<capsule_dir>/manifests/`. Returns a handle
/// that keeps the watcher alive; dropping it stops watching.
///
/// Events are coalesced per-path with a short debounce. After the quiet
/// window, the path is read fresh: if the file exists, the manifest is
/// (re-)inserted; if not, the capsule is removed from the registry. This
/// means the event kind (Create/Modify/Remove) doesn't affect correctness —
/// only the final filesystem state does.
pub fn spawn(capsule_dir: &Path, registry: Registry) -> anyhow::Result<WatcherHandle> {
    let manifests_dir = capsule_dir.join("manifests");
    std::fs::create_dir_all(&manifests_dir)?;

    // notify emits on a std thread; bridge to tokio via an mpsc channel.
    let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )?;
    watcher.watch(&manifests_dir, RecursiveMode::NonRecursive)?;
    info!(path = %manifests_dir.display(), "watching manifests directory");

    let dir_for_task = manifests_dir.clone();
    let task = tokio::spawn(async move {
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
        loop {
            let wait = next_wait(&pending);
            tokio::select! {
                maybe_evt = rx.recv() => {
                    let Some(res) = maybe_evt else { break; };
                    match res {
                        Ok(event) => collect_paths(event, &dir_for_task, &mut pending),
                        Err(e) => warn!(error = %e, "fs watcher error"),
                    }
                }
                _ = tokio::time::sleep(wait) => {
                    drain_ready(&mut pending, &registry);
                }
            }
        }
        debug!("fs watcher task exiting");
    });

    Ok(WatcherHandle {
        _watcher: watcher,
        _task: task,
    })
}

fn next_wait(pending: &HashMap<PathBuf, Instant>) -> Duration {
    let Some(soonest) = pending.values().min().copied() else {
        return IDLE_SLEEP;
    };
    soonest.saturating_duration_since(Instant::now())
}

fn collect_paths(
    event: notify::Event,
    manifests_dir: &Path,
    pending: &mut HashMap<PathBuf, Instant>,
) {
    let is_relevant = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    );
    if !is_relevant {
        return;
    }
    let deadline = Instant::now() + DEBOUNCE;
    for path in event.paths {
        if !is_manifest_filename(&path) {
            continue;
        }
        if path.parent() != Some(manifests_dir) {
            continue;
        }
        // Overwrite to extend the quiet window on every event. We only act
        // once the path has been quiet for the full DEBOUNCE duration.
        pending.insert(path, deadline);
    }
}

fn drain_ready(pending: &mut HashMap<PathBuf, Instant>, registry: &Registry) {
    let now = Instant::now();
    let ready: Vec<PathBuf> = pending
        .iter()
        .filter(|(_, deadline)| **deadline <= now)
        .map(|(path, _)| path.clone())
        .collect();
    for path in ready {
        pending.remove(&path);
        if path.exists() {
            handle_upsert(&path, registry);
        } else {
            handle_removal(&path, registry);
        }
    }
}

fn handle_upsert(path: &Path, registry: &Registry) {
    match load_manifest_file(path) {
        Ok(manifest) => {
            info!(
                capsule_id = %manifest.capsule_id,
                path = %path.display(),
                "manifest registered"
            );
            registry.insert(manifest);
        }
        Err(ManifestLoadError::PartialFile) => {
            // Writer wasn't really done despite the debounce. Debug-level;
            // the next event (or a later one during the same write sequence)
            // will retry. If it never recovers, the user will notice the
            // capsule never registering — louder signal than a log warning.
            debug!(path = %path.display(), "transient partial file, retrying on next event");
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to load manifest");
        }
    }
}

fn handle_removal(path: &Path, registry: &Registry) {
    let Some(id) = capsule_id_from_path(path) else {
        return;
    };
    if registry.remove(&id).is_some() {
        info!(capsule_id = %id, path = %path.display(), "manifest removed");
    }
}

/// Keeping this struct alive keeps the watcher and its bridge task alive.
pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
    _task: tokio::task::JoinHandle<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{CapsuleId, CapsuleStatus, ComputationClass, Manifest};
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let p =
            std::env::temp_dir().join(format!("capsuled-watch-{}-{}", std::process::id(), suffix));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample(id: &str) -> Manifest {
        Manifest {
            capsule_id: CapsuleId::new(id).unwrap(),
            schema: "capsule://test".into(),
            status: CapsuleStatus::Active,
            floor_price: "0.01".into(),
            computation_classes: vec![ComputationClass::A],
            tags: vec![],
            payload_cid: None,
            earnings_total: None,
            queries_served: None,
            last_accessed: None,
        }
    }

    #[tokio::test]
    async fn watcher_picks_up_new_manifest() {
        let vault = tempdir();
        let capsule_dir = vault.join(".capsule");
        std::fs::create_dir_all(capsule_dir.join("manifests")).unwrap();

        let registry = Registry::new();
        let _handle = spawn(&capsule_dir, registry.clone()).unwrap();

        // Allow watcher thread to initialize before we write.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let manifest = sample("cap_watch1");
        let path = capsule_dir.join("manifests").join("cap_watch1.json");
        std::fs::write(&path, serde_json::to_string(&manifest).unwrap()).unwrap();

        // Poll up to 2s. Debounce adds ~150ms, so headroom is plenty.
        let found = poll_for(|| registry.len() == 1, Duration::from_secs(2)).await;
        assert!(found, "expected registry to contain new manifest");

        // Remove file → registry should drop it.
        std::fs::remove_file(&path).unwrap();
        let empty = poll_for(|| registry.is_empty(), Duration::from_secs(2)).await;
        assert!(empty, "expected registry to drop removed manifest");

        let _ = std::fs::remove_dir_all(&vault);
    }

    /// AppleDouble files (macOS metadata) and plugin-side `.tmp` files must
    /// never reach the registry. The filter lives in registry.rs but the
    /// watcher is the hot path that matters in production.
    #[tokio::test]
    async fn watcher_ignores_hidden_and_tmp_files() {
        let vault = tempdir();
        let capsule_dir = vault.join(".capsule");
        let manifests = capsule_dir.join("manifests");
        std::fs::create_dir_all(&manifests).unwrap();

        let registry = Registry::new();
        let _handle = spawn(&capsule_dir, registry.clone()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // AppleDouble side-car that Syncthing-from-Mac produces.
        std::fs::write(manifests.join("._cap_abc.json"), "garbage").unwrap();
        // Plugin atomic-write tempfile (ends in .tmp, not .json).
        std::fs::write(manifests.join("cap_abc.json.tmp"), "{}").unwrap();

        // Give the watcher + debounce plenty of time to NOT register anything.
        tokio::time::sleep(DEBOUNCE + Duration::from_millis(300)).await;
        assert!(
            registry.is_empty(),
            "registry should ignore hidden and tmp files"
        );

        // Now a real manifest lands — it should register.
        let manifest = sample("cap_abc");
        std::fs::write(
            manifests.join("cap_abc.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let found = poll_for(|| registry.len() == 1, Duration::from_secs(2)).await;
        assert!(found, "real manifest should register after noise");

        let _ = std::fs::remove_dir_all(&vault);
    }

    /// A burst of writes during a single logical save must coalesce into one
    /// registry update, not one per event. We simulate the partial-then-
    /// final pattern observed in production: empty file first, then the
    /// real content.
    #[tokio::test]
    async fn watcher_coalesces_partial_then_final_write() {
        let vault = tempdir();
        let capsule_dir = vault.join(".capsule");
        let manifests = capsule_dir.join("manifests");
        std::fs::create_dir_all(&manifests).unwrap();

        let registry = Registry::new();
        let _handle = spawn(&capsule_dir, registry.clone()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let path = manifests.join("cap_burst.json");
        // Simulate three rapid writes within the debounce window.
        std::fs::write(&path, "").unwrap();
        std::fs::write(&path, "{").unwrap();
        std::fs::write(&path, serde_json::to_string(&sample("cap_burst")).unwrap()).unwrap();

        let found = poll_for(|| registry.len() == 1, Duration::from_secs(2)).await;
        assert!(
            found,
            "expected burst-write to resolve to a single registered manifest"
        );
        let _ = std::fs::remove_dir_all(&vault);
    }

    async fn poll_for<F: Fn() -> bool>(cond: F, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        cond()
    }
}
