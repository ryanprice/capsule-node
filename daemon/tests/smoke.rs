use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use capsuled::registry::Registry;
use capsuled::server::{AppState, KeyringSlot};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

async fn bind_ephemeral_on(ip: [u8; 4]) -> (TcpListener, SocketAddr) {
    let addr = SocketAddr::from((ip, 0));
    let listener = TcpListener::bind(addr).await.expect("bind");
    let bound = listener.local_addr().expect("local_addr");
    (listener, bound)
}

#[tokio::test]
async fn endpoints_respond_on_both_surfaces() {
    let tempdir = tempdir();
    let capsule_dir = tempdir.join(".capsule");
    std::fs::create_dir_all(&capsule_dir).unwrap();
    let state = AppState::new(
        tempdir.clone(),
        capsule_dir,
        Registry::new(),
        KeyringSlot::None,
        None, // auto-lock disabled in the smoke test
    );

    let (mgmt_listener, mgmt_addr) = bind_ephemeral_on([127, 0, 0, 1]).await;
    let (public_listener, public_addr) = bind_ephemeral_on([127, 0, 0, 1]).await;

    assert!(mgmt_addr.ip().is_loopback(), "mgmt must bind loopback");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };

    let server = tokio::spawn(capsuled::serve(
        mgmt_listener,
        public_listener,
        state,
        shutdown,
    ));

    // Give the servers a tick to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();

    let status: serde_json::Value = client
        .get(format!("http://{}/api/v1/status", mgmt_addr))
        .send()
        .await
        .expect("status request")
        .json()
        .await
        .expect("status json");
    assert_eq!(status["running"], true);
    assert!(status["uptime_seconds"].is_u64());
    assert!(status["vault_path"].is_string());
    assert!(status["version"].is_string());

    let info: serde_json::Value = client
        .get(format!("http://{}/v1/node/info", public_addr))
        .send()
        .await
        .expect("node/info request")
        .json()
        .await
        .expect("node/info json");
    assert!(info["did"].is_null());
    assert!(info["tier"].is_null());
    assert!(info["supported_schemas"].is_array());
    assert_eq!(info["supported_schemas"].as_array().unwrap().len(), 0);

    let _ = shutdown_tx.send(());
    let _ = server.await;

    // Best-effort cleanup; a leftover temp dir is not fatal.
    let _ = std::fs::remove_dir_all(&tempdir);
}

#[tokio::test]
async fn prepare_vault_creates_capsule_dir() {
    let vault = tempdir();
    let capsule_dir = capsuled::prepare_vault(&vault).expect("prepare_vault");
    assert!(capsule_dir.is_dir());
    assert_eq!(capsule_dir, vault.join(".capsule"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&capsule_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "expected 0700 perms on .capsule");
    }

    let _ = std::fs::remove_dir_all(&vault);
}

#[cfg(unix)]
#[tokio::test]
async fn prepare_vault_tightens_loose_perms_on_existing_dir() {
    use std::os::unix::fs::PermissionsExt;
    let vault = tempdir();
    let capsule_dir = vault.join(".capsule");
    std::fs::create_dir(&capsule_dir).unwrap();
    std::fs::set_permissions(&capsule_dir, std::fs::Permissions::from_mode(0o775)).unwrap();
    assert_eq!(
        std::fs::metadata(&capsule_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o775
    );

    let returned = capsuled::prepare_vault(&vault).expect("prepare_vault");
    assert_eq!(returned, capsule_dir);
    let mode = std::fs::metadata(&capsule_dir)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o700,
        "expected daemon to tighten loose .capsule perms"
    );

    let _ = std::fs::remove_dir_all(&vault);
}

#[tokio::test]
async fn prepare_vault_rejects_missing_path() {
    let missing = PathBuf::from("/tmp/capsuled-definitely-missing-xyz");
    let result = capsuled::prepare_vault(&missing);
    assert!(result.is_err());
}

fn tempdir() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "capsuled-test-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&base).expect("tempdir");
    base
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// Silence unused-import warning on non-Unix.
#[allow(dead_code)]
fn _loopback_marker(addr: SocketAddr) -> bool {
    addr.ip() == std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
}
