use crate::db::DbPool;
use crate::errors::AppResult;
use chrono::Utc;
use rust_decimal::Decimal;
use uuid::Uuid;

// ── Internal row type for pricing lookup ─────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct PricingRow {
    input_per_1m_tokens: Decimal,
    output_per_1m_tokens: Decimal,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct PricingRow {
    input_per_1m_tokens: String,
    output_per_1m_tokens: String,
}

// ── SQLite-only row type for `requests` table ─────────────────────────────────

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct RequestSqliteRow {
    id: Uuid,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: String,
    model: String,
    base_url: Option<String>,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<String>,
    latency_ms: Option<i32>,
    ttfb_ms: Option<i32>,
    status: String,
    cache_type: Option<String>,
    cache_similarity: Option<String>,
    http_status: Option<i32>,
    error_code: Option<String>,
    error_message: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
    stream: bool,
    prompt_version_id: Option<Uuid>,
    created_at: chrono::DateTime<Utc>,
}

#[cfg(feature = "sqlite")]
impl From<RequestSqliteRow> for crate::models::request::Request {
    fn from(r: RequestSqliteRow) -> Self {
        Self {
            id: r.id,
            api_key_id: r.api_key_id,
            workspace_id: r.workspace_id,
            provider: r.provider,
            model: r.model,
            base_url: r.base_url,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
            cost_usd: r.cost_usd.and_then(|s| s.parse().ok()),
            latency_ms: r.latency_ms,
            ttfb_ms: r.ttfb_ms,
            status: r.status,
            cache_type: r.cache_type,
            cache_similarity: r.cache_similarity.and_then(|s| s.parse().ok()),
            http_status: r.http_status,
            error_code: r.error_code,
            error_message: r.error_message,
            request_body: r.request_body,
            response_body: r.response_body,
            stream: r.stream,
            prompt_version_id: r.prompt_version_id,
            created_at: r.created_at,
        }
    }
}

// ── Database operations ───────────────────────────────────────────────────────

/// Insert a completed request into the audit log.
/// Intentionally takes flat params to keep callers readable.
#[allow(clippy::too_many_arguments)]
pub async fn insert_request(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    latency_ms: i32,
    status: &str,
    is_stream: bool,
    ttfb_ms: Option<i32>,
    prompt_version_id: Option<Uuid>,
    downgrade_triggered: bool,
) -> AppResult<()> {
    // SQLite stores cost_usd as TEXT; rebind as string in sqlite builds.
    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());

    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, completion_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, ttfb_ms, prompt_version_id,
             downgrade_triggered, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(Uuid::new_v4())
    .bind(api_key_id)
    .bind(workspace_id)
    .bind(provider)
    .bind(model)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(total_tokens)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(status)
    .bind(is_stream)
    .bind(ttfb_ms)
    .bind(prompt_version_id)
    .bind(downgrade_triggered)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// List requests with optional filters. Returns (rows, total_count).
