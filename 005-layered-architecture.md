# 005 – Recommended architecture: layers of responsibility

## Goal

Split the application into layers with clear responsibilities and one-directional dependencies,
so business logic stays independent of HTTP and storage.

## Key concepts

### Layers

```
┌──────────────────────────────────────────────┐
│ api             routing, handlers, JSON, HTTP status codes    │
├──────────────────────────────────────────────┤
│ domain          models, services, business rules              │
├──────────────────────────────────────────────┤
│ infrastructure  repositories (memory, PostgreSQL), external systems │
└──────────────────────────────────────────────┘
  app / server  – composition root + process lifecycle (knows everything, used by main)
```

| Layer | Knows about | Must not know about |
|-------|-------------|---------------------|
| `api` | `axum`, domain services and models | SQL, storage details |
| `domain` | only itself (+ small utility crates) | `axum`, HTTP, SQL, `sqlx` |
| `infrastructure` | domain models, DB drivers | HTTP |
| `app` | all of the above | – |

Why:
- business rules testable without a server or a database,
- storage / transport replaceable (in-memory → PostgreSQL, REST → gRPC) without touching rules,
- smaller, focused modules.

Current compromise: `RoomService` imports the concrete `InMemoryRoomRepository`
(domain → infrastructure). Step 012 fixes it with *dependency inversion*: the domain defines a
`RoomRepository` trait, infrastructure implements it.

Other common names for the same idea: hexagonal architecture (ports & adapters), clean architecture,
onion architecture. For small services, a module-per-feature layout (`rooms/{handlers,service,repository}.rs`)
is also fine – the dependency direction matters more than the folder names.

### Project layout

```
src/
├── main.rs                  binary: env, config, logging, runtime -> server::run
├── lib.rs                   library crate root, module declarations
├── config.rs                Settings
├── telemetry.rs             tracing subscriber
├── server.rs                bind, serve, graceful shutdown
├── app.rs                   AppState + composition root (build_router)
├── api/
│   ├── mod.rs               router assembly + with_state
│   ├── health.rs
│   └── rooms.rs             handlers
├── domain/
│   ├── mod.rs
│   ├── room.rs              Room model
│   └── room_service.rs      RoomService
└── infrastructure/
    ├── mod.rs
    └── memory/
        ├── mod.rs           re-exports
        └── room_repository.rs
```

### Binary + library crate

A package can contain `src/lib.rs` (library crate) **and** `src/main.rs` (binary crate).
`main.rs` uses the library by its name (`rust_services::...`). Integration tests in `tests/`
can only access a library crate – that is why the application code moved to `lib.rs`.

### Rust modules – quick reference

```rust
mod rooms;               // loads rooms.rs or rooms/mod.rs, private to the parent
pub mod domain;          // public module
pub use repo::InMemoryRoomRepository;  // re-export – shorter public path
use crate::app::AppState;              // absolute path from the crate root
use super::something;                  // path relative to the parent module
pub(crate) fn helper()                 // visible in the whole crate, not outside
```

### Shared state – `State` extractor

```rust
#[derive(Clone)]
pub struct AppState { pub room_service: Arc<RoomService> }

Router::new()
    .route("/rooms", get(list_rooms))
    .with_state(state);                     // Router<AppState> -> Router<()>

async fn list_rooms(State(state): State<AppState>) -> Json<Vec<Room>> { ... }
```

- State is cloned per request → wrap heavy parts in `Arc`.
- Type-checked at compile time: a handler asking for `State<X>` on a router with a different state does not compile.
- Alternative: `Extension<T>` layer – works, but is checked only at runtime (500 when missing). Prefer `State`.

### Shared mutable data

| Primitive | When |
|-----------|------|
| `std::sync::Mutex` / `RwLock` | short critical sections, **never held across `.await`** |
| `tokio::sync::Mutex` / `RwLock` | guard must live across `.await` |
| atomics (`AtomicU64`) | counters, flags |
| channels / actor task | complex state owned by one task |
| database | the usual answer for business data (step 014) |

## What changed in this branch

- `src/lib.rs` (new) – library crate, architecture overview
- `src/main.rs` – reduced to bootstrap only
- `src/server.rs`, `src/telemetry.rs` – moved out of `main.rs`
- `src/app.rs` – `AppState`, composition root
- `src/api/{mod,health,rooms}.rs` – handlers, `State` extractor
- `src/domain/{room,room_service}.rs` – model and service
- `src/infrastructure/memory/room_repository.rs` – in-memory repository with `RwLock`
- removed: `src/runtime_demo.rs` (demo of step 002), the background reporter task

## Try it

```bash
cargo run
curl localhost:3000/rooms
curl localhost:3000/rooms/2
curl -i localhost:3000/rooms/9     # 404
```

## Exercises

1. Add `RoomService::find_by_min_capacity(min: u32)` and expose it as `GET /rooms/large`.
2. Try to use `axum::http::StatusCode` inside `domain/` – why is that a design smell?
3. Replace `std::sync::RwLock` with `tokio::sync::RwLock` – what changes in the code?
