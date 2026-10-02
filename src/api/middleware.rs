//! HTTP middleware: cross-cutting concerns applied to every request/response.
//!
//! A middleware wraps the inner service (router/handlers) and can:
//! - inspect/modify the request before it reaches the handler,
//! - short-circuit (return a response without calling the handler),
//! - inspect/modify the response on its way out.
//!
//! Two kinds are used:
//! - ready-made Tower layers from `tower-http` (request id, tracing, CORS, timeout, compression...),
//! - custom middleware written as plain async functions (`axum::middleware::from_fn` & co.).

use std::{any::Any, time::Duration, time::Instant};

use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use tower::ServiceBuilder;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{api::problem::ProblemDetails, config::HttpSettings};

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const RESPONSE_TIME: HeaderName = HeaderName::from_static("x-response-time");

/// Wraps the router with the whole middleware stack.
pub fn apply(router: Router, settings: &HttpSettings) -> Router {
    // `ServiceBuilder` composes layers. Order matters: the FIRST layer is the OUTERMOST –
    // it sees the request first and the response last:
    //
    //   request ──▶ set id ─▶ trace ─▶ ... ─▶ handler
    //   response ◀─ set id ◀─ trace ◀─ ... ◀─ handler
    //
    // (Calling `router.layer(a).layer(b)` repeatedly is the opposite: the LAST call is outermost.)
    let layers = ServiceBuilder::new()
        // 0. Mark credentials as sensitive (step 018): `Debug` output of these header values
        //    shows `Sensitive` instead of the value, so tracing/logging can't leak them.
        .layer(SetSensitiveRequestHeadersLayer::new([
            header::AUTHORIZATION,
            header::COOKIE,
        ]))
        // 1. Generate `x-request-id` (UUID) unless the client/proxy already sent one.
        .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
        // 2. A tracing span per request: method, URI, request id; logs status + latency at the end.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| {
                    let request_id = request
                        .headers()
                        .get(&REQUEST_ID)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("-");
                    tracing::info_span!(
                        "http",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id,
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // 3. Copy `x-request-id` from the request to the response (clients can report it).
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        // 4. A panic in a handler becomes a 500 response instead of a dropped connection.
        .layer(CatchPanicLayer::custom(panic_to_problem))
        // 5. Abort requests running too long (slow clients, stuck downstream calls).
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(settings.request_timeout_secs),
        ))
        // 6. CORS – which browser origins may call the API.
        .layer(cors(settings))
        // 7. gzip/brotli response compression, negotiated with the `Accept-Encoding` header.
        .layer(CompressionLayer::new())
        // 8. Max body size for body extractors (`Json`...). Exceeding it -> 413 via `JsonRejection`,
        //    so the error is rendered as Problem Details like all others (default limit: 2 MB).
        .layer(DefaultBodyLimit::max(settings.body_limit_bytes))
        // 9. Custom middleware (functions below).
        .layer(middleware::from_fn_with_state(
            Duration::from_millis(settings.slow_request_threshold_ms),
            response_time,
        ))
        .layer(middleware::map_request(attach_request_context))
        .layer(middleware::from_fn(enrich_problem_details))
        .layer(middleware::map_response(security_headers));

    router.layer(layers)
}

fn cors(settings: &HttpSettings) -> CorsLayer {
    let origins: Vec<HeaderValue> = settings
        .cors_allowed_origins
        .iter()
        .map(|origin| {
            origin
                .parse()
                .expect("invalid origin in http.cors_allowed_origins")
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        // Response headers JavaScript is allowed to read (besides the CORS-safelisted ones).
        .expose_headers([header::LOCATION, REQUEST_ID])
        // Browsers may cache the preflight (OPTIONS) result for an hour.
        .max_age(Duration::from_secs(3600))
}

// `Box<dyn Any + Send>` is the panic payload. It is not exposed to the client.
fn panic_to_problem(_payload: Box<dyn Any + Send + 'static>) -> Response {
    tracing::error!("handler panicked");
    ProblemDetails::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal-error",
        "Internal server error",
    )
    .into_response()
}

// ---------------------------------------------------------------------------
// Custom middleware
// ---------------------------------------------------------------------------

/// Per-request data shared through request *extensions* – a type-keyed map attached to every
/// request. Middleware inserts values; later middleware or handlers read them
/// (handlers with the `Extension<RequestContext>` extractor).
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub request_id: String,
    pub path: String,
}

// `map_request` – the simplest form: transform the request, no access to the response.
// Returning `Result<Request, impl IntoResponse>` instead would allow rejecting the request.
async fn attach_request_context(mut request: Request) -> Request {
    let context = RequestContext {
        request_id: request
            .headers()
            .get(&REQUEST_ID)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        path: request.uri().path().to_string(),
    };
    request.extensions_mut().insert(context);
    request
}

// `from_fn_with_state` – a full middleware: gets the request and `Next` (the rest of the stack).
// Code before `next.run()` runs on the way in, code after it on the way out.
// `State` here is the middleware's own state (the threshold), independent of `AppState`.
async fn response_time(
    State(threshold): State<Duration>,
    request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    let mut response = next.run(request).await;

    let elapsed = started.elapsed();
    if elapsed > threshold {
        tracing::warn!(%method, path, elapsed_ms = elapsed.as_millis(), "slow request");
    }
    // Header values must be valid ASCII; `from_str` would fail otherwise.
    if let Ok(value) = HeaderValue::from_str(&format!("{:.3}ms", elapsed.as_secs_f64() * 1000.0)) {
        response.headers_mut().insert(RESPONSE_TIME, value);
    }
    response
}

// Modifying a response BODY: buffer it, change it, put it back.
// Adds RFC 9457 `instance` (request path) and `requestId` to every Problem Details response,
// information `ApiError::into_response` doesn't have access to.
async fn enrich_problem_details(request: Request, next: Next) -> Response {
    let context = request.extensions().get::<RequestContext>().cloned();
    let response = next.run(request).await;

    let is_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value == "application/problem+json");
    let Some(context) = context.filter(|_| is_problem) else {
        return response;
    };

    // `into_parts` separates status/headers from the body stream.
    let (mut parts, body) = response.into_parts();
    // Problem bodies are tiny; the limit protects against buffering something unexpected.
    let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(mut problem) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    if let Some(object) = problem.as_object_mut() {
        object.insert("instance".into(), context.path.into());
        object.insert("requestId".into(), context.request_id.into());
    }
    // The body length changed – drop the stale header (hyper computes it again).
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(problem.to_string()))
}

// `map_response` – transform only the response.
async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    // Don't let browsers guess (sniff) a different content type than declared.
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // API responses contain user data – don't store them in shared caches.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
