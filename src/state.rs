use crate::config::Config;
use sqlx::PgPool;

/// Shared application state threaded through all axum handlers via `Arc<AppState>`.
///
/// Phase 0: db pool + config only.
/// Phase 1 adds: providers: Arc<ProviderRegistry>
/// Phase 3 adds: metrics: Arc<MetricsStore>
/// Phase 4 adds: cache: Arc<CacheEngine>
///
/// NOTE: The `pool` field will be renamed to `db` (per CLAUDE.md spec) once Phase 1
/// allows touching the handler files that reference `state.pool`.
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
}
