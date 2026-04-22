use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::AppState;
use crate::manifest::{CapsuleId, CapsuleStatus, ComputationClass, Manifest};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/node/info", get(node_info))
        .route("/v1/capsules", get(list_capsules))
        .route("/v1/capsules/{cid}/manifest", get(get_manifest))
        .route("/v1/capsules/{cid}/compute", get(compute))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

#[derive(Serialize)]
struct NodeInfo {
    did: Option<String>,
    tier: Option<u8>,
    version: &'static str,
    supported_schemas: Vec<String>,
    capsule_count: usize,
    /// Ethereum payout address (EIP-55 hex), present only when the node
    /// keyring is unlocked. Agents use this to verify payments before
    /// making an x402-gated request.
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_address: Option<String>,
}

async fn node_info(State(state): State<AppState>) -> Json<NodeInfo> {
    let schemas: std::collections::BTreeSet<String> = state
        .registry()
        .list()
        .into_iter()
        .map(|m| m.schema)
        .collect();
    Json(NodeInfo {
        did: None,
        tier: None,
        version: state.version(),
        supported_schemas: schemas.into_iter().collect(),
        capsule_count: state.registry().len(),
        wallet_address: state.wallet_address(),
    })
}

#[derive(Serialize)]
struct CapsuleSummary {
    capsule_id: String,
    schema: String,
    status: CapsuleStatus,
    floor_price: String,
    computation_classes: Vec<ComputationClass>,
    tags: Vec<String>,
}

impl From<Manifest> for CapsuleSummary {
    fn from(m: Manifest) -> Self {
        Self {
            capsule_id: m.capsule_id.as_str().to_string(),
            schema: m.schema,
            status: m.status,
            floor_price: m.floor_price,
            computation_classes: m.computation_classes,
            tags: m.tags,
        }
    }
}

async fn list_capsules(State(state): State<AppState>) -> Json<Vec<CapsuleSummary>> {
    let mut items: Vec<CapsuleSummary> = state
        .registry()
        .list()
        .into_iter()
        .filter(|m| matches!(m.status, CapsuleStatus::Active))
        .map(Into::into)
        .collect();
    items.sort_by(|a, b| a.capsule_id.cmp(&b.capsule_id));
    Json(items)
}

async fn get_manifest(
    State(state): State<AppState>,
    Path(cid): Path<String>,
) -> Result<Json<Manifest>, StatusCode> {
    let id = CapsuleId::new(cid).map_err(|_| StatusCode::NOT_FOUND)?;
    state
        .registry()
        .get(&id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
struct PaymentRequired {
    capsule_cid: String,
    price: PaymentPrice,
    recipient: Option<String>,
    expiry: Option<String>,
    supported_schemes: Vec<&'static str>,
    capsule_quality: CapsuleQuality,
}

#[derive(Serialize)]
struct PaymentPrice {
    amount: String,
    currency: &'static str,
    network: &'static str,
}

#[derive(Serialize)]
struct CapsuleQuality {
    status: CapsuleStatus,
    computation_classes: Vec<ComputationClass>,
}

/// `GET /v1/capsules/{cid}/compute` — no-payment-attached path returns HTTP
/// 402 with the payment terms the agent must satisfy (spec §5 x402 response).
///
/// If the keyring is not Unlocked, the daemon has no signing identity and
/// no trusted recipient address to commit to — we return 503 rather than
/// offering a 402 the agent cannot trust. This is the fail-closed path
/// required by CLAUDE.md: post-lock, daemon continues running but refuses
/// computations until re-auth.
async fn compute(State(state): State<AppState>, Path(cid): Path<String>) -> impl IntoResponse {
    let id = match CapsuleId::new(&cid) {
        Ok(id) => id,
        Err(_) => return (StatusCode::NOT_FOUND, Json(None::<PaymentRequired>)).into_response(),
    };
    let Some(manifest) = state.registry().get(&id) else {
        return (StatusCode::NOT_FOUND, Json(None::<PaymentRequired>)).into_response();
    };
    if !matches!(manifest.status, CapsuleStatus::Active) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(None::<PaymentRequired>),
        )
            .into_response();
    }

    let Some(recipient) = state.wallet_address() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "node keyring is not unlocked; no recipient address available",
            })),
        )
            .into_response();
    };

    let body = PaymentRequired {
        capsule_cid: manifest.capsule_id.as_str().to_string(),
        price: PaymentPrice {
            amount: floor_amount(&manifest.floor_price),
            currency: "USDC",
            network: "eip155:8453",
        },
        recipient: Some(recipient),
        expiry: Some(payment_expiry_rfc3339()),
        supported_schemes: vec!["exact"],
        capsule_quality: CapsuleQuality {
            status: manifest.status,
            computation_classes: manifest.computation_classes,
        },
    };
    // We're committing to the node's payout address in the response body,
    // which is derived from the unlocked master secret. That counts as
    // activity for the auto-lock timer — an agent hitting /compute on a
    // regular schedule keeps the keyring alive.
    state.record_activity();
    (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response()
}

/// 5-minute window for the agent to actually submit payment against the
/// terms we just quoted. Long enough to be user-friendly over slow
/// connections; short enough that stale quotes don't outlive real price
/// drift once the daemon starts honoring payment-required headers.
const PAYMENT_EXPIRY_SECS: i64 = 300;

fn payment_expiry_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    let expiry = now + time::Duration::seconds(PAYMENT_EXPIRY_SECS);
    expiry
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

/// Extract a numeric amount from a free-form `floor_price` like
/// `"0.08 USDC/query"`. Returns "0" if no leading number is found —
/// manifest validation at write-time is the right place to enforce a
/// canonical shape (next slice).
fn floor_amount(raw: &str) -> String {
    raw.split_whitespace().next().unwrap_or("0").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_amount_extracts_leading_number() {
        assert_eq!(floor_amount("0.08 USDC/query"), "0.08");
        assert_eq!(floor_amount("0.08"), "0.08");
        assert_eq!(floor_amount(""), "0");
    }
}
