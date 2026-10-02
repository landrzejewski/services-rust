use axum::{Json, Router, routing::get};
use serde::Serialize;

use crate::app::AppState;

// The router type parameter says which state its handlers need. `health` needs none,
// but it must have the same type as the routers it is merged with.
pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

// `#[derive(Serialize)]` (serde) generates code converting the struct to JSON.
// `Json<T>` as a return type serializes `T` and sets `Content-Type: application/json`.
#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
}

async fn health() -> Json<HealthStatus> {
    Json(HealthStatus { status: "UP" })
}
