use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use zeroize::Zeroizing;

use super::{AppState, KeyringSlot};
use crate::keyring::{self, KeyringError};
use crate::manifest::{CapsuleStatus, ComputationClass};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/capsules", get(list_capsules))
        .route("/api/v1/keyring/status", get(keyring_status))
        .route("/api/v1/keyring/init", post(keyring_init))
        .route("/api/v1/keyring/unlock", post(keyring_unlock))
        .route("/api/v1/keyring/lock", post(keyring_lock))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    uptime_seconds: u64,
    vault_path: String,
    version: &'static str,
    capsule_count: usize,
    keyring: &'static str,
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let keyring_label = state
        .keyring()
        .read()
        .map(|slot| slot.status_label())
        .unwrap_or("unknown");
    Json(StatusResponse {
        running: true,
        uptime_seconds: state.uptime_seconds(),
        vault_path: state.vault_path().display().to_string(),
        version: state.version(),
        capsule_count: state.registry().len(),
        keyring: keyring_label,
    })
}

/// Mgmt-side listing exposes every capsule regardless of status, including
/// paused and draft — the owner needs visibility into their full set.
#[derive(Serialize)]
struct MgmtCapsuleView {
    capsule_id: String,
    schema: String,
    status: CapsuleStatus,
    floor_price: String,
    computation_classes: Vec<ComputationClass>,
    tags: Vec<String>,
}

async fn list_capsules(State(state): State<AppState>) -> Json<Vec<MgmtCapsuleView>> {
    let mut items: Vec<MgmtCapsuleView> = state
        .registry()
        .list()
        .into_iter()
        .map(|m| MgmtCapsuleView {
            capsule_id: m.capsule_id.as_str().to_string(),
            schema: m.schema,
            status: m.status,
            floor_price: m.floor_price,
            computation_classes: m.computation_classes,
            tags: m.tags,
        })
        .collect();
    items.sort_by(|a, b| a.capsule_id.cmp(&b.capsule_id));
    Json(items)
}

// ─── Keyring endpoints ──────────────────────────────────────────────────────
//
// Security notes for every handler below:
//   * The incoming passphrase is taken out of the parsed JSON into a
//     Zeroizing<Vec<u8>> so the bytes are zeroed on handler exit, not left
//     in the Axum/Serde heap allocator's reuse pool.
//   * Error responses never echo the passphrase or leak the distinction
//     between "wrong passphrase" and "corrupted ciphertext" — KeyringError
//     already collapses both to BadPassphrase.
//   * tower_http::trace::TraceLayer logs method + path + status + latency,
//     not request bodies. Do not add body logging here.

#[derive(Deserialize)]
struct KeyringPassphraseBody {
    passphrase: String,
}

#[derive(Serialize)]
struct KeyringStatus {
    status: &'static str,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl ErrorBody {
    fn new(msg: impl Into<String>) -> Self {
        Self { error: msg.into() }
    }
}

async fn keyring_status(State(state): State<AppState>) -> Json<KeyringStatus> {
    let slot = state.keyring().read().expect("keyring lock poisoned");
    Json(KeyringStatus {
        status: slot.status_label(),
    })
}

async fn keyring_init(
    State(state): State<AppState>,
    Json(body): Json<KeyringPassphraseBody>,
) -> impl IntoResponse {
    let path = keyring::keyring_path(state.capsule_dir());
    let mut slot = state.keyring().write().expect("keyring lock poisoned");
    if !matches!(*slot, KeyringSlot::None) {
        return err_response(
            StatusCode::CONFLICT,
            "keyring already exists; delete keyring.enc to reset identity",
        );
    }
    let passphrase = Zeroizing::new(body.passphrase.into_bytes());
    match keyring::create(&path, &passphrase) {
        Ok(unlocked) => {
            *slot = KeyringSlot::Unlocked(unlocked);
            status_response("unlocked")
        }
        Err(e) => keyring_error_response(e),
    }
}

async fn keyring_unlock(
    State(state): State<AppState>,
    Json(body): Json<KeyringPassphraseBody>,
) -> impl IntoResponse {
    let mut slot = state.keyring().write().expect("keyring lock poisoned");
    let passphrase = Zeroizing::new(body.passphrase.into_bytes());
    match &*slot {
        KeyringSlot::Unlocked(_) => status_response("unlocked"),
        KeyringSlot::None => err_response(
            StatusCode::BAD_REQUEST,
            "no keyring exists; call POST /api/v1/keyring/init first",
        ),
        KeyringSlot::Locked(locked) => match locked.unlock(&passphrase) {
            Ok(unlocked) => {
                *slot = KeyringSlot::Unlocked(unlocked);
                status_response("unlocked")
            }
            Err(e) => keyring_error_response(e),
        },
    }
}

async fn keyring_lock(State(state): State<AppState>) -> impl IntoResponse {
    let path = keyring::keyring_path(state.capsule_dir());
    let mut slot = state.keyring().write().expect("keyring lock poisoned");
    match &*slot {
        KeyringSlot::Unlocked(_) => match keyring::load(&path) {
            Ok(locked) => {
                *slot = KeyringSlot::Locked(locked);
                status_response("locked")
            }
            Err(e) => keyring_error_response(e),
        },
        KeyringSlot::Locked(_) => status_response("locked"),
        KeyringSlot::None => err_response(StatusCode::BAD_REQUEST, "no keyring exists to lock"),
    }
}

fn status_response(label: &'static str) -> axum::response::Response {
    (StatusCode::OK, Json(KeyringStatus { status: label })).into_response()
}

fn err_response(code: StatusCode, msg: &'static str) -> axum::response::Response {
    (code, Json(ErrorBody::new(msg))).into_response()
}

/// Translate a KeyringError into an HTTP response. Deliberately generic
/// about why decryption failed: a caller-facing "wrong passphrase vs.
/// tampered file" distinction is an oracle we don't want to offer.
fn keyring_error_response(e: KeyringError) -> axum::response::Response {
    match e {
        KeyringError::BadPassphrase => err_response(
            StatusCode::UNAUTHORIZED,
            "bad passphrase or corrupted keyring",
        ),
        KeyringError::EmptyPassphrase => {
            err_response(StatusCode::BAD_REQUEST, "passphrase must not be empty")
        }
        KeyringError::AlreadyExists(_) => {
            err_response(StatusCode::CONFLICT, "keyring already exists on disk")
        }
        KeyringError::NotFound(_) => err_response(StatusCode::NOT_FOUND, "no keyring file on disk"),
        KeyringError::Mlock(_) => err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cannot mlock key material; check RLIMIT_MEMLOCK",
        ),
        KeyringError::BadMagic
        | KeyringError::UnsupportedVersion(_)
        | KeyringError::UnsupportedKdf(_, _)
        | KeyringError::TooShort { .. } => err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "keyring file is corrupted or from an unsupported version",
        ),
        KeyringError::Io(_)
        | KeyringError::KdfParams(_)
        | KeyringError::KdfRun(_)
        | KeyringError::WalletDerive(_) => {
            tracing::error!(error = %e, "keyring internal error");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal keyring error")
        }
    }
}