///
/// V3-5 extended filters: start_time, end_time, has_cache_hit.
#[allow(clippy::too_many_arguments)]
pub async fn list_requests(
    pool: &DbPool,
    page: i64,
    per_page: i64,
    provider: Option<&str>,
    model: Option<&str>,
    status: Option<&str>,
    api_key_id: Option<Uuid>,
    start_time: Option<chrono::DateTime<Utc>>,
    end_time: Option<chrono::DateTime<Utc>>,
    has_cache_hit: Option<bool>,
) -> AppResult<(Vec<crate::models::request::Request>, i64)> {
    // PostgreSQL: ::text / ::uuid / ::timestamptz casts are required so the planner
    // knows the parameter type when the value is NULL.
    // SQLite: no cast syntax; plain `$N IS NULL` works for both NULL and non-NULL.
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let (count_sql, list_sql) = (
        "SELECT COUNT(*) FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)
           AND ($5::timestamptz IS NULL OR created_at >= $5)
           AND ($6::timestamptz IS NULL OR created_at <= $6)
           AND ($7::boolean IS NULL
                OR ($7 = TRUE  AND cache_type IS NOT NULL)
                OR ($7 = FALSE AND cache_type IS NULL))",
        "SELECT * FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)
           AND ($5::timestamptz IS NULL OR created_at >= $5)
           AND ($6::timestamptz IS NULL OR created_at <= $6)
           AND ($7::boolean IS NULL
                OR ($7 = TRUE  AND cache_type IS NOT NULL)
                OR ($7 = FALSE AND cache_type IS NULL))
         ORDER BY created_at DESC
         LIMIT $8 OFFSET $9",
    );

    #[cfg(feature = "sqlite")]
    let (count_sql, list_sql) = (
        "SELECT COUNT(*) FROM requests
         WHERE ($1 IS NULL OR provider = $1)
           AND ($2 IS NULL OR model = $2)
           AND ($3 IS NULL OR status = $3)
           AND ($4 IS NULL OR api_key_id = $4)
           AND ($5 IS NULL OR created_at >= $5)
           AND ($6 IS NULL OR created_at <= $6)
           AND ($7 IS NULL
                OR ($7 = 1 AND cache_type IS NOT NULL)
                OR ($7 = 0 AND cache_type IS NULL))",
        "SELECT * FROM requests
         WHERE ($1 IS NULL OR provider = $1)
           AND ($2 IS NULL OR model = $2)
           AND ($3 IS NULL OR status = $3)
           AND ($4 IS NULL OR api_key_id = $4)
           AND ($5 IS NULL OR created_at >= $5)
           AND ($6 IS NULL OR created_at <= $6)
           AND ($7 IS NULL
                OR ($7 = 1 AND cache_type IS NOT NULL)
                OR ($7 = 0 AND cache_type IS NULL))
         ORDER BY created_at DESC
         LIMIT $8 OFFSET $9",
    );

    // SQLite stores booleans as integers; bind None/Some(0)/Some(1).
    #[cfg(feature = "sqlite")]
    let cache_hit_bind: Option<i64> = has_cache_hit.map(|b| if b { 1 } else { 0 });
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let cache_hit_bind = has_cache_hit;

    let total: (i64,) = sqlx::query_as(count_sql)
        .bind(provider)
        .bind(model)
        .bind(status)
        .bind(api_key_id)
        .bind(start_time)
        .bind(end_time)
        .bind(cache_hit_bind)
        .fetch_one(pool)
        .await?;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let rows = sqlx::query_as::<_, crate::models::request::Request>(list_sql)
        .bind(provider)
        .bind(model)
        .bind(status)
        .bind(api_key_id)
        .bind(start_time)
        .bind(end_time)
        .bind(cache_hit_bind)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;

    #[cfg(feature = "sqlite")]
    let rows: Vec<crate::models::request::Request> =
        sqlx::query_as::<_, RequestSqliteRow>(list_sql)
            .bind(provider)
            .bind(model)
            .bind(status)
            .bind(api_key_id)
            .bind(start_time)
            .bind(end_time)
            .bind(cache_hit_bind)
            .bind(per_page)
            .bind((page - 1) * per_page)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();

    Ok((rows, total.0))
}

pub async fn get_by_id(
    pool: &DbPool,
    id: Uuid,
) -> AppResult<Option<crate::models::request::Request>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let row = sqlx::query_as::<_, crate::models::request::Request>(
        "SELECT * FROM requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    let row: Option<crate::models::request::Request> =
        sqlx::query_as::<_, RequestSqliteRow>("SELECT * FROM requests WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .map(Into::into);

    Ok(row)
}

/// Insert an embedding request into the audit log (`request_type = 'embedding'`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_embedding_request(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    prompt_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    latency_ms: i32,
    status: &str,
) -> AppResult<()> {
    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());

    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, request_type, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(Uuid::new_v4())
    .bind(api_key_id)
    .bind(workspace_id)
    .bind(provider)
    .bind(model)
    .bind(prompt_tokens)
    .bind(total_tokens)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(status)
    .bind(false)
    .bind("embedding")
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// Look up per-token prices for a provider+model pair.
/// Returns `(input_per_1m, output_per_1m)` or `None` if not found.
pub async fn find_pricing(
    pool: &DbPool,
    provider: &str,
    model: &str,
) -> AppResult<Option<(Decimal, Decimal)>> {
    let row = sqlx::query_as::<_, PricingRow>(
        "SELECT input_per_1m_tokens, output_per_1m_tokens
         FROM model_pricing
         WHERE provider = $1 AND model_id = $2 AND is_active = TRUE
         LIMIT 1",
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(pool)
    .await?;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    return Ok(row.map(|r| (r.input_per_1m_tokens, r.output_per_1m_tokens)));

    #[cfg(feature = "sqlite")]
    return Ok(row.and_then(|r| {
        let input = r.input_per_1m_tokens.parse::<Decimal>().ok()?;
        let output = r.output_per_1m_tokens.parse::<Decimal>().ok()?;
        Some((input, output))
    }));
}
