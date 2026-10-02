//! HTTP server lifecycle: bind, serve, graceful shutdown.

use std::time::Duration;

use anyhow::Context;
use tokio_util::sync::CancellationToken;

use crate::{app, config::Settings};

// The async entry point of the application – exactly what `#[tokio::main] async fn main` used to contain.
// Returns `anyhow::Result` since step 014: startup can fail (database unreachable, port in use).
pub async fn run(settings: Settings) -> anyhow::Result<()> {
    // A `CancellationToken` is a cheap, cloneable "stop" signal shared between tasks.
    // The shutdown handler cancels it; background tasks (if any) and the grace-period timer watch it.
    let shutdown = CancellationToken::new();

    // Composition root builds the whole object graph and returns a ready `Router`.
    let (state, router) = app::build(&settings).await?;

    let addr = settings
        .server
        .address()
        .context("invalid server.host / server.port")?;

    // Bind a TCP socket. `0.0.0.0` accepts connections on all interfaces (containers, see
    // `config/production.toml`); `127.0.0.1` (default) accepts local connections only.
    // Port `0` lets the OS pick a free port – handy in tests.
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to {addr}"))?;

    // `expect` instead of `unwrap` (denied by the `clippy::unwrap_used` lint):
    // if it ever panics, the message explains which assumption was broken.
    let local_addr = listener
        .local_addr()
        .expect("bound listener always has a local address");

    // Structured logging: `%value` records a field using `Display`, `?value` using `Debug`.
    // Fields are key-value pairs that log processors can filter on (not just text).
    tracing::info!(address = %local_addr, "server started");

    // `axum::serve` accepts connections and drives each one with hyper,
    // passing every request to the router. Each connection is handled in its own Tokio task,
    // so many requests are processed concurrently on the worker threads.
    //
    // `with_graceful_shutdown(future)`: when the future completes, the server stops accepting
    // new connections and waits until in-flight requests finish, then `serve` returns.
    let server =
        axum::serve(listener, router).with_graceful_shutdown(shutdown_signal(shutdown.clone()));

    // Graceful shutdown waits for in-flight requests without limit (a slow client could block it
    // forever). Race the server against "shutdown requested + grace period elapsed".
    let grace_period = Duration::from_secs(settings.server.shutdown_grace_period_secs);
    tokio::select! {
        result = server => {
            if let Err(err) = result {
                tracing::error!(error = %err, "server error");
            }
        }
        _ = async {
            shutdown.cancelled().await;
            tokio::time::sleep(grace_period).await;
        } => {
            tracing::warn!(?grace_period, "grace period elapsed, aborting in-flight requests");
        }
    }

    // Close the pool gracefully: waits for borrowed connections to be returned, then sends
    // a proper termination message to PostgreSQL for each connection.
    state.db.close().await;

    tracing::info!("server stopped");
    // Returning from `run` -> `block_on` returns -> runtime is dropped -> remaining tasks are cancelled.
    Ok(())
}

// Completes when the process receives Ctrl+C (SIGINT) or SIGTERM.
// SIGTERM is what Docker / Kubernetes send on `docker stop` / pod termination.
async fn shutdown_signal(token: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    // Unix signals are not available on Windows – `#[cfg]` compiles the right variant per platform.
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }

    // Notify background tasks and the grace period timer in `run`.
    token.cancel();
}
