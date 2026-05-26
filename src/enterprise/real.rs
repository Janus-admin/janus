// src/enterprise/real.rs — Real enterprise implementation.
//
// Compiled ONLY with `--features enterprise`.
// Contains the concrete `EnterpriseState` that writes audit events to the DB,
// enforces the license, and (in future) evaluates the policy engine.
//
// The community binary never sees this file.

use crate::{
    db::DbPool,
    enterprise::{
        license::{self, LicenseFeature, LicenseState},
        AuditEvent, ChargebackQuery, ChargebackRow, EnterpriseExt, PolicyRequest,
    },
};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use std::sync::Arc;

/// Shared state for the enterprise edition.
///
/// Held in `AppState.enterprise` as `Arc<dyn EnterpriseExt>`.
/// The `ArcSwap<LicenseState>` allows the background refresh task to swap in a
/// new state atomically without blocking any in-flight request.
pub struct EnterpriseState {
    pool: DbPool,
    pub(crate) license: Arc<ArcSwap<LicenseState>>,
}

impl EnterpriseState {
    /// Build an `EnterpriseState`, validating the license JWT from the environment.
    pub fn new(pool: DbPool) -> Arc<Self> {
        let initial = license::load_from_env();
        Arc::new(Self {
            pool,
            license: Arc::new(ArcSwap::from_pointee(initial)),
        })
    }

    /// Expose the inner `ArcSwap` so the background refresh task can update it.
    pub fn license_arc(&self) -> Arc<ArcSwap<LicenseState>> {
        self.license.clone()
    }
}

#[async_trait]
impl EnterpriseExt for EnterpriseState {
    fn license_state(&self) -> LicenseState {
        (**self.license.load()).clone()
    }

    fn has_feature(&self, feature: LicenseFeature) -> bool {
        self.license.load().has_feature(&feature)
    }

    /// Spawn a fire-and-forget Tokio task for the DB write.
    /// Returns immediately; the handler is never blocked by I/O.
    fn audit(&self, event: AuditEvent) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::db::audit::insert_event(&pool, &event).await {
                tracing::warn!(
                    action = event.action,
                    resource_type = event.resource_type,
                    "Audit log write failed: {e}"
                );
            }
        });
    }

    async fn policy_check(&self, _req: &PolicyRequest) -> Result<(), String> {
        // Policy engine is a future feature.
        // The ArcSwap for the policy will be read here without any lock once built.
        Ok(())
    }

    async fn refresh_license(&self) {
        let new_state = license::load_from_env();
        self.license.store(Arc::new(new_state));
        tracing::debug!("License refreshed from environment");
    }

    async fn chargeback(&self, query: ChargebackQuery) -> Vec<ChargebackRow> {
        match crate::db::audit::chargeback_report(&self.pool, &query).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Chargeback report query failed: {e}");
                vec![]
            }
        }
    }
}
