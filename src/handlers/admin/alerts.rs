use crate::{
    alerts::webhook::{WebhookContext, WebhookFormat},
    db::alerts::{self as db_alerts, CreateAlertParams, UpdateAlertParams},
    errors::AppResult,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAlertRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub alert_type: String,
    pub threshold: f64,
    #[serde(default = "default_window")]
    pub window_minutes: i32,
    pub webhook_url: Option<String>,
    #[serde(default = "default_format")]
    pub webhook_format: String,
    pub webhook_secret: Option<String>,
}

fn default_window() -> i32 {
    60
}
fn default_format() -> String {
    "generic".to_string()
}

#[derive(Debug, Deserialize)]
pub struct UpdateAlertRequest {
    pub name: Option<String>,
    pub threshold: Option<f64>,
    pub window_minutes: Option<i32>,
    pub is_active: Option<bool>,
    /// Pass `null` to clear the webhook URL.
    pub webhook_url: Option<Value>,
    pub webhook_format: Option<String>,
    /// Pass `null` to clear the secret.
    pub webhook_secret: Option<Value>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /admin/alerts
#[utoipa::path(
    post,
    path = "/admin/alerts",
    tag = "Alerts",
    request_body = serde_json::Value,
    responses(
        (status = 201, description = "Created alert", body = serde_json::Value),
        (status = 400, description = "Invalid threshold value"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn create_alert(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateAlertRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let threshold = Decimal::try_from(body.threshold)
        .map_err(|_| crate::errors::AppError::BadRequest("Invalid threshold value".to_string()))?;

    let alert = db_alerts::create(
        &state.pool,
        CreateAlertParams {
            name: &body.name,
            alert_type: &body.alert_type,
            threshold,
            window_minutes: body.window_minutes,
            webhook_url: body.webhook_url.as_deref(),
            webhook_format: &body.webhook_format,
            webhook_secret: body.webhook_secret.as_deref(),
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(json!({ "data": alert }))))
}

/// GET /admin/alerts
#[utoipa::path(
    get,
    path = "/admin/alerts",
    tag = "Alerts",
    responses(
        (status = 200, description = "Alerts list", body = serde_json::Value),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_alerts(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let alerts = db_alerts::list(&state.pool).await?;
    Ok(Json(json!({
        "data": alerts,
        "meta": { "total": alerts.len() }
    })))
}

/// GET /admin/alerts/:id  — returns alert + history
#[utoipa::path(
    get,
    path = "/admin/alerts/{id}",
    tag = "Alerts",
    params(("id" = uuid::Uuid, Path, description = "Alert UUID")),
    responses(
        (status = 200, description = "Alert + recent history", body = serde_json::Value),
        (status = 404, description = "Alert not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn get_alert(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let alert = db_alerts::get(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Alert {id}")))?;

    let history = db_alerts::get_history(&state.pool, id).await?;

    Ok(Json(json!({
        "data": {
            "alert": alert,
            "history": history,
        }
    })))
}

/// PATCH /admin/alerts/:id
#[utoipa::path(
    patch,
    path = "/admin/alerts/{id}",
    tag = "Alerts",
    params(("id" = uuid::Uuid, Path, description = "Alert UUID")),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Updated alert", body = serde_json::Value),
        (status = 404, description = "Alert not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn update_alert(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAlertRequest>,
) -> AppResult<Json<Value>> {
    let threshold = body
        .threshold
        .map(|f| {
            Decimal::try_from(f)
                .map_err(|_| crate::errors::AppError::BadRequest("Invalid threshold".to_string()))
        })
        .transpose()?;

    fn parse_nullable_string(v: Option<Value>) -> Option<Option<String>> {
        match v {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(s)) => Some(Some(s)),
            _ => None,
        }
    }

    let params = UpdateAlertParams {
        name: body.name,
        threshold,
        window_minutes: body.window_minutes,
        is_active: body.is_active,
        webhook_url: parse_nullable_string(body.webhook_url),
        webhook_format: body.webhook_format,
        webhook_secret: parse_nullable_string(body.webhook_secret),
    };

    let alert = db_alerts::update(&state.pool, id, params)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Alert {id}")))?;

    Ok(Json(json!({ "data": alert })))
}

/// DELETE /admin/alerts/:id
#[utoipa::path(
    delete,
    path = "/admin/alerts/{id}",
    tag = "Alerts",
    params(("id" = uuid::Uuid, Path, description = "Alert UUID")),
    responses(
        (status = 200, description = "Alert deleted", body = serde_json::Value),
        (status = 404, description = "Alert not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn delete_alert(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let deleted = db_alerts::delete(&state.pool, id).await?;
    if !deleted {
        return Err(crate::errors::AppError::NotFound(format!("Alert {id}")));
    }
    Ok((StatusCode::OK, Json(json!({ "data": { "deleted": true } }))))
}

/// POST /admin/alerts/:id/test  — deliver a test webhook regardless of threshold
#[utoipa::path(
    post,
    path = "/admin/alerts/{id}/test",
    tag = "Alerts",
    params(("id" = uuid::Uuid, Path, description = "Alert UUID")),
    responses(
        (status = 200, description = "Webhook delivered successfully", body = serde_json::Value),
        (status = 400, description = "Alert has no webhook URL or delivery failed"),
        (status = 404, description = "Alert not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn test_alert(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let alert = db_alerts::get(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Alert {id}")))?;

    let Some(ref url) = alert.webhook_url else {
        return Err(crate::errors::AppError::BadRequest(
            "Alert has no webhook URL configured".to_string(),
        ));
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let format = WebhookFormat::parse(&alert.webhook_format);
    let now = chrono::Utc::now();
    let ctx = WebhookContext {
        alert_id: alert.id,
        alert_type: &alert.alert_type,
        alert_name: &alert.name,
        message: "Test webhook delivery from Velox",
        value: 0.0,
        threshold: alert.threshold,
        triggered_at: now,
    };

    let secret = db_alerts::get_secret(&state.pool, id).await?;

    let (delivered, error_msg) =
        match crate::alerts::webhook::deliver(&client, url, &format, secret.as_deref(), &ctx).await
        {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };

    db_alerts::record_test_delivery(&state.pool, id, delivered, error_msg.as_deref()).await?;

    if delivered {
        Ok(Json(json!({ "data": { "delivered": true } })))
    } else {
        Err(crate::errors::AppError::BadRequest(
            error_msg.unwrap_or_else(|| "Webhook delivery failed".to_string()),
        ))
    }
}
