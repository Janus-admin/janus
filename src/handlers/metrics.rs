use crate::metrics;
use axum::http::header;
use axum::response::IntoResponse;

/// Handler to serve Prometheus metrics.
/// Returns metrics in Prometheus text format (0.0.4).
pub async fn prometheus_handler() -> impl IntoResponse {
    let metrics_output = metrics::render_metrics();

    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics_output,
    )
}
