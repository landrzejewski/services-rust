# 004 – Creating, configuring and running the HTTP server

## Goal

Turn the demo into a properly started service: typed configuration, structured logging,
correct socket binding and graceful shutdown.

## Key concepts

### Startup sequence

```
main()                       (sync)
 ├─ dotenvy::dotenv()        .env -> process env
 ├─ Settings::load()         config files + env -> typed struct (fail fast)
 ├─ init_tracing()           logging subscriber
 ├─ runtime Builder          configured from Settings
 └─ block_on(run(settings))  (async)
      ├─ spawn background tasks
      ├─ build Router
      ├─ TcpListener::bind
      └─ axum::serve(..).with_graceful_shutdown(..)
```

### Layered configuration – `config` crate

Sources (later wins):

| # | Source | Example |
|---|--------|---------|
| 1 | `config/default.toml` | `port = 3000` |
| 2 | `config/{APP_ENVIRONMENT}.toml` (optional) | `config/production.toml`: `host = "0.0.0.0"` |
| 3 | env vars `APP_<SECTION>__<KEY>` | `APP_SERVER__PORT=8080` |

```rust
Config::builder()
    .add_source(File::with_name("config/default"))
    .add_source(File::with_name(&format!("config/{env}")).required(false))
    .add_source(Environment::with_prefix("APP").prefix_separator("_").separator("__").try_parsing(true))
    .build()?
    .try_deserialize::<Settings>()
```

- Deserializing into a struct gives type safety and fails at startup on missing/invalid values.
- `Option<T>` fields are optional keys.
- Secrets (passwords, keys) come from env / secret stores – never committed in config files.
- Alternatives: `figment` (used by Rocket), `envy` (env only), hand-written `std::env` parsing.

### Binding

```rust
let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
```

| Host | Meaning |
|------|---------|
| `127.0.0.1` | loopback only – not reachable from other machines (safe local default) |
| `0.0.0.0` | all IPv4 interfaces – required in containers |
| `[::]` | all IPv6 (often also IPv4, OS dependent) |
| port `0` | OS assigns a free port – read it with `listener.local_addr()` (tests) |

### Logging with `tracing`

```rust
tracing::info!(address = %addr, "server started");   // %  -> Display
tracing::debug!(?settings, "configuration loaded");   // ?  -> Debug
tracing::warn!(user_id = 42, "suspicious request");    // plain value
```

- **Events** (`info!`, `warn!`...) carry a message + structured key-value **fields**.
- **Spans** (`#[instrument]`, `info_span!`) give events context (request id, user) – step 023.
- A **subscriber** decides output; here `tracing_subscriber::fmt` + `EnvFilter`:

```bash
RUST_LOG=info                                   # global level
RUST_LOG=info,rust_services=debug,hyper=warn    # per crate (module path, '-' -> '_')
```

Levels: `error` > `warn` > `info` > `debug` > `trace`.

### Graceful shutdown

On SIGTERM (Docker/Kubernetes stop) or SIGINT (Ctrl+C):

1. stop accepting new connections,
2. let in-flight requests finish,
3. stop background tasks,
4. close resources (DB pool – later), exit with code 0.

```rust
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal(token.clone()))   // future that completes on signal
    .await
```

- `with_graceful_shutdown` waits for in-flight requests **without a limit** → race it against a
  grace period (`tokio::select!`), shorter than the orchestrator's kill timeout
  (Docker: 10 s, Kubernetes `terminationGracePeriodSeconds`: 30 s).
- `CancellationToken` (`tokio-util`) – cloneable stop signal for background tasks:

```rust
tokio::select! {
    _ = interval.tick() => do_work().await,
    _ = token.cancelled() => return,
}
```

## What changed in this branch

- `src/config.rs` – `Settings` loaded from files + env
- `config/default.toml`, `config/local.toml`, `config/production.toml` – configuration files
- `src/main.rs` – startup sequence, `tracing` subscriber, runtime from config, graceful shutdown,
  cancellation of the background task
- `.env.example` – `APP_ENVIRONMENT`, `APP_*` overrides, `RUST_LOG` (`APP_ADDR` removed)
- `Cargo.toml` – `config`, `tracing`, `tracing-subscriber`, `tokio-util`

Note: the runtime now uses the default number of workers (CPU cores). For the blocking demo
from step 002 run with `APP_RUNTIME__WORKER_THREADS=2`.

## Try it

```bash
cargo run                                          # 127.0.0.1:3000, local config
APP_SERVER__PORT=8080 cargo run                    # env override
APP_ENVIRONMENT=production cargo run               # 0.0.0.0, config/production.toml
RUST_LOG=debug cargo run                           # see loaded settings and hyper internals
APP_SERVER__PORT=abc cargo run                     # fails fast: invalid configuration

# graceful shutdown: start a 5 s request, then stop the server – the request still completes
curl localhost:3000/demo/non-blocking & sleep 1; pkill -TERM rust-services

# grace period exceeded: request is aborted after 1 s
APP_SERVER__SHUTDOWN_GRACE_PERIOD_SECS=1 cargo run
curl localhost:3000/demo/non-blocking & sleep 1; pkill -INT rust-services
```

## Exercises

1. Add `server.request_log` (bool) to the configuration and log every request in `health` only when enabled.
2. Print the configuration source precedence by setting the same key in all three sources.
3. Extend the shutdown sequence with a second background task and wait for both using `tokio::join!`.
