use crate::{db::DbPool, errors::AppResult};
#[cfg(feature = "sqlite")]
use chrono::Utc;
#[cfg(feature = "sqlite")]
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

// ── Daily costs upsert ────────────────────────────────────────────────────────

/// Upsert one row in `daily_costs` for a completed proxy call.
///
/// Called fire-and-forget from `pipeline::run()` and `pipeline::run_streaming()`.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_daily_cost(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_usd: Option<Decimal>,
    is_cache_hit: bool,
) -> AppResult<()> {
    let cost = cost_usd.unwrap_or(Decimal::ZERO);
    let cache_hit_count = if is_cache_hit { 1i64 } else { 0i64 };

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        sqlx::query(
            "INSERT INTO daily_costs (
                 date, api_key_id, workspace_id, provider, model,
                 request_count, cache_hits, prompt_tokens, completion_tokens, total_cost_usd
             )
             VALUES (CURRENT_DATE, $1, $2, $3, $4, 1, $5, $6, $7, $8)
             ON CONFLICT (
                 date, provider, model,
                 COALESCE(api_key_id, '00000000-0000-0000-0000-000000000000'::UUID),
                 COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::UUID)
             ) DO UPDATE SET
                 request_count     = daily_costs.request_count + 1,
                 cache_hits        = daily_costs.cache_hits + EXCLUDED.cache_hits,
                 prompt_tokens     = daily_costs.prompt_tokens + EXCLUDED.prompt_tokens,
                 completion_tokens = daily_costs.completion_tokens + EXCLUDED.completion_tokens,
                 total_cost_usd    = daily_costs.total_cost_usd + EXCLUDED.total_cost_usd",
        )
        .bind(api_key_id)
        .bind(workspace_id)
        .bind(provider)
        .bind(model)
        .bind(cache_hit_count)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(cost)
        .execute(pool)
        .await?;
        Ok(())
    }

    // SQLite: api_key_id / workspace_id are stored as NOT NULL TEXT with the nil
    // UUID as the sentinel for "no key / no workspace".  The UNIQUE constraint on
    // (date, provider, model, api_key_id, workspace_id) replaces the PG partial
    // index with COALESCE.
    #[cfg(feature = "sqlite")]
    {
        let nil = "00000000-0000-0000-0000-000000000000";
        let ak = api_key_id
            .map(|u| u.to_string())
            .unwrap_or_else(|| nil.to_string());
        let ws = workspace_id
            .map(|u| u.to_string())
            .unwrap_or_else(|| nil.to_string());
        let cost_f64 = f64::try_from(cost).unwrap_or(0.0);
        sqlx::query(
            "INSERT INTO daily_costs (
                 date, api_key_id, workspace_id, provider, model,
                 request_count, cache_hits, prompt_tokens, completion_tokens, total_cost_usd
             )
             VALUES (date('now'), $1, $2, $3, $4, 1, $5, $6, $7, $8)
             ON CONFLICT (date, provider, model, api_key_id, workspace_id) DO UPDATE SET
                 request_count     = daily_costs.request_count + 1,
                 cache_hits        = daily_costs.cache_hits + EXCLUDED.cache_hits,
                 prompt_tokens     = daily_costs.prompt_tokens + EXCLUDED.prompt_tokens,
                 completion_tokens = daily_costs.completion_tokens + EXCLUDED.completion_tokens,
                 total_cost_usd    = PRINTF('%.8f', CAST(daily_costs.total_cost_usd AS REAL) + CAST(EXCLUDED.total_cost_usd AS REAL))",
        )
        .bind(ak)
        .bind(ws)
        .bind(provider)
        .bind(model)
        .bind(cache_hit_count)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(cost_f64)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ── Overview stats ────────────────────────────────────────────────────────────

#[cfg_attr(
    all(feature = "postgres", not(feature = "sqlite")),
    derive(sqlx::FromRow)
)]
#[derive(Debug, Serialize)]
pub struct PeriodStats {
    pub requests: i64,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub tokens: Option<i64>,
    pub cache_hits: i64,
    pub errors: i64,
    pub avg_latency_ms: Option<f64>,
}

