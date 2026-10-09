//! Room Booking Service – step 001: the smallest useful Axum application.
//!
//! Building blocks shown here:
//! - `Router`      – maps (HTTP method, path) pairs to handlers,
//! - handlers      – plain `async fn`s; their arguments are *extractors*,
//!   their return type must implement `IntoResponse`,
//! - `axum::serve` – glues a `tokio::net::TcpListener` with the router (hyper runs underneath).

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Serialize;
use tokio::process::Command;
use tower_http::services::ServeDir;

// `#[tokio::main]` turns `async fn main` into a regular `fn main` that starts
// the Tokio runtime and blocks on the future. Axum has no runtime of its own –
// it relies on Tokio for networking, timers and task scheduling (details in step 002).
#[tokio::main]
async fn main() {
   /* let result = Command::new("ls")
        .arg("-la")
        .args(["--color=never"])
        .current_dir("/Users/lukas/Desktop/services-rust")
        .env("LANGUAGE", "en")
        .output()
        .await;

   println!("{:?}", result.unwrap());*/


    // Router is built with a fluent API. Every `.route()` call registers a path
    // and a `MethodRouter` (here `get(...)`, later also `post`, `put`, `delete`...).
    // Path parameters use the `{name}` syntax (Axum 0.8+; older versions used `:name`).
    let app = Router::new()
        //.route("/", get(index))
        .route("/health", get(health))
        .route("/rooms/{id}", get(room_by_id))
        .route("/git", get(git_version))
        //.nest_service("/", ServeDir::new("static"));
        .fallback_service(ServeDir::new("static"));

    // Bind a TCP socket. `0.0.0.0` accepts connections on all interfaces
    // (needed later inside containers); use `127.0.0.1` to listen locally only.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("listening on http://{}", listener.local_addr().unwrap());

    // `axum::serve` accepts connections and drives each one with hyper,
    // passing every request to the router. It runs until the process is stopped.
    axum::serve(listener, app).await.expect("server error");
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

async fn git_version() -> Result<String, (StatusCode, String)> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
