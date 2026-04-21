use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

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
}

impl AppState {
    pub fn new(vault_path: PathBuf, capsule_dir: PathBuf, registry: Registry) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                start_time: Instant::now(),
                vault_path,
                capsule_dir,
                version: env!("CARGO_PKG_VERSION"),
                registry,
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
}
