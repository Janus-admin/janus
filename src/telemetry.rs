//! OpenTelemetry initialization and context-propagation helpers (V3-2).
//!
//! When `tracing.enabled = false`, `init_tracer` returns `(None, None)` and the
//! `tracing-opentelemetry` layer is not added to the subscriber — zero export overhead.
//!
//! When enabled, a gRPC OTLP exporter is created. The returned `SdkTracerProvider`
//! must be shut down at program exit via `shutdown(provider)`.
//!
//! W3C Trace Context headers:
//! - `extract_context` reads `traceparent` from incoming request headers.
//! - `inject_trace_headers` injects the current span's context into outgoing HTTP calls.

use crate::config::TracingConfig;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::{Config as TraceConfig, TracerProvider as SdkTracerProvider},
};
use std::collections::HashMap;

/// Initialise the global OTel tracer and register the W3C propagator.
///
/// Returns `Some(provider)` when `config.enabled = true` — the caller must add
/// the OTel subscriber layer inline (see `main.rs`) and call `shutdown(provider)`
/// at process exit to flush pending spans.
///
/// Returns `None` when disabled — no OTLP connection, no export overhead.
pub fn init_tracer(config: &TracingConfig) -> anyhow::Result<Option<SdkTracerProvider>> {
    // Always register the W3C propagator so `extract_context` can parse
    // `traceparent` headers regardless of whether export is enabled.
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    if !config.enabled {
        return Ok(None);
    }

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build_span_exporter()?;

    let sampler = if config.sample_rate >= 1.0 {
        opentelemetry_sdk::trace::Sampler::AlwaysOn
    } else {
        opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(config.sample_rate)
    };

    let trace_config = TraceConfig::default().with_sampler(sampler).with_resource(
        opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            config.service_name.clone(),
        )]),
    );

    let provider = SdkTracerProvider::builder()
        .with_config(trace_config)
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(Some(provider))
}

/// Flush all pending spans and shut down the exporter.
///
/// In SDK 0.23, the provider shuts down its processors via `Drop` on the
/// inner `Arc`. We call `force_flush()` first to drain any buffered spans,
/// then drop the provider to trigger the processor shutdown chain.
pub fn shutdown(provider: SdkTracerProvider) {
    for result in provider.force_flush() {
        if let Err(e) = result {
            tracing::warn!("OTel force-flush error: {e}");
        }
    }
    drop(provider);
}

// ── W3C Trace Context propagation ─────────────────────────────────────────────

/// Extract the OTel `Context` encoded in incoming request headers.
///
/// Returns an empty root context when no `traceparent` header is present.
/// Pass the result to `OpenTelemetrySpanExt::set_parent` on the root span.
pub fn extract_context(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    opentelemetry::global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderMapExtractor(headers))
    })
}

/// Inject the current span's OTel context into an outgoing `reqwest::RequestBuilder`.
///
/// Attaches the `traceparent` (and `tracestate`) header so downstream services
/// and LLM providers can correlate their traces with this gateway request.
/// When no span is active this is a no-op (nothing is injected).
pub fn inject_trace_headers(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let cx = tracing::Span::current().context();
    let mut carrier: HashMap<String, String> = HashMap::new();
    opentelemetry::global::get_text_map_propagator(|prop| {
        prop.inject_context(&cx, &mut HashMapInjector(&mut carrier));
    });

    let mut builder = builder;
    for (k, v) in carrier {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(&v),
        ) {
            builder = builder.header(name, val);
        }
    }
    builder
}

// ── Private extractor / injector adapters ─────────────────────────────────────

struct HeaderMapExtractor<'a>(&'a axum::http::HeaderMap);

impl Extractor for HeaderMapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

struct HashMapInjector<'a>(&'a mut HashMap<String, String>);

impl Injector for HashMapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_lowercase(), value);
    }
}
