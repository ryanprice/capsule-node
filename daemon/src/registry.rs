use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use tracing::{debug, info, warn};

use crate::manifest::{CapsuleId, Manifest};

/// Thread-safe in-memory index of capsule manifests, indexed by CapsuleId.
///
/// The filesystem under `.capsule/manifests/` is the source of truth; this
/// registry is a cache rebuilt on daemon boot and kept in sync by the fs
/// watcher task.
#[derive(Clone)]
pub struct Registry {
    inner: Arc<RwLock<HashMap<CapsuleId, Manifest>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, manifest: Manifest) {
        let mut guard = self.inner.write().expect("registry poisoned");
        guard.insert(manifest.capsule_id.clone(), manifest);
    }

    pub fn remove(&self, id: &CapsuleId) -> Option<Manifest> {
        let mut guard = self.inner.write().expect("registry poisoned");
        guard.remove(id)
    }

    pub fn get(&self, id: &CapsuleId) -> Option<Manifest> {
        let guard = self.inner.read().expect("registry poisoned");
        guard.get(id).cloned()
    }

    pub fn list(&self) -> Vec<Manifest> {
        let guard = self.inner.read().expect("registry poisoned");
        guard.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.read().expect("registry poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Scan `<capsule_dir>/manifests/*.json` and load each parseable manifest
/// into the registry. Unparseable files are logged and skipped — one bad
/// file must not block the daemon from starting.
pub fn load_from_disk(registry: &Registry, capsule_dir: &Path) -> std::io::Result<usize> {
    let manifests_dir = capsule_dir.join("manifests");
    if !manifests_dir.exists() {
        std::fs::create_dir(&manifests_dir)?;
        info!(path = %manifests_dir.display(), "created manifests directory");
        return Ok(0);
    }

    let mut loaded = 0;
    for entry in std::fs::read_dir(&manifests_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match load_manifest_file(&path) {
            Ok(manifest) => {
                debug!(capsule_id = %manifest.capsule_id, path = %path.display(), "loaded manifest");
                registry.insert(manifest);
                loaded += 1;
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "skipping unparseable manifest");
            }
        }
    }
    info!(count = loaded, path = %manifests_dir.display(), "loaded manifests");
    Ok(loaded)
}

pub fn load_manifest_file(path: &Path) -> Result<Manifest, ManifestLoadError> {
    let bytes = std::fs::read(path).map_err(ManifestLoadError::Io)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(ManifestLoadError::Parse)?;

    // Defense-in-depth: the filename must match the capsule_id inside.
    // Prevents a manifest with id "cap_a" being served at path "cap_b.json".
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(ManifestLoadError::BadFilename)?;
    if stem != manifest.capsule_id.as_str() {
        return Err(ManifestLoadError::FilenameMismatch {
            expected: manifest.capsule_id.as_str().to_string(),
            found: stem.to_string(),
        });
    }
    Ok(manifest)
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("io: {0}")]
    Io(std::io::Error),
    #[error("parse: {0}")]
    Parse(serde_json::Error),
    #[error("manifest filename is not valid UTF-8")]
    BadFilename,
    #[error("manifest filename `{found}` does not match capsule_id `{expected}`")]
    FilenameMismatch { expected: String, found: String },
}

/// Helper for `path → CapsuleId` derived from the filename stem.
pub fn capsule_id_from_path(path: &Path) -> Option<CapsuleId> {
    let stem = path.file_stem()?.to_str()?;
    CapsuleId::new(stem).ok()
}

/// Path where a manifest with this id should live.
pub fn manifest_path(capsule_dir: &Path, id: &CapsuleId) -> PathBuf {
    capsule_dir
        .join("manifests")
        .join(format!("{}.json", id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{CapsuleStatus, ComputationClass};

    fn sample_manifest(id: &str) -> Manifest {
        Manifest {
            capsule_id: CapsuleId::new(id).unwrap(),
            schema: "capsule://test".into(),
            status: CapsuleStatus::Active,
            floor_price: "0.01".into(),
            computation_classes: vec![ComputationClass::A],
            tags: vec![],
        }
    }

    #[test]
    fn registry_insert_get_list() {
        let reg = Registry::new();
        assert!(reg.is_empty());
        reg.insert(sample_manifest("cap_a1"));
        reg.insert(sample_manifest("cap_b2"));
        assert_eq!(reg.len(), 2);
        let got = reg.get(&CapsuleId::new("cap_a1").unwrap()).unwrap();
        assert_eq!(got.capsule_id.as_str(), "cap_a1");
        let listed = reg.list();
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn registry_remove() {
        let reg = Registry::new();
        reg.insert(sample_manifest("cap_a1"));
        let removed = reg.remove(&CapsuleId::new("cap_a1").unwrap());
        assert!(removed.is_some());
        assert!(reg.is_empty());
    }

    #[test]
    fn load_from_disk_handles_missing_manifests_dir() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        std::fs::create_dir(&capsule_dir).unwrap();
        let reg = Registry::new();
        let count = load_from_disk(&reg, &capsule_dir).unwrap();
        assert_eq!(count, 0);
        assert!(capsule_dir.join("manifests").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_disk_skips_unparseable() {
        let tmp = tempdir();
        let capsule_dir = tmp.join(".capsule");
        let manifests = capsule_dir.join("manifests");
        std::fs::create_dir_all(&manifests).unwrap();

        let good = sample_manifest("cap_good");
        std::fs::write(
            manifests.join("cap_good.json"),
            serde_json::to_string(&good).unwrap(),
        )
        .unwrap();
        std::fs::write(manifests.join("cap_bad.json"), "{ not json").unwrap();
        std::fs::write(manifests.join("ignored.txt"), "not a manifest").unwrap();

        let reg = Registry::new();
        let count = load_from_disk(&reg, &capsule_dir).unwrap();
        assert_eq!(count, 1);
        assert_eq!(reg.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_manifest_rejects_filename_mismatch() {
        let tmp = tempdir();
        let manifests = tmp.join("manifests");
        std::fs::create_dir_all(&manifests).unwrap();
        let path = manifests.join("cap_wrong.json");
        let manifest = sample_manifest("cap_right");
        std::fs::write(&path, serde_json::to_string(&manifest).unwrap()).unwrap();
        let err = load_manifest_file(&path).unwrap_err();
        assert!(matches!(err, ManifestLoadError::FilenameMismatch { .. }));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn tempdir() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let p =
            std::env::temp_dir().join(format!("capsuled-reg-{}-{}", std::process::id(), suffix));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
