//! Room Booking Service.
//!
//! Step 001 – the smallest useful Axum application:
//! - `Router`      – maps (HTTP method, path) pairs to handlers,
//! - handlers      – plain `async fn`s; their arguments are *extractors*,
//!   their return type must implement `IntoResponse`,
//! - `axum::serve` – glues a `tokio::net::TcpListener` with the router (hyper runs underneath).
//!
//! Step 002 – Tokio runtime:
//! - the runtime is built manually (instead of `#[tokio::main]`) to show its configuration,
//! - a background task is spawned next to the HTTP server,
//! - `runtime_demo` module shows `join!`, `select!`, `spawn_blocking` and a blocking anti-pattern.
//!
//! Step 003 – development environment:
//! - `.env` file loaded with `dotenvy`,
//! - project-wide lints (`Cargo.toml [lints]`), formatting (`rustfmt.toml`), toolchain pinning.
//!
//! Step 004 – HTTP server:
//! - typed, layered configuration (`config` module),
//! - structured logging with `tracing`,
//! - graceful shutdown on SIGINT/SIGTERM with a grace period and background task cancellation.

// Declares the `src/runtime_demo.rs` module. Modules are private by default;
// `main.rs` can still use their `pub` items.
mod config;
mod runtime_demo;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::config::Settings;

// In step 001 we used `#[tokio::main]`. That attribute is only syntactic sugar –
// it expands to roughly:
//
//     fn main() {
//         tokio::runtime::Builder::new_multi_thread()
//             .enable_all()
//             .build()
//             .unwrap()
//             .block_on(async { /* body of async main */ })
//     }
//
// The attribute also accepts options, e.g. `#[tokio::main(flavor = "multi_thread", worker_threads = 2)]`
// or `#[tokio::main(flavor = "current_thread")]`. Building the runtime by hand gives full control
// (thread names, stack size, blocking pool size, hooks) – useful when tuning a service.
//
// Startup order matters: env -> config -> logging -> runtime. All of these are synchronous,
// so they happen before the runtime exists (configuration even decides how the runtime is built).
fn main() {
    // Load variables from `.env` into the process environment (if the file exists).
    // Must run before anything reads env variables and before other threads are spawned.
    // Existing environment variables are NOT overwritten – real env (Docker, CI, shell) wins.
    // `.ok()` ignores the "file not found" error: in production there is usually no `.env`.
    dotenvy::dotenv().ok();

    // Fail fast: an invalid configuration should stop the process immediately with a clear message.
    let settings = Settings::load().expect("failed to load configuration");

    init_tracing();
    tracing::debug!(?settings, "configuration loaded");

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    // Number of worker threads executing async tasks. Default: number of CPU cores.
    // Set `APP_RUNTIME__WORKER_THREADS=2` to observe the blocking anti-pattern (`/demo/blocking`).
    if let Some(threads) = settings.runtime.worker_threads {
        builder.worker_threads(threads);
    }
    let runtime = builder
        // Upper limit of extra threads used by `spawn_blocking` (default 512).
        .max_blocking_threads(16)
        // Names are visible in debuggers, profilers and `top -H`.
        .thread_name("booking-worker")
        // Enables the I/O driver (sockets) and the time driver (sleep, interval, timeout).
        // Without it `TcpListener` or `tokio::time::sleep` would panic.
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");

    // `block_on` runs the future on the current (main) thread until it completes.
    // Everything spawned inside it runs on the worker threads.
    runtime.block_on(run(settings));
}

