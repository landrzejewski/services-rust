# 010 – Error handling

## Goal

Model errors as types, propagate them with `?`, and convert them in one place into consistent
HTTP responses following RFC 9457 (Problem Details).

## Key concepts

### Error flow

```
repository ──▶ domain service ──▶ handler ──▶ IntoResponse
  (015: RepositoryError)  DomainError    ApiError     Problem Details JSON
```

- Each layer has its own error type describing failures in **its** terms.
- `From` impls connect them, so `?` converts automatically.
- Only `ApiError::into_response` knows HTTP status codes.

### `thiserror` – error enums without boilerplate

```rust
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{entity} {id} not found")]          // Display
    NotFound { entity: &'static str, id: Uuid },

    #[error(transparent)]                          // Display/source forwarded
    Invalid(#[from] InvalidValue),                 // impl From<InvalidValue> for DomainError
}
```

| Attribute | Generates |
|-----------|-----------|
| `#[error("...{field}...")]` | `Display` impl |
| `#[from]` | `From<Inner>` impl (+ `source()`) |
| `#[source]` | `source()` without `From` |
| `#[error(transparent)]` | delegate `Display` and `source` to the inner error |

### `thiserror` vs `anyhow`

| | `thiserror` | `anyhow` |
|---|---|---|
| What | derive macro for **your own** error types | one opaque error type `anyhow::Error` |
| Caller can `match` on variants | yes | no (only downcast) |
| Use in | libraries, domain, API layer – where callers react differently | application glue, startup, CLI, tests – where you only report |
| Context | add fields/variants | `.context("loading config")?` |

Typical service: `thiserror` for domain/API errors, `anyhow` (optionally) in `main`/startup code.

### Handlers returning `Result`

```rust
pub type ApiResult<T> = Result<T, ApiError>;

async fn get_room(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Json<RoomResponse>> {
    let room = s.room_service.get_room(id).await?;   // DomainError -> ApiError via From
    Ok(Json(room.into()))
}
```

`Result<T, E>` implements `IntoResponse` when both `T` and `E` do.

`?` applies **one** `From` conversion – for `InvalidValue → DomainError → ApiError` an explicit
`impl From<InvalidValue> for ApiError` is added.

### One place for HTTP mapping

```rust
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem = match &self {
            ApiError::Domain(DomainError::NotFound { .. }) => ProblemDetails::new(404, ...),
            ApiError::Domain(DomainError::Invalid(_)) | ApiError::Validation(_) => ... 422,
            ApiError::Json(rejection) => ProblemDetails::new(rejection.status(), ...),
            ...
        };
        problem.into_response()
    }
}
```

| Error | Status |
|-------|--------|
| `DomainError::NotFound` | 404 |
| `DomainError::Invalid`, `ValidationErrors` | 422 |
| `JsonRejection` | 400 / 415 / 422 |
| `PathRejection`, `QueryRejection` | 400 |
| business rule violation, conflict (013, 016) | 409 / 422 |
| infrastructure failure (015) | 500 – details only in logs |

Security: never put internal details (SQL, stack traces, file paths) into 5xx responses.
Log them server-side with `tracing::error!`.

### RFC 9457 – Problem Details

```http
HTTP/1.1 422 Unprocessable Entity
Content-Type: application/problem+json

{
  "type": "/problems/validation-error",
  "title": "Validation failed",
  "status": 422,
  "detail": "one or more fields are invalid",
  "errors": { "name": ["must have 1-100 characters"] }
}
```

| Member | Meaning |
|--------|---------|
| `type` | URI identifying the problem type (can point to docs) |
| `title` | short summary, constant per type |
| `status` | HTTP status |
| `detail` | occurrence-specific explanation |
| `instance` | URI of this occurrence (optional) |
| extensions | any additional members, e.g. `errors` |

### Consistent rejections – wrapped extractors

Built-in extractors return plain-text errors. Wrapping them changes only the rejection type:

```rust
#[derive(FromRequestParts)]                         // axum feature "macros"
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);
```

Handlers import `api::extractors::{Path, Query, Json}` instead of the axum originals.
Alternative: `axum_extra::extract::WithRejection<Path<T>, ApiError>`.

## What changed in this branch

- `src/domain/error.rs` (new) – `DomainError`, `DomainResult`
- `src/domain/validation.rs` – `InvalidValue` via `thiserror`
- `src/domain/*_service.rs` – all operations return `DomainResult<T>`
- `src/api/problem.rs` (new) – `ProblemDetails` + `IntoResponse`
- `src/api/error.rs` (new) – `ApiError`, `ApiResult`, HTTP mapping, tests
- `src/api/extractors.rs` – `Path`/`Query`/`Json` wrappers, `ValidatedJson` rejects with `ApiError`
- `src/api/{rooms,bookings,mod}.rs` – handlers use `?`; fallbacks return `ApiError`
- `Cargo.toml` – `thiserror`, axum feature `macros`

## Try it

```bash
cargo test
cargo run
B=localhost:3000/api/v1
curl -i $B/rooms/0199a3f0-0000-7000-8000-000000000001     # 404 problem+json
curl -i $B/rooms/abc                                      # 400 invalid-path
curl -i "$B/rooms?minCapacity=x"                          # 400 invalid-query
curl -i -X POST $B/rooms -d '{}'                          # 415 invalid-body
curl -i -X POST $B/rooms -H 'content-type: application/json' -d '{"name":"","capacity":0}'   # 422
curl -i -X DELETE $B/bookings                             # 405
```

## Exercises

1. Add `DomainError::Conflict(String)` mapped to 409 and use it when creating a room with a duplicate name.
2. Add the `instance` member (request path) to problems – hint: a middleware (step 011) or the `OriginalUri` extractor.
3. Use `anyhow` with `.context(...)` in `Settings::load` / server startup and compare the error output.
