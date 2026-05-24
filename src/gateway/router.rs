use super::ProviderRegistry;
use crate::db::DbPool;
use crate::gateway::strategies::{
    cost::sort_by_cost, latency::sort_by_latency, round_robin::sort_round_robin, RoutingStrategy,
};
use crate::providers::Provider;
use std::sync::Arc;

/// Select the highest-priority enabled provider (original priority routing).
pub fn select_provider(registry: &ProviderRegistry, _model: &str) -> Option<Arc<dyn Provider>> {
    registry
        .providers()
        .iter()
        .find(|p| p.is_enabled())
        .cloned()
}

/// Return all enabled providers sorted by ascending priority.
/// Used by the retry/failover loop in the pipeline for priority routing.
pub fn select_all_providers(registry: &ProviderRegistry) -> Vec<Arc<dyn Provider>> {
    registry
        .providers()
        .iter()
        .filter(|p| p.is_enabled())
        .cloned()
        .collect()
}

/// Return enabled providers ordered according to `strategy`.
/// For `CostOptimized` and `LatencyOptimized` this issues a DB query, so the
/// function is async.  `Priority` and `RoundRobin` are synchronous no-ops.
pub async fn select_providers_for_strategy(
    pool: &DbPool,
    registry: &ProviderRegistry,
    strategy: &RoutingStrategy,
    model: &str,
) -> Vec<Arc<dyn Provider>> {
    let enabled: Vec<Arc<dyn Provider>> = registry
        .providers()
        .iter()
        .filter(|p| p.is_enabled())
        .cloned()
        .collect();

    if enabled.is_empty() {
        return enabled;
    }

    match strategy {
        RoutingStrategy::Priority => enabled,
        RoutingStrategy::CostOptimized => sort_by_cost(pool, enabled, model).await,
        RoutingStrategy::LatencyOptimized => sort_by_latency(pool, enabled).await,
        RoutingStrategy::RoundRobin => sort_round_robin(enabled, &registry.round_robin_counter),
    }
}
