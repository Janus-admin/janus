use crate::providers::Provider;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reorder `providers` starting from the slot indicated by `counter`, wrapping
/// around. The counter is incremented once per call so successive requests
/// distribute across all enabled providers.
pub fn sort_round_robin(
    providers: Vec<Arc<dyn Provider>>,
    counter: &AtomicU64,
) -> Vec<Arc<dyn Provider>> {
    if providers.is_empty() {
        return providers;
    }
    let start = counter.fetch_add(1, Ordering::Relaxed) as usize % providers.len();
    (0..providers.len())
        .map(|i| providers[(start + i) % providers.len()].clone())
        .collect()
}
