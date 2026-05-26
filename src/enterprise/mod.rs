// src/enterprise/mod.rs — Open-core boundary.
//
// Everything in this file compiles in BOTH community and enterprise builds.
// The `EnterpriseExt` trait is the single seam: community edition gets
// `CommunityEnterprise` (zero-cost no-ops), enterprise edition gets
// `EnterpriseState` (real DB writes, license enforcement, policy checks).
//
// Build variants:
//   cargo build                              → community (CommunityEnterprise)
//   cargo build --features enterprise        → enterprise (EnterpriseState)

pub mod license;

#[cfg(feature = "enterprise")]
pub mod real;

use crate::enterprise::license::{LicenseFeature, LicenseState};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Audit event ───────────────────────────────────────────────────────────────

/// Describes a single admin mutation to be written to the SOC2 audit log.
///
/// Callers construct this and pass it to `state.enterprise.audit(event)`.
/// In community builds the call compiles to nothing; in enterprise builds
/// it fires off a non-blocking DB write.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    /// Stable action identifier in dot-notation, e.g. `"key.create"`.
    pub action: &'static str,
    /// Category of the affected resource, e.g. `"api_key"`.
    pub resource_type: &'static str,
    /// UUID or slug of the specific resource instance, if applicable.
    pub resource_id: Option<String>,
    /// Workspace context for multi-tenant scoping.
    pub workspace_id: Option<Uuid>,
    /// User ID of the actor performing the mutation.
    pub actor_user_id: Option<Uuid>,
    /// Email of the actor (denormalized for readability in exported reports).
    pub actor_email: Option<String>,
    /// Structured before/after or extra context (provider patch body, key name…).
    pub metadata: Value,
    /// Client IP forwarded by the load balancer or read from the socket.
    pub ip_address: Option<String>,
}

impl AuditEvent {
    /// Convenience constructor for common admin mutations.
    pub fn new(
        action: &'static str,
        resource_type: &'static str,
        resource_id: impl Into<Option<String>>,
        actor_user_id: impl Into<Option<Uuid>>,
        actor_email: impl Into<Option<String>>,
    ) -> Self {
        Self {
            action,
            resource_type,
            resource_id: resource_id.into(),
            workspace_id: None,
            actor_user_id: actor_user_id.into(),
            actor_email: actor_email.into(),
            metadata: Value::Object(Default::default()),
            ip_address: None,
        }
    }

    pub fn with_workspace(mut self, id: Uuid) -> Self {
        self.workspace_id = Some(id);
        self
    }

    pub fn with_metadata(mut self, meta: Value) -> Self {
        self.metadata = meta;
        self
    }
}

// ── Policy check ──────────────────────────────────────────────────────────────

/// Describes an inbound gateway request for the policy engine.
/// Populated in `pipeline.rs` before the provider call.
/// Community builds skip this check (always `Ok(())`).
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    pub workspace_id: Option<Uuid>,
    pub api_key_id: Uuid,
    pub model: String,
    pub endpoint: String,
}

// ── Chargeback types (FinOps stub) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargebackQuery {
    pub workspace_id: Option<Uuid>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargebackRow {
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub cost_usd: f64,
    pub request_count: i64,
}

// ── The boundary trait ────────────────────────────────────────────────────────

/// Implemented by `CommunityEnterprise` (no-ops) and `EnterpriseState` (real).
///
/// Trait is object-safe and `Send + Sync`, so `Arc<dyn EnterpriseExt>` works
/// as an `AppState` field without a generic parameter on every handler.
#[async_trait]
pub trait EnterpriseExt: Send + Sync {
    /// Returns the current license state (Community / Active / Degraded / Expired).
    fn license_state(&self) -> LicenseState;

    /// Fast path: check whether a specific enterprise feature is licensed.
    fn has_feature(&self, feature: LicenseFeature) -> bool;

    /// Fire-and-forget audit write.
    ///
    /// Community: empty function body, zero cost.
    /// Enterprise: spawns a Tokio task to insert into `audit_events`.
    /// Callers do NOT await; control returns immediately either way.
    fn audit(&self, event: AuditEvent);

    /// Gateway policy check (called per-request on the hot path).
    ///
    /// Community: always `Ok(())`.
    /// Enterprise: reads from an in-memory ArcSwap policy — effectively free.
    async fn policy_check(&self, req: &PolicyRequest) -> Result<(), String>;

    /// Re-validates the license JWT from the environment.
    /// Called by a background task every 24 h; no-op in community edition.
    async fn refresh_license(&self);

    /// FinOps chargeback report (stub for a future billing integration).
    async fn chargeback(&self, _query: ChargebackQuery) -> Vec<ChargebackRow> {
        vec![]
    }
}

// ── Community (no-op) implementation ─────────────────────────────────────────

pub struct CommunityEnterprise;

#[async_trait]
impl EnterpriseExt for CommunityEnterprise {
    fn license_state(&self) -> LicenseState {
        LicenseState::Community
    }

    fn has_feature(&self, _feature: LicenseFeature) -> bool {
        false
    }

    /// True zero-cost no-op: the optimizer removes this call entirely.
    fn audit(&self, _event: AuditEvent) {}

    async fn policy_check(&self, _req: &PolicyRequest) -> Result<(), String> {
        Ok(())
    }

    async fn refresh_license(&self) {}
}
