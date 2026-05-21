#![allow(dead_code)] // structs used in Phase 1+ handlers

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Terminal state of a proxied request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Success,
    Error,
    Cached,
}

impl RequestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequestStatus::Success => "success",
            RequestStatus::Error => "error",
            RequestStatus::Cached => "cached",
        }
    }
}

impl std::fmt::Display for RequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a request was served from cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheType {
    Exact,
    Semantic,
}

impl CacheType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CacheType::Exact => "exact",
            CacheType::Semantic => "semantic",
        }
    }
}

/// Database row from the `requests` table.
///
/// Every LLM request proxied through Velox is logged here.
/// `request_body` and `response_body` are NULL unless logging is enabled in config.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Request {
    pub id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,

    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,

    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cost_usd: Option<Decimal>,

    pub latency_ms: Option<i32>,
    pub ttfb_ms: Option<i32>,

    pub status: String,
    pub cache_type: Option<String>,
    pub cache_similarity: Option<Decimal>,
    pub http_status: Option<i32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,

    pub request_body: Option<String>,
    pub response_body: Option<String>,

    pub stream: bool,
    pub created_at: DateTime<Utc>,
}

/// Minimal summary shown in the live feed / list endpoints.
#[derive(Debug, Serialize)]
pub struct RequestSummary {
    pub id: Uuid,
    pub provider: String,
    pub model: String,
    pub total_tokens: Option<i32>,
    pub cost_usd: Option<Decimal>,
    pub latency_ms: Option<i32>,
    pub status: String,
    pub cache_type: Option<String>,
    pub stream: bool,
    pub created_at: DateTime<Utc>,
}

impl From<Request> for RequestSummary {
    fn from(r: Request) -> Self {
        Self {
            id: r.id,
            provider: r.provider,
            model: r.model,
            total_tokens: r.total_tokens,
            cost_usd: r.cost_usd,
            latency_ms: r.latency_ms,
            status: r.status,
            cache_type: r.cache_type,
            stream: r.stream,
            created_at: r.created_at,
        }
    }
}

/// Query parameters for `GET /admin/requests`.
#[derive(Debug, Deserialize)]
pub struct RequestFilter {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub api_key_id: Option<Uuid>,
}
