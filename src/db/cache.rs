use crate::db::DbPool;
use crate::{errors::AppResult, models::cache_entry::CacheStats};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

// ── Row type for warm-up load ─────────────────────────────────────────────────

/// Minimal DB row fetched during cache warm-up on startup.
#[derive(sqlx::FromRow)]
pub struct CacheEntryRow {
    pub prompt_hash: String,
    pub response_body: String,
    pub embedding: Option<Vec<u8>>,
    /// Non-null when the entry was inserted with a TTL.
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    pub expires_at: Option<DateTime<Utc>>,
    #[cfg(feature = "sqlite")]
    pub expires_at: Option<String>,
}

impl CacheEntryRow {
    /// Returns the expiry as `DateTime<Utc>`, normalizing across PG (native type)
    /// and SQLite (ISO-8601 text).
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    pub fn expires_at_utc(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }

    #[cfg(feature = "sqlite")]
    pub fn expires_at_utc(&self) -> Option<DateTime<Utc>> {
        self.expires_at.as_deref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
    }
}

// ── Row types ─────────────────────────────────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct StatsRow {
    total_entries: i64,
    total_hits: i64,
    total_tokens_saved: i64,
    total_cost_saved: Decimal,
    exact_entries: i64,
    semantic_entries: i64,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct StatsRow {
    total_entries: i64,
    total_hits: i64,
    total_tokens_saved: i64,
    total_cost_saved: f64,
    exact_entries: i64,
    semantic_entries: i64,
}

// ── Write operations ──────────────────────────────────────────────────────────

/// Persist a new cache entry. Silently ignores conflicts (same hash already stored).
/// `ttl_secs = 0` means no expiry.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_entry(
    pool: &DbPool,
    prompt_hash: &str,
    provider: &str,
    model: &str,
    request_body: &str,
    response_body: &str,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    ttl_secs: u64,
) -> AppResult<()> {
    let now = Utc::now();
    let expires_at: Option<DateTime<Utc>> = if ttl_secs > 0 {
        Some(now + chrono::Duration::seconds(ttl_secs as i64))
    } else {
        None
    };
    let ttl_db: Option<i32> = if ttl_secs > 0 {
        Some(ttl_secs as i32)
    } else {
        None
    };

    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());
    #[cfg(feature = "sqlite")]
    let expires_at = expires_at.map(|dt| dt.to_rfc3339());

    sqlx::query(
        "INSERT INTO cache_entries (
             id, prompt_hash, provider, model,
             request_body, response_body,
             prompt_tokens, completion_tokens, cost_usd,
             hit_count, tokens_saved, cost_saved, created_at,
             ttl_secs, expires_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, 0, 0, 0, $10, $11, $12)
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
    .bind(now)
    .bind(ttl_db)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Record a cache hit: increment counters and update last_hit_at.
pub async fn record_hit(
    pool: &DbPool,
    prompt_hash: &str,
    tokens: i64,
    cost: Decimal,
) -> AppResult<()> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
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
    }

    #[cfg(feature = "sqlite")]
    {
        let cost_f64 = f64::try_from(cost).unwrap_or(0.0);
        sqlx::query(
            "UPDATE cache_entries
             SET hit_count    = hit_count + 1,
                 tokens_saved = tokens_saved + $1,
                 cost_saved   = PRINTF('%.8f', CAST(cost_saved AS REAL) + $2),
                 last_hit_at  = $3
             WHERE prompt_hash = $4",
        )
        .bind(tokens)
        .bind(cost_f64)
        .bind(Utc::now())
        .bind(prompt_hash)
        .execute(pool)
        .await?;
    }

    Ok(())
}

// ── Read operations ───────────────────────────────────────────────────────────

/// Aggregate cache statistics across all entries.
pub async fn get_stats(pool: &DbPool) -> AppResult<CacheStats> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let sql = "SELECT
             COUNT(*)::bigint                                            AS total_entries,
             COALESCE(SUM(hit_count), 0)::bigint                        AS total_hits,
             COALESCE(SUM(tokens_saved), 0::numeric)::bigint            AS total_tokens_saved,
             COALESCE(SUM(cost_saved), 0::numeric)                      AS total_cost_saved,
             COUNT(*) FILTER (WHERE embedding IS NULL)::bigint           AS exact_entries,
             COUNT(*) FILTER (WHERE embedding IS NOT NULL)::bigint       AS semantic_entries
         FROM cache_entries";

    #[cfg(feature = "sqlite")]
    let sql = "SELECT
             COUNT(*)                                   AS total_entries,
             COALESCE(SUM(hit_count), 0)                AS total_hits,
             COALESCE(SUM(tokens_saved), 0)             AS total_tokens_saved,
             COALESCE(SUM(cost_saved), 0)               AS total_cost_saved,
             COUNT(*) FILTER (WHERE embedding IS NULL)  AS exact_entries,
             COUNT(*) FILTER (WHERE embedding IS NOT NULL) AS semantic_entries
         FROM cache_entries";

    let row = sqlx::query_as::<_, StatsRow>(sql).fetch_one(pool).await?;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let total_cost_saved = row.total_cost_saved;

    #[cfg(feature = "sqlite")]
    let total_cost_saved = Decimal::try_from(row.total_cost_saved).unwrap_or_default();

    Ok(CacheStats {
        total_entries: row.total_entries,
        total_hits: row.total_hits,
        total_tokens_saved: row.total_tokens_saved,
        total_cost_saved,
        exact_entries: row.exact_entries,
        semantic_entries: row.semantic_entries,
    })
}

// ── Semantic embedding operations ─────────────────────────────────────────────

/// Persist a computed embedding for an existing cache entry (raw f32 little-endian bytes).
pub async fn save_embedding(pool: &DbPool, prompt_hash: &str, embedding: &[u8]) -> AppResult<()> {
    sqlx::query("UPDATE cache_entries SET embedding = $1 WHERE prompt_hash = $2")
        .bind(embedding)
        .bind(prompt_hash)
        .execute(pool)
        .await?;

    Ok(())
}

/// Load all cache entries for startup warm-up, including their expiry timestamps.
/// Entries that have already expired in the DB are excluded via the WHERE clause.
pub async fn load_all_entries(pool: &DbPool) -> AppResult<Vec<CacheEntryRow>> {
    let rows = sqlx::query_as::<_, CacheEntryRow>(
        "SELECT prompt_hash, response_body, embedding, expires_at
         FROM cache_entries
         WHERE expires_at IS NULL OR expires_at > $1",
    )
    .bind(Utc::now())
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Delete all cache entries whose TTL has elapsed. Returns the number of rows deleted.
pub async fn prune_expired(pool: &DbPool) -> AppResult<u64> {
    let result =
        sqlx::query("DELETE FROM cache_entries WHERE expires_at IS NOT NULL AND expires_at <= $1")
            .bind(Utc::now())
            .execute(pool)
            .await?;

    Ok(result.rows_affected())
}

// ── Single-entry delete ───────────────────────────────────────────────────────

/// Delete a single cache entry by UUID.
///
/// Returns `Some(prompt_hash)` if the row was found and deleted,
/// `None` if no row with that id exists.
pub async fn delete_entry(pool: &DbPool, id: Uuid) -> AppResult<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("DELETE FROM cache_entries WHERE id = $1 RETURNING prompt_hash")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    Ok(row.map(|(h,)| h))
}

// ── Flush ─────────────────────────────────────────────────────────────────────

/// Delete all cache entries. Returns the number of rows deleted.
pub async fn flush_all(pool: &DbPool) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM cache_entries")
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}
