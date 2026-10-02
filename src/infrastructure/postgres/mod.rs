//! PostgreSQL infrastructure: connection pool and migrations (step 014),
//! repositories implemented with sqlx (step 015).

mod booking_repository;
mod room_repository;
mod unit_of_work;
mod user_repository;

use std::time::Duration;

use anyhow::Context;
use sqlx::{PgPool, postgres::PgPoolOptions};

pub use booking_repository::PostgresBookingRepository;
pub use room_repository::PostgresRoomRepository;
// Re-exported for the ORM-based implementations; compiled only when one of them is enabled
// (otherwise the import would be unused -> warning).
#[cfg(any(feature = "orm-sea", feature = "orm-diesel"))]
pub(crate) use room_repository::{room_from_columns, to_db_int};
pub use unit_of_work::PostgresBookingUnitOfWork;
pub use user_repository::PostgresUserRepository;

use crate::{config::DatabaseSettings, domain::repositories::RepositoryError};

/// Creates the connection pool.
///
/// A pool keeps several open connections and lends them to tasks. Opening a PostgreSQL connection
/// is expensive (TCP + TLS + auth + process fork on the server); reusing them is essential.
/// `PgPool` is internally an `Arc` – cloning is cheap, all clones share the same connections.
pub async fn connect(settings: &DatabaseSettings) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        // Upper bound of open connections. Sum over all app instances must stay below the
        // server's `max_connections` (default 100). More connections != faster.
        .max_connections(settings.max_connections)
        // Connections kept open even when idle – avoids connect latency after quiet periods.
        .min_connections(settings.min_connections)
        // How long a request waits for a free connection before failing (pool exhausted).
        .acquire_timeout(Duration::from_secs(settings.acquire_timeout_secs))
        // Close connections idle for longer than this (down to `min_connections`).
        .idle_timeout(Duration::from_secs(600))
        // Recycle connections periodically (load balancers, server-side memory growth).
        .max_lifetime(Duration::from_secs(1800))
        // `connect` opens the first connection immediately -> fail fast on wrong URL/credentials.
        // `connect_lazy` would defer it to the first query.
        .connect(&settings.url)
        .await
        // `anyhow::Context` adds a human-readable layer on top of the original error.
        .context("failed to connect to PostgreSQL")?;

    tracing::info!(
        max_connections = settings.max_connections,
        "database pool created"
    );
    Ok(pool)
}

/// Applies pending migrations from `./migrations`.
///
/// `sqlx::migrate!()` embeds the SQL files into the binary at COMPILE time – the deployed
/// binary needs no migration files. Each migration runs in a transaction; a table
/// `_sqlx_migrations` stores what was applied (with checksums).
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!()
        .run(pool)
        .await
        .context("failed to run database migrations")?;
    tracing::info!("database migrations applied");
    Ok(())
}

/// Readiness probe: can we get a connection and run a trivial query?
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

// Translation of driver errors into the domain's port error (step 015).
// Implemented here, in the adapter – the domain never sees `sqlx::Error`.
impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        // `Database` = the server rejected the statement; inspect SQLSTATE / constraint name.
        if let sqlx::Error::Database(db_error) = &error
            && db_error.is_unique_violation()
        {
            let message = match db_error.constraint() {
                Some("rooms_name_idx") => "a room with this name already exists".to_string(),
                Some("users_email_idx") => "a user with this e-mail already exists".to_string(),
                other => format!("unique constraint violated: {}", other.unwrap_or("unknown")),
            };
            return RepositoryError::Conflict(message);
        }
        // SQLSTATE 23P01 = exclusion_violation (`bookings_no_overlap`, step 016).
        // sqlx has no dedicated helper for it, so compare the code.
        if let sqlx::Error::Database(db_error) = &error
            && db_error.code().as_deref() == Some("23P01")
        {
            return RepositoryError::Conflict(
                "room is already booked in the requested period".to_string(),
            );
        }
        RepositoryError::unexpected("database operation failed", error)
    }
}

/// Escapes `%`, `_` and `\` so user input is matched literally inside a `LIKE` pattern.
/// (SQL injection is already impossible thanks to bind parameters; this is about correctness:
/// searching for "50%" must not match everything starting with "50".)
pub(crate) fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}
