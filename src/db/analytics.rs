use crate::errors::AppResult;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;

// ── Overview stats ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PeriodStats {
    pub requests: i64,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub tokens: Option<i64>,
    pub cache_hits: i64,
    pub errors: i64,
    pub avg_latency_ms: Option<f64>,
}

pub async fn overview_stats(pool: &PgPool) -> AppResult<serde_json::Value> {
    let query = "
        SELECT
            COUNT(*)                                                           AS requests,
            SUM(cost_usd)                                                      AS cost_usd,
            SUM(total_tokens)::bigint                                          AS tokens,
            COUNT(*) FILTER (WHERE cache_type IS NOT NULL)                     AS cache_hits,
            COUNT(*) FILTER (WHERE status = 'error')                           AS errors,
            AVG(latency_ms)                                                    AS avg_latency_ms
        FROM requests
        WHERE created_at >= NOW() - $1::interval";

    let today = sqlx::query_as::<_, PeriodStats>(query)
        .bind("24 hours")
        .fetch_one(pool)
        .await?;

    let last_7d = sqlx::query_as::<_, PeriodStats>(query)
        .bind("7 days")
        .fetch_one(pool)
        .await?;

    let last_30d = sqlx::query_as::<_, PeriodStats>(query)
        .bind("30 days")
        .fetch_one(pool)
        .await?;

    Ok(serde_json::json!({
        "today":    today,
        "last_7d":  last_7d,
        "last_30d": last_30d,
    }))
}

// ── Cost breakdown ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyCostRow {
    pub day: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub requests: i64,
    pub tokens: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct GroupCostRow {
    pub group_key: String,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub requests: i64,
}

pub async fn cost_breakdown(pool: &PgPool, days: i32) -> AppResult<serde_json::Value> {
    let interval = format!("{} days", days);

    let by_day = sqlx::query_as::<_, DailyCostRow>(
        "SELECT
             DATE_TRUNC('day', created_at) AS day,
             SUM(cost_usd)                 AS cost_usd,
             COUNT(*)                      AS requests,
             SUM(total_tokens)::bigint     AS tokens
         FROM requests
         WHERE created_at >= NOW() - $1::interval
         GROUP BY DATE_TRUNC('day', created_at)
         ORDER BY day DESC",
    )
    .bind(&interval)
    .fetch_all(pool)
    .await?;

    let by_provider = sqlx::query_as::<_, GroupCostRow>(
        "SELECT
             provider       AS group_key,
             SUM(cost_usd)  AS cost_usd,
             COUNT(*)       AS requests
         FROM requests
         WHERE created_at >= NOW() - $1::interval
         GROUP BY provider
         ORDER BY cost_usd DESC NULLS LAST",
    )
    .bind(&interval)
    .fetch_all(pool)
    .await?;

    let by_model = sqlx::query_as::<_, GroupCostRow>(
        "SELECT
             model          AS group_key,
             SUM(cost_usd)  AS cost_usd,
             COUNT(*)       AS requests
         FROM requests
         WHERE created_at >= NOW() - $1::interval
         GROUP BY model
         ORDER BY cost_usd DESC NULLS LAST",
    )
    .bind(&interval)
    .fetch_all(pool)
    .await?;

    Ok(serde_json::json!({
        "by_day":      by_day,
        "by_provider": by_provider,
        "by_model":    by_model,
    }))
}

// ── Latency percentiles ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LatencyRow {
    pub model: String,
    pub provider: String,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub avg_ms: Option<f64>,
    pub sample_count: i64,
}

pub async fn latency_percentiles(pool: &PgPool, hours: i32) -> AppResult<Vec<LatencyRow>> {
    let interval = format!("{} hours", hours);
    let rows = sqlx::query_as::<_, LatencyRow>(
        "SELECT
             model,
             provider,
             PERCENTILE_CONT(0.50) WITHIN GROUP (ORDER BY latency_ms) AS p50,
             PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms) AS p95,
             PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency_ms) AS p99,
             AVG(latency_ms)                                           AS avg_ms,
             COUNT(*)                                                  AS sample_count
         FROM requests
         WHERE latency_ms IS NOT NULL
           AND created_at >= NOW() - $1::interval
         GROUP BY model, provider
         ORDER BY avg_ms DESC NULLS LAST",
    )
    .bind(&interval)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ── Cache analytics ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CacheTypeRow {
    pub cache_type: String,
    pub hit_count: i64,
    pub tokens_saved: Option<i64>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_saved: Option<Decimal>,
}

pub async fn cache_analytics(pool: &PgPool, hours: i32) -> AppResult<serde_json::Value> {
    let interval = format!("{} hours", hours);

    let total: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM requests WHERE created_at >= NOW() - $1::interval")
            .bind(&interval)
            .fetch_one(pool)
            .await?;

    let by_type = sqlx::query_as::<_, CacheTypeRow>(
        "SELECT
             cache_type,
             COUNT(*)                  AS hit_count,
             SUM(total_tokens)::bigint AS tokens_saved,
             SUM(cost_usd)             AS cost_saved
         FROM requests
         WHERE cache_type IS NOT NULL
           AND created_at >= NOW() - $1::interval
         GROUP BY cache_type",
    )
    .bind(&interval)
    .fetch_all(pool)
    .await?;

    let total_hits: i64 = by_type.iter().map(|r| r.hit_count).sum();
    let hit_rate = if total.0 > 0 {
        total_hits as f64 / total.0 as f64
    } else {
        0.0
    };

    let avg_similarity: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(cache_similarity::float8)
         FROM requests
         WHERE cache_type = 'semantic'
           AND cache_similarity IS NOT NULL
           AND created_at >= NOW() - $1::interval",
    )
    .bind(&interval)
    .fetch_optional(pool)
    .await?
    .flatten();

    Ok(serde_json::json!({
        "total_requests": total.0,
        "total_hits":     total_hits,
        "hit_rate":       hit_rate,
        "by_type":        by_type,
        "avg_semantic_similarity": avg_similarity,
    }))
}
