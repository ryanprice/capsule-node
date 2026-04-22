use axum::{
    extract::{Path, State},
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
use crate::manifest::{CapsuleId, CapsuleStatus, ComputationClass};
use crate::payload;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/capsules", get(list_capsules))
        .route("/api/v1/keyring/status", get(keyring_status))
        .route("/api/v1/keyring/init", post(keyring_init))
        .route("/api/v1/keyring/unlock", post(keyring_unlock))
        .route("/api/v1/keyring/lock", post(keyring_lock))
        .route("/api/v1/capsules/{cid}/payload", post(publish_payload))
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
    /// EIP-55 Ethereum address. Only present when `keyring == "unlocked"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_address: Option<String>,
    /// Seconds until auto-lock fires, when keyring is unlocked and
    /// auto-lock is enabled. `None` when the timer is disabled or the
    /// keyring isn't currently unlocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_lock_seconds_remaining: Option<u64>,
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
        wallet_address: state.wallet_address(),
        auto_lock_seconds_remaining: state.auto_lock_seconds_remaining(),
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
            drop(slot);
            state.record_activity();
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
                drop(slot);
                state.record_activity();
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

// ─── Payload publishing ─────────────────────────────────────────────────────
//
// POST /api/v1/capsules/{cid}/payload accepts { records: [...] } from the
// plugin, encrypts the serialized body with the capsule's derived payload
// key, writes .capsule/payloads/{cid}.enc (0600), and returns the new
// payload_cid. Fails closed:
//   * 404 if the capsule isn't in the registry (no manifest)
//   * 503 if the keyring is not unlocked — encrypt needs the master secret
//   * 500 on I/O or crypto failures (with a generic message)

#[derive(Deserialize)]
struct PublishBody {
    records: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct PublishResponse {
    payload_cid: String,
    size: u64,
    record_count: usize,
}

async fn publish_payload(
    State(state): State<AppState>,
    Path(cid): Path<String>,
    Json(body): Json<PublishBody>,
) -> impl IntoResponse {
    let Ok(id) = CapsuleId::new(&cid) else {
        return err_response(StatusCode::NOT_FOUND, "no such capsule");
    };
    if state.registry().get(&id).is_none() {
        return err_response(StatusCode::NOT_FOUND, "no such capsule");
    }

    // Acquire the master secret under the read lock and call with_secret
    // which hands us a &[u8; 32] scoped to the closure. The secret never
    // leaves the daemon as a value.
    let slot = state.keyring().read().expect("keyring lock poisoned");
    let KeyringSlot::Unlocked(ref unlocked) = *slot else {
        return err_response(StatusCode::SERVICE_UNAVAILABLE, "keyring is not unlocked");
    };

    // Serialize the records. We wrap in `{ "records": [...] }` on disk so
    // future slices can add sibling metadata (schema hash, recorded-at
    // timestamp) without bumping the payload format version.
    let wrapper = serde_json::json!({ "records": body.records });
    let plaintext = match serde_json::to_vec(&wrapper) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize payload");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, "serialize failed");
        }
    };
    let record_count = body.records.len();

    let result = unlocked
        .with_secret(|secret| payload::write(state.capsule_dir(), id.as_str(), secret, &plaintext));
    drop(slot);

    match result {
        Ok(written) => {
            state.record_activity();
            tracing::info!(
                capsule_id = %id,
                record_count,
                payload_cid = %written.payload_cid,
                "payload published"
            );
            (
                StatusCode::OK,
                Json(PublishResponse {
                    payload_cid: written.payload_cid,
                    size: written.size,
                    record_count,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "payload write failed");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, "payload write failed")
        }
    }
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
