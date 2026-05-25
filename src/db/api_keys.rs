use crate::{db::DbPool, errors::AppResult, models::api_key::ApiKey};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ── Key-generation helpers ────────────────────────────────────────────────────

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Generate a fresh `jn-sk-<48 alphanumeric chars>` key (54 chars total).
pub fn generate_key() -> String {
    use rand::Rng;
    let suffix: String = (0..48)
        .map(|_| {
            let idx = rand::thread_rng().gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("jn-sk-{}", suffix)
}

/// Compute the hex-encoded SHA-256 of `key` for dashmap storage.
pub fn sha256_hex(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Parse a 64-char hex SHA-256 string back to bytes for dashmap lookup.
pub fn sha256_bytes(key: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    h.finalize().into()
}

/// Decode a stored 64-char hex SHA-256 string to bytes.
/// Returns `None` if the string is malformed.
pub fn parse_sha256_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

// ── SQLite-only row type ──────────────────────────────────────────────────────
//
// SQLite stores `allowed_models` as TEXT (a JSON array string) because SQLite
// has no native array type.  We read it into this intermediate row type and
// convert to ApiKey by deserialising the JSON.

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct ApiKeyRowSqlite {
    pub id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub key_sha256: Option<String>,
    pub previous_key_sha256: Option<String>,
    pub rotation_expires_at: Option<DateTime<Utc>>,
    pub key_prefix: String,
    pub workspace_id: Option<Uuid>,
    // budget columns stored as TEXT in SQLite; read as String, parsed to Decimal in From<>
    pub budget_limit: Option<String>,
    pub budget_used: String,
    pub rate_limit_rpm: Option<i32>,
    pub rate_limit_tpm: Option<i32>,
    pub allowed_models: Option<String>, // JSON: '["gpt-4o","claude-3-5-sonnet"]'
    pub routing_strategy: String,
    pub downgrade_at_percent: Option<i32>,
    pub downgrade_strategy: Option<String>,
    pub downgrade_to_model: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[cfg(feature = "sqlite")]
impl From<ApiKeyRowSqlite> for ApiKey {
    fn from(row: ApiKeyRowSqlite) -> Self {
        let allowed_models = row
            .allowed_models
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        ApiKey {
            id: row.id,
            name: row.name,
            key_hash: row.key_hash,
            key_sha256: row.key_sha256,
            previous_key_sha256: row.previous_key_sha256,
            rotation_expires_at: row.rotation_expires_at,
            key_prefix: row.key_prefix,
            workspace_id: row.workspace_id,
            budget_limit: row.budget_limit.and_then(|s| s.parse().ok()),
            budget_used: row.budget_used.parse().unwrap_or_default(),
            rate_limit_rpm: row.rate_limit_rpm,
            rate_limit_tpm: row.rate_limit_tpm,
            allowed_models,
            routing_strategy: row.routing_strategy,
            downgrade_at_percent: row.downgrade_at_percent,
            downgrade_strategy: row.downgrade_strategy,
            downgrade_to_model: row.downgrade_to_model,
            is_active: row.is_active,
            created_at: row.created_at,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
        }
    }
}

// ── Shared helper: encode allowed_models for the active backend ───────────────

#[cfg(feature = "sqlite")]
fn encode_allowed_models(models: &Option<Vec<String>>) -> Option<String> {
    models
        .as_ref()
        .map(|m| serde_json::to_string(m).unwrap_or_default())
}

// ── Database operations ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &DbPool,
    id: Uuid,
    name: &str,
    key_hash: &str,
    key_sha256: &str,
    key_prefix: &str,
    workspace_id: Option<Uuid>,
    budget_limit: Option<Decimal>,
    rate_limit_rpm: Option<i32>,
    rate_limit_tpm: Option<i32>,
    allowed_models: Option<Vec<String>>,
    expires_at: Option<DateTime<Utc>>,
    routing_strategy: &str,
    downgrade_at_percent: Option<i32>,
    downgrade_strategy: Option<&str>,
    downgrade_to_model: Option<&str>,
) -> AppResult<ApiKey> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let key = sqlx::query_as::<_, ApiKey>(
            "INSERT INTO api_keys (
                 id, name, key_hash, key_sha256, key_prefix, workspace_id,
                 budget_limit, budget_used, rate_limit_rpm, rate_limit_tpm,
                 allowed_models, routing_strategy,
                 downgrade_at_percent, downgrade_strategy, downgrade_to_model,
                 is_active, created_at, expires_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,0,$8,$9,$10,$11,$12,$13,$14,TRUE,$15,$16)
             RETURNING *",
        )
        .bind(id)
        .bind(name)
        .bind(key_hash)
        .bind(key_sha256)
        .bind(key_prefix)
        .bind(workspace_id)
        .bind(budget_limit)
        .bind(rate_limit_rpm)
        .bind(rate_limit_tpm)
        .bind(allowed_models)
        .bind(routing_strategy)
        .bind(downgrade_at_percent)
        .bind(downgrade_strategy)
        .bind(downgrade_to_model)
        .bind(Utc::now())
        .bind(expires_at)
        .fetch_one(pool)
        .await?;
        Ok(key)
    }

    #[cfg(feature = "sqlite")]
    {
        let models_json = encode_allowed_models(&allowed_models);
        let budget_limit_str = budget_limit.map(|d| d.to_string());
        let row = sqlx::query_as::<_, ApiKeyRowSqlite>(
            "INSERT INTO api_keys (
                 id, name, key_hash, key_sha256, key_prefix, workspace_id,
                 budget_limit, budget_used, rate_limit_rpm, rate_limit_tpm,
                 allowed_models, routing_strategy,
                 downgrade_at_percent, downgrade_strategy, downgrade_to_model,
                 is_active, created_at, expires_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,'0',$8,$9,$10,$11,$12,$13,$14,1,$15,$16)
             RETURNING *",
        )
        .bind(id)
        .bind(name)
        .bind(key_hash)
        .bind(key_sha256)
        .bind(key_prefix)
        .bind(workspace_id)
        .bind(budget_limit_str)
        .bind(rate_limit_rpm)
        .bind(rate_limit_tpm)
        .bind(models_json)
        .bind(routing_strategy)
        .bind(downgrade_at_percent)
        .bind(downgrade_strategy)
        .bind(downgrade_to_model)
        .bind(Utc::now())
        .bind(expires_at)
        .fetch_one(pool)
        .await?;
        Ok(row.into())
    }
}

