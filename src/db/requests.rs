use crate::errors::AppResult;
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

// ── Internal row type for pricing lookup ─────────────────────────────────────

#[derive(sqlx::FromRow)]
struct PricingRow {
    input_per_1m_tokens: Decimal,
    output_per_1m_tokens: Decimal,
}

// ── Database operations ───────────────────────────────────────────────────────

/// Insert a completed request into the audit log.
/// Intentionally takes flat params to keep callers readable.
#[allow(clippy::too_many_arguments)]
pub async fn insert_request(
    pool: &PgPool,
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
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, completion_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, ttfb_ms, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
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
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// List requests with optional filters. Returns (rows, total_count).
pub async fn list_requests(
    pool: &PgPool,
    page: i64,
    per_page: i64,
    provider: Option<&str>,
    model: Option<&str>,
    status: Option<&str>,
    api_key_id: Option<Uuid>,
) -> AppResult<(Vec<crate::models::request::Request>, i64)> {
    // Build dynamic WHERE clauses via CASE-style binding trick compatible with sqlx.
    // We use a fixed query with nullable parameters instead of dynamic SQL.
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)",
    )
    .bind(provider)
    .bind(model)
    .bind(status)
    .bind(api_key_id)
    .fetch_one(pool)
    .await?;

    let rows = sqlx::query_as::<_, crate::models::request::Request>(
        "SELECT * FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)
         ORDER BY created_at DESC
         LIMIT $5 OFFSET $6",
    )
    .bind(provider)
    .bind(model)
    .bind(status)
    .bind(api_key_id)
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(pool)
    .await?;

    Ok((rows, total.0))
}

pub async fn get_by_id(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<Option<crate::models::request::Request>> {
    let row = sqlx::query_as::<_, crate::models::request::Request>(
        "SELECT * FROM requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Look up per-token prices for a provider+model pair.
/// Returns `(input_per_1m, output_per_1m)` or `None` if not found.
pub async fn find_pricing(
    pool: &PgPool,
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

    Ok(row.map(|r| (r.input_per_1m_tokens, r.output_per_1m_tokens)))
}