pub async fn overview_stats(pool: &DbPool) -> AppResult<serde_json::Value> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let query = "
            SELECT
                COUNT(*)                                                           AS requests,
                SUM(cost_usd)                                                      AS cost_usd,
                SUM(total_tokens)::bigint                                          AS tokens,
                COUNT(*) FILTER (WHERE cache_type IS NOT NULL)                     AS cache_hits,
                COUNT(*) FILTER (WHERE status = 'error')                           AS errors,
                AVG(latency_ms)::float8                                            AS avg_latency_ms
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

    #[cfg(feature = "sqlite")]
    {
        let now = Utc::now();
        let cut_1d = now - chrono::Duration::hours(24);
        let cut_7d = now - chrono::Duration::days(7);
        let cut_30d = now - chrono::Duration::days(30);

        // SQLite: FILTER is supported since 3.30.0 (2019); SUM(total_tokens)
        // returns INTEGER without a cast needed; AVG returns REAL.
        // SUM(cost_usd) over a TEXT column returns REAL, so use f64 here.
        #[derive(sqlx::FromRow)]
        struct PeriodStatsSqlite {
            requests: i64,
            cost_usd: Option<f64>,
            tokens: Option<i64>,
            cache_hits: i64,
            errors: i64,
            avg_latency_ms: Option<f64>,
        }

        let query = "
            SELECT
                COUNT(*)                                         AS requests,
                SUM(cost_usd)                                    AS cost_usd,
                SUM(total_tokens)                                AS tokens,
                COUNT(*) FILTER (WHERE cache_type IS NOT NULL)   AS cache_hits,
                COUNT(*) FILTER (WHERE status = 'error')         AS errors,
                AVG(latency_ms)                                  AS avg_latency_ms
            FROM requests
            WHERE created_at >= $1";

        let today: PeriodStats = {
            let r = sqlx::query_as::<_, PeriodStatsSqlite>(query)
                .bind(cut_1d)
                .fetch_one(pool)
                .await?;
            PeriodStats {
                requests: r.requests,
                cost_usd: r.cost_usd.and_then(|v| Decimal::try_from(v).ok()),
                tokens: r.tokens,
                cache_hits: r.cache_hits,
                errors: r.errors,
                avg_latency_ms: r.avg_latency_ms,
            }
        };
        let last_7d: PeriodStats = {
            let r = sqlx::query_as::<_, PeriodStatsSqlite>(query)
                .bind(cut_7d)
                .fetch_one(pool)
                .await?;
            PeriodStats {
                requests: r.requests,
                cost_usd: r.cost_usd.and_then(|v| Decimal::try_from(v).ok()),
                tokens: r.tokens,
                cache_hits: r.cache_hits,
                errors: r.errors,
                avg_latency_ms: r.avg_latency_ms,
            }
        };
        let last_30d: PeriodStats = {
            let r = sqlx::query_as::<_, PeriodStatsSqlite>(query)
                .bind(cut_30d)
                .fetch_one(pool)
                .await?;
            PeriodStats {
                requests: r.requests,
                cost_usd: r.cost_usd.and_then(|v| Decimal::try_from(v).ok()),
                tokens: r.tokens,
                cache_hits: r.cache_hits,
                errors: r.errors,
                avg_latency_ms: r.avg_latency_ms,
            }
        };

        Ok(serde_json::json!({
            "today":    today,
            "last_7d":  last_7d,
            "last_30d": last_30d,
        }))
    }
}

// ── Cost breakdown ────────────────────────────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyCostRow {
    pub day: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub requests: i64,
    pub tokens: Option<i64>,
}

#[cfg_attr(
    all(feature = "postgres", not(feature = "sqlite")),
    derive(sqlx::FromRow)
)]
#[derive(Debug, Serialize)]
pub struct GroupCostRow {
    pub group_key: String,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_usd: Option<Decimal>,
    pub requests: i64,
}

