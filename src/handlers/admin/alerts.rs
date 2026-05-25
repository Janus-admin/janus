use crate::{
    alerts::{
        email::{self, EmailContext},
        slack::{self, SlackContext},
        webhook::{WebhookContext, WebhookFormat},
    },
    db::alerts::{self as db_alerts, CreateAlertParams, UpdateAlertParams},
    errors::AppResult,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
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
    /// Native Slack incoming-webhook URL. Receives block-kit payloads.
    pub slack_webhook_url: Option<String>,
    /// Recipient email addresses. Requires SMTP configured in velox.toml.
    #[serde(default)]
    pub email_to: Vec<String>,
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
    /// Pass `null` to clear the Slack webhook URL.
    pub slack_webhook_url: Option<Value>,
    /// Omit to leave unchanged; pass `[]` to clear all recipients.
    pub email_to: Option<Vec<String>>,
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
            slack_webhook_url: body.slack_webhook_url.as_deref(),
            email_to: &body.email_to,
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
        slack_webhook_url: parse_nullable_string(body.slack_webhook_url),
        email_to: body.email_to,
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

/// POST /admin/alerts/:id/test
///
/// Delivers a sample payload to every configured channel (generic webhook,
/// Slack, and/or email) regardless of whether the threshold is currently met.
/// Requires at least one channel to be configured.
#[utoipa::path(
    post,
    path = "/admin/alerts/{id}/test",
    tag = "Alerts",
    params(("id" = uuid::Uuid, Path, description = "Alert UUID")),
    responses(
        (status = 200, description = "Sample payload delivered to all channels", body = serde_json::Value),
        (status = 400, description = "No channels configured, or one or more deliveries failed"),
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

    let has_webhook = alert.webhook_url.is_some();
    let has_slack = alert.slack_webhook_url.is_some();
    let has_email = !alert.email_to.is_empty();

    if !has_webhook && !has_slack && !has_email {
        return Err(crate::errors::AppError::BadRequest(
            "Alert has no delivery channels configured (webhook_url, slack_webhook_url, or email_to)"
                .to_string(),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let now = Utc::now();
    let mut errors: Vec<String> = Vec::new();

    // Channel 1: generic HTTP webhook.
    if let Some(ref url) = alert.webhook_url {
        let format = WebhookFormat::parse(&alert.webhook_format);
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
        if let Err(e) =
            crate::alerts::webhook::deliver(&client, url, &format, secret.as_deref(), &ctx).await
        {
            errors.push(format!("webhook: {e}"));
        }
    }

    // Channel 2: Slack block-kit.
    if let Some(ref url) = alert.slack_webhook_url {
        let ctx = SlackContext {
            alert_id: alert.id,
            alert_name: &alert.name,
            alert_type: &alert.alert_type,
            value: 0.0,
            threshold: alert.threshold,
            triggered_at: now,
        };
        if let Err(e) = slack::send(&client, url, &ctx).await {
            errors.push(format!("slack: {e}"));
        }
    }

    // Channel 3: email.
    if has_email {
        let ctx = EmailContext {
            alert_name: &alert.name,
            alert_type: &alert.alert_type,
            value: 0.0,
            threshold: alert.threshold,
            triggered_at: now,
        };
        if let Err(e) = email::send(&state.config.smtp, &alert.email_to, &ctx).await {
            errors.push(format!("email: {e}"));
        }
    }

    let delivered = errors.is_empty();
    let error_msg = if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };

    db_alerts::record_test_delivery(&state.pool, id, delivered, error_msg.as_deref()).await?;

    if delivered {
        Ok(Json(json!({ "data": { "delivered": true } })))
    } else {
        Err(crate::errors::AppError::BadRequest(
            error_msg.unwrap_or_else(|| "One or more channel deliveries failed".to_string()),
        ))
    }
}