pub async fn list(pool: &DbPool, page: i64, per_page: i64) -> AppResult<(Vec<ApiKey>, i64)> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys")
        .fetch_one(pool)
        .await?;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let keys = sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;
        Ok((keys, total.0))
    }

    #[cfg(feature = "sqlite")]
    {
        let rows = sqlx::query_as::<_, ApiKeyRowSqlite>(
            "SELECT * FROM api_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;
        Ok((rows.into_iter().map(Into::into).collect(), total.0))
    }
}

/// Load every active key together with its sha256 bytes.
/// Used at startup to populate the in-memory dashmap.
///
/// For keys with an active rotation grace period (rotation_expires_at in the future),
/// also emits an entry under the previous_key_sha256 so old keys stay valid.
pub async fn load_all_active(pool: &DbPool) -> AppResult<Vec<([u8; 32], ApiKey)>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let keys = sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys WHERE is_active = TRUE AND key_sha256 IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;

        let mut out = Vec::with_capacity(keys.len());
        for k in keys {
            if let Some(hash) = parse_sha256_hex(k.key_sha256.as_deref().unwrap_or("")) {
                // Also register the previous (old) key hash during its grace period.
                if let (Some(prev_hex), Some(expires_at)) =
                    (&k.previous_key_sha256, &k.rotation_expires_at)
                {
                    if *expires_at > chrono::Utc::now() {
                        if let Some(prev_hash) = parse_sha256_hex(prev_hex) {
                            out.push((prev_hash, k.clone()));
                        }
                    }
                }
                out.push((hash, k));
            }
        }
        Ok(out)
    }

    #[cfg(feature = "sqlite")]
    {
        let rows = sqlx::query_as::<_, ApiKeyRowSqlite>(
            "SELECT * FROM api_keys WHERE is_active = 1 AND key_sha256 IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let key: ApiKey = r.into();
            if let Some(hash) = parse_sha256_hex(key.key_sha256.as_deref().unwrap_or("")) {
                if let (Some(prev_hex), Some(expires_at)) =
                    (&key.previous_key_sha256, &key.rotation_expires_at)
                {
                    if *expires_at > chrono::Utc::now() {
                        if let Some(prev_hash) = parse_sha256_hex(prev_hex) {
                            out.push((prev_hash, key.clone()));
                        }
                    }
                }
                out.push((hash, key));
            }
        }
        Ok(out)
    }
}

