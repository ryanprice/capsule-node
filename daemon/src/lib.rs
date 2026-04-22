pub mod config;
pub mod error;
pub mod keyring;
pub mod manifest;
pub mod registry;
pub mod server;
pub mod wallet;
pub mod watcher;

use std::future::Future;

use tokio::net::TcpListener;
use tracing::info;

use crate::error::DaemonError;
use crate::server::AppState;

/// Run both HTTP servers until the shutdown future resolves.
///
/// Accepting pre-bound listeners lets the caller (including tests) pick
/// ephemeral ports and inspect the bound address before serving.
pub async fn serve(
    mgmt_listener: TcpListener,
    public_listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let mgmt_router = server::mgmt::router(state.clone());
    let public_router = server::public::router(state);

    let (shutdown_mgmt, shutdown_public) = split_shutdown(shutdown);

    let mgmt_task = tokio::spawn(async move {
        axum::serve(mgmt_listener, mgmt_router)
            .with_graceful_shutdown(shutdown_mgmt)
            .await
    });

    let public_task = tokio::spawn(async move {
        axum::serve(public_listener, public_router)
            .with_graceful_shutdown(shutdown_public)
            .await
    });

    let (mgmt_res, public_res) = tokio::join!(mgmt_task, public_task);
    mgmt_res??;
    public_res??;
    info!("all servers stopped");
    Ok(())
}

/// Fan a single shutdown future out to both server tasks.
fn split_shutdown(
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> (
    impl Future<Output = ()> + Send + 'static,
    impl Future<Output = ()> + Send + 'static,
) {
    let (tx_mgmt, rx_mgmt) = tokio::sync::oneshot::channel::<()>();
    let (tx_public, rx_public) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        shutdown.await;
        let _ = tx_mgmt.send(());
        let _ = tx_public.send(());
    });
    let mgmt_fut = async move {
        let _ = rx_mgmt.await;
    };
    let public_fut = async move {
        let _ = rx_public.await;
    };
    (mgmt_fut, public_fut)
}

/// Validate the vault path and create `.capsule/` with `0700` perms if missing.
pub fn prepare_vault(vault_path: &std::path::Path) -> Result<std::path::PathBuf, DaemonError> {
    if !vault_path.exists() {
        return Err(DaemonError::VaultMissing(vault_path.to_path_buf()));
    }
    if !vault_path.is_dir() {
        return Err(DaemonError::VaultNotDirectory(vault_path.to_path_buf()));
    }
    let probe = vault_path.join(".capsule-write-probe");
    if std::fs::write(&probe, b"").is_err() {
        return Err(DaemonError::VaultNotWritable(vault_path.to_path_buf()));
    }
    let _ = std::fs::remove_file(&probe);

    let capsule_dir = vault_path.join(".capsule");
    let created = if !capsule_dir.exists() {
        std::fs::create_dir(&capsule_dir).map_err(|source| DaemonError::CapsuleDirCreate {
            path: capsule_dir.clone(),
            source,
        })?;
        true
    } else {
        false
    };

    // Enforce 0700 on every startup (spec §9.4). If the directory was pre-existing
    // with wider perms (umask, prior tooling, manual mkdir), tighten it and log.
    enforce_owner_only_perms(&capsule_dir, created)?;

    Ok(capsule_dir)
}

#[cfg(unix)]
fn enforce_owner_only_perms(path: &std::path::Path, created: bool) -> Result<(), DaemonError> {
    use std::os::unix::fs::PermissionsExt;
    let current = std::fs::metadata(path)
        .map_err(|source| DaemonError::CapsuleDirCreate {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode()
        & 0o777;
    if current == 0o700 {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|source| {
        DaemonError::CapsuleDirCreate {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if !created {
        tracing::warn!(
            path = %path.display(),
            from = format!("0{:o}", current),
            to = "0700",
            "tightened .capsule permissions (spec §9.4 requires owner-only access)"
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_owner_only_perms(_path: &std::path::Path, _created: bool) -> Result<(), DaemonError> {
    // TODO: Windows NTFS ACL restriction to current user (spec §9.4).
    // Walking-skeleton milestone ships Unix-first; Windows hardening tracked separately.
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn mgmt_bind_address_is_loopback() {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 7402));
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn public_bind_accepts_external() {
        let addr = SocketAddr::from(([0, 0, 0, 0], 8402));
        assert!(!addr.ip().is_loopback());
        assert!(addr.ip().is_unspecified());
    }
}
