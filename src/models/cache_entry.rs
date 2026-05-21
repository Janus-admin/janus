#![allow(dead_code)] // structs used in Phase 4+ handlers

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Database row from the `cache_entries` table.
///
/// Stores both exact-match and semantic cache entries.
/// The HNSW vector index (Phase 5) lives in memory — `embedding` here is for
/// persistence and recovery after restart.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CacheEntry {
    pub id: Uuid,
    /// SHA-256 hex of the normalized request body (exact cache key).
    pub prompt_hash: String,
    /// Serialized f32[] embedding vector. NULL until Phase 5.
    #[serde(skip_serializing)]
    pub embedding: Option<Vec<u8>>,

    pub provider: String,
    pub model: String,
    pub request_body: String,
    pub response_body: String,

    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub cost_usd: Option<Decimal>,

    pub hit_count: i32,
    pub tokens_saved: i64,
    pub cost_saved: Decimal,

    pub created_at: DateTime<Utc>,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Aggregate cache statistics for `GET /admin/cache/stats`.
#[derive(Debug, Serialize)]
pub struct CacheStats {
    pub total_entries: i64,
    pub total_hits: i64,
    pub total_tokens_saved: i64,
    pub total_cost_saved: Decimal,
    pub exact_entries: i64,
    pub semantic_entries: i64,
}

/// Payload for `POST /admin/cache/flush`.
#[derive(Debug, Deserialize)]
pub struct FlushCacheRequest {
    /// Flush only entries for this provider. NULL = flush all.
    pub provider: Option<String>,
    /// Flush only entries for this model. NULL = flush all.
    pub model: Option<String>,
}
