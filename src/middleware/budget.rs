use crate::{config::BudgetDowngradeConfig, errors::AppError, models::api_key::ApiKey};
use rust_decimal::prelude::ToPrimitive;

/// Decision returned alongside a passing budget check.
///
/// Use `header_value()` to get the string for the `X-Janus-Downgraded` header.
///
/// When budget spend crosses the downgrade threshold the pipeline should
/// override either the routing strategy or the target model.  `None` means
/// no override — the request proceeds as normal.
#[derive(Debug, Clone, PartialEq)]
pub enum DowngradeDecision {
    /// No downgrade triggered; proceed normally.
    None,
    /// Switch routing strategy to the given string (e.g. "cost_optimized").
    UseStrategy(String),
    /// Override the requested model with the given model ID.
    UseModel(String),
}

impl DowngradeDecision {
    /// Value for the `X-Janus-Downgraded` response header.
    pub fn header_value(&self) -> &str {
        match self {
            DowngradeDecision::None => "",
            DowngradeDecision::UseStrategy(s) => s.as_str(),
            DowngradeDecision::UseModel(_) => "specific_model",
        }
    }
}

/// Check whether the given API key has remaining budget and whether a
/// budget-aware downgrade should be applied.
///
/// - Returns `Err(BudgetExceeded)` if `budget_used >= budget_limit`.
/// - Returns `Ok(DowngradeDecision::*)` with the appropriate override when
///   spend is at or above the downgrade threshold.
/// - Returns `Ok(DowngradeDecision::None)` otherwise.
///
/// Per-key columns (`downgrade_at_percent`, `downgrade_strategy`,
/// `downgrade_to_model`) take precedence over the global `cfg` defaults.
/// If neither the key nor the global config specifies a threshold the
/// function always returns `Ok(None)`.
pub fn check_budget(
    key: &ApiKey,
    cfg: &BudgetDowngradeConfig,
) -> Result<DowngradeDecision, AppError> {
    let Some(limit) = key.budget_limit else {
        return Ok(DowngradeDecision::None);
    };

    // Hard block at 100%.
    if key.budget_used >= limit {
        return Err(AppError::BudgetExceeded);
    }

    // Determine effective threshold: per-key overrides global.
    let effective_threshold: Option<u8> = key
        .downgrade_at_percent
        .map(|p| p.clamp(0, 100) as u8)
        .or(if cfg.enabled {
            Some(cfg.threshold_percent)
        } else {
            None
        });

    let Some(threshold) = effective_threshold else {
        return Ok(DowngradeDecision::None);
    };

    let spend_pct = key
        .budget_used
        .to_f64()
        .zip(limit.to_f64())
        .map(|(used, lim)| if lim > 0.0 { used / lim * 100.0 } else { 0.0 })
        .unwrap_or(0.0);

    if spend_pct < threshold as f64 {
        return Ok(DowngradeDecision::None);
    }

    // Threshold crossed — determine override: per-key model > per-key strategy > global.
    if let Some(model) = key
        .downgrade_to_model
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let s = cfg.fallback_model.as_str();
            if !s.is_empty() {
                // Only use global fallback_model when per-key strategy is "specific_model"
                // or global strategy is "specific_model".
                let strategy = key.downgrade_strategy.as_deref().unwrap_or(&cfg.strategy);
                if strategy == "specific_model" {
                    Some(s)
                } else {
                    None
                }
            } else {
                None
            }
        })
    {
        return Ok(DowngradeDecision::UseModel(model.to_string()));
    }

    let strategy = key
        .downgrade_strategy
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&cfg.strategy);

    Ok(DowngradeDecision::UseStrategy(strategy.to_string()))
}
