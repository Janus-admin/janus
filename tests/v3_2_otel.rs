// tests/v3_2_otel.rs
// Phase V3-2 acceptance tests — OpenTelemetry distributed tracing.
//
// Run with: cargo test v3_2
// Pure unit tests — no DB, no HTTP server, no real OTLP endpoint required.

use std::sync::OnceLock;

use janus::config::TracingConfig;
use opentelemetry::trace::TraceContextExt;
use opentelemetry_sdk::{
    testing::trace::InMemorySpanExporter, trace::TracerProvider as SdkTracerProvider,
};
use serial_test::serial;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

// ── Test harness ──────────────────────────────────────────────────────────────

/// Install a single in-memory exporter once per test process.
/// All tests share the same exporter; call `exporter.reset()` at the start of
/// each test that checks span content.
static TEST_EXPORTER: OnceLock<InMemorySpanExporter> = OnceLock::new();

fn get_exporter() -> &'static InMemorySpanExporter {
    TEST_EXPORTER.get_or_init(|| {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        // Install as global tracer so tracing-opentelemetry picks it up.
        opentelemetry::global::set_tracer_provider(provider.clone());

        // Also register the W3C propagator (mirrors production init).
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        // Wire into the tracing subscriber. `try_init` is safe to call multiple
        // times — subsequent calls return Err and are silently ignored.
        let otel_layer = tracing_opentelemetry::layer().with_tracer(
            opentelemetry::trace::TracerProvider::tracer(&provider, "janus-test"),
        );
        let _ = tracing_subscriber::registry().with(otel_layer).try_init();

        exporter
    })
}

// ── TracingConfig defaults ────────────────────────────────────────────────────

#[test]
fn v3_2_tracing_config_defaults_to_disabled() {
    let cfg = TracingConfig::default();
    assert!(!cfg.enabled, "tracing must be off by default");
}

#[test]
fn v3_2_tracing_config_default_endpoint() {
    let cfg = TracingConfig::default();
    assert_eq!(cfg.otlp_endpoint, "http://localhost:4317");
}

#[test]
fn v3_2_tracing_config_default_service_name() {
    let cfg = TracingConfig::default();
    assert_eq!(cfg.service_name, "janus");
}

#[test]
fn v3_2_tracing_config_default_sample_rate_is_one() {
    let cfg = TracingConfig::default();
    assert!(
        (cfg.sample_rate - 1.0).abs() < f64::EPSILON,
        "default sample rate must be 1.0"
    );
}

// ── init_tracer disabled path ────────────────────────────────────────────────

#[test]
fn v3_2_tracing_disabled_init_returns_none() {
    let cfg = TracingConfig::default(); // enabled = false
    let result = janus::telemetry::init_tracer(&cfg);
    assert!(result.is_ok(), "init_tracer must not error when disabled");
    assert!(
        result.unwrap().is_none(),
        "disabled init must return None provider"
    );
}

// ── Span creation ─────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn v3_2_request_produces_root_span() {
    let exporter = get_exporter();
    exporter.reset();

    let span = tracing::info_span!("janus.request", janus.model = "gpt-4o");
    async {
        // simulate pipeline work
        let _inner = tracing::info_span!("janus.cache.exact_lookup").entered();
    }
    .instrument(span)
    .await;

    let spans = exporter.get_finished_spans().unwrap();
    assert!(
        spans.iter().any(|s| s.name == "janus.request"),
        "root span 'janus.request' must be exported"
    );
}

#[tokio::test]
#[serial]
async fn v3_2_cache_hit_span_produced() {
    let exporter = get_exporter();
    exporter.reset();

    let root = tracing::info_span!("janus.request");
    async {
        let _exact = tracing::info_span!("janus.cache.exact_lookup").entered();
        drop(_exact);
        let _sem = tracing::info_span!("janus.cache.semantic_lookup").entered();
    }
    .instrument(root)
    .await;

    let spans = exporter.get_finished_spans().unwrap();
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"janus.cache.exact_lookup"));
    assert!(names.contains(&"janus.cache.semantic_lookup"));
}

#[tokio::test]
#[serial]
async fn v3_2_provider_call_span_has_model_attribute() {
    let exporter = get_exporter();
    exporter.reset();

    let root = tracing::info_span!("janus.request");
    async {
        let _prov = tracing::info_span!(
            "janus.provider.call",
            janus.provider = "openai",
            janus.model = "gpt-4o",
        )
        .entered();
    }
    .instrument(root)
    .await;

    let spans = exporter.get_finished_spans().unwrap();
    let provider_span = spans
        .iter()
        .find(|s| s.name == "janus.provider.call")
        .expect("janus.provider.call span must be exported");

    let model_attr = provider_span
        .attributes
        .iter()
        .find(|kv| kv.key.as_str() == "janus.model");
    assert!(
        model_attr.is_some(),
        "janus.model attribute must be present"
    );
    assert_eq!(
        model_attr.unwrap().value.as_str(),
        "gpt-4o",
        "janus.model must equal the request model"
    );
}

