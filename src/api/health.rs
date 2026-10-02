//! Health endpoints for orchestrators (Docker, Kubernetes) and load balancers.
//!
//! - liveness  `/health/live`  – the process runs and serves HTTP. Failing -> restart the container.
//! - readiness `/health/ready` – dependencies (database) work. Failing -> stop routing traffic
//!   to this instance, but don't restart it (the database may come back).

use std::time::Duration;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;
use sqlx::PgPool;

use crate::{app::AppState, infrastructure::postgres};

// The router type parameter says which state its handlers need. It must have the same type as
// the routers it is merged with.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(live))
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
}

// `#[derive(Serialize)]` (serde) generates code converting the struct to JSON.
// `Json<T>` as a return type serializes `T` and sets `Content-Type: application/json`.
#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    database: Option<&'static str>,
}

async fn live() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "UP",
        database: None,
    })
}

// `State<PgPool>` – sub-state from `AppState` via `FromRef` (step 012).
async fn ready(State(db): State<PgPool>) -> (StatusCode, Json<HealthStatus>) {
    // A probe must answer quickly even when the database hangs.
    let check = tokio::time::timeout(Duration::from_secs(2), postgres::ping(&db)).await;
    match check {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(HealthStatus {
                status: "UP",
                database: Some("UP"),
            }),
        ),
        // 503 tells load balancers/orchestrators "not ready" – standard for readiness probes.
        Ok(Err(error)) => {
            tracing::warn!(%error, "readiness check failed");
            not_ready()
        }
        Err(_) => {
            tracing::warn!("readiness check timed out");
            not_ready()
        }
    }
}

fn not_ready() -> (StatusCode, Json<HealthStatus>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(HealthStatus {
            status: "DOWN",
            database: Some("DOWN"),
        }),
    )
}