pub async fn cost_breakdown(pool: &DbPool, days: i32) -> AppResult<serde_json::Value> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
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

    #[cfg(feature = "sqlite")]
    {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        // SQLite: DATE() truncates to day; no NULLS LAST needed (SQLite puts NULLs
        // last for DESC by default).
        // SUM over TEXT columns returns REAL, so use f64.
        #[derive(sqlx::FromRow)]
        struct DayCostRowSqlite {
            day: Option<String>, // "YYYY-MM-DD"
            cost_usd: Option<f64>,
            requests: i64,
            tokens: Option<i64>,
        }

        #[derive(sqlx::FromRow)]
        struct GroupCostRowSqlite {
            group_key: String,
            cost_usd: Option<f64>,
            requests: i64,
        }

        let raw = sqlx::query_as::<_, DayCostRowSqlite>(
            "SELECT
                 DATE(created_at)  AS day,
                 SUM(cost_usd)     AS cost_usd,
                 COUNT(*)          AS requests,
                 SUM(total_tokens) AS tokens
             FROM requests
             WHERE created_at >= $1
             GROUP BY DATE(created_at)
             ORDER BY day DESC",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let by_day: Vec<serde_json::Value> = raw
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "day":      r.day,
                    "cost_usd": r.cost_usd,
                    "requests": r.requests,
                    "tokens":   r.tokens,
                })
            })
            .collect();

        let by_provider_raw = sqlx::query_as::<_, GroupCostRowSqlite>(
            "SELECT
                 provider       AS group_key,
                 SUM(cost_usd)  AS cost_usd,
                 COUNT(*)       AS requests
             FROM requests
             WHERE created_at >= $1
             GROUP BY provider
             ORDER BY cost_usd DESC",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let by_model_raw = sqlx::query_as::<_, GroupCostRowSqlite>(
            "SELECT
                 model          AS group_key,
                 SUM(cost_usd)  AS cost_usd,
                 COUNT(*)       AS requests
             FROM requests
             WHERE created_at >= $1
             GROUP BY model
             ORDER BY cost_usd DESC",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let by_provider: Vec<serde_json::Value> = by_provider_raw
            .into_iter()
            .map(|r| serde_json::json!({ "group_key": r.group_key, "cost_usd": r.cost_usd, "requests": r.requests }))
            .collect();

        let by_model: Vec<serde_json::Value> = by_model_raw
            .into_iter()
            .map(|r| serde_json::json!({ "group_key": r.group_key, "cost_usd": r.cost_usd, "requests": r.requests }))
            .collect();

        Ok(serde_json::json!({
            "by_day":      by_day,
            "by_provider": by_provider,
            "by_model":    by_model,
        }))
    }
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

