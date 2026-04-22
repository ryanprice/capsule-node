use std::net::{Ipv4Addr, SocketAddr};

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use capsuled::config::Config;
use capsuled::error::DaemonError;
use capsuled::keyring;
use capsuled::registry::{self, Registry};
use capsuled::server::{AppState, KeyringSlot};
use capsuled::watcher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let config = Config::parse();
    init_tracing(&config.log_level);

    info!(
        version = env!("CARGO_PKG_VERSION"),
        vault = %config.vault_path.display(),
        "capsuled starting"
    );

    let capsule_dir = capsuled::prepare_vault(&config.vault_path)?;
    info!(path = %capsule_dir.display(), "vault ready");

    let capsule_registry = Registry::new();
    registry::load_from_disk(&capsule_registry, &capsule_dir)
        .context("loading manifests from disk")?;
    let _watcher_handle = watcher::spawn(&capsule_dir, capsule_registry.clone())
        .context("starting filesystem watcher")?;

    let mgmt_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.daemon_port));
    let public_addr = SocketAddr::from(([0, 0, 0, 0], config.external_port));

    let mgmt_listener = TcpListener::bind(mgmt_addr)
        .await
        .map_err(|source| DaemonError::Bind {
            label: "management",
            addr: mgmt_addr,
            source,
        })?;
    let public_listener =
        TcpListener::bind(public_addr)
            .await
            .map_err(|source| DaemonError::Bind {
                label: "public",
                addr: public_addr,
                source,
            })?;

    info!(addr = %mgmt_addr, "management API listening (loopback only)");
    info!(addr = %public_addr, "public API listening");

    let keyring_slot = load_keyring_slot(&capsule_dir);
    info!(state = keyring_slot.status_label(), "keyring initialized");

    let state = AppState::new(
        config.vault_path.clone(),
        capsule_dir.clone(),
        capsule_registry,
        keyring_slot,
    );

    capsuled::serve(mgmt_listener, public_listener, state, shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

fn init_tracing(default_level: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}

/// On boot, inspect `.capsule/identity/keyring.enc`. If present, load it
/// into a Locked slot; if absent, start in None and let the operator run
/// `POST /api/v1/keyring/init` to create one. A corrupt or version-
/// mismatched file logs a warning and falls back to None rather than
/// aborting startup — the daemon must still be able to serve the mgmt
/// endpoints so the operator can reset identity.
fn load_keyring_slot(capsule_dir: &std::path::Path) -> KeyringSlot {
    let path = keyring::keyring_path(capsule_dir);
    if !path.exists() {
        return KeyringSlot::None;
    }
    match keyring::load(&path) {
        Ok(locked) => KeyringSlot::Locked(locked),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "existing keyring file could not be loaded; starting in None state"
            );
            KeyringSlot::None
        }
    }
}
