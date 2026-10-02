//! Binary entry point of the Room Booking Service.
//!
//! Since step 005 the application code lives in the library crate (`src/lib.rs`).
//! `main.rs` only bootstraps the process: environment, configuration, logging, runtime.
//! Keeping `main` thin lets integration tests (step 022) start the same application
//! from `tests/` by calling into the library.

// The package name `rust-services` becomes the library crate name `rust_services`
// (`-` is replaced by `_`). The binary uses it like any external crate.
use anyhow::Context;
use rust_services::{config::Settings, server, telemetry};

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
//
// `main` may return `Result`: on `Err` the error is printed (with `anyhow`: including the whole
// context chain) and the process exits with code 1.
fn main() -> anyhow::Result<()> {
    // Load variables from `.env` into the process environment (if the file exists).
    // Must run before anything reads env variables and before other threads are spawned.
    // Existing environment variables are NOT overwritten – real env (Docker, CI, shell) wins.
    // `.ok()` ignores the "file not found" error: in production there is usually no `.env`.
    dotenvy::dotenv().ok();

    // Fail fast: an invalid configuration should stop the process immediately with a clear message.
    let settings = Settings::load().context("failed to load configuration")?;

    telemetry::init_tracing();
    tracing::debug!(?settings, "configuration loaded");

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    // Number of worker threads executing async tasks. Default: number of CPU cores.
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
        .context("failed to build Tokio runtime")?;

    // `block_on` runs the future on the current (main) thread until it completes.
    // Everything spawned inside it runs on the worker threads.
    runtime.block_on(server::run(settings))
}
