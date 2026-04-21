use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/node/info", get(node_info))
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
}

async fn node_info(State(state): State<AppState>) -> Json<NodeInfo> {
    Json(NodeInfo {
        did: None,
        tier: None,
        version: state.version(),
        supported_schemas: Vec::new(),
    })
}
