// src/handlers/admin/system.rs — V4-0 system readiness endpoint
//
// GET /admin/system/readiness
//   → 200 when all checks pass
//   → 503 when any check fails (warnings do not affect the status code)

use crate::{doctor, state::AppState};
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::Arc;

/// GET /admin/system/readiness — run all readiness checks and return results.
pub async fn readiness(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    let report = doctor::run_checks(&state.pool, &state.config).await;
    let status = if report.healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(json!({ "data": report })))
}
