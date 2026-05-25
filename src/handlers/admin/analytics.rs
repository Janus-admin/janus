use crate::{
    db::analytics as db_analytics,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct CostQuery {
    /// Number of days to look back (default: 30).
    #[serde(default = "default_30")]
    pub days: i32,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
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
#[utoipa::path(
    get,
    path = "/admin/analytics/overview",
    tag = "Analytics",
    responses(
        (status = 200, description = "Aggregated stats for today/7d/30d", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires BillingViewer role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn overview(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
    let stats = db_analytics::overview_stats(&state.pool).await?;
    Ok(Json(stats))
}

/// GET /admin/analytics/costs?days=30
///
/// Cost breakdown split three ways: by calendar day, by provider, by model.
#[utoipa::path(
    get,
    path = "/admin/analytics/costs",
    tag = "Analytics",
    params(CostQuery),
    responses(
        (status = 200, description = "Cost breakdown by day/provider/model", body = serde_json::Value),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn costs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<CostQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
    let days = params.days.clamp(1, 365);
    let data = db_analytics::cost_breakdown(&state.pool, days).await?;
    Ok(Json(data))
}

/// GET /admin/analytics/latency?hours=24
///
/// p50/p95/p99 latency per model+provider over the requested window.
#[utoipa::path(
    get,
    path = "/admin/analytics/latency",
    tag = "Analytics",
    params(HoursQuery),
    responses(
        (status = 200, description = "p50/p95/p99 latency per model+provider", body = serde_json::Value),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn latency(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HoursQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
    let hours = params.hours.clamp(1, 720);
    let rows = db_analytics::latency_percentiles(&state.pool, hours).await?;
    Ok(Json(serde_json::json!({ "data": rows })))
}

/// GET /admin/analytics/cache?hours=24
///
/// Cache hit rate, tokens saved, cost saved — split by exact vs semantic.
#[utoipa::path(
    get,
    path = "/admin/analytics/cache",
    tag = "Analytics",
    params(HoursQuery),
    responses(
        (status = 200, description = "Cache hit rate / tokens-saved / cost-saved", body = serde_json::Value),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn cache(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HoursQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
    let hours = params.hours.clamp(1, 720);
    let data = db_analytics::cache_analytics(&state.pool, hours).await?;
    Ok(Json(data))
}

// ── Cost by tag (V5-L3) ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct CostByTagQuery {
    /// The tag key to group by (e.g. `team`, `project`, `env`).
    pub tag: String,
    /// Number of days to look back (default: 30).
    #[serde(default = "default_30")]
    pub days: i32,
}

/// GET /admin/analytics/cost-by-tag?tag=team&days=30
///
/// Returns cost, request count, and share grouped by the value of a single tag key.
/// Rows where the tag is absent are reported with `tag_value = null`.
#[utoipa::path(
    get,
    path = "/admin/analytics/cost-by-tag",
    tag = "Analytics",
    params(CostByTagQuery),
    responses(
        (status = 200, description = "Cost grouped by tag value", body = serde_json::Value),
        (status = 400, description = "Invalid tag key"),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn cost_by_tag(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<CostByTagQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;

    // Validate the tag key: alphanumeric + underscore only.
    if params.tag.is_empty()
        || !params
            .tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
    {
        return Err(crate::errors::AppError::BadRequest(
            "tag key must be non-empty and contain only alphanumeric characters or underscores"
                .to_string(),
        ));
    }

    let days = params.days.clamp(1, 365);
    let data = db_analytics::cost_by_tag(&state.pool, &params.tag, days).await?;
    Ok(Json(data))
}

// ── Cost simulator ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SimulateQuery {
    /// `cost_optimized` | `round_robin` | `priority` (default: cost_optimized)
    #[serde(default = "default_cost_optimized")]
    pub strategy: String,
    /// `7d` | `30d` | `90d` (default: 30d)
    #[serde(default = "default_period_30d")]
    pub period: String,
    /// JSON object mapping model_id → replacement model_id, e.g. `{"gpt-4o":"gpt-4o-mini"}`
    pub model_overrides: Option<String>,
}

fn default_cost_optimized() -> String {
    "cost_optimized".to_string()
}
fn default_period_30d() -> String {
    "30d".to_string()
}

/// GET /admin/analytics/simulate?strategy=cost_optimized&period=30d
///
/// Recalculates costs for past requests under a different routing strategy and/or
/// with model substitutions. Returns original vs simulated cost + per-model breakdown.
#[utoipa::path(
    get,
    path = "/admin/analytics/simulate",
    tag = "Analytics",
    params(SimulateQuery),
    responses(
        (status = 200, description = "Original vs simulated cost", body = serde_json::Value),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn simulate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<SimulateQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;
    let strategy = match params.strategy.as_str() {
        "cost_optimized" | "round_robin" | "priority" => params.strategy.clone(),
        _ => {
            return Err(crate::errors::AppError::BadRequest(
                "strategy must be cost_optimized, round_robin, or priority".to_string(),
            ))
        }
    };

    let period_days = match params.period.as_str() {
        "7d" => 7,
        "30d" => 30,
        "90d" => 90,
        _ => {
            return Err(crate::errors::AppError::BadRequest(
                "period must be 7d, 30d, or 90d".to_string(),
            ))
        }
    };

    let model_overrides: HashMap<String, String> = if let Some(ref raw) = params.model_overrides {
        serde_json::from_str(raw).map_err(|_| {
            crate::errors::AppError::BadRequest(
                "model_overrides must be a valid JSON object".to_string(),
            )
        })?
    } else {
        HashMap::new()
    };

    let sim_params = db_analytics::SimulateParams {
        strategy,
        period_days,
        model_overrides,
    };

    let result = db_analytics::simulate_cost(&state.pool, &sim_params).await?;
    Ok(Json(result))
}
