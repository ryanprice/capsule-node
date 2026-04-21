use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("vault path does not exist: {0}")]
    VaultMissing(PathBuf),

    #[error("vault path is not a directory: {0}")]
    VaultNotDirectory(PathBuf),

    #[error("cannot write to vault: {0}")]
    VaultNotWritable(PathBuf),

    #[error("failed to create .capsule directory at {path}: {source}")]
    CapsuleDirCreate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to bind {label} server to {addr}: {source}")]
    Bind {
        label: &'static str,
        addr: std::net::SocketAddr,
        #[source]
        source: std::io::Error,
    },
}
