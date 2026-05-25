use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

static PROMETHEUS_HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();

// Gauge state tracked natively because our module is also named `metrics`, which
// shadows the external `metrics` crate and makes `metrics::gauge!()` ambiguous.
static EXACT_CACHE_SIZE: AtomicU64 = AtomicU64::new(0);
static SEMANTIC_CACHE_SIZE: AtomicU64 = AtomicU64::new(0);
static TOTAL_REQUESTS: AtomicU64 = AtomicU64::new(0);
static CACHE_HIT_REQUESTS: AtomicU64 = AtomicU64::new(0);

/// Initialize Prometheus metrics recorder and store handle for rendering.
pub fn init_prometheus() -> anyhow::Result<()> {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("Failed to install Prometheus recorder: {}", e))?;
    PROMETHEUS_HANDLE.set(handle).ok();
    Ok(())
}

pub fn set_exact_cache_size(n: usize) {
    EXACT_CACHE_SIZE.store(n as u64, Ordering::Relaxed);
}

pub fn set_semantic_cache_size(n: usize) {
    SEMANTIC_CACHE_SIZE.store(n as u64, Ordering::Relaxed);
}

/// Record one completed request. `cache_hit` is true for exact or semantic hits.
pub fn record_request(cache_hit: bool) {
    TOTAL_REQUESTS.fetch_add(1, Ordering::Relaxed);
    if cache_hit {
        CACHE_HIT_REQUESTS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Current cache hit ratio in [0.0, 1.0]. Returns 0.0 when no requests recorded yet.
pub fn cache_hit_ratio() -> f64 {
    let total = TOTAL_REQUESTS.load(Ordering::Relaxed);
    if total == 0 {
        return 0.0;
    }
    CACHE_HIT_REQUESTS.load(Ordering::Relaxed) as f64 / total as f64
}

/// Get the rendered Prometheus metrics, including cache gauges appended at the end.
pub fn render_metrics() -> String {
    let mut output = if let Some(handle) = PROMETHEUS_HANDLE.get() {
        handle.render()
    } else {
        String::new()
    };

    let exact = EXACT_CACHE_SIZE.load(Ordering::Relaxed);
    let semantic = SEMANTIC_CACHE_SIZE.load(Ordering::Relaxed);
    let ratio = cache_hit_ratio();

    output.push_str(&format!(
        "# HELP janus_cache_size Number of entries in the in-memory cache layer\n\
         # TYPE janus_cache_size gauge\n\
         janus_cache_size{{layer=\"exact\"}} {exact}\n\
         janus_cache_size{{layer=\"semantic\"}} {semantic}\n\
         # HELP janus_cache_hit_ratio Fraction of requests served from cache since process start\n\
         # TYPE janus_cache_hit_ratio gauge\n\
         janus_cache_hit_ratio {ratio:.6}\n"
    ));

    output
}
