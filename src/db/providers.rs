use crate::{errors::AppResult, models::provider::Provider};
use chrono::Utc;
use sqlx::PgPool;

pub async fn list_providers(pool: &PgPool) -> AppResult<Vec<Provider>> {
    let rows = sqlx::query_as::<_, Provider>("SELECT * FROM providers ORDER BY priority ASC")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_provider(pool: &PgPool, id: &str) -> AppResult<Option<Provider>> {
    let row = sqlx::query_as::<_, Provider>("SELECT * FROM providers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub struct UpdateProviderParams {
    pub is_enabled: Option<bool>,
    pub priority: Option<i32>,
    pub api_key_encrypted: Option<Option<String>>,
    pub timeout_ms: Option<i32>,
    pub max_retries: Option<i32>,
    pub health_status: Option<String>,
}

pub async fn update_provider(
    pool: &PgPool,
    id: &str,
    p: UpdateProviderParams,
) -> AppResult<Option<Provider>> {
    let existing = match get_provider(pool, id).await? {
        Some(p) => p,
        None => return Ok(None),
    };

    let is_enabled = p.is_enabled.unwrap_or(existing.is_enabled);
    let priority = p.priority.unwrap_or(existing.priority);
    let api_key_encrypted = p.api_key_encrypted.unwrap_or(existing.api_key_encrypted);
    let timeout_ms = p.timeout_ms.unwrap_or(existing.timeout_ms);
    let max_retries = p.max_retries.unwrap_or(existing.max_retries);
    let health_status = p.health_status.unwrap_or(existing.health_status);

    let row = sqlx::query_as::<_, Provider>(
        "UPDATE providers SET
             is_enabled = $1, priority = $2, api_key_encrypted = $3,
             timeout_ms = $4, max_retries = $5, health_status = $6, updated_at = $7
         WHERE id = $8
         RETURNING *",
    )
    .bind(is_enabled)
    .bind(priority)
    .bind(api_key_encrypted)
    .bind(timeout_ms)
    .bind(max_retries)
    .bind(health_status)
    .bind(Utc::now())
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn set_health_status(pool: &PgPool, id: &str, status: &str) -> AppResult<()> {
    sqlx::query("UPDATE providers SET health_status = $1, last_health_check = $2 WHERE id = $3")
        .bind(status)
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
