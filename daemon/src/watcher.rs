use std::path::Path;

use notify::{
    event::{EventKind, ModifyKind, RenameMode},
    Config, RecommendedWatcher, RecursiveMode, Watcher,
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::registry::{capsule_id_from_path, load_manifest_file, Registry};

/// Spawn a filesystem watcher on `<capsule_dir>/manifests/`. Returns a handle
/// that keeps the watcher alive; dropping it stops watching.
///
/// The watcher reloads the registry entry for each Create/Modify event and
/// removes the entry on Remove. Unparseable files are logged and skipped —
/// the daemon continues serving whatever is already in the registry.
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
        while let Some(res) = rx.recv().await {
            match res {
                Ok(event) => handle_event(event, &dir_for_task, &registry),
                Err(e) => warn!(error = %e, "fs watcher error"),
            }
        }
        debug!("fs watcher task exiting");
    });

    Ok(WatcherHandle {
        _watcher: watcher,
        _task: task,
    })
}

fn handle_event(event: notify::Event, manifests_dir: &Path, registry: &Registry) {
    let is_relevant = matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Remove(_)
    );
    if !is_relevant {
        return;
    }

    for path in event.paths {
        // Defense-in-depth: only react to paths inside the watched dir and
        // ending in .json. notify should only deliver those, but a symlink
        // or race could in principle smuggle one in.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if path.parent() != Some(manifests_dir) {
            continue;
        }

        match event.kind {
            EventKind::Remove(_) => handle_removal(&path, registry),
            _ if path.exists() => handle_upsert(&path, registry),
            _ => handle_removal(&path, registry),
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
    use std::time::Duration;

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

        // Poll up to 2s for the registry to reflect the write.
        let found = poll_for(|| registry.len() == 1, Duration::from_secs(2)).await;
        assert!(found, "expected registry to contain new manifest");

        // Remove file → registry should drop it.
        std::fs::remove_file(&path).unwrap();
        let empty = poll_for(|| registry.is_empty(), Duration::from_secs(2)).await;
        assert!(empty, "expected registry to drop removed manifest");

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
