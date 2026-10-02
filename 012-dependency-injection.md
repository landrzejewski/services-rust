# 012 – Dependency injection

## Goal

Decouple business logic from concrete implementations: services depend on traits, a single
composition root decides which implementations are used, handlers receive only what they need.

## Key concepts

### Dependency inversion

Before (005–011): `domain::RoomService` → `infrastructure::InMemoryRoomRepository` (concrete).
After:

```
domain::RoomService ──▶ domain::RoomRepository (trait, "port")
                                ▲
infrastructure::InMemoryRoomRepository (impl, "adapter")    [PostgresRoomRepository – step 015]
```

The domain owns the interface; infrastructure depends on the domain.

### DI in Rust = constructor injection + composition root

```rust
pub struct RoomService { repository: Arc<dyn RoomRepository> }
impl RoomService {
    pub fn new(repository: Arc<dyn RoomRepository>) -> Self { Self { repository } }
}

// app.rs – the only place that knows concrete types
let rooms: Arc<dyn RoomRepository> = Arc::new(InMemoryRoomRepository::with_sample_data());
let room_service = Arc::new(RoomService::new(rooms.clone()));
```

No container, no reflection, no runtime lookup – missing dependencies are compile errors.
(DI crates exist, e.g. `shaku`, but are rarely needed.)

### Trait objects vs generics

| | `Arc<dyn Repo>` (dynamic dispatch) | `Service<R: Repo>` (static dispatch) |
|---|---|---|
| Call cost | vtable call (+ boxed future with `async_trait`) | inlined, zero-cost |
| Types | `RoomService` – one type | `RoomService<PgRepo>` – type parameter spreads into `AppState`, handlers, tests |
| Choose impl at runtime (config) | yes | no (compile time) |
| Compile time / binary size | smaller | monomorphization per type |
| Async fn in trait | needs `async_trait` (or manual boxing) | native `async fn` works |

For I/O-bound services the dispatch cost is irrelevant → `Arc<dyn Trait>` is the pragmatic default.
Generics fit hot paths and libraries.

### Async traits and `dyn`

```rust
#[async_trait]                                  // makes the trait dyn-compatible
pub trait RoomRepository: Send + Sync {         // Send + Sync: shared across threads via Arc
    async fn find_by_id(&self, id: Uuid) -> RepositoryResult<Option<Room>>;
}

#[async_trait]
impl RoomRepository for InMemoryRoomRepository { ... }
```

Native `async fn` in traits (Rust 1.75+) is not dyn-compatible yet; `async_trait` boxes the returned future.

### Errors in ports

A port must allow failure even if one adapter never fails:
`RepositoryResult<T> = Result<T, RepositoryError>`. `RepositoryError` hides driver-specific
types (`sqlx::Error`) from the domain but keeps them as `source` for logs.
`DomainError::Repository` → HTTP 500 with a generic message; details logged with `tracing::error!`.

### Sub-states with `FromRef`

```rust
#[derive(Clone, FromRef)]
pub struct AppState {
    pub room_service: Arc<RoomService>,
    pub booking_service: Arc<BookingService>,
}

async fn get_room(State(rooms): State<Arc<RoomService>>, ...) { ... }
async fn list_room_bookings(
    State(rooms): State<Arc<RoomService>>,
    State(bookings): State<Arc<BookingService>>,
    ...
)
```

`FromRef<AppState> for Arc<RoomService>` is generated per field. Handlers declare precise
dependencies; the router still holds a single `AppState`.

### Testing benefit

```rust
struct FailingRepository;
#[async_trait]
impl RoomRepository for FailingRepository { /* every method returns Err(...) */ }

let service = RoomService::new(Arc::new(FailingRepository));
assert!(matches!(service.get_room(id).await, Err(DomainError::Repository(_))));
```

Fakes/stubs replace infrastructure without a database (step 022 adds `mockall`).

## What changed in this branch

- `src/domain/repositories.rs` (new) – `RoomRepository`, `BookingRepository` traits, `RepositoryError`
- `src/domain/error.rs` – `DomainError::Repository`
- `src/domain/room_service.rs`, `booking_service.rs` – depend on `Arc<dyn ...Repository>`; stub-based test
- `src/infrastructure/memory/*` – `impl ...Repository for InMemory...` (`#[async_trait]`)
- `src/app.rs` – `#[derive(FromRef)]`, `build_state` composition root
- `src/api/error.rs` – 500 mapping + error-level logging for server errors
- `src/api/rooms.rs`, `bookings.rs` – `State<Arc<RoomService>>` / `State<Arc<BookingService>>`
- `Cargo.toml` – `async-trait`

## Try it

```bash
cargo test            # includes repository_failure_becomes_domain_error
cargo run
curl localhost:3000/api/v1/rooms
```

## Exercises

1. Write `LoggingRoomRepository` – a decorator implementing `RoomRepository` that wraps another
   `Arc<dyn RoomRepository>` and logs every call; wire it in `build_state`.
2. Rewrite `RoomService` as `RoomService<R: RoomRepository>` with native `async fn` in the trait – what has to change in `AppState` and handlers?
3. Add `storage.sample_data = true/false` to the config and use it in `build_state`.
