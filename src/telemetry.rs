//! Observability: logs, traces and metrics.
//!
//! Step 004: basic `tracing` subscriber. Step 023:
//! - logs as pretty text (dev) or JSON lines (production),
//! - traces exported via OpenTelemetry (OTLP) to Jaeger / any OTel-compatible backend,
//! - metrics in Prometheus format at `GET /metrics`.

use std::sync::OnceLock;

use anyhow::Context;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{LogFormat, TelemetrySettings};

/// Keeps the trace exporter alive; call `shutdown` before exiting to flush buffered spans.
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl TelemetryGuard {
    pub fn shutdown(self) {
        if let Some(provider) = self.tracer_provider
            && let Err(error) = provider.shutdown()
        {
            eprintln!("failed to flush traces: {error}");
        }
    }
}

// `tracing` separates *producing* events/spans from *consuming* them. A subscriber is built from
// layers – each layer gets every event/span: one filters, one prints, one exports to OpenTelemetry.
pub fn init(settings: &TelemetrySettings) -> anyhow::Result<TelemetryGuard> {
    // `EnvFilter` reads the `RUST_LOG` variable, e.g. `info,rust_services=debug,tower_http=trace`.
    // Fallback when the variable is not set: `info` for everything.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // `Option<Layer>` is itself a layer (`None` = disabled), which keeps the types simple.
    let (pretty, json) = match settings.log_format {
        LogFormat::Pretty => (Some(tracing_subscriber::fmt::layer()), None),
        LogFormat::Json => (
            None,
            Some(
                tracing_subscriber::fmt::layer()
                    .json()
                    // Fields of the event at the top level instead of nested in "fields".
                    .flatten_event(true)
                    // Fields of the current span (method, uri, request_id...) on every line.
                    .with_current_span(true)
                    .with_span_list(false),
            ),
        ),
    };

    let tracer_provider = settings
        .otlp_endpoint
        .as_deref()
        .map(|endpoint| tracer_provider(endpoint, &settings.service_name))
        .transpose()?;
    // Bridge `tracing` spans to OpenTelemetry spans.
    let otel = tracer_provider.as_ref().map(|provider| {
        tracing_opentelemetry::layer().with_tracer(provider.tracer("room-booking"))
    });

    // W3C Trace Context (`traceparent` header): continue traces started by callers (step 023,
    // see `api::middleware`) – one trace across many services.
    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(filter)
        .with(pretty)
        .with(json)
        .with(otel)
        .try_init()
        .context("failed to install tracing subscriber")?;

    Ok(TelemetryGuard { tracer_provider })
}

fn tracer_provider(endpoint: &str, service_name: &str) -> anyhow::Result<SdkTracerProvider> {
    // OTLP over HTTP/protobuf (port 4318). The batch processor buffers spans and sends them
    // from a background thread – request handling never waits for the exporter.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{endpoint}/v1/traces"))
        .build()
        .context("failed to create OTLP exporter")?;

    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(service_name.to_string())
                .build(),
        )
        .build())
}

static METRICS: OnceLock<PrometheusHandle> = OnceLock::new();

/// Installs the global metrics recorder (Prometheus format).
///
/// `metrics` is a facade like `tracing`/`log`: code records with `counter!`, `histogram!`,
/// `gauge!`; the installed recorder decides where values go. Without a recorder (tests) the
/// macros are no-ops.
pub fn init_metrics() -> anyhow::Result<()> {
    let handle = PrometheusBuilder::new()
        // Histogram buckets (seconds) for request latency – choose them around your SLOs.
        .set_buckets_for_metric(
            Matcher::Full("http_request_duration_seconds".into()),
            &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0],
        )?
        .install_recorder()?;
    METRICS
        .set(handle)
        .map_err(|_| anyhow::anyhow!("metrics already initialized"))
}

/// Handle used by `GET /metrics` and by the periodic upkeep task; `None` when not initialized.
pub fn metrics_handle() -> Option<&'static PrometheusHandle> {
    METRICS.get()
}
