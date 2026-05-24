pub mod circuit_breaker;
pub mod dedup;
pub mod pipeline;
pub mod router;
pub mod strategies;
pub mod tool_extract;

use crate::models::api_key::ApiKey;
use crate::providers::Provider;
use circuit_breaker::CircuitBreaker;
use dashmap::DashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

const FAILURE_THRESHOLD: u32 = 5;
const RECOVERY_TIMEOUT_SECS: u64 = 30;

/// Registry of all active providers, sorted by priority.
/// Lower priority value = preferred provider.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
    key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    /// One circuit breaker per provider instance, keyed by provider priority.
    pub circuit_breakers: DashMap<u8, CircuitBreaker>,
    /// Monotonically incrementing counter for round-robin routing.
    pub round_robin_counter: AtomicU64,
}

impl ProviderRegistry {
    pub fn new(
        providers: Vec<Arc<dyn Provider>>,
        key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
    ) -> Self {
        let breakers: DashMap<u8, CircuitBreaker> = DashMap::new();
        for p in &providers {
            breakers.insert(
                p.priority(),
                CircuitBreaker::new(FAILURE_THRESHOLD, RECOVERY_TIMEOUT_SECS),
            );
        }
        let mut providers = providers;
        providers.sort_by_key(|p| p.priority());
        Self {
            providers,
            key_cache,
            circuit_breakers: breakers,
            round_robin_counter: AtomicU64::new(0),
        }
    }

    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    pub fn key_cache(&self) -> &Arc<DashMap<[u8; 32], ApiKey>> {
        &self.key_cache
    }
}
