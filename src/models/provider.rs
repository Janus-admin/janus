#![allow(dead_code)] // structs used in Phase 1+ handlers

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Provider health states as stored in the `providers` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Down,
    Unknown,
}

impl HealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Down => "down",
            HealthStatus::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Database row from the `providers` table.
///
/// `api_key_encrypted` stores the provider's API key encrypted with AES-256-GCM.
/// It is never serialized in API responses.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub is_enabled: bool,
    pub priority: i32,
    #[serde(skip_serializing)]
    pub api_key_encrypted: Option<String>,
    pub base_url: String,
    pub timeout_ms: i32,
    pub max_retries: i32,
    pub retry_delay_ms: i32,
    pub health_status: String,
    pub last_health_check: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Safe public view of a provider (no encrypted key).
#[derive(Debug, Serialize)]
pub struct ProviderView {
    pub id: String,
    pub display_name: String,
    pub is_enabled: bool,
    pub priority: i32,
    pub base_url: String,
    pub timeout_ms: i32,
    pub max_retries: i32,
    pub health_status: String,
    pub last_health_check: Option<DateTime<Utc>>,
}

impl From<Provider> for ProviderView {
    fn from(p: Provider) -> Self {
        Self {
            id: p.id,
            display_name: p.display_name,
            is_enabled: p.is_enabled,
            priority: p.priority,
            base_url: p.base_url,
            timeout_ms: p.timeout_ms,
            max_retries: p.max_retries,
            health_status: p.health_status,
            last_health_check: p.last_health_check,
        }
    }
}

/// Payload for updating a provider (PATCH /admin/providers/:id).
#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    pub is_enabled: Option<bool>,
    pub priority: Option<i32>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub timeout_ms: Option<i32>,
    pub max_retries: Option<i32>,
}
