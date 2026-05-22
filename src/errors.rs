use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    // ── Domain errors ─────────────────────────────────────────────────────────
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    #[allow(dead_code)]
    Forbidden(String),

    #[error("Bad request: {0}")]
    #[allow(dead_code)] // used in Phase 1+ request validation
    BadRequest(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal server error")]
    #[allow(dead_code)] // used in Phase 1+ where a specific code isn't available
    InternalServerError,

    // ── Gateway errors (Phase 1+) ─────────────────────────────────────────────
    /// Gateway-level rate limit. Payload is the `Retry-After` value in seconds
    /// (Some = gateway key limit with a calculable wait; None = provider 429).
    #[error("Rate limit exceeded")]
    RateLimitExceeded(Option<u64>),

    #[error("Budget exceeded")]
    #[allow(dead_code)]
    BudgetExceeded,

    #[error("Provider unavailable: {0}")]
    #[allow(dead_code)]
    ProviderUnavailable(String),

    // ── Infrastructure errors ─────────────────────────────────────────────────
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error(transparent)]
    Bcrypt(#[from] bcrypt::BcryptError),

    #[error(transparent)]
    Config(#[from] config::ConfigError),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl AppError {
    fn error_code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Unauthorized(_) => "UNAUTHORIZED",
            AppError::Forbidden(_) => "FORBIDDEN",
            AppError::BadRequest(_) => "BAD_REQUEST",
            AppError::Conflict(_) => "CONFLICT",
            AppError::InternalServerError => "INTERNAL_SERVER_ERROR",
            AppError::RateLimitExceeded(_) => "RATE_LIMIT_EXCEEDED",
            AppError::BudgetExceeded => "BUDGET_EXCEEDED",
            AppError::ProviderUnavailable(_) => "PROVIDER_UNAVAILABLE",
            AppError::Database(_) => "INTERNAL_SERVER_ERROR",
            AppError::Jwt(_) => "UNAUTHORIZED",
            AppError::Bcrypt(_) => "INTERNAL_SERVER_ERROR",
            AppError::Config(_) => "CONFIGURATION_ERROR",
            AppError::Anyhow(_) => "INTERNAL_SERVER_ERROR",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Unauthorized(_) | AppError::Jwt(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::BudgetExceeded => StatusCode::PAYMENT_REQUIRED,
            AppError::ProviderUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            AppError::InternalServerError
            | AppError::Database(_)
            | AppError::Bcrypt(_)
            | AppError::Config(_)
            | AppError::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> String {
        match self {
            // Never expose internal details for 5xx errors
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                "Internal server error".to_string()
            }
            AppError::Bcrypt(e) => {
                tracing::error!("Bcrypt error: {:?}", e);
                "Internal server error".to_string()
            }
            AppError::Config(e) => {
                tracing::error!("Config error: {:?}", e);
                "Configuration error".to_string()
            }
            AppError::Anyhow(e) => {
                tracing::error!("Internal error: {:?}", e);
                "Internal server error".to_string()
            }
            AppError::InternalServerError => "Internal server error".to_string(),
            other => other.to_string(),
        }
    }
}

/// Admin API error format:
/// `{ "error": { "code": "NOT_FOUND", "message": "..." } }`
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        // Capture Retry-After before self is consumed by error_code/message.
        let retry_after = if let AppError::RateLimitExceeded(Some(secs)) = &self {
            Some(*secs)
        } else {
            None
        };
        let body = json!({
            "error": {
                "code": self.error_code(),
                "message": self.message()
            }
        });
        let mut response = (status, Json(body)).into_response();
        if let Some(secs) = retry_after {
            if let Ok(v) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        response
    }
}

pub type AppResult<T> = Result<T, AppError>;
