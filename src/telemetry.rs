//! Logging setup (extended with JSON output, spans and metrics in step 023).

use tracing_subscriber::EnvFilter;

// `tracing` separates *producing* events (`info!`, `debug!`, spans) from *consuming* them.
// A subscriber decides where events go and in which format. `tracing_subscriber::fmt`
// prints human-readable lines to stdout.
pub fn init_tracing() {
    // `EnvFilter` reads the `RUST_LOG` variable, e.g. `info,rust_services=debug,tower_http=trace`.
    // Fallback when the variable is not set: `info` for everything.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}