pub async fn update_last_used(pool: &DbPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn add_budget_used(pool: &DbPool, id: Uuid, amount: Decimal) -> AppResult<()> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    sqlx::query("UPDATE api_keys SET budget_used = budget_used + $1 WHERE id = $2")
        .bind(amount)
        .bind(id)
        .execute(pool)
        .await?;

    #[cfg(feature = "sqlite")]
    {
        // SQLite stores budget_used as TEXT; use PRINTF to keep decimal notation.
        let amount_f64 = f64::try_from(amount).unwrap_or(0.0);
        sqlx::query(
            "UPDATE api_keys SET budget_used = PRINTF('%.8f', CAST(budget_used AS REAL) + $1) WHERE id = $2",
        )
        .bind(amount_f64)
        .bind(id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn get_by_id(pool: &DbPool, id: Uuid) -> AppResult<Option<ApiKey>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        Ok(
            sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?,
        )
    }

    #[cfg(feature = "sqlite")]
    {
        let row = sqlx::query_as::<_, ApiKeyRowSqlite>("SELECT * FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(Into::into))
    }
}

#[derive(Debug)]
pub struct UpdateKeyParams {
    pub name: Option<String>,
    pub budget_limit: Option<Option<Decimal>>,
    pub rate_limit_rpm: Option<Option<i32>>,
    pub rate_limit_tpm: Option<Option<i32>>,
    pub allowed_models: Option<Option<Vec<String>>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub is_active: Option<bool>,
    pub routing_strategy: Option<String>,
    pub downgrade_at_percent: Option<Option<i32>>,
    pub downgrade_strategy: Option<Option<String>>,
    pub downgrade_to_model: Option<Option<String>>,
}

pub async fn update_key(pool: &DbPool, id: Uuid, p: UpdateKeyParams) -> AppResult<Option<ApiKey>> {
    let existing = match get_by_id(pool, id).await? {
        Some(k) => k,
        None => return Ok(None),
    };

    let name = p.name.unwrap_or(existing.name);
    let budget_limit = p.budget_limit.unwrap_or(existing.budget_limit);
    let rate_limit_rpm = p.rate_limit_rpm.unwrap_or(existing.rate_limit_rpm);
    let rate_limit_tpm = p.rate_limit_tpm.unwrap_or(existing.rate_limit_tpm);
    let allowed_models = p.allowed_models.unwrap_or(existing.allowed_models);
    let expires_at = p.expires_at.unwrap_or(existing.expires_at);
    let is_active = p.is_active.unwrap_or(existing.is_active);
    let routing_strategy = p.routing_strategy.unwrap_or(existing.routing_strategy);
    let downgrade_at_percent = p
        .downgrade_at_percent
        .unwrap_or(existing.downgrade_at_percent);
    let downgrade_strategy = p.downgrade_strategy.unwrap_or(existing.downgrade_strategy);
    let downgrade_to_model = p.downgrade_to_model.unwrap_or(existing.downgrade_to_model);

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let key = sqlx::query_as::<_, ApiKey>(
            "UPDATE api_keys SET
                 name = $1, budget_limit = $2, rate_limit_rpm = $3, rate_limit_tpm = $4,
                 allowed_models = $5, expires_at = $6, is_active = $7, routing_strategy = $8,
                 downgrade_at_percent = $9, downgrade_strategy = $10, downgrade_to_model = $11
             WHERE id = $12
             RETURNING *",
        )
        .bind(name)
        .bind(budget_limit)
        .bind(rate_limit_rpm)
        .bind(rate_limit_tpm)
        .bind(allowed_models)
        .bind(expires_at)
        .bind(is_active)
        .bind(routing_strategy)
        .bind(downgrade_at_percent)
        .bind(downgrade_strategy)
        .bind(downgrade_to_model)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(key)
    }

    #[cfg(feature = "sqlite")]
    {
        let models_json = encode_allowed_models(&allowed_models);
        let budget_limit_str = budget_limit.map(|d| d.to_string());
        let row = sqlx::query_as::<_, ApiKeyRowSqlite>(
            "UPDATE api_keys SET
                 name = $1, budget_limit = $2, rate_limit_rpm = $3, rate_limit_tpm = $4,
                 allowed_models = $5, expires_at = $6, is_active = $7, routing_strategy = $8,
                 downgrade_at_percent = $9, downgrade_strategy = $10, downgrade_to_model = $11
             WHERE id = $12
             RETURNING *",
        )
        .bind(name)
        .bind(budget_limit_str)
        .bind(rate_limit_rpm)
        .bind(rate_limit_tpm)
        .bind(models_json)
        .bind(expires_at)
        .bind(is_active as i64)
        .bind(routing_strategy)
        .bind(downgrade_at_percent)
        .bind(downgrade_strategy)
        .bind(downgrade_to_model)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Into::into))
    }
}

