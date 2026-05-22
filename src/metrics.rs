use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::OnceLock;

static PROMETHEUS_HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();

/// Initialize Prometheus metrics recorder and store handle for rendering.
pub fn init_prometheus() -> anyhow::Result<()> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("Failed to install Prometheus recorder: {}", e))?;
    PROMETHEUS_HANDLE.set(handle).ok();
    Ok(())
}

/// Get the rendered Prometheus metrics.
pub fn render_metrics() -> String {
    if let Some(handle) = PROMETHEUS_HANDLE.get() {
        handle.render()
    } else {
        String::new()
    }
}
