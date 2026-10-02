//! API layer: HTTP routing and handlers.
//!
//! Handlers are thin adapters: extract input from the request, call a domain service,
//! convert the result into an HTTP response. No business rules here.
//!
//! URL structure (step 006):
//! ```text
//! /health                         technical endpoint, not versioned
//! /api/v1/rooms                   GET (list, filters), POST (create)
//! /api/v1/rooms/{id}              GET, PUT, DELETE
//! /api/v1/rooms/{id}/bookings     GET – bookings of one room (sub-resource)
//! /api/v1/bookings                GET (list, filters), POST (create)
//! /api/v1/bookings/{id}           GET
//! /api/v1/bookings/{id}/cancel    POST – state transition (action)
//! ```

mod bookings;
pub mod dto;
pub mod error;
pub mod extractors;
mod health;
pub mod problem;
mod rooms;
pub mod serde_formats;

use axum::{Router, http::Uri};

use crate::{api::error::ApiError, app::AppState};

/// Assembles routers of all API areas and attaches the shared state.
pub fn router(state: AppState) -> Router {
    // Routers of one API version merged together...
    let api_v1 = Router::new()
        .merge(rooms::router())
        .merge(bookings::router());

    Router::new()
        .merge(health::router())
        // ...and mounted under a common prefix. `nest` strips the prefix before routing,
        // so `rooms::router()` defines `/rooms`, which becomes reachable at `/api/v1/rooms`.
        // A new incompatible API version can be added as `nest("/api/v2", ...)` side by side.
        .nest("/api/v1", api_v1)
        // Called when no route matches the path (default: empty 404).
        .fallback(not_found)
        // Called when the path matches but the method does not (default: empty 405).
        .method_not_allowed_fallback(method_not_allowed)
        // `with_state` provides the state to every handler that asks for `State<AppState>`.
        // Before this call the type is `Router<AppState>` ("router still missing AppState");
        // after it – `Router<()>`, which is what `axum::serve` accepts.
        .with_state(state)
}

// `Uri` is an extractor too – the full request URI.
// Fallbacks return `ApiError` like any handler, so 404/405 use the Problem Details format as well.
async fn not_found(uri: Uri) -> ApiError {
    ApiError::RouteNotFound(uri.path().to_string())
}

async fn method_not_allowed() -> ApiError {
    ApiError::MethodNotAllowed
}
