# 001 – Axum overview

## Goal

Understand what Axum is, which libraries it is built on, and run the smallest working service.

## Key concepts

### The stack

```
your handlers
    │
  Axum      – routing, extractors, responses (ergonomic layer)
    │
  Tower     – `Service` / `Layer` abstraction: middleware, timeouts, retries...
    │
  Hyper     – HTTP/1 and HTTP/2 implementation
    │
  Tokio     – async runtime: event loop, TCP sockets, timers, task scheduler
```

- **Axum** is maintained by the Tokio team. It has no macros-based routing and no custom runtime – everything is plain Rust types and traits.
- Every Axum `Router` is a Tower `Service`, so the whole Tower / `tower-http` middleware ecosystem works out of the box (step 011).
- Compile-time type checking of handlers: if a handler's arguments or return type are not valid, the code does not compile.

### Router

```rust
let app = Router::new()
    .route("/health", get(health))
    .route("/rooms/{id}", get(room_by_id).delete(delete_room));
```

- `route(path, method_router)` – `get`, `post`, `put`, `patch`, `delete` can be chained for the same path.
- Path parameters: `{id}`, wildcard: `{*rest}` (Axum 0.8+).
- Requests that match no route get `404`; a known path with a wrong method gets `405 Method Not Allowed`.

### Handlers

A handler is an `async fn` that:

1. takes zero or more **extractors** as arguments (max. 16),
2. returns something implementing **`IntoResponse`**.

```rust
async fn handler(Path(id): Path<u64>, Query(params): Query<Filter>, Json(body): Json<NewRoom>)
    -> impl IntoResponse
```

### Extractors

Types implementing `FromRequestParts` (read only head: method, URI, headers) or `FromRequest` (may consume the body).

| Extractor        | Source                          |
|------------------|---------------------------------|
| `Path<T>`        | path parameters                 |
| `Query<T>`       | query string                    |
| `Json<T>`        | JSON body (consumes the body)   |
| `HeaderMap`      | all headers                     |
| `State<T>`       | shared application state (005+) |
| `Extension<T>`   | values inserted by middleware   |

Rules:
- an extractor that consumes the body (`Json`, `String`, `Bytes`, `Form`) must be the **last** argument,
- failed extraction = **rejection** → handler is not called, an error response is returned (e.g. `400`, `415`, `422`).

### Responses – `IntoResponse`

| Return type                        | Result                                    |
|------------------------------------|-------------------------------------------|
| `&str`, `String`                   | `200`, `text/plain`                       |
| `Html<T>`                          | `200`, `text/html`                        |
| `Json<T>`                          | `200`, `application/json`                 |
| `StatusCode`                       | empty body with given status              |
| `(StatusCode, T)`                  | given status + body `T`                   |
| `(StatusCode, HeaderMap, T)`       | status + extra headers + body             |
| `Result<T, E>` (both `IntoResponse`) | `Ok` or `Err` converted (step 010)      |
| `impl IntoResponse` / `Response`   | when branches return different types      |

### Running the server

```rust
let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, app).await?;
```

## What changed in this branch

- `Cargo.toml` – dependencies `axum`, `tokio` (feature `full`), `serde` (feature `derive`)
- `src/main.rs` – router with three handlers: HTML, JSON, path parameter
- `requests/001-axum-overview.http` – sample requests

## Try it

```bash
cargo run
curl -i localhost:3000/
curl -i localhost:3000/health
curl -i localhost:3000/rooms/1
curl -i localhost:3000/rooms/2     # 404 from the handler
curl -i localhost:3000/rooms/abc   # 400 – Path rejection
curl -i -X POST localhost:3000/health  # 405 – method not allowed
```

## Exercises

1. Add `GET /rooms` returning a JSON array of two rooms.
2. Add `GET /hello/{name}` returning `Hello, <name>!` as plain text.
3. Return a custom header from `/health` using `(StatusCode, [(header::CACHE_CONTROL, "no-cache")], Json(...))`.