pub async fn latency_percentiles(pool: &DbPool, hours: i32) -> AppResult<Vec<LatencyRow>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
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

    // SQLite: PERCENTILE_CONT is not available.  Fetch all latency values per
    // (model, provider) group and compute p50/p95/p99 in Rust.
    // This is acceptable for the low-volume single-node SQLite deployment target.
    #[cfg(feature = "sqlite")]
    {
        let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
        #[derive(sqlx::FromRow)]
        struct GroupKey {
            model: String,
            provider: String,
        }

        let groups = sqlx::query_as::<_, GroupKey>(
            "SELECT DISTINCT model, provider FROM requests
             WHERE latency_ms IS NOT NULL AND created_at >= $1",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::with_capacity(groups.len());

        for g in groups {
            // Fetch all latency_ms values for this (model, provider) group, sorted.
            let values: Vec<(i32,)> = sqlx::query_as(
                "SELECT latency_ms FROM requests
                 WHERE latency_ms IS NOT NULL
                   AND created_at >= $1
                   AND model = $2
                   AND provider = $3
                 ORDER BY latency_ms ASC",
            )
            .bind(cutoff)
            .bind(&g.model)
            .bind(&g.provider)
            .fetch_all(pool)
            .await?;

            let n = values.len();
            if n == 0 {
                continue;
            }

            let sorted: Vec<f64> = values.iter().map(|(v,)| *v as f64).collect();
            let avg_ms = sorted.iter().sum::<f64>() / n as f64;

            // Linear interpolation percentile.
            let percentile = |p: f64| -> f64 {
                let idx = p * (n - 1) as f64;
                let lo = idx.floor() as usize;
                let hi = (lo + 1).min(n - 1);
                let frac = idx - lo as f64;
                sorted[lo] + frac * (sorted[hi] - sorted[lo])
            };

            result.push(LatencyRow {
                model: g.model,
                provider: g.provider,
                p50: Some(percentile(0.50)),
                p95: Some(percentile(0.95)),
                p99: Some(percentile(0.99)),
                avg_ms: Some(avg_ms),
                sample_count: n as i64,
            });
        }

        // Sort descending by avg_ms to match PG output order.
        result.sort_by(|a, b| {
            b.avg_ms
                .partial_cmp(&a.avg_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(result)
    }
}

// ── Cache analytics ───────────────────────────────────────────────────────────

#[cfg_attr(
    all(feature = "postgres", not(feature = "sqlite")),
    derive(sqlx::FromRow)
)]
#[derive(Debug, Serialize)]
pub struct CacheTypeRow {
    pub cache_type: String,
    pub hit_count: i64,
    pub tokens_saved: Option<i64>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub cost_saved: Option<Decimal>,
}

pub async fn cache_analytics(pool: &DbPool, hours: i32) -> AppResult<serde_json::Value> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let interval = format!("{} hours", hours);

        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM requests WHERE created_at >= NOW() - $1::interval",
        )
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

    #[cfg(feature = "sqlite")]
    {
        let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
        // SUM over TEXT column (cost_usd) returns REAL in SQLite.
        #[derive(sqlx::FromRow, Serialize)]
        struct CacheTypeRowSqlite {
            cache_type: String,
            hit_count: i64,
            tokens_saved: Option<i64>,
            cost_saved: Option<f64>,
        }

        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM requests WHERE created_at >= $1")
            .bind(cutoff)
            .fetch_one(pool)
            .await?;

        let by_type = sqlx::query_as::<_, CacheTypeRowSqlite>(
            "SELECT
                 cache_type,
                 COUNT(*)          AS hit_count,
                 SUM(total_tokens) AS tokens_saved,
                 SUM(cost_usd)     AS cost_saved
             FROM requests
             WHERE cache_type IS NOT NULL
               AND created_at >= $1
             GROUP BY cache_type",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let total_hits: i64 = by_type.iter().map(|r| r.hit_count).sum();
        let hit_rate = if total.0 > 0 {
            total_hits as f64 / total.0 as f64
        } else {
            0.0
        };

        // Remove ::float8 cast — SQLite returns REAL naturally.
        let avg_similarity: Option<f64> = sqlx::query_scalar(
            "SELECT AVG(cache_similarity)
             FROM requests
             WHERE cache_type = 'semantic'
               AND cache_similarity IS NOT NULL
               AND created_at >= $1",
        )
        .bind(cutoff)
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
}

// ── Cost simulator ────────────────────────────────────────────────────────────

pub struct SimulateParams {
    pub strategy: String,
    pub period_days: i32,
    /// model_id → replacement model_id for pricing lookup
    pub model_overrides: HashMap<String, String>,
}

/// Recalculate costs for all requests in the period under a different strategy
/// and/or with model substitutions. Returns the original vs simulated breakdown.
pub async fn simulate_cost(pool: &DbPool, params: &SimulateParams) -> AppResult<serde_json::Value> {
    let rows = fetch_model_aggregates(pool, params.period_days).await?;
    let pricing = fetch_pricing_map(pool).await?;

    let mut original_total = Decimal::ZERO;
    let mut simulated_total = Decimal::ZERO;
    let mut request_count: i64 = 0;
    let mut by_model: Vec<serde_json::Value> = Vec::new();

    for row in &rows {
        let orig_cost = row.original_cost.unwrap_or(Decimal::ZERO);
        original_total += orig_cost;
        request_count += row.request_count;

        // Apply model override (e.g. gpt-4o → gpt-4o-mini for pricing lookup).
        let lookup_model = params
            .model_overrides
            .get(&row.model)
            .cloned()
            .unwrap_or_else(|| row.model.clone());

        let sim_cost = match params.strategy.as_str() {
            "priority" => orig_cost,
            "cost_optimized" => {
                let best = pricing
                    .values()
                    .filter(|p| p.model_id == lookup_model)
                    .min_by(|a, b| {
                        let cost_a = a.input_per_1m + a.output_per_1m;
                        let cost_b = b.input_per_1m + b.output_per_1m;
                        cost_a
                            .partial_cmp(&cost_b)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                best.map(|p| {
                    crate::pricing::calculate_cost(
                        row.prompt_tokens as u32,
                        row.completion_tokens as u32,
                        p.input_per_1m,
                        p.output_per_1m,
                    )
                })
                .unwrap_or(orig_cost)
            }
            "round_robin" => {
                let matching: Vec<_> = pricing
                    .values()
                    .filter(|p| p.model_id == lookup_model)
                    .collect();
                if matching.is_empty() {
                    orig_cost
                } else {
                    let avg_input = matching.iter().map(|p| p.input_per_1m).sum::<Decimal>()
                        / Decimal::from(matching.len() as u32);
                    let avg_output = matching.iter().map(|p| p.output_per_1m).sum::<Decimal>()
                        / Decimal::from(matching.len() as u32);
                    crate::pricing::calculate_cost(
                        row.prompt_tokens as u32,
                        row.completion_tokens as u32,
                        avg_input,
                        avg_output,
                    )
                }
            }
            _ => orig_cost,
        };

        simulated_total += sim_cost;

        by_model.push(serde_json::json!({
            "model":              row.model,
            "request_count":      row.request_count,
            "original_cost_usd":  decimal_to_f64(orig_cost),
            "simulated_cost_usd": decimal_to_f64(sim_cost),
        }));
    }

    let savings = original_total - simulated_total;
    let savings_pct = if original_total.is_zero() {
        0.0_f64
    } else {
        (savings / original_total * Decimal::from(100))
            .try_into()
            .unwrap_or(0.0_f64)
    };

    Ok(serde_json::json!({
        "data": {
            "strategy":           params.strategy,
            "period":             format!("{}d", params.period_days),
            "original_cost_usd":  decimal_to_f64(original_total),
            "simulated_cost_usd": decimal_to_f64(simulated_total),
            "savings_usd":        decimal_to_f64(savings),
            "savings_percent":    savings_pct,
            "request_count":      request_count,
            "by_model":           by_model,
        }
    }))
}

fn decimal_to_f64(d: Decimal) -> f64 {
    f64::try_from(d).unwrap_or(0.0)
}

// ── Cost breakdown by tag (V5-L3) ─────────────────────────────────────────────

/// Return cost grouped by a single tag key over a rolling window.
///
/// `tag_key` is a simple string (e.g. `"team"`). Rows where the tag is absent
/// are reported with `tag_value = null`.
/// `days` is clamped to 1–365 by the caller.
pub async fn cost_by_tag(
    pool: &DbPool,
    tag_key: &str,
    days: i32,
) -> AppResult<serde_json::Value> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let interval = format!("{} days", days);

        // Total cost in the period (for share calculation).
        let total_cost: Option<Decimal> = sqlx::query_scalar(
            "SELECT SUM(cost_usd) FROM requests WHERE created_at >= NOW() - $1::interval",
        )
        .bind(&interval)
        .fetch_optional(pool)
        .await?
        .flatten();

        // Group by the value of tags->>key; NULL when the tag is absent.
        #[derive(sqlx::FromRow)]
        struct Row {
            tag_value: Option<String>,
            cost_usd: Option<Decimal>,
            request_count: i64,
        }

        let rows = sqlx::query_as::<_, Row>(
            "SELECT
                 tags->>$2                AS tag_value,
                 SUM(cost_usd)            AS cost_usd,
                 COUNT(*)                 AS request_count
             FROM requests
             WHERE created_at >= NOW() - $1::interval
             GROUP BY tags->>$2
             ORDER BY cost_usd DESC NULLS LAST",
        )
        .bind(&interval)
        .bind(tag_key)
        .fetch_all(pool)
        .await?;

        let total = total_cost.unwrap_or(Decimal::ZERO);
        let groups: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "tag_value":     r.tag_value,
                    "cost_usd":      decimal_to_f64(r.cost_usd.unwrap_or(Decimal::ZERO)),
                    "request_count": r.request_count,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "data": {
                "tag_key":        tag_key,
                "period":         format!("{}d", days),
                "total_cost_usd": decimal_to_f64(total),
                "groups":         groups,
            }
        }))
    }

    #[cfg(feature = "sqlite")]
    {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);

        let total_cost: Option<f64> =
            sqlx::query_scalar("SELECT SUM(cost_usd) FROM requests WHERE created_at >= $1")
                .bind(cutoff)
                .fetch_optional(pool)
                .await?
                .flatten();

        // SQLite JSON: json_extract(tags, '$.<key>') extracts the value for key.
        // Build the JSON path dynamically (key is validated by the handler).
        let json_path = format!("$.{}", tag_key);

        #[derive(sqlx::FromRow)]
        struct Row {
            tag_value: Option<String>,
            cost_usd: Option<f64>,
            request_count: i64,
        }

        let rows = sqlx::query_as::<_, Row>(
            "SELECT
                 json_extract(tags, $2)  AS tag_value,
                 SUM(cost_usd)           AS cost_usd,
                 COUNT(*)                AS request_count
             FROM requests
             WHERE created_at >= $1
             GROUP BY json_extract(tags, $2)
             ORDER BY cost_usd DESC",
        )
        .bind(cutoff)
        .bind(&json_path)
        .fetch_all(pool)
        .await?;

        let total = total_cost.unwrap_or(0.0);
        let groups: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "tag_value":     r.tag_value,
                    "cost_usd":      r.cost_usd.unwrap_or(0.0),
                    "request_count": r.request_count,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "data": {
                "tag_key":        tag_key,
                "period":         format!("{}d", days),
                "total_cost_usd": total,
                "groups":         groups,
            }
        }))
    }
}

