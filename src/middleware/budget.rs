use crate::{errors::AppError, models::api_key::ApiKey};

/// Check whether the given API key has remaining budget.
///
/// Returns `AppError::BudgetExceeded` if the key has a `budget_limit` and
/// `budget_used >= budget_limit`. Returns `Ok(())` otherwise.
pub fn check_budget(key: &ApiKey) -> Result<(), AppError> {
    if let Some(limit) = key.budget_limit {
        if key.budget_used >= limit {
            return Err(AppError::BudgetExceeded);
        }
    }
    Ok(())
}
