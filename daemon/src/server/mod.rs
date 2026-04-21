use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

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
    version: &'static str,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                start_time: Instant::now(),
                vault_path,
                version: env!("CARGO_PKG_VERSION"),
            }),
        }
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.inner.start_time.elapsed().as_secs()
    }

    pub fn vault_path(&self) -> &PathBuf {
        &self.inner.vault_path
    }

    pub fn version(&self) -> &'static str {
        self.inner.version
    }
}
