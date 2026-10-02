# 023 – Logging and monitoring

## Goal

Make the service observable: structured logs, distributed traces (OpenTelemetry → Jaeger),
metrics (Prometheus), health probes – and know which signal answers which question.

## Key concepts

### Three signals

| Signal | Answers | Tooling here |
|--------|---------|--------------|
| **Logs** | what exactly happened in this request? | `tracing` + `tracing-subscriber` (pretty / JSON) |
| **Traces** | where did the time go, across services? | `tracing-opentelemetry` → OTLP → Jaeger |
| **Metrics** | how is the system doing overall, trends, alerts? | `metrics` + Prometheus exporter → Prometheus |

Plus health probes (step 014): `/health/live`, `/health/ready`.

### `tracing` – spans and events, layered subscriber

```rust
tracing_subscriber::registry()
    .with(EnvFilter::from_default_env())         // RUST_LOG
    .with(pretty_layer)                          // Option<Layer>: dev
    .with(json_layer)                            // Option<Layer>: production
    .with(otel_layer)                            // Option<Layer>: trace export
    .init();
```

JSON (one object per line) – parsed by Loki/ELK/CloudWatch without regexes:

```json
{"timestamp":"...","level":"INFO","message":"finished processing request","latency":"23 ms","status":200,
 "span":{"method":"POST","uri":"/api/v1/auth/login","request_id":"cbd69a3c-...","name":"http"}}
```

`APP_TELEMETRY__LOG_FORMAT=json` (default in `config/production.toml`).

### Spans with `#[instrument]`

```rust
#[tracing::instrument(
    skip(self, new_booking),                                     // don't Debug-print arguments
    fields(room_id = %new_booking.room_id, user_id = %new_booking.user_id),
    err(level = "info", Display)                                 // log returned errors
)]
pub async fn create_booking(&self, new_booking: NewBooking) -> DomainResult<Booking>
```

- events inside inherit span fields → every log line has `request_id`, `room_id`...,
- with OpenTelemetry each span becomes a trace span with its duration,
- **never record secrets** (`skip(password)`), be careful with personal data (GDPR),
- log levels: ERROR = needs attention, WARN = suspicious, INFO = business-relevant event,
  DEBUG/TRACE = diagnostics. Expected outcomes (409, 401) are not errors.

### Distributed tracing – OpenTelemetry

```
client ──traceparent: 00-<trace-id>-<span-id>-01──▶ room-booking (span "http" ─▶ "create_booking")
                                                         └── OTLP/HTTP :4318 ──▶ Jaeger UI :16686
```

```rust
let exporter = opentelemetry_otlp::SpanExporter::builder().with_http()
    .with_endpoint("http://localhost:4318/v1/traces").build()?;
let provider = SdkTracerProvider::builder().with_batch_exporter(exporter)
    .with_resource(Resource::builder().with_service_name("room-booking-service").build()).build();
let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("room-booking"));
global::set_text_map_propagator(TraceContextPropagator::new());   // W3C traceparent
...
provider.shutdown()?;                                              // flush on exit
```

Incoming `traceparent` is extracted in the `TraceLayer` span (`span.set_parent(...)`), so traces
continue across services. Outgoing calls should inject it (e.g. `reqwest` middleware).
OpenTelemetry crates must have matching versions (`opentelemetry*` 0.33 ↔ `tracing-opentelemetry` 0.34).
Sampling (e.g. 10 % of traces) keeps costs under control in production.

### Metrics – Prometheus

```rust
PrometheusBuilder::new()
    .set_buckets_for_metric(Matcher::Full("http_request_duration_seconds".into()), &[0.005, ..., 5.0])?
    .install_recorder()?;                                   // global recorder; GET /metrics renders it

metrics::counter!("http_requests_total", &labels).increment(1);
metrics::histogram!("http_request_duration_seconds", &labels).record(secs);
metrics::counter!("bookings_created_total").increment(1);           // business metric
metrics::counter!("api_problems_total", "type" => "conflict").increment(1);
```

| Type | Use |
|------|-----|
| counter | monotonically increasing (requests, errors, bookings) → `rate()` in PromQL |
| gauge | current value (pool connections, queue length) |
| histogram | distributions (latency) → percentiles: `histogram_quantile(0.95, ...)` |

RED method per endpoint: **R**ate, **E**rrors, **D**uration. Labels must have **low cardinality** –
use the route template (`MatchedPath`: `/api/v1/rooms/{id}`), never raw URIs, user ids or e-mails.
`track_metrics` is installed with `route_layer` so `MatchedPath` is already known.

Useful queries (http://localhost:9090):

```promql
sum by (path) (rate(http_requests_total[1m]))
sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m]))
histogram_quantile(0.95, sum by (le, path) (rate(http_request_duration_seconds_bucket[5m])))
increase(bookings_created_total[1h])
```

The recorder needs periodic upkeep → background task with cancellation token (`server::metrics_upkeep`).
Expose `/metrics` only internally in production.

### Local stack (`compose.yaml`)

| Service | URL |
|---------|-----|
| Jaeger | http://localhost:16686 (OTLP 4317/4318) |
| Prometheus | http://localhost:9090 (scrapes `host.docker.internal:3000/metrics`) |

Grafana (dashboards for both) and Loki (logs) are the usual next additions.

## What changed in this branch

- `src/telemetry.rs` – layered subscriber (pretty/JSON), OTLP trace export, Prometheus recorder
- `src/main.rs` – telemetry init + flush on exit; `src/server.rs` – metrics upkeep task
- `src/api/middleware.rs` – `traceparent` extraction, `track_metrics` (RED metrics)
- `src/api/mod.rs` – `route_layer(track_metrics)`; `src/api/health.rs` – `GET /metrics`
- `src/domain/{booking,auth}_service.rs` – `#[instrument]`; `src/api/{bookings,error}.rs` – business/error counters
- `src/config.rs`, `config/*.toml`, `.env.example` – `[telemetry]`
- `compose.yaml` – `jaeger`, `prometheus`; `observability/prometheus.yml` (new)
- `Cargo.toml` – `opentelemetry*`, `tracing-opentelemetry`, `metrics`, `metrics-exporter-prometheus`, `tracing-subscriber/json`

## Try it

```bash
docker compose up -d
APP_TELEMETRY__LOG_FORMAT=json APP_TELEMETRY__OTLP_ENDPOINT=http://localhost:4318 cargo run
curl localhost:3000/api/v1/rooms; curl localhost:3000/api/v1/rooms/0199a3f0-0000-7000-8000-00000000ffff
curl -s localhost:3000/metrics | grep http_requests_total
curl -H 'traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01' localhost:3000/api/v1/rooms
open http://localhost:16686/trace/4bf92f3577b34da6a3ce929d0e0e4736    # Jaeger
open http://localhost:9090                                             # Prometheus
```

## Exercises

1. Add a gauge `db_pool_connections{state="idle|used"}` updated by the upkeep task from `PgPool::size()`/`num_idle()`.
2. Add `trace_id` to every JSON log line (read it from the current OpenTelemetry span context).
3. Add Grafana to compose with Prometheus and Jaeger as data sources and build a RED dashboard.
