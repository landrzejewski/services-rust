# 022 – Testing

## Goal

Build a test suite on several levels: fast unit tests of the domain, API tests through the full
HTTP stack without a server, repository tests against real PostgreSQL, and a concurrency test in
a throw-away container.

## Key concepts

### Test pyramid in this project

| Level | Where | Speed | Needs | Example |
|-------|-------|-------|-------|---------|
| Unit | `#[cfg(test)] mod tests` next to the code | ms | nothing | booking rules, `TimeRange`, JWT, DTO JSON shape |
| Unit with mocks | same | ms | `mockall` | `delete_room` never calls `delete` when bookings exist |
| API (in-process) | `tests/api_*.rs` | ms | nothing (in-memory repos) | status codes, problem details, access matrix |
| Repository | `tests/postgres_repositories.rs` | 100 ms | PostgreSQL (`DATABASE_URL`) | SQL filters, constraints, upsert |
| Container | `tests/concurrency_testcontainers.rs` | seconds | Docker | 10 parallel bookings → 1 success |

```bash
cargo test                          # unit + API + repository tests
cargo test --lib                    # unit tests only
cargo test --test api_bookings      # one integration test file
cargo test access_matrix            # tests whose name contains the filter
cargo test -- --ignored             # opt-in tests (Docker)
cargo test -- --nocapture           # show println!/logs
```

### Unit vs integration tests in Rust

- **unit**: `#[cfg(test)] mod tests { use super::*; ... }` in the same file – can test private items,
- **integration**: each file in `tests/` is a separate crate using only the public API of the library
  (one reason the app lives in `lib.rs` since step 005); shared helpers in `tests/common/mod.rs`,
- async tests: `#[tokio::test]` (current-thread runtime), `#[tokio::test(flavor = "multi_thread")]` for real parallelism.

### Designing for testability

Everything external is behind a trait and injected (steps 012–016):

```rust
pub struct Repositories { pub rooms: Arc<dyn RoomRepository>, ... }
impl Repositories {
    pub fn postgres(settings: &Settings, db: &PgPool) -> anyhow::Result<Self>;
    pub fn in_memory() -> Self;
}
pub fn build_state_with(settings: &Settings, repositories: Repositories, db: PgPool) -> anyhow::Result<AppState>;
```

`Settings::load_with_overrides(&[("auth.jwt_secret", "..."), ("oidc.enabled", "false")])` gives
tests real configuration without depending on `.env` or env variables.
`Clock` (013) makes time-based rules deterministic.

### API tests without a server – `oneshot`

```rust
let request = Request::builder().method(Method::POST).uri("/api/v1/rooms")
    .header(header::AUTHORIZATION, format!("Bearer {token}"))
    .header(header::CONTENT_TYPE, "application/json")
    .body(Body::from(json.to_string()))?;
let response = router.clone().oneshot(request).await?;      // tower::ServiceExt
let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
```

The router is a `tower::Service`: requests go through middleware, routing, extractors, handlers –
no socket, no port, tests run in parallel. Tokens are issued directly with `JwtService` for speed;
one test covers the real register → login → call flow.

Table-driven tests keep matrices readable:

```rust
let cases = [ (Method::GET, "/api/v1/bookings", 401, 403, 200), ... ];
for (method, path, anonymous, user, admin) in cases { ... }
```

Alternatives: `axum-test` crate (fluent API), or start the real server on port 0 and use `reqwest`.

### Database tests – `#[sqlx::test]`

```rust
#[sqlx::test]
async fn database_rejects_overlapping_bookings(pool: PgPool) { ... }
```

For each test: new database (unique name) → migrations from `./migrations` → test → drop.
Isolation without cleanup code. Options: `#[sqlx::test(migrations = false)]`,
`#[sqlx::test(fixtures("rooms"))]` (SQL files in `tests/fixtures/`). Needs `DATABASE_URL`
pointing to a server where the user may create databases.

### Containers – testcontainers

```rust
let container = Postgres::default().with_tag("18-alpine").start().await?;
let port = container.get_host_port_ipv4(5432).await?;
// container removed when `container` is dropped
```

Self-contained (no pre-started DB), same image as production, ideal for CI with Docker. Slower →
`#[ignore = "starts a Docker container"]`, run with `--ignored`.

### Mocks – mockall

```rust
#[cfg_attr(test, mockall::automock)]   // above #[async_trait]
#[async_trait]
pub trait RoomRepository: Send + Sync { ... }

let mut rooms = MockRoomRepository::new();
rooms.expect_delete().never();                                        // interaction expectation
bookings.expect_count_active_by_room().withf(move |id, _| *id == room_id).times(1).returning(|_, _| Ok(2));
```

Prefer fakes (in-memory repositories) for state-based tests; use mocks when the interaction
itself is the requirement ("must not call", "called once with"). Over-mocking couples tests to
implementation details.

### Tooling

| Tool | Purpose |
|------|---------|
| `cargo nextest run` | faster runner, per-test processes, retries, JUnit output for CI |
| `cargo llvm-cov` / `cargo tarpaulin` | code coverage |
| `insta` | snapshot tests (e.g. JSON responses) |
| `proptest` / `quickcheck` | property-based tests (e.g. `TimeRange::overlaps` symmetry) |
| `wiremock` | mock HTTP servers (e.g. OIDC discovery/JWKS) |

## What changed in this branch

- `src/app.rs` – `Repositories` (`postgres`, `in_memory`), `build_state_with`
- `src/config.rs` – `Settings::load_with_overrides`
- `src/domain/repositories.rs` – `#[cfg_attr(test, mockall::automock)]`
- `src/domain/room_service.rs` – mock-based interaction tests
- `tests/common/mod.rs` (new) – `TestApp`, request helper, test settings
- `tests/api_rooms.rs`, `tests/api_bookings.rs` (new) – API tests incl. access matrix
- `tests/postgres_repositories.rs` (new) – `#[sqlx::test]`
- `tests/concurrency_testcontainers.rs` (new) – testcontainers, `#[ignore]`
- `Cargo.toml` – dev-dependencies `mockall`, `testcontainers-modules`; `tower` feature `util`
- `.sqlx/` – regenerated

No new endpoints in this step.

## Try it

```bash
docker compose up -d postgres
cargo test
cargo test --test concurrency_testcontainers -- --ignored
cargo tarpaulin --skip-clean --out Html     # coverage report (installed separately)
```

## Exercises

1. Add a property-based test (`proptest`): `a.overlaps(&b) == b.overlaps(&a)` for any valid ranges.
2. Test the OIDC verifier with `wiremock` serving a discovery document and a JWKS generated in the test.
3. Add a `#[sqlx::test(fixtures(...))]` test loading bookings from `tests/fixtures/bookings.sql`.
