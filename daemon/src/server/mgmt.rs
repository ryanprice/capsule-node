use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use super::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    uptime_seconds: u64,
    vault_path: String,
    version: &'static str,
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        running: true,
        uptime_seconds: state.uptime_seconds(),
        vault_path: state.vault_path().display().to_string(),
        version: state.version(),
    })
}
