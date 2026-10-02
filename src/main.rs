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

// Declares the `src/runtime_demo.rs` module. Modules are private by default;
// `main.rs` can still use their `pub` items.
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
fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        // Number of worker threads executing async tasks. Default: number of CPU cores.
        // Deliberately small here so the blocking anti-pattern (`/demo/blocking`) is easy to observe.
        .worker_threads(2)
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
    runtime.block_on(run());
}

// The async entry point of the application – exactly what `#[tokio::main] async fn main` used to contain.
async fn run() {
    // `tokio::spawn` starts an independent *task* (a lightweight, runtime-managed "green thread").
    // The task runs concurrently with the server. Spawned futures must be `Send + 'static`
    // because the scheduler may move them between worker threads.
    // The returned `JoinHandle` can be awaited to get the task's result; dropping it
    // does NOT cancel the task (use `handle.abort()` for that).
    let _reporter = tokio::spawn(occupancy_reporter(Duration::from_secs(60)));

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/rooms/{id}", get(room_by_id))
        // `merge` combines two routers into one – the demo routes live in their own module.
        .merge(runtime_demo::router());

    // Bind a TCP socket. `0.0.0.0` accepts connections on all interfaces
    // (needed later inside containers); use `127.0.0.1` to listen locally only.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("listening on http://{}", listener.local_addr().unwrap());

    // `axum::serve` accepts connections and drives each one with hyper,
    // passing every request to the router. Each connection is handled in its own Tokio task,
    // so many requests are processed concurrently on the worker threads.
    axum::serve(listener, app).await.expect("server error");
}

// A periodic background job. Later in the course this could e.g. expire stale bookings.
async fn occupancy_reporter(period: Duration) {
    // `interval` yields ticks at a fixed rate; the first tick completes immediately.
    // Unlike `sleep` in a loop, it compensates for the time spent doing the work.
    let mut interval = tokio::time::interval(period);
    loop {
        interval.tick().await;
        // `.await` is a suspension point: while waiting for the next tick the task
        // gives the worker thread back to the scheduler, so it costs nothing.
        println!("[reporter] occupancy report generated");
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
