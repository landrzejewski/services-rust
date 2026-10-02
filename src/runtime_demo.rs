//! Step 002 – Tokio runtime in practice.
//!
//! Each handler demonstrates one runtime technique:
//! - `GET /rooms/{id}/summary`      – `tokio::join!`: run independent async operations concurrently,
//! - `GET /rooms/{id}/availability` – `tokio::select!`: race an operation against a deadline,
//! - `GET /reports/occupancy`       – `spawn_blocking`: move CPU-heavy work off the async workers,
//! - `GET /demo/blocking`           – ANTI-PATTERN: blocking a worker thread,
//! - `GET /demo/non-blocking`       – the correct async equivalent.
//!
//! "External" calls are simulated with `tokio::time::sleep`.

use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};

// `pub` makes the function visible to `main.rs`. Returning a `Router` from a module
// is the usual way to keep route definitions next to their handlers.
pub fn router() -> Router {
    Router::new()
        .route("/rooms/{id}/summary", get(room_summary))
        .route("/rooms/{id}/availability", get(room_availability))
        .route("/reports/occupancy", get(occupancy_report))
        .route("/demo/blocking", get(blocking_sleep))
        .route("/demo/non-blocking", get(non_blocking_sleep))
}

// ---------------------------------------------------------------------------
// join! – concurrency inside a single task
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RoomSummary {
    room_id: u64,
    name: String,
    active_bookings: u32,
    elapsed_ms: u128,
}

// Simulated I/O: a database / remote call taking 100 ms.
async fn fetch_room_name(id: u64) -> String {
    tokio::time::sleep(Duration::from_millis(100)).await;
    format!("Room #{id}")
}

// Simulated I/O taking 150 ms.
async fn count_active_bookings(_room_id: u64) -> u32 {
    tokio::time::sleep(Duration::from_millis(150)).await;
    3
}

