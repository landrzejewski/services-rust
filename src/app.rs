//! Composition root – the only place that knows all concrete types and wires them together.
//!
//! Dependency injection in Rust needs no framework: build objects bottom-up and pass them to
//! constructors. The compiler checks that the graph is complete – there is no runtime lookup
//! that could fail with "bean not found".

use std::sync::Arc;

use anyhow::Context;
use axum::{Router, extract::FromRef};
use sqlx::PgPool;

use crate::{
    api,
    config::Settings,
    domain::{
        booking_policy::BookingPolicy,
        booking_service::BookingService,
        clock::{Clock, SystemClock},
        repositories::{BookingRepository, RoomRepository},
        room_service::RoomService,
        transaction::TxManager,
    },
    infrastructure::postgres::{
        self, PostgresBookingRepository, PostgresRoomRepository, PostgresTxManager,
    },
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
    /// Connection pool – used directly only by infrastructure concerns (readiness probe);
    /// business code accesses the database through repositories (step 015).
    pub db: PgPool,
}

/// Builds the object graph: database pool -> repositories -> services -> state.
///
/// `async` + `anyhow::Result` since step 014: connecting to the database can fail at startup.
/// `anyhow` is a good fit here – the caller only reports the error and exits, it doesn't
/// need to `match` on error variants.
pub async fn build_state(settings: &Settings) -> anyhow::Result<AppState> {
    let db = postgres::connect(&settings.database).await?;
    if settings.database.run_migrations {
        postgres::run_migrations(&db).await?;
    }

    // The concrete implementation is chosen here and only here. Switching to PostgreSQL
    // (step 015) changed only these lines – services and handlers stayed untouched.
    // Type annotation `Arc<dyn Trait>` performs the *unsizing coercion* from the concrete type.
    // In-memory repositories remain available for tests.
    let room_repository: Arc<dyn RoomRepository> =
        Arc::new(PostgresRoomRepository::new(db.clone()));
    let booking_repository: Arc<dyn BookingRepository> =
        Arc::new(PostgresBookingRepository::new(db.clone()));
    // One transaction manager for all services (step 016). It must match the repositories'
    // storage – `PostgresTxManager` with `Postgres*Repository` – which `dyn` can't enforce at
    // compile time (a mismatch fails at runtime with `RepositoryError::Unexpected`).
    let tx_manager: Arc<dyn TxManager> = Arc::new(PostgresTxManager::new(db.clone()));

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    // Configuration -> domain value object; the domain never reads `Settings` directly.
    let policy = BookingPolicy {
        max_active_bookings_per_user: settings.booking.max_active_bookings_per_user,
        max_duration: chrono::TimeDelta::minutes(settings.booking.max_duration_minutes),
    };

    // `Arc::clone(&x)` (same as `x.clone()`) – a second owner of the same repository instance.
    let room_service = Arc::new(RoomService::new(
        Arc::clone(&room_repository),
        Arc::clone(&booking_repository),
        Arc::clone(&tx_manager),
        Arc::clone(&clock),
    ));
    let booking_service = Arc::new(BookingService::new(
        room_repository,
        booking_repository,
        tx_manager,
        clock,
        policy,
    ));

    Ok(AppState {
        room_service,
        booking_service,
        db,
    })
}

/// Builds the router with middleware for the given state.
pub fn build_router(state: AppState, settings: &Settings) -> Router {
    // Middleware wraps the complete router (all routes + fallbacks), step 011.
    api::middleware::apply(api::router(state), &settings.http)
}

/// Convenience used by `server::run`: state + router, with context for startup errors.
pub async fn build(settings: &Settings) -> anyhow::Result<(AppState, Router)> {
    let state = build_state(settings)
        .await
        .context("failed to initialize application state")?;
    let router = build_router(state.clone(), settings);
    Ok((state, router))
}
