use super::ProviderRegistry;
use crate::providers::Provider;
use std::sync::Arc;

/// Select the highest-priority enabled provider for the given model name.
///
/// Current strategy: return the first enabled provider by ascending priority.
/// Future phases can add model-aware routing (e.g. "only bedrock for anthropic.*").
pub fn select_provider(registry: &ProviderRegistry, _model: &str) -> Option<Arc<dyn Provider>> {
    registry
        .providers()
        .iter()
        .find(|p| p.is_enabled())
        .cloned()
}

/// Return all enabled providers sorted by ascending priority.
/// Used by the retry/failover loop in the pipeline — the caller iterates in order
/// and moves to the next entry when a provider exhausts its retry budget.
pub fn select_all_providers(registry: &ProviderRegistry) -> Vec<Arc<dyn Provider>> {
    registry
        .providers()
        .iter()
        .filter(|p| p.is_enabled())
        .cloned()
        .collect()
}
