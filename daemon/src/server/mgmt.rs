use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use super::AppState;
use crate::manifest::{CapsuleStatus, ComputationClass};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/capsules", get(list_capsules))
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
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        running: true,
        uptime_seconds: state.uptime_seconds(),
        vault_path: state.vault_path().display().to_string(),
        version: state.version(),
        capsule_count: state.registry().len(),
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
