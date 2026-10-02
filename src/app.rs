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
    config::{RoomRepositoryKind, Settings},
    domain::{
        booking_policy::BookingPolicy,
        booking_service::BookingService,
        clock::{Clock, SystemClock},
        repositories::{BookingRepository, BookingUnitOfWork, RoomRepository},
        room_service::RoomService,
    },
    infrastructure::postgres::{
        self, PostgresBookingRepository, PostgresBookingUnitOfWork, PostgresRoomRepository,
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
    let room_repository = room_repository(settings, &db)?;
    let booking_repository: Arc<dyn BookingRepository> =
        Arc::new(PostgresBookingRepository::new(db.clone()));
    let booking_unit_of_work: Arc<dyn BookingUnitOfWork> =
        Arc::new(PostgresBookingUnitOfWork::new(db.clone()));

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
        Arc::clone(&clock),
    ));
    let booking_service = Arc::new(BookingService::new(
        booking_repository,
        booking_unit_of_work,
        clock,
        policy,
    ));

    Ok(AppState {
        room_service,
        booking_service,
        db,
    })
}

/// Selects the `RoomRepository` implementation from configuration (step 017).
/// All three talk to the same PostgreSQL tables; the rest of the application can't tell
/// the difference – it only sees `Arc<dyn RoomRepository>`.
fn room_repository(settings: &Settings, db: &PgPool) -> anyhow::Result<Arc<dyn RoomRepository>> {
    let kind = settings.storage.room_repository;
    tracing::info!(?kind, "room repository implementation");

    // `#[cfg]` on match arms: an arm exists only when its feature is compiled in.
    // With all features enabled the last arm is unreachable – hence the `allow`.
    #[allow(unreachable_patterns)]
    let repository: Arc<dyn RoomRepository> = match kind {
        RoomRepositoryKind::Sqlx => Arc::new(PostgresRoomRepository::new(db.clone())),
        #[cfg(feature = "orm-sea")]
        RoomRepositoryKind::SeaOrm => Arc::new(
            crate::infrastructure::sea_orm::SeaOrmRoomRepository::new(db.clone()),
        ),
        #[cfg(feature = "orm-diesel")]
        RoomRepositoryKind::Diesel => Arc::new(
            crate::infrastructure::diesel::DieselRoomRepository::connect(&settings.database)?,
        ),
        other => anyhow::bail!(
            "room repository {other:?} is not compiled in – rebuild with `--features orm-sea` / `orm-diesel`"
        ),
    };
    Ok(repository)
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
