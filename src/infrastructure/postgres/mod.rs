//! PostgreSQL infrastructure: connection pool and migrations (step 014).
//! Repository implementations follow in step 015.

use std::time::Duration;

use anyhow::Context;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::config::DatabaseSettings;

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
