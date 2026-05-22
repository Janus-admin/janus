use crate::state::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<Value> {
    let db_status = match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => "connected",
        Err(_) => "error",
    };

    let providers =
        sqlx::query("SELECT id, is_enabled, health_status FROM providers ORDER BY priority")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                use sqlx::Row;
                json!({
                    "id": r.get::<String, _>("id"),
                    "is_enabled": r.get::<bool, _>("is_enabled"),
                    "health_status": r.get::<String, _>("health_status")
                })
            })
            .collect::<Vec<_>>();

    let status = if db_status == "connected" {
        "ok"
    } else {
        "degraded"
    };

    Json(json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "database": {
            "status": db_status
        },
        "cache": {
            "enabled": state.config.cache_enabled
        },
        "providers": providers
    }))
}
