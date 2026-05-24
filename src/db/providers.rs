use crate::db::DbPool;
use crate::{errors::AppResult, models::provider::Provider};
use chrono::Utc;
use std::collections::HashMap;

pub async fn list_providers(pool: &DbPool) -> AppResult<Vec<Provider>> {
    let rows = sqlx::query_as::<_, Provider>("SELECT * FROM providers ORDER BY priority ASC")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_provider(pool: &DbPool, id: &str) -> AppResult<Option<Provider>> {
    let row = sqlx::query_as::<_, Provider>("SELECT * FROM providers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Load base URLs for all providers from the DB.
/// Returns a map of provider_id → effective base URL.
/// An empty string means "use the adapter's compiled-in default".
pub async fn load_base_urls(pool: &DbPool) -> HashMap<String, String> {
    match sqlx::query_as::<_, (String, String)>(
        "SELECT id, base_url FROM providers WHERE is_enabled = true",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows.into_iter().collect(),
        Err(e) => {
            tracing::warn!("Failed to load provider base_urls from DB: {e}");
            HashMap::new()
        }
    }
}

pub struct UpdateProviderParams {
    pub is_enabled: Option<bool>,
    pub priority: Option<i32>,
    pub api_key_encrypted: Option<Option<String>>,
    pub base_url: Option<String>,
    pub timeout_ms: Option<i32>,
    pub max_retries: Option<i32>,
    pub health_status: Option<String>,
}

pub async fn update_provider(
    pool: &DbPool,
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
    let base_url = p.base_url.unwrap_or(existing.base_url);
    let timeout_ms = p.timeout_ms.unwrap_or(existing.timeout_ms);
    let max_retries = p.max_retries.unwrap_or(existing.max_retries);
    let health_status = p.health_status.unwrap_or(existing.health_status);

    let row = sqlx::query_as::<_, Provider>(
        "UPDATE providers SET
             is_enabled = $1, priority = $2, api_key_encrypted = $3,
             base_url = $4, timeout_ms = $5, max_retries = $6,
             health_status = $7, updated_at = $8
         WHERE id = $9
         RETURNING *",
    )
    .bind(is_enabled)
    .bind(priority)
    .bind(api_key_encrypted)
    .bind(base_url)
    .bind(timeout_ms)
    .bind(max_retries)
    .bind(health_status)
    .bind(Utc::now())
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn set_health_status(pool: &DbPool, id: &str, status: &str) -> AppResult<()> {
    sqlx::query("UPDATE providers SET health_status = $1, last_health_check = $2 WHERE id = $3")
        .bind(status)
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