// `tracing` separates *producing* events (`info!`, `debug!`, spans) from *consuming* them.
// A subscriber decides where events go and in which format. `tracing_subscriber::fmt`
// prints human-readable lines to stdout (JSON output and more in step 023).
fn init_tracing() {
    // `EnvFilter` reads the `RUST_LOG` variable, e.g. `info,rust_services=debug,tower_http=trace`.
    // Fallback when the variable is not set: `info` for everything.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

// The async entry point of the application – exactly what `#[tokio::main] async fn main` used to contain.
async fn run(settings: Settings) {
    // A `CancellationToken` is a cheap, cloneable "stop" signal shared between tasks.
    // The shutdown handler cancels it; every background task watches it and exits cleanly.
    let shutdown = CancellationToken::new();

    // `tokio::spawn` starts an independent *task* (a lightweight, runtime-managed "green thread").
    // The task runs concurrently with the server. Spawned futures must be `Send + 'static`
    // because the scheduler may move them between worker threads.
    // The returned `JoinHandle` can be awaited to get the task's result; dropping it
    // does NOT cancel the task (use `handle.abort()` or a cancellation token for that).
    let reporter = tokio::spawn(occupancy_reporter(
        Duration::from_secs(60),
        shutdown.clone(),
    ));

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/rooms/{id}", get(room_by_id))
        // `merge` combines two routers into one – the demo routes live in their own module.
        .merge(runtime_demo::router());

    let addr = settings
        .server
        .address()
        .expect("invalid server.host / server.port");

    // Bind a TCP socket. `0.0.0.0` accepts connections on all interfaces (containers, see
    // `config/production.toml`); `127.0.0.1` (default) accepts local connections only.
    // Port `0` lets the OS pick a free port – handy in tests.
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind to {addr}: {err}"));

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
        axum::serve(listener, app).with_graceful_shutdown(shutdown_signal(shutdown.clone()));

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

    // Wait for background tasks to finish their current iteration.
    if let Err(err) = reporter.await {
        tracing::error!(error = %err, "background task failed");
    }
    tracing::info!("server stopped");
    // Returning from `run` -> `block_on` returns -> runtime is dropped -> remaining tasks are cancelled.
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

    // Notify background tasks (and the grace period timer in `run`).
    token.cancel();
}

// A periodic background job. Later in the course this could e.g. expire stale bookings.
async fn occupancy_reporter(period: Duration, shutdown: CancellationToken) {
    // `interval` yields ticks at a fixed rate; the first tick completes immediately.
    // Unlike `sleep` in a loop, it compensates for the time spent doing the work.
    let mut interval = tokio::time::interval(period);
    loop {
        // `.await` is a suspension point: while waiting for the next tick the task
        // gives the worker thread back to the scheduler, so it costs nothing.
        // `select!` lets the task react to cancellation even while it waits for the tick.
        tokio::select! {
            _ = interval.tick() => tracing::info!("occupancy report generated"),
            _ = shutdown.cancelled() => {
                tracing::info!("occupancy reporter stopped");
                return;
            }
        }
    }
}

// The simplest handler: no extractors, returns `Html<&str>`.
// Any type implementing `IntoResponse` can be returned – `&str`, `String`,
// `Html<_>`, `Json<_>`, `StatusCode`, tuples of these and many more.
async fn index() -> Html<&'static str> {
    Html("<h1>Room Booking Service</h1>")
}

// `#[derive(Serialize)]` (serde) generates code converting the struct to JSON.
// `Json<T>` as a return type serializes `T` and sets `Content-Type: application/json`.
#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
}

async fn health() -> Json<HealthStatus> {
    Json(HealthStatus { status: "UP" })
}

#[derive(Serialize)]
struct Room {
    id: u64,
    name: String,
    capacity: u32,
}

// `Path<u64>` is an *extractor*: Axum parses the `{id}` segment into `u64`
// before calling the handler. If parsing fails (e.g. `/rooms/abc`), the handler
// is not called at all and Axum responds with `400 Bad Request`.
//
// Returning `impl IntoResponse` lets different branches return different types.
// A tuple `(StatusCode, body)` overrides the default `200 OK` status.
async fn room_by_id(Path(id): Path<u64>) -> impl IntoResponse {
    // Hard-coded data for now – a repository arrives in step 005.
    if id == 1 {
        let room = Room {
            id,
            name: "Blue room".to_string(),
            capacity: 8,
        };
        (StatusCode::OK, Json(room)).into_response()
    } else {
        // `.into_response()` converts both branches to the same concrete type
        // (`Response`), which is required because `if`/`else` arms must match.
        (StatusCode::NOT_FOUND, format!("room {id} not found")).into_response()
    }
}
