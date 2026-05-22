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
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, completion_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,FALSE,$12)",
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
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
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
