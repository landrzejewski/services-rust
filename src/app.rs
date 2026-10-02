//! Composition root – the only place that knows all concrete types and wires them together.

use std::sync::Arc;

use axum::Router;

use crate::{
    api,
    config::Settings,
    domain::{booking_service::BookingService, room_service::RoomService},
    infrastructure::memory::{InMemoryBookingRepository, InMemoryRoomRepository},
};

// Application state shared by all handlers.
//
// Axum clones the state for every request, so cloning must be cheap:
// keep heavy objects behind `Arc` (atomic reference counting – clone = counter increment).
// `Clone` derive is required by `Router::with_state`.
#[derive(Clone)]
pub struct AppState {
    pub room_service: Arc<RoomService>,
    pub booking_service: Arc<BookingService>,
}

/// Builds the object graph (repositories -> services -> state) and the router.
pub fn build_router(_settings: &Settings) -> Router {
    let room_repository = Arc::new(InMemoryRoomRepository::with_sample_data());
    let booking_repository = Arc::new(InMemoryBookingRepository::new());

    // `Arc::clone(&x)` (same as `x.clone()`) – a second owner of the same repository instance.
    let room_service = Arc::new(RoomService::new(Arc::clone(&room_repository)));
    let booking_service = Arc::new(BookingService::new(booking_repository, room_repository));

    let state = AppState {
        room_service,
        booking_service,
    };

    api::router(state)
}
