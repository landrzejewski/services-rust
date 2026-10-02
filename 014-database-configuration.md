# 014 – Configuring the database connection

## Goal

Run PostgreSQL locally, connect to it with a properly configured sqlx pool, manage the schema
with migrations, and expose liveness/readiness probes.

## Key concepts

### Local database

```bash
docker compose up -d postgres         # compose.yaml from step 003
docker compose exec postgres psql -U booking -d booking
```

`DATABASE_URL=postgres://booking:booking@localhost:5432/booking` (in `.env`).
URL format: `postgres://user:password@host:port/database?sslmode=require&application_name=booking`.

### sqlx in one paragraph

Async, pure-Rust SQL toolkit (not an ORM): you write SQL, sqlx executes it and maps rows.
Features chosen in `Cargo.toml`:

```toml
sqlx = { version = "0.9", default-features = false, features = [
  "runtime-tokio", "tls-rustls", "postgres", "uuid", "chrono", "migrate", "macros" ] }
```

### Connection pool

```rust
let pool = PgPoolOptions::new()
    .max_connections(10)                       // per app instance
    .min_connections(1)
    .acquire_timeout(Duration::from_secs(5))   // wait for a free connection
    .idle_timeout(Duration::from_secs(600))
    .max_lifetime(Duration::from_secs(1800))
    .connect(&url).await?;                     // fails fast; connect_lazy() defers
```

- `PgPool` is cheap to clone (`Arc` inside) → store it in `AppState`.
- Each query borrows a connection and returns it immediately; transactions hold one until commit/rollback.
- Sizing: total connections of all instances < server `max_connections`; start small (≈ 2× CPU cores),
  measure; for many instances use PgBouncer.
- Close on shutdown: `pool.close().await`.

### Configuration and secrets

```toml
[database]                      # config/default.toml – no secrets here
max_connections = 10
min_connections = 1
acquire_timeout_secs = 5
run_migrations = true
```

URL comes from `DATABASE_URL` (default) or `APP_DATABASE__URL` (override).
`DatabaseSettings` implements `Debug` manually to mask the URL (passwords must never reach logs).

### Migrations

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
sqlx migrate add create_rooms          # migrations/<timestamp>_create_rooms.sql
sqlx migrate run                       # apply (uses DATABASE_URL)
sqlx migrate info                      # status
sqlx migrate add -r name               # reversible: .up.sql + .down.sql
```

In the application:

```rust
sqlx::migrate!().run(&pool).await?;    // embedded at compile time from ./migrations
```

Rules:
- migrations are append-only – never edit an applied one (checksum mismatch error),
- each runs in a transaction; applied versions are stored in `_sqlx_migrations`,
- running at startup is convenient; with many instances or strict change control, run them as a
  separate deployment step (`sqlx migrate run`, init container).

Schema design used here: UUID primary keys, `TIMESTAMPTZ` for instants, `CHECK` constraints
mirroring domain invariants, partial indexes on active bookings for the rule queries.

### Startup errors with `anyhow`

```rust
pub async fn build_state(settings: &Settings) -> anyhow::Result<AppState> {
    let db = postgres::connect(&settings.database).await?;   // .context("failed to connect to PostgreSQL")
    ...
}
fn main() -> anyhow::Result<()> { ... }
```

```
Error: failed to initialize application state

Caused by:
    0: failed to connect to PostgreSQL
    1: pool timed out while waiting for an open connection
```

### Liveness vs readiness

| Probe | Endpoint | Checks | On failure the orchestrator... |
|-------|----------|--------|-------------------------------|
| liveness | `/health/live` | process answers HTTP | restarts the container |
| readiness | `/health/ready` | `SELECT 1` within 2 s | stops sending traffic (503) |

Never put dependency checks into liveness – a database outage would cause restart loops.

## What changed in this branch

- `migrations/*_create_rooms.sql`, `*_create_bookings.sql` (new)
- `src/infrastructure/postgres/mod.rs` (new) – `connect`, `run_migrations`, `ping`
- `src/config.rs`, `config/default.toml` – `[database]`, `DATABASE_URL` default, masked `Debug`
- `src/app.rs` – async `build_state` with pool + migrations, `db: PgPool` in `AppState`, `build`
- `src/server.rs`, `src/main.rs` – `anyhow::Result`, pool closed on shutdown
- `src/api/health.rs` – `/health/live`, `/health/ready`
- `Cargo.toml` – `sqlx`, `anyhow`

Repositories are still in-memory – PostgreSQL repositories come in step 015.

## Try it

```bash
docker compose up -d
cargo run                                            # logs: pool created, migrations applied
curl -i localhost:3000/health/ready                  # 200
docker compose exec postgres psql -U booking -d booking -c '\d bookings'
docker compose stop postgres; curl -i localhost:3000/health/ready   # 503
docker compose start postgres
APP_DATABASE__URL=postgres://booking:wrong@localhost/booking cargo run   # fails with context chain
```

## Exercises

1. Add a reversible migration adding `rooms.floor INTEGER` (`sqlx migrate add -r`), run and revert it.
2. Add `database.statement_timeout_ms` and set it per connection with `PgConnectOptions::options([("statement_timeout", ...)])`.
3. Report pool statistics (`pool.size()`, `pool.num_idle()`) in `/health/ready`.
