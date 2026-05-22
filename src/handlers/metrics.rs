use crate::{metrics, state::AppState};
use axum::{extract::State, http::header, response::IntoResponse};
use std::sync::Arc;

/// GET /metrics — Prometheus text format (0.0.4).
///
/// Cache size and hit-ratio gauges are refreshed from live in-memory state on every
/// scrape so they reflect the current snapshot rather than lagging counters.
pub async fn prometheus_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    metrics::set_exact_cache_size(state.cache.len());

    let semantic_len = state.cache.semantic.as_ref().map(|s| s.len()).unwrap_or(0);
    metrics::set_semantic_cache_size(semantic_len);

    let output = metrics::render_metrics();
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        output,
    )
}
