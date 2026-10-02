//! Typed application configuration.
//!
//! Values are merged from several sources; later sources override earlier ones:
//! 1. `config/default.toml`                 – defaults for every environment (committed),
//! 2. `config/{APP_ENVIRONMENT}.toml`       – per-environment overrides, optional (`local`, `production`),
//! 3. environment variables `APP_<SECTION>__<KEY>`, e.g. `APP_SERVER__PORT=8080`.
//!
//! The merged result is deserialized (serde) into the `Settings` struct, so a typo or a wrong type
//! fails fast at startup instead of somewhere deep in the application.

use std::net::SocketAddr;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

// `Deserialize` – serde generates code building the struct from the merged config tree.
// `Clone` – settings are small; later parts of the app can get their own copy.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub server: ServerSettings,
    pub runtime: RuntimeSettings,
    pub http: HttpSettings,
    pub booking: BookingSettings,
    pub database: DatabaseSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    /// Max time to wait for in-flight requests after a shutdown signal.
    pub shutdown_grace_period_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeSettings {
    /// Number of Tokio worker threads; `None` (key absent) = number of CPU cores.
    pub worker_threads: Option<usize>,
}

/// HTTP middleware settings (step 011).
#[derive(Debug, Clone, Deserialize)]
pub struct HttpSettings {
    /// Requests running longer are aborted with 408.
    pub request_timeout_secs: u64,
    /// Max accepted request body size; larger bodies -> 413.
    pub body_limit_bytes: usize,
    /// Browser origins allowed to call the API (CORS). Empty = cross-origin calls blocked.
    pub cors_allowed_origins: Vec<String>,
    /// Requests slower than this are logged as warnings.
    pub slow_request_threshold_ms: u64,
}

/// Business limits of the booking rules (step 013).
#[derive(Debug, Clone, Deserialize)]
pub struct BookingSettings {
    pub max_active_bookings_per_user: usize,
    pub max_duration_minutes: i64,
}

/// PostgreSQL connection settings (step 014).
// No `Debug` derive: the URL contains the password and settings are logged at startup.
#[derive(Clone, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout_secs: u64,
    /// Apply pending migrations at startup (convenient for development; in production often
    /// a separate deployment step).
    pub run_migrations: bool,
}

// Manual `Debug` that masks the secret – `{:?}` on `Settings` must never print passwords.
impl std::fmt::Debug for DatabaseSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseSettings")
            .field("url", &"***")
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("acquire_timeout_secs", &self.acquire_timeout_secs)
            .field("run_migrations", &self.run_migrations)
            .finish()
    }
}

impl ServerSettings {
    /// Parses `host:port` into a socket address – invalid host fails at startup.
    pub fn address(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.host, self.port).parse()
    }
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        let environment = std::env::var("APP_ENVIRONMENT").unwrap_or_else(|_| "local".into());

        let mut builder = Config::builder();
        // `DATABASE_URL` is the de-facto standard variable (sqlx-cli, sqlx macros, PaaS
        // platforms). Used as a default; `APP_DATABASE__URL` still overrides it.
        if let Ok(url) = std::env::var("DATABASE_URL") {
            builder = builder.set_default("database.url", url)?;
        }

        builder
            // `required(true)` – startup fails when the defaults file is missing.
            .add_source(File::with_name("config/default").required(true))
            .add_source(File::with_name(&format!("config/{environment}")).required(false))
            // APP_SERVER__PORT -> server.port: prefix `APP` + "_", nested keys separated with "__"
            // (single "_" can't be used as a separator because key names contain it).
            // `try_parsing` converts "8080" to a number, "true" to bool etc.
            // Lists from env: `APP_HTTP__CORS_ALLOWED_ORIGINS=http://a.com,http://b.com`.
            // `with_list_parse_key` marks which keys are lists (others stay plain strings).
            .add_source(
                Environment::with_prefix("APP")
                    .prefix_separator("_")
                    .separator("__")
                    .try_parsing(true)
                    .list_separator(",")
                    .with_list_parse_key("http.cors_allowed_origins"),
            )
            .build()?
            .try_deserialize()
    }
}
