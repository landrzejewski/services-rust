//! Composition root – the only place that knows all concrete types and wires them together.
//!
//! Dependency injection in Rust needs no framework: build objects bottom-up and pass them to
//! constructors. The compiler checks that the graph is complete – there is no runtime lookup
//! that could fail with "bean not found".

use std::sync::Arc;

use axum::{Router, extract::FromRef};

use crate::{
    api,
    config::Settings,
    domain::{
        booking_service::BookingService,
        repositories::{BookingRepository, RoomRepository},
        room_service::RoomService,
    },
    infrastructure::memory::{InMemoryBookingRepository, InMemoryRoomRepository},
};

// Application state shared by all handlers.
//
// Axum clones the state for every request, so cloning must be cheap:
// keep heavy objects behind `Arc` (atomic reference counting – clone = counter increment).
// `Clone` derive is required by `Router::with_state`.
//
// `#[derive(FromRef)]` (axum `macros` feature) generates `impl FromRef<AppState> for <FieldType>`
// for every field. Handlers can then ask for a *part* of the state – `State<Arc<RoomService>>` –
// instead of the whole `AppState`. Each handler declares exactly what it depends on.
#[derive(Clone, FromRef)]
pub struct AppState {
    pub room_service: Arc<RoomService>,
    pub booking_service: Arc<BookingService>,
}

/// Builds the object graph: repositories -> services -> state.
pub fn build_state(_settings: &Settings) -> AppState {
    // The concrete implementation is chosen here and only here. Switching to PostgreSQL
    // (step 015) changes these lines – services and handlers stay untouched.
    // Type annotation `Arc<dyn Trait>` performs the *unsizing coercion* from the concrete type.
    let room_repository: Arc<dyn RoomRepository> =
        Arc::new(InMemoryRoomRepository::with_sample_data());
    let booking_repository: Arc<dyn BookingRepository> = Arc::new(InMemoryBookingRepository::new());

    // `Arc::clone(&x)` (same as `x.clone()`) – a second owner of the same repository instance.
    let room_service = Arc::new(RoomService::new(Arc::clone(&room_repository)));
    let booking_service = Arc::new(BookingService::new(booking_repository, room_repository));

    AppState {
        room_service,
        booking_service,
    }
}

/// Builds the state and the router with middleware.
pub fn build_router(settings: &Settings) -> Router {
    let state = build_state(settings);
    // Middleware wraps the complete router (all routes + fallbacks), step 011.
    api::middleware::apply(api::router(state), &settings.http)
}
