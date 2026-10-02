# 011 – Middleware: enriching and modifying requests and responses

## Goal

Implement cross-cutting concerns (request ids, logging, CORS, timeouts, compression, limits,
security headers) once, outside of handlers.

## Key concepts

### Tower model

- `Service<Request>` – async function `Request -> Result<Response, Error>`. Axum handlers and
  routers are services.
- `Layer<S>` – wraps a service and returns a new service (decorator).
- Middleware = a layer. Same middleware works in Axum, Tonic (gRPC), Hyper.

### Applying layers

```rust
router.layer(layer)            // all routes of this router + fallback
router.route_layer(layer)      // only matched routes (no effect on 404 – used for auth, step 021)
Router::new().route("/x", get(h).layer(layer))   // single route / method
```

Order:

```rust
ServiceBuilder::new()      // top = outermost
    .layer(a)              // a sees request 1st, response last
    .layer(b)
    .layer(c)              // c is closest to the handler

router.layer(a).layer(b)   // opposite! b is outermost
```

```
request  ─▶ set-request-id ─▶ trace ─▶ propagate-id ─▶ catch-panic ─▶ timeout ─▶ cors ─▶ compression ─▶ body-limit
         ─▶ response_time ─▶ request_context ─▶ problem enrichment ─▶ security headers ─▶ router ─▶ handler
```

### `tower-http` layers used

| Layer | Purpose |
|-------|---------|
| `SetRequestIdLayer` + `PropagateRequestIdLayer` | `x-request-id` generated (UUID) or accepted from client, returned in response |
| `TraceLayer` | span per request (method, URI, request id) + log line with status and latency |
| `CatchPanicLayer` | panic in handler → 500 response instead of dropped connection |
| `TimeoutLayer::with_status_code(408, d)` | abort long requests |
| `CorsLayer` | browser cross-origin rules, preflight `OPTIONS` handling |
| `CompressionLayer` | gzip/brotli per `Accept-Encoding` (cargo features `compression-*`) |
| `DefaultBodyLimit` (axum) | body size limit for extractors → 413 through `JsonRejection` |

Others worth knowing: `SetResponseHeaderLayer`, `NormalizePathLayer` (must wrap the router
from outside), `SensitiveHeadersLayer` (hide `Authorization` in logs), `ValidateRequestHeaderLayer`,
`ServeDir` (static files), `tower::limit::ConcurrencyLimitLayer`, `tower_governor` (rate limiting).

### CORS in short

Browsers block JS from reading responses of another origin unless the server allows it.
Non-simple requests (JSON body, `Authorization`) trigger a preflight `OPTIONS` request.

```rust
CorsLayer::new()
    .allow_origin(["http://localhost:5173".parse()?])   // never `Any` together with credentials
    .allow_methods([GET, POST, PUT, DELETE])
    .allow_headers([CONTENT_TYPE, AUTHORIZATION])
    .expose_headers([LOCATION, "x-request-id"])
    .max_age(Duration::from_secs(3600))
```

CORS is a browser mechanism, not access control: curl/servers ignore it.

### Custom middleware with functions

| Helper | Signature | Use |
|--------|-----------|-----|
| `middleware::from_fn(f)` | `async fn(Request, Next) -> Response` | full control, before + after |
| `middleware::from_fn_with_state(s, f)` | `async fn(State<S>, Request, Next) -> Response` | needs config/services |
| `middleware::map_request(f)` | `async fn(Request) -> Request` (or `Result<Request, impl IntoResponse>`) | modify/reject requests |
| `middleware::map_response(f)` | `async fn(Response) -> Response` | modify responses |

Functions may also take extractors (`HeaderMap`, `State`, ...) before `Request`.

```rust
async fn response_time(State(threshold): State<Duration>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let mut response = next.run(request).await;          // call the inner service
    response.headers_mut().insert("x-response-time", ...);
    response
}
```

Short-circuit (e.g. auth): return a response without calling `next.run`.

Modifying a body: `response.into_parts()` → `axum::body::to_bytes(body, limit)` → change →
`Response::from_parts(parts, Body::from(...))`, remove stale `Content-Length`.

### Request extensions

A type-keyed map attached to each request – the way middleware passes data to later middleware and handlers.

```rust
request.extensions_mut().insert(RequestContext { request_id, path });   // middleware
request.extensions().get::<RequestContext>()                              // another middleware
async fn handler(Extension(ctx): Extension<RequestContext>) { ... }      // handler (500 if missing)
```

Step 019 uses the same idea for the authenticated user.

## What changed in this branch

- `src/api/middleware.rs` (new) – middleware stack, `RequestContext`, `response_time`,
  `enrich_problem_details` (adds `instance` + `requestId` to problems), `security_headers`
- `src/config.rs`, `config/default.toml` – `[http]` section (timeout, body limit, CORS origins, slow threshold)
- `src/app.rs` – applies the stack to the router
- `Cargo.toml` – `tower-http` (features), `tower`

## Try it

```bash
cargo run
B=localhost:3000/api/v1
curl -i $B/rooms                                              # x-request-id, x-response-time, security headers
curl -i $B/rooms/abc -H 'x-request-id: my-trace-123'          # problem with instance + requestId
curl -s -D - -o /dev/null -H 'accept-encoding: gzip' $B/rooms # content-encoding: gzip
curl -i -X OPTIONS $B/rooms -H 'origin: http://localhost:5173' -H 'access-control-request-method: POST'
APP_HTTP__BODY_LIMIT_BYTES=100 cargo run                      # then POST a larger body -> 413
APP_HTTP__CORS_ALLOWED_ORIGINS=http://a.test,http://b.test cargo run
```

## Exercises

1. Add a `from_fn` middleware that rejects non-GET requests with 503 when `APP_HTTP__READ_ONLY=true`.
2. Use `Extension<RequestContext>` in a handler and log the request id when a booking is created.
3. Add `tower::limit::ConcurrencyLimitLayer` and observe behavior under load (`hey`, `oha`).
