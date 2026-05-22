use crate::{db::analytics as db_analytics, errors::AppResult, state::AppState};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CostQuery {
    /// Number of days to look back (default: 30).
    #[serde(default = "default_30")]
    pub days: i32,
}

#[derive(Debug, Deserialize)]
pub struct HoursQuery {
    /// Number of hours to look back (default: 24).
    #[serde(default = "default_24")]
    pub hours: i32,
}

fn default_30() -> i32 {
    30
}
fn default_24() -> i32 {
    24
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /admin/analytics/overview
///
/// Returns aggregated stats for today, last 7 days, and last 30 days:
/// request count, cost, tokens, cache-hit count, error count, avg latency.
pub async fn overview(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let stats = db_analytics::overview_stats(&state.pool).await?;
    Ok(Json(stats))
}

/// GET /admin/analytics/costs?days=30
///
/// Cost breakdown split three ways: by calendar day, by provider, by model.
pub async fn costs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CostQuery>,
) -> AppResult<Json<Value>> {
    let days = params.days.clamp(1, 365);
    let data = db_analytics::cost_breakdown(&state.pool, days).await?;
    Ok(Json(data))
}

/// GET /admin/analytics/latency?hours=24
///
/// p50/p95/p99 latency per model+provider over the requested window.
pub async fn latency(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HoursQuery>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.clamp(1, 720);
    let rows = db_analytics::latency_percentiles(&state.pool, hours).await?;
    Ok(Json(serde_json::json!({ "data": rows })))
}

/// GET /admin/analytics/cache?hours=24
///
/// Cache hit rate, tokens saved, cost saved — split by exact vs semantic.
pub async fn cache(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HoursQuery>,
) -> AppResult<Json<Value>> {
    let hours = params.hours.clamp(1, 720);
    let data = db_analytics::cache_analytics(&state.pool, hours).await?;
    Ok(Json(data))
}
