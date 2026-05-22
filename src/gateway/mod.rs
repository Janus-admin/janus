pub mod pipeline;
pub mod router;

use crate::models::api_key::ApiKey;
use crate::providers::Provider;
use dashmap::DashMap;
use std::sync::Arc;

/// Registry of all active providers, sorted by priority.
/// Lower priority value = preferred provider.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
    key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
}

impl ProviderRegistry {
    pub fn new(
        providers: Vec<Arc<dyn Provider>>,
        key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    ) -> Self {
        let mut providers = providers;
        providers.sort_by_key(|p| p.priority());
        Self {
            providers,
            key_cache,
        }
    }

    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    pub fn key_cache(&self) -> &Arc<DashMap<[u8; 32], ApiKey>> {
        &self.key_cache
    }
}
