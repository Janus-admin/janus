use crate::{errors::AppResult, state::AppState};
use axum::{extract::State, Json};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Serialize)]
pub struct ModelRow {
    pub model_id: String,
    pub provider: String,
    pub model_display_name: Option<String>,
    pub input_per_1m_tokens: Decimal,
    pub output_per_1m_tokens: Decimal,
    pub context_window: Option<i32>,
    pub supports_functions: bool,
}

/// GET /admin/models — list all active models with pricing info.
pub async fn list_models(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let rows = sqlx::query_as!(
        ModelRow,
        r#"SELECT model_id, provider, model_display_name,
                  input_per_1m_tokens, output_per_1m_tokens,
                  context_window, supports_functions
           FROM model_pricing
           WHERE is_active = TRUE
           ORDER BY provider, model_display_name"#
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(json!({ "data": rows })))
}
