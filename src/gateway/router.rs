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

/// Look up which provider owns a given model_id in the model_pricing catalogue.
/// Returns `None` when the model is unknown (caller falls back to strategy order).
async fn lookup_model_provider(pool: &DbPool, model: &str) -> Option<String> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let result: Option<String> = sqlx::query_scalar(
        "SELECT provider FROM model_pricing WHERE model_id = $1 AND is_active = TRUE LIMIT 1",
    )
    .bind(model)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    #[cfg(feature = "sqlite")]
    let result: Option<String> = sqlx::query_scalar(
        "SELECT provider FROM model_pricing WHERE model_id = ?1 AND is_active = 1 LIMIT 1",
    )
    .bind(model)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    result
}

/// Return enabled providers ordered according to `strategy`.
///
/// **Model-aware routing**: before applying the strategy ordering we look up
/// the model in `model_pricing` to find its owning provider and move that
/// provider to the front of the list.  This ensures e.g. `claude-*` requests
/// go to Anthropic first and `gemini-*` requests go to Google Gemini first,
/// rather than always starting with the globally highest-priority provider.
///
/// Unknown models (not in `model_pricing`) fall back to pure strategy order.
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

    // Model-aware routing: when the model is catalogued, use ONLY its owning
    // provider.  This prevents requests for e.g. `claude-*` from falling
    // through to DeepSeek when Anthropic rejects with an "unknown model" error.
    // For unknown models we keep the full priority-ordered list as before.
    let ordered = if let Some(owner) = lookup_model_provider(pool, model).await {
        let preferred: Vec<_> = enabled
            .into_iter()
            .filter(|p| p.name() == owner.as_str())
            .collect();
        if preferred.is_empty() {
            // Owning provider not registered/enabled — fall back to full list.
            registry
                .providers()
                .iter()
                .filter(|p| p.is_enabled())
                .cloned()
                .collect()
        } else {
            preferred
        }
    } else {
        enabled
    };

    match strategy {
        RoutingStrategy::Priority => ordered,
        RoutingStrategy::CostOptimized => sort_by_cost(pool, ordered, model).await,
        RoutingStrategy::LatencyOptimized => sort_by_latency(pool, ordered).await,
        RoutingStrategy::RoundRobin => sort_round_robin(ordered, &registry.round_robin_counter),
    }
}