pub async fn revoke_key(pool: &DbPool, id: Uuid) -> AppResult<bool> {
    let result = sqlx::query("UPDATE api_keys SET is_active = FALSE WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Rotate an API key: generate new credentials while keeping the same ID.
///
/// The old `key_sha256` is moved to `previous_key_sha256`, which remains valid
/// until `rotation_expires_at` (= now + grace_period_secs). The new key is active
/// immediately. Returns the updated ApiKey or None if the id does not exist.
pub async fn rotate_key(
    pool: &DbPool,
    id: Uuid,
    new_key_sha256: &str,
    new_key_hash: &str,
    new_key_prefix: &str,
    grace_period_secs: u64,
) -> AppResult<Option<ApiKey>> {
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(grace_period_secs as i64);

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let key = sqlx::query_as::<_, ApiKey>(
            "UPDATE api_keys
             SET previous_key_sha256 = key_sha256,
                 rotation_expires_at = $1,
                 key_sha256          = $2,
                 key_hash            = $3,
                 key_prefix          = $4
             WHERE id = $5 AND is_active = TRUE
             RETURNING *",
        )
        .bind(expires_at)
        .bind(new_key_sha256)
        .bind(new_key_hash)
        .bind(new_key_prefix)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(key)
    }

    #[cfg(feature = "sqlite")]
    {
        let row = sqlx::query_as::<_, ApiKeyRowSqlite>(
            "UPDATE api_keys
             SET previous_key_sha256 = key_sha256,
                 rotation_expires_at = $1,
                 key_sha256          = $2,
                 key_hash            = $3,
                 key_prefix          = $4
             WHERE id = $5 AND is_active = 1
             RETURNING *",
        )
        .bind(expires_at)
        .bind(new_key_sha256)
        .bind(new_key_hash)
        .bind(new_key_prefix)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Into::into))
    }
}
