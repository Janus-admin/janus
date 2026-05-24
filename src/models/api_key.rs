#![allow(dead_code)] // structs used in Phase 1+ handlers

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Database row from the `api_keys` table.
///
/// The full API key (vx-sk-...) is NEVER stored here.
/// `key_hash` is the bcrypt hash (source of truth for validation).
/// `key_prefix` is the first 12 chars shown in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub key_hash: String,
    /// SHA-256 hex of the full key — used for dashmap lookup (never exposed via API).
    #[serde(skip)]
    pub key_sha256: Option<String>,
    pub key_prefix: String,
    pub workspace_id: Option<Uuid>,

    pub budget_limit: Option<Decimal>,
    pub budget_used: Decimal,

    pub rate_limit_rpm: Option<i32>,
    pub rate_limit_tpm: Option<i32>,

    pub allowed_models: Option<Vec<String>>,

    /// Routing strategy name as stored in the DB (e.g. "priority", "cost", "latency", "round_robin").
    pub routing_strategy: String,

    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Payload for creating a new API key.
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub workspace_id: Option<Uuid>,
    pub budget_limit: Option<Decimal>,
    pub rate_limit_rpm: Option<i32>,
    pub rate_limit_tpm: Option<i32>,
    pub allowed_models: Option<Vec<String>>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Routing strategy for this key. Defaults to "priority" (original behavior).
    #[serde(default = "default_routing_strategy")]
    pub routing_strategy: String,
}

fn default_routing_strategy() -> String {
    "priority".to_string()
}

/// Response returned once on key creation — full key shown here, never again.
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub routing_strategy: String,
    pub created_at: DateTime<Utc>,
}

/// Safe public view of an API key (no hashes, no full key).
#[derive(Debug, Serialize)]
pub struct ApiKeyView {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub workspace_id: Option<Uuid>,
    #[serde(with = "rust_decimal::serde::float_option")]
    pub budget_limit: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::float")]
    pub budget_used: Decimal,
    pub rate_limit_rpm: Option<i32>,
    pub rate_limit_tpm: Option<i32>,
    pub allowed_models: Option<Vec<String>>,
    pub routing_strategy: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<ApiKey> for ApiKeyView {
    fn from(k: ApiKey) -> Self {
        Self {
            id: k.id,
            name: k.name,
            key_prefix: k.key_prefix,
            workspace_id: k.workspace_id,
            budget_limit: k.budget_limit,
            budget_used: k.budget_used,
            rate_limit_rpm: k.rate_limit_rpm,
            rate_limit_tpm: k.rate_limit_tpm,
            allowed_models: k.allowed_models,
            routing_strategy: k.routing_strategy,
            is_active: k.is_active,
            created_at: k.created_at,
            expires_at: k.expires_at,
            last_used_at: k.last_used_at,
        }
    }
}