struct ModelAggregate {
    model: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    original_cost: Option<Decimal>,
}

struct PricingEntry {
    model_id: String,
    input_per_1m: Decimal,
    output_per_1m: Decimal,
}

async fn fetch_model_aggregates(pool: &DbPool, days: i32) -> AppResult<Vec<ModelAggregate>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let interval = format!("{} days", days);
        #[derive(sqlx::FromRow)]
        struct Row {
            model: String,
            request_count: i64,
            prompt_tokens: Option<i64>,
            completion_tokens: Option<i64>,
            original_cost: Option<Decimal>,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT
                 model,
                 COUNT(*)                      AS request_count,
                 SUM(prompt_tokens)::bigint    AS prompt_tokens,
                 SUM(completion_tokens)::bigint AS completion_tokens,
                 SUM(cost_usd)                 AS original_cost
             FROM requests
             WHERE created_at >= NOW() - $1::interval
               AND status = 'success'
             GROUP BY model",
        )
        .bind(&interval)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ModelAggregate {
                model: r.model,
                request_count: r.request_count,
                prompt_tokens: r.prompt_tokens.unwrap_or(0),
                completion_tokens: r.completion_tokens.unwrap_or(0),
                original_cost: r.original_cost,
            })
            .collect())
    }

    #[cfg(feature = "sqlite")]
    {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        #[derive(sqlx::FromRow)]
        struct Row {
            model: String,
            request_count: i64,
            prompt_tokens: Option<i64>,
            completion_tokens: Option<i64>,
            original_cost: Option<f64>,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT
                 model,
                 COUNT(*)              AS request_count,
                 SUM(prompt_tokens)    AS prompt_tokens,
                 SUM(completion_tokens) AS completion_tokens,
                 SUM(cost_usd)         AS original_cost
             FROM requests
             WHERE created_at >= $1 AND status = 'success'
             GROUP BY model",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ModelAggregate {
                model: r.model,
                request_count: r.request_count,
                prompt_tokens: r.prompt_tokens.unwrap_or(0),
                completion_tokens: r.completion_tokens.unwrap_or(0),
                original_cost: r.original_cost.and_then(|v| Decimal::from_f64(v)),
            })
            .collect())
    }
}

