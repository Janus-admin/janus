use crate::{errors::AppResult, models::api_key::ApiKey};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

// ── Key-generation helpers ────────────────────────────────────────────────────

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Generate a fresh `vx-sk-<48 alphanumeric chars>` key (54 chars total).
pub fn generate_key() -> String {
    use rand::Rng;
    let suffix: String = (0..48)
        .map(|_| {
            let idx = rand::thread_rng().gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("vx-sk-{}", suffix)
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

// ── Database operations ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &PgPool,
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
) -> AppResult<ApiKey> {
    let key = sqlx::query_as::<_, ApiKey>(
        "INSERT INTO api_keys (
             id, name, key_hash, key_sha256, key_prefix, workspace_id,
             budget_limit, budget_used, rate_limit_rpm, rate_limit_tpm,
             allowed_models, is_active, created_at, expires_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,0,$8,$9,$10,TRUE,$11,$12)
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
    .bind(Utc::now())
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(key)
}

pub async fn list(pool: &PgPool, page: i64, per_page: i64) -> AppResult<(Vec<ApiKey>, i64)> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys")
        .fetch_one(pool)
        .await?;

    let keys = sqlx::query_as::<_, ApiKey>(
        "SELECT * FROM api_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
    )
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(pool)
    .await?;

    Ok((keys, total.0))
}

/// Load every active key together with its sha256 bytes.
/// Used at startup to populate the in-memory dashmap.
pub async fn load_all_active(pool: &PgPool) -> AppResult<Vec<([u8; 32], ApiKey)>> {
    let keys = sqlx::query_as::<_, ApiKey>(
        "SELECT * FROM api_keys WHERE is_active = TRUE AND key_sha256 IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(keys
        .into_iter()
        .filter_map(|k| {
            let hash = parse_sha256_hex(k.key_sha256.as_deref()?)?;
            Some((hash, k))
        })
        .collect())
}

pub async fn update_last_used(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn add_budget_used(pool: &PgPool, id: Uuid, amount: Decimal) -> AppResult<()> {
    sqlx::query("UPDATE api_keys SET budget_used = budget_used + $1 WHERE id = $2")
        .bind(amount)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