async fn load_summary(id: u64) -> RoomSummary {
    let started = Instant::now();

    // Futures in Rust are lazy – nothing runs until they are awaited (polled).
    // Sequential version would take 100 + 150 = 250 ms:
    //     let name = fetch_room_name(id).await;
    //     let active = count_active_bookings(id).await;
    //
    // `join!` polls both futures concurrently *within the current task* and waits for all of them,
    // so the total time is max(100, 150) = 150 ms. No new tasks or threads are created.
    // For fallible futures use `tokio::try_join!` – it returns early on the first `Err`.
    let (name, active_bookings) = tokio::join!(fetch_room_name(id), count_active_bookings(id));

    RoomSummary {
        room_id: id,
        name,
        active_bookings,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

async fn room_summary(Path(id): Path<u64>) -> Json<RoomSummary> {
    Json(load_summary(id).await)
}

// ---------------------------------------------------------------------------
// select! – racing futures, timeouts, cancellation
// ---------------------------------------------------------------------------

// `Query<T>` extractor deserializes the query string (`?latency_ms=800`) into `T`.
// `Option` makes the parameter optional. (Routing and extractors in detail: step 006.)
#[derive(Deserialize)]
struct AvailabilityParams {
    latency_ms: Option<u64>,
}

#[derive(Serialize)]
struct Availability {
    room_id: u64,
    available: bool,
}

const CALENDAR_DEADLINE: Duration = Duration::from_millis(500);

// Simulated call to an external calendar system with configurable latency.
async fn check_external_calendar(room_id: u64, latency: Duration) -> Availability {
    tokio::time::sleep(latency).await;
    Availability {
        room_id,
        available: room_id % 2 == 1,
    }
}

async fn room_availability(
    Path(id): Path<u64>,
    Query(params): Query<AvailabilityParams>,
) -> impl IntoResponse {
    let latency = Duration::from_millis(params.latency_ms.unwrap_or(200));

    // `select!` waits on several futures and continues with the FIRST one that completes.
    // All other branches are dropped – dropping a future cancels it (it is never polled again).
    // This is how cancellation works in async Rust: no flags, no exceptions – just `drop`.
    //
    // A plain timeout has a shorter form: `tokio::time::timeout(CALENDAR_DEADLINE, fut).await`
    // returning `Result<T, Elapsed>`. `select!` is more general (e.g. racing two replicas,
    // waiting for a shutdown signal or a channel message).
    tokio::select! {
        availability = check_external_calendar(id, latency) => {
            (StatusCode::OK, Json(availability)).into_response()
        }
        _ = tokio::time::sleep(CALENDAR_DEADLINE) => {
            (StatusCode::GATEWAY_TIMEOUT, "calendar service did not answer in time").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// spawn_blocking – CPU-bound or blocking work
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ReportParams {
    iterations: Option<u64>,
}

#[derive(Serialize)]
struct OccupancyReport {
    checksum: u64,
    elapsed_ms: u128,
}

// Plain (non-async) CPU-heavy function. Calling it directly inside an async handler
// would occupy a worker thread for its whole duration and starve other tasks.
fn compute_report(iterations: u64) -> u64 {
    let mut acc: u64 = 0;
    for i in 0..iterations {
        // `black_box` prevents the optimizer from removing the loop.
        acc = acc.wrapping_mul(31).wrapping_add(std::hint::black_box(i));
    }
    acc
}

async fn occupancy_report(Query(params): Query<ReportParams>) -> impl IntoResponse {
    let iterations = params.iterations.unwrap_or(50_000_000);
    let started = Instant::now();

    // `spawn_blocking` runs the closure on a separate pool of threads dedicated to
    // blocking work (`max_blocking_threads` in the runtime builder). The async worker
    // stays free to serve other requests; we just `.await` the result.
    //
    // The closure must be `Send + 'static` – hence `move` (it takes ownership of `iterations`).
    // Use it for: CPU-heavy computations, password hashing (step 018), blocking libraries,
    // synchronous file/database APIs. For long-running CPU pipelines consider `rayon`.
    let result = tokio::task::spawn_blocking(move || compute_report(iterations)).await;

    // Awaiting a task returns `Result<T, JoinError>` – `Err` when the task panicked
    // or was aborted. A panic inside a task does not crash the whole server.
    match result {
        Ok(checksum) => (
            StatusCode::OK,
            Json(OccupancyReport {
                checksum,
                elapsed_ms: started.elapsed().as_millis(),
            }),
        )
            .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Blocking vs non-blocking
// ---------------------------------------------------------------------------

// ANTI-PATTERN – never do this in async code.
// `std::thread::sleep` (like any blocking call: `std::fs`, blocking HTTP clients, `Mutex::lock`
// held across long work, heavy loops) blocks the *whole worker thread*. With 2 workers
// (`APP_RUNTIME__WORKER_THREADS=2`), two concurrent calls to this endpoint freeze the entire
// server – even `/health` hangs.
async fn blocking_sleep() -> &'static str {
    std::thread::sleep(Duration::from_secs(5));
    "done (blocking)"
}

// Correct: `tokio::time::sleep` registers a timer and yields at `.await`.
// The worker thread serves other tasks in the meantime; thousands of these can wait concurrently.
async fn non_blocking_sleep() -> &'static str {
    tokio::time::sleep(Duration::from_secs(5)).await;
    "done (non-blocking)"
}

// `#[cfg(test)]` compiles this module only for `cargo test`.
#[cfg(test)]
mod tests {
    use super::*;

    // `#[tokio::test]` is the test counterpart of `#[tokio::main]`: it creates a
    // (single-threaded by default) runtime for each test function.
    #[tokio::test]
    async fn join_runs_operations_concurrently() {
        let summary = load_summary(7).await;

        assert_eq!(summary.name, "Room #7");
        // Sequential execution would take at least 250 ms.
        assert!(summary.elapsed_ms < 240, "took {} ms", summary.elapsed_ms);
    }

    #[tokio::test]
    async fn spawn_blocking_returns_result_of_closure() {
        let result = tokio::task::spawn_blocking(|| compute_report(10))
            .await
            .unwrap();

        assert_eq!(result, compute_report(10));
    }
}
