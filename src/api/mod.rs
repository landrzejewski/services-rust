//! API layer: HTTP routing and handlers.
//!
//! Handlers are thin adapters: extract input from the request, call a domain service,
//! convert the result into an HTTP response. No business rules here.

mod health;
mod rooms;

use axum::Router;

use crate::app::AppState;

/// Assembles routers of all API areas and attaches the shared state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(rooms::router())
        // `with_state` provides the state to every handler that asks for `State<AppState>`.
        // Before this call the type is `Router<AppState>` ("router still missing AppState");
        // after it – `Router<()>`, which is what `axum::serve` accepts.
        .with_state(state)
}
