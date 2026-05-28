//! Admin endpoints for the Smart Routing Engine (V5-L6).
//!
//! All endpoints are scoped to a workspace and require ApiManager role or above.
//!
//! Routes:
//!   GET    /admin/workspaces/:workspace_id/smart-routing/config
//!   PUT    /admin/workspaces/:workspace_id/smart-routing/config
//!   GET    /admin/workspaces/:workspace_id/smart-routing/rules
//!   POST   /admin/workspaces/:workspace_id/smart-routing/rules
//!   PATCH  /admin/workspaces/:workspace_id/smart-routing/rules/:rule_id
//!   DELETE /admin/workspaces/:workspace_id/smart-routing/rules/:rule_id

use crate::{
    db::smart_routing::{self as sr_db, UpsertSmartConfig},
    errors::AppError,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role_in_workspace, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpsertConfigRequest {
    pub enabled: bool,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub meta_classifier_enabled: bool,
    #[serde(default = "default_meta_provider")]
    pub meta_classifier_provider: String,
    #[serde(default = "default_meta_model")]
    pub meta_classifier_model: String,
    #[serde(default = "default_meta_timeout")]
    pub meta_classifier_timeout_ms: i32,
    pub max_cost_per_request: Option<Decimal>,
}

fn default_meta_provider() -> String {
    "groq".to_string()
}
fn default_meta_model() -> String {
    "llama-3.1-8b-instant".to_string()
}
fn default_meta_timeout() -> i32 {
    300
}

#[derive(Debug, Serialize)]
pub struct SmartConfigView {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub enabled: bool,
    pub default_model: String,
    pub meta_classifier_enabled: bool,
    pub meta_classifier_provider: String,
    pub meta_classifier_model: String,
    pub meta_classifier_timeout_ms: i32,
    pub max_cost_per_request: Option<Decimal>,
}

impl From<sr_db::WorkspaceSmartConfig> for SmartConfigView {
    fn from(c: sr_db::WorkspaceSmartConfig) -> Self {
        Self {
            id: c.id,
            workspace_id: c.workspace_id,
            enabled: c.enabled,
            default_model: c.default_model,
            meta_classifier_enabled: c.meta_classifier_enabled,
            meta_classifier_provider: c.meta_classifier_provider,
            meta_classifier_model: c.meta_classifier_model,
            meta_classifier_timeout_ms: c.meta_classifier_timeout_ms,
            max_cost_per_request: c.max_cost_per_request,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default = "default_rule_order")]
    pub rule_order: i32,
    pub tag_key: Option<String>,
    pub tag_value: Option<String>,
    pub min_token_estimate: Option<i32>,
    pub max_token_estimate: Option<i32>,
    pub requires_tools: Option<bool>,
    pub requires_vision: Option<bool>,
    pub target_model: String,
}

fn default_rule_order() -> i32 {
    100
}

#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    pub is_enabled: Option<bool>,
}

// ── GET /admin/workspaces/:workspace_id/smart-routing/config ──────────────────

pub async fn get_config(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    match sr_db::get_workspace_smart_config(&state.pool, Some(workspace_id)).await {
        Ok(Some(cfg)) => Json(json!({ "data": SmartConfigView::from(cfg) })).into_response(),
        Ok(None) => Json(json!({
            "data": {
                "enabled": false,
                "default_model": "",
                "meta_classifier_enabled": false,
                "meta_classifier_provider": "groq",
                "meta_classifier_model": "llama-3.1-8b-instant",
                "meta_classifier_timeout_ms": 300,
                "max_cost_per_request": null
            }
        }))
        .into_response(),
        Err(e) => e.into_response(),
    }
}

// ── PUT /admin/workspaces/:workspace_id/smart-routing/config ──────────────────

pub async fn put_config(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<UpsertConfigRequest>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    match sr_db::upsert_workspace_smart_config(
        &state.pool,
        workspace_id,
        UpsertSmartConfig {
            enabled: body.enabled,
            default_model: &body.default_model,
            meta_classifier_enabled: body.meta_classifier_enabled,
            meta_classifier_provider: &body.meta_classifier_provider,
            meta_classifier_model: &body.meta_classifier_model,
            meta_classifier_timeout_ms: body.meta_classifier_timeout_ms,
            max_cost_per_request: body.max_cost_per_request,
        },
    )
    .await
    {
        Ok(cfg) => Json(json!({ "data": SmartConfigView::from(cfg) })).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── GET /admin/workspaces/:workspace_id/smart-routing/rules ───────────────────

pub async fn list_rules(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    match sr_db::list_rules(&state.pool, workspace_id).await {
        Ok(rules) => {
            let total = rules.len();
            Json(json!({ "data": rules, "meta": { "total": total } })).into_response()
        }
        Err(e) => e.into_response(),
    }
}

// ── POST /admin/workspaces/:workspace_id/smart-routing/rules ──────────────────

pub async fn create_rule(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateRuleRequest>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    if body.target_model.is_empty() {
        return AppError::BadRequest("target_model must not be empty".to_string()).into_response();
    }
    if body.name.is_empty() {
        return AppError::BadRequest("name must not be empty".to_string()).into_response();
    }

    match sr_db::create_rule(
        &state.pool,
        workspace_id,
        body.rule_order,
        &body.name,
        body.tag_key.as_deref(),
        body.tag_value.as_deref(),
        body.min_token_estimate,
        body.max_token_estimate,
        body.requires_tools,
        body.requires_vision,
        &body.target_model,
    )
    .await
    {
        Ok(rule) => (StatusCode::CREATED, Json(json!({ "data": rule }))).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── PATCH /admin/workspaces/:workspace_id/smart-routing/rules/:rule_id ────────

pub async fn update_rule(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path((workspace_id, rule_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRuleRequest>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    if let Some(enabled) = body.is_enabled {
        if let Err(e) = sr_db::set_rule_enabled(&state.pool, rule_id, enabled).await {
            return e.into_response();
        }
    }

    // Return updated rule list so the client can refresh its view in one call.
    match sr_db::list_rules(&state.pool, workspace_id).await {
        Ok(rules) => Json(json!({ "data": rules })).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── DELETE /admin/workspaces/:workspace_id/smart-routing/rules/:rule_id ───────

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    AuthUser(user): AuthUser,
    Path((workspace_id, rule_id)): Path<(Uuid, Uuid)>,
) -> impl axum::response::IntoResponse {
    if let Err(e) = require_role_in_workspace(Role::ApiManager, &user, workspace_id, &state).await {
        return e.into_response();
    }

    match sr_db::delete_rule(&state.pool, rule_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