async fn fetch_pricing_map(pool: &DbPool) -> AppResult<HashMap<String, PricingEntry>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        #[derive(sqlx::FromRow)]
        struct Row {
            provider: String,
            model_id: String,
            input_per_1m_tokens: Decimal,
            output_per_1m_tokens: Decimal,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT provider, model_id, input_per_1m_tokens, output_per_1m_tokens
             FROM model_pricing WHERE is_active = true",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let key = format!("{}:{}", r.provider, r.model_id);
                (
                    key,
                    PricingEntry {
                        model_id: r.model_id,
                        input_per_1m: r.input_per_1m_tokens,
                        output_per_1m: r.output_per_1m_tokens,
                    },
                )
            })
            .collect())
    }

    #[cfg(feature = "sqlite")]
    {
        #[derive(sqlx::FromRow)]
        struct Row {
            provider: String,
            model_id: String,
            input_per_1m_tokens: f64,
            output_per_1m_tokens: f64,
        }
        let rows = sqlx::query_as::<_, Row>(
            "SELECT provider, model_id, input_per_1m_tokens, output_per_1m_tokens
             FROM model_pricing WHERE is_active = 1",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let input = Decimal::from_f64(r.input_per_1m_tokens)?;
                let output = Decimal::from_f64(r.output_per_1m_tokens)?;
                let key = format!("{}:{}", r.provider, r.model_id);
                Some((
                    key,
                    PricingEntry {
                        model_id: r.model_id,
                        input_per_1m: input,
                        output_per_1m: output,
                    },
                ))
            })
            .collect())
    }
}