#[tokio::test]
#[serial]
async fn v3_2_span_includes_token_counts_on_success() {
    let exporter = get_exporter();
    exporter.reset();

    let root = tracing::info_span!("janus.request");
    async {
        let prov_span = tracing::info_span!(
            "janus.provider.call",
            janus.provider = "openai",
            janus.model = "gpt-4o",
            janus.prompt_tokens = tracing::field::Empty,
            janus.completion_tokens = tracing::field::Empty,
        );
        let prov_ref = prov_span.clone();
        async {}.instrument(prov_span).await;
        // Record after the future, simulating what pipeline.rs does.
        prov_ref
            .record("janus.prompt_tokens", 142u64)
            .record("janus.completion_tokens", 88u64);
    }
    .instrument(root)
    .await;

    let spans = exporter.get_finished_spans().unwrap();
    let prov = spans
        .iter()
        .find(|s| s.name == "janus.provider.call")
        .expect("provider span must exist");

    let has_prompt = prov
        .attributes
        .iter()
        .any(|kv| kv.key.as_str() == "janus.prompt_tokens");
    let has_completion = prov
        .attributes
        .iter()
        .any(|kv| kv.key.as_str() == "janus.completion_tokens");
    assert!(has_prompt, "prompt_tokens attribute must be recorded");
    assert!(
        has_completion,
        "completion_tokens attribute must be recorded"
    );
}

// ── Incoming traceparent propagation ─────────────────────────────────────────

#[test]
fn v3_2_extract_context_parses_valid_traceparent() {
    get_exporter(); // ensure propagator is registered

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "traceparent",
        axum::http::HeaderValue::from_static(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
    );

    let ctx = janus::telemetry::extract_context(&headers);
    let otel_span = ctx.span();
    let span_ctx = otel_span.span_context();
    assert!(
        span_ctx.is_valid(),
        "extracted span context must be valid when traceparent is present"
    );
    assert_eq!(
        format!("{:032x}", span_ctx.trace_id()),
        "4bf92f3577b34da6a3ce929d0e0e4736",
        "trace ID must match the traceparent header"
    );
}

#[test]
fn v3_2_extract_context_returns_empty_when_no_traceparent() {
    get_exporter(); // ensure propagator is registered

    let headers = axum::http::HeaderMap::new();
    let ctx = janus::telemetry::extract_context(&headers);
    let otel_span = ctx.span();
    let span_ctx = otel_span.span_context();
    assert!(
        !span_ctx.is_valid(),
        "context must be invalid (root) when no traceparent header present"
    );
}

#[tokio::test]
#[serial]
async fn v3_2_incoming_traceparent_header_linked_to_root_span() {
    let exporter = get_exporter();
    exporter.reset();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "traceparent",
        axum::http::HeaderValue::from_static(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
    );

    let parent_cx = janus::telemetry::extract_context(&headers);
    let span = tracing::info_span!("janus.request");
    {
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        span.set_parent(parent_cx);
    }
    async {}.instrument(span).await;

    let spans = exporter.get_finished_spans().unwrap();
    let root = spans
        .iter()
        .find(|s| s.name == "janus.request")
        .expect("root span must be exported");

    // When set_parent is called with an upstream context, the span's trace_id
    // must equal the upstream trace_id from the traceparent header.
    let trace_id_hex = format!("{:?}", root.span_context.trace_id());
    assert!(
        trace_id_hex.to_lowercase().contains("4bf92f3577b34da6a3ce929d0e0e4736"),
        "root span trace_id must equal the upstream trace_id from traceparent header, got: {trace_id_hex}"
    );
}

// ── Outgoing trace header injection ──────────────────────────────────────────

#[tokio::test]
#[serial]
async fn v3_2_inject_trace_headers_is_noop_without_active_span() {
    let client = reqwest::Client::new();
    // Build a request but don't send it — verify no panic outside a span.
    let builder = client.get("http://127.0.0.1:1/noop");
    let builder = janus::telemetry::inject_trace_headers(builder);
    drop(builder); // should not panic
}

#[tokio::test]
#[serial]
async fn v3_2_inject_trace_headers_adds_traceparent_inside_span() {
    get_exporter(); // ensure OTel subscriber is active

    let client = reqwest::Client::new();
    let span = tracing::info_span!("janus.provider.call", janus.provider = "openai");

    let captured_headers = async {
        // Inside the span, inject headers and capture them.
        let builder = client.get("http://127.0.0.1:1/test");
        let builder = janus::telemetry::inject_trace_headers(builder);
        // Build the request to inspect its headers (without sending).
        builder
            .build()
            .ok()
            .map(|r| r.headers().contains_key("traceparent"))
    }
    .instrument(span)
    .await;

    // When OTel is active and we're inside a span, traceparent should be injected.
    assert!(
        captured_headers.unwrap_or(false),
        "traceparent header must be injected when inside an active span"
    );
}

// ── Disabled tracing regression ──────────────────────────────────────────────

#[test]
fn v3_2_tracing_disabled_produces_no_provider() {
    let cfg = TracingConfig::default(); // enabled = false
    let result = janus::telemetry::init_tracer(&cfg).unwrap();
    assert!(
        result.is_none(),
        "disabled tracing must produce no SdkTracerProvider"
    );
}

#[test]
fn v3_2_regression_gateway_latency_tracing_disabled_is_fast() {
    use std::time::Instant;

    let cfg = TracingConfig::default(); // enabled = false
    let _result = janus::telemetry::init_tracer(&cfg).unwrap();

    // Simulate the overhead path: create + drop a span with no active exporter.
    let start = Instant::now();
    for _ in 0..1_000 {
        let span = tracing::info_span!("janus.request", janus.model = "gpt-4o");
        drop(span);
    }
    let elapsed = start.elapsed();

    // 1 000 no-op span creates must complete in well under 50 ms on any hardware.
    assert!(
        elapsed.as_millis() < 50,
        "1000 span creates when disabled must complete < 50ms, got {:?}",
        elapsed
    );
}
