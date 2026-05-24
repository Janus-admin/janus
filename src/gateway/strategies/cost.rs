use crate::db::DbPool;
use crate::providers::Provider;
use std::sync::Arc;

/// Sort `providers` by ascending total per-1M token cost (input + output) for `model`.
/// Providers with no pricing data in the DB are sorted to the end, retaining
/// their relative priority order among themselves.
pub async fn sort_by_cost(
    pool: &DbPool,
    providers: Vec<Arc<dyn Provider>>,
    model: &str,
) -> Vec<Arc<dyn Provider>> {
    let mut scored: Vec<(rust_decimal::Decimal, Arc<dyn Provider>)> = Vec::new();

    for p in providers {
        let total_cost = crate::db::requests::find_pricing(pool, p.name(), model)
            .await
            .ok()
            .flatten()
            .map(|(inp, out)| inp + out)
            .unwrap_or(rust_decimal::Decimal::MAX);
        scored.push((total_cost, p));
    }

    // stable sort preserves original priority order within equal-cost groups
    scored.sort_by_key(|a| a.0);
    scored.into_iter().map(|(_, p)| p).collect()
}
