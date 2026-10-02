//! Composition root – the only place that knows all concrete types and wires them together.

use std::sync::Arc;

use axum::Router;

use crate::{
    api, config::Settings, domain::room_service::RoomService,
    infrastructure::memory::InMemoryRoomRepository,
};

// Application state shared by all handlers.
//
// Axum clones the state for every request, so cloning must be cheap:
// keep heavy objects behind `Arc` (atomic reference counting – clone = counter increment).
// `Clone` derive is required by `Router::with_state`.
#[derive(Clone)]
pub struct AppState {
    pub room_service: Arc<RoomService>,
}

/// Builds the object graph (repositories -> services -> state) and the router.
pub fn build_router(_settings: &Settings) -> Router {
    let room_repository = Arc::new(InMemoryRoomRepository::with_sample_data());
    let room_service = Arc::new(RoomService::new(room_repository));

    let state = AppState { room_service };

    api::router(state)
}
