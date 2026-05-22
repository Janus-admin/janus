use crate::{errors::AppResult, models::cache_entry::CacheStats};
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct StatsRow {
    total_entries: i64,
    total_hits: i64,
    total_tokens_saved: i64,
    total_cost_saved: Decimal,
    exact_entries: i64,
    semantic_entries: i64,
}

// ── Write operations ──────────────────────────────────────────────────────────

/// Persist a new cache entry. Silently ignores conflicts (same hash already stored).
#[allow(clippy::too_many_arguments)]
pub async fn upsert_entry(
    pool: &PgPool,
    prompt_hash: &str,
    provider: &str,
    model: &str,
    request_body: &str,
    response_body: &str,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO cache_entries (
             id, prompt_hash, provider, model,
             request_body, response_body,
             prompt_tokens, completion_tokens, cost_usd,
             hit_count, tokens_saved, cost_saved, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, 0, 0, 0, $10)
         ON CONFLICT (prompt_hash) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(prompt_hash)
    .bind(provider)
    .bind(model)
    .bind(request_body)
    .bind(response_body)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(cost_usd)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// Record a cache hit: increment counters and update last_hit_at.
pub async fn record_hit(
    pool: &PgPool,
    prompt_hash: &str,
    tokens: i64,
    cost: Decimal,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE cache_entries
         SET hit_count    = hit_count + 1,
             tokens_saved = tokens_saved + $1,
             cost_saved   = cost_saved + $2,
             last_hit_at  = $3
         WHERE prompt_hash = $4",
    )
    .bind(tokens)
    .bind(cost)
    .bind(Utc::now())
    .bind(prompt_hash)
    .execute(pool)
    .await?;

    Ok(())
}

// ── Read operations ───────────────────────────────────────────────────────────

/// Aggregate cache statistics across all entries.
pub async fn get_stats(pool: &PgPool) -> AppResult<CacheStats> {
    // Explicit casts needed because:
    //   SUM(BIGINT) → numeric in PostgreSQL (not bigint)
    //   COALESCE fallback literal must match the branch type
    let row = sqlx::query_as::<_, StatsRow>(
        "SELECT
             COUNT(*)::bigint                                            AS total_entries,
             COALESCE(SUM(hit_count), 0)::bigint                        AS total_hits,
             COALESCE(SUM(tokens_saved), 0::numeric)::bigint            AS total_tokens_saved,
             COALESCE(SUM(cost_saved), 0::numeric)                      AS total_cost_saved,
             COUNT(*) FILTER (WHERE embedding IS NULL)::bigint           AS exact_entries,
             COUNT(*) FILTER (WHERE embedding IS NOT NULL)::bigint       AS semantic_entries
         FROM cache_entries",
    )
    .fetch_one(pool)
    .await?;

    Ok(CacheStats {
        total_entries: row.total_entries,
        total_hits: row.total_hits,
        total_tokens_saved: row.total_tokens_saved,
        total_cost_saved: row.total_cost_saved,
        exact_entries: row.exact_entries,
        semantic_entries: row.semantic_entries,
    })
}

// ── Flush ─────────────────────────────────────────────────────────────────────

/// Delete all cache entries. Returns the number of rows deleted.
pub async fn flush_all(pool: &PgPool) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM cache_entries")
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}
