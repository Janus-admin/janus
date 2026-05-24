//! Background task: recomputes provider quality scores every 15 minutes.
//!
//! Formula (0.0–1.0):
//!   0.40 × availability  = success_count / total in last hour
//!   0.35 × latency       = 1 - clamp(p95_ms / 10_000, 0, 1)
//!   0.25 × reliability   = 1 - error_rate

use crate::{db::DbPool, db::providers as db_providers};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

const INTERVAL_SECS: u64 = 900; // 15 minutes

/// Spawn the quality-score refresh loop as a background Tokio task.
pub fn start(pool: DbPool) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(INTERVAL_SECS));
        loop {
            interval.tick().await;
            if let Err(e) = refresh_all(&pool).await {
                tracing::warn!("Quality score refresh failed: {e}");
            }
        }
    });
}

/// Compute and persist quality scores for all providers with data in the last hour.
pub async fn refresh_all(pool: &DbPool) -> anyhow::Result<()> {
    let scores = compute_scores(pool).await?;
    for (provider_id, score) in scores {
        if let Err(e) = db_providers::update_quality_score(pool, &provider_id, score).await {
            tracing::warn!("Failed to persist quality score for {provider_id}: {e}");
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ProviderStats {
    provider: String,
    total: i64,
    success: i64,
    errors: i64,
    p95_ms: f64,
}

pub async fn compute_scores(pool: &DbPool) -> anyhow::Result<Vec<(String, Decimal)>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        #[derive(sqlx::FromRow)]
        struct Row {
            provider: String,
            total: i64,
            success: i64,
            errors: i64,
            p95_ms: Option<f64>,
        }

        let rows = sqlx::query_as::<_, Row>(
            "SELECT
                 provider,
                 COUNT(*) AS total,
                 COUNT(*) FILTER (WHERE status = 'success')  AS success,
                 COUNT(*) FILTER (WHERE status = 'error')    AS errors,
                 PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms) AS p95_ms
             FROM requests
             WHERE created_at >= NOW() - INTERVAL '1 hour'
             GROUP BY provider",
        )
        .fetch_all(pool)
        .await?;

        let scores = rows
            .into_iter()
            .map(|r| {
                let stats = ProviderStats {
                    provider: r.provider,
                    total: r.total,
                    success: r.success,
                    errors: r.errors,
                    p95_ms: r.p95_ms.unwrap_or(0.0),
                };
                let score = score_from_stats(&stats);
                (stats.provider, score)
            })
            .collect();

        Ok(scores)
    }

    #[cfg(feature = "sqlite")]
    {
        #[derive(sqlx::FromRow)]
        struct GroupRow {
            provider: String,
            total: i64,
            success: i64,
            errors: i64,
        }

        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);

        let groups = sqlx::query_as::<_, GroupRow>(
            "SELECT
                 provider,
                 COUNT(*)                                       AS total,
                 COUNT(*) FILTER (WHERE status = 'success')    AS success,
                 COUNT(*) FILTER (WHERE status = 'error')      AS errors
             FROM requests
             WHERE created_at >= $1
             GROUP BY provider",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        let mut result = Vec::with_capacity(groups.len());

        for g in groups {
            // SQLite has no PERCENTILE_CONT — compute p95 in Rust.
            let latencies: Vec<(Option<i32>,)> = sqlx::query_as(
                "SELECT latency_ms FROM requests
                 WHERE provider = $1 AND latency_ms IS NOT NULL AND created_at >= $2
                 ORDER BY latency_ms ASC",
            )
            .bind(&g.provider)
            .bind(cutoff)
            .fetch_all(pool)
            .await
            .unwrap_or_default();

            let vals: Vec<f64> = latencies.iter().filter_map(|(v,)| v.map(|x| x as f64)).collect();
            let p95 = percentile_95(&vals);

            let stats = ProviderStats {
                provider: g.provider.clone(),
                total: g.total,
                success: g.success,
                errors: g.errors,
                p95_ms: p95,
            };
            result.push((g.provider, score_from_stats(&stats)));
        }

        Ok(result)
    }
}

fn score_from_stats(s: &ProviderStats) -> Decimal {
    if s.total == 0 {
        return Decimal::ONE;
    }

    let availability = s.success as f64 / s.total as f64;
    let error_rate = s.errors as f64 / s.total as f64;
    let latency_score = (1.0 - (s.p95_ms / 10_000.0)).clamp(0.0, 1.0);

    let raw = 0.40 * availability + 0.35 * latency_score + 0.25 * (1.0 - error_rate);
    let clamped = raw.clamp(0.0, 1.0);

    // Round to 4 decimal places to fit DECIMAL(5,4).
    let rounded = (clamped * 10_000.0).round() / 10_000.0;
    Decimal::from_f64(rounded).unwrap_or(Decimal::ONE)
}

#[cfg(feature = "sqlite")]
fn percentile_95(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    let idx = 0.95 * (n - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    sorted[lo] + (idx - lo as f64) * (sorted[hi] - sorted[lo])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: f64) -> Decimal {
        Decimal::from_f64(v).unwrap()
    }

    #[test]
    fn v4_5_quality_score_defaults_to_one_with_no_data() {
        let s = ProviderStats {
            provider: "openai".into(),
            total: 0,
            success: 0,
            errors: 0,
            p95_ms: 0.0,
        };
        assert_eq!(score_from_stats(&s), Decimal::ONE);
    }

    #[test]
    fn v4_5_quality_score_decreases_on_high_error_rate() {
        let perfect = ProviderStats {
            provider: "openai".into(),
            total: 100,
            success: 100,
            errors: 0,
            p95_ms: 500.0,
        };
        let degraded = ProviderStats {
            provider: "openai".into(),
            total: 100,
            success: 50,
            errors: 50,
            p95_ms: 500.0,
        };
        assert!(score_from_stats(&degraded) < score_from_stats(&perfect));
    }

    #[test]
    fn v4_5_quality_score_decreases_on_high_latency() {
        let fast = ProviderStats {
            provider: "openai".into(),
            total: 100,
            success: 100,
            errors: 0,
            p95_ms: 200.0,
        };
        let slow = ProviderStats {
            provider: "openai".into(),
            total: 100,
            success: 100,
            errors: 0,
            p95_ms: 8_000.0,
        };
        assert!(score_from_stats(&slow) < score_from_stats(&fast));
    }

    #[test]
    fn v4_5_quality_score_clamped_to_unit_interval() {
        let perfect = ProviderStats {
            provider: "openai".into(),
            total: 100,
            success: 100,
            errors: 0,
            p95_ms: 0.0,
        };
        let score = score_from_stats(&perfect);
        assert!(score >= d(0.0) && score <= d(1.0));
    }
}
