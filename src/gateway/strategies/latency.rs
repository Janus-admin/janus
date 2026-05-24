use crate::db::DbPool;
use crate::providers::Provider;
use std::sync::Arc;

/// Sort `providers` by ascending 15-minute p95 latency from the `requests` table.
/// Providers with no recent data are sorted to the end, retaining their relative
/// priority order among themselves.
pub async fn sort_by_latency(
    pool: &DbPool,
    providers: Vec<Arc<dyn Provider>>,
) -> Vec<Arc<dyn Provider>> {
    let mut scored: Vec<(i64, Arc<dyn Provider>)> = Vec::new();

    for p in providers {
        let p95 = get_provider_p95(pool, p.name()).await.unwrap_or(i64::MAX);
        scored.push((p95, p));
    }

    scored.sort_by_key(|(latency, _)| *latency);
    scored.into_iter().map(|(_, p)| p).collect()
}

/// Query p95 latency for a single provider over the past 15 minutes.
/// Returns `None` when there are no qualifying rows.
pub async fn get_provider_p95(pool: &DbPool, provider: &str) -> Option<i64> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let sql = "SELECT latency_ms FROM requests \
               WHERE provider = $1 AND status = 'success' \
                 AND created_at > NOW() - INTERVAL '15 minutes' \
               ORDER BY latency_ms";

    #[cfg(feature = "sqlite")]
    let sql = "SELECT latency_ms FROM requests \
               WHERE provider = $1 AND status = 'success' \
                 AND created_at > datetime('now', '-15 minutes') \
               ORDER BY latency_ms";

    let rows: Vec<(i32,)> = sqlx::query_as(sql)
        .bind(provider)
        .fetch_all(pool)
        .await
        .ok()?;

    if rows.is_empty() {
        return None;
    }

    // p95: row at the 95th-percentile index (ceiling)
    let idx = ((rows.len() as f64 * 0.95).ceil() as usize)
        .min(rows.len())
        .saturating_sub(1);
    Some(rows[idx].0 as i64)
}
