// src/handlers/admin/members.rs — Workspace member management (V4-8c)
//
// All endpoints require admin role in the target workspace.

use crate::{
    db::rbac as db_rbac,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role_in_workspace, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub email: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemberRequest {
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct MemberView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db_rbac::MemberRow> for MemberView {
    fn from(r: db_rbac::MemberRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            user_id: r.user_id,
            email: r.email,
            name: r.name,
            role: r.role.as_str().to_string(),
            created_at: r.created_at,
        }
    }
}

fn valid_role(role: &str) -> bool {
    matches!(
        role,
        "admin" | "api_manager" | "billing_viewer" | "read_only"
    )
}

#[derive(Debug, Serialize)]
struct WorkspaceView {
    id: Uuid,
    name: String,
    slug: String,
    member_count: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /admin/workspaces — list all workspaces with member counts.
#[utoipa::path(
    get,
    path = "/admin/workspaces",
    tag = "Workspaces",
    responses(
        (status = 200, description = "Workspaces + member counts", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires Admin role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<Value>> {
    crate::middleware::rbac::require_role(Role::Admin, &auth.0, &state).await?;

    let rows = db_rbac::list_workspaces(&state.pool).await?;
    let views: Vec<WorkspaceView> = rows
        .into_iter()
        .map(|r| WorkspaceView {
            id: r.id,
            name: r.name,
            slug: r.slug,
            member_count: r.member_count,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(
        json!({ "data": views, "meta": { "total": views.len() } }),
    ))
}

/// GET /admin/workspaces/:workspace_id/members
#[utoipa::path(
    get,
    path = "/admin/workspaces/{workspace_id}/members",
    tag = "Workspaces",
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace UUID")),
    responses(
        (status = 200, description = "Workspace members", body = serde_json::Value),
        (status = 403, description = "Forbidden — requires Admin role in this workspace"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_members(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    require_role_in_workspace(Role::Admin, &auth.0, workspace_id, &state).await?;

    let members = db_rbac::list_members(&state.pool, workspace_id).await?;
    let views: Vec<MemberView> = members.into_iter().map(MemberView::from).collect();

    Ok(Json(json!({
        "data": views,
        "meta": { "total": views.len() }
    })))
}

/// POST /admin/workspaces/:workspace_id/members
#[utoipa::path(
    post,
    path = "/admin/workspaces/{workspace_id}/members",
    tag = "Workspaces",
    params(("workspace_id" = uuid::Uuid, Path, description = "Workspace UUID")),
    request_body = serde_json::Value,
    responses(
        (status = 201, description = "Member added", body = serde_json::Value),
        (status = 400, description = "Invalid role"),
        (status = 404, description = "User not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn add_member(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    require_role_in_workspace(Role::Admin, &auth.0, workspace_id, &state).await?;

    if !valid_role(&body.role) {
        return Err(crate::errors::AppError::BadRequest(format!(
            "Invalid role '{}'. Valid roles: admin, api_manager, billing_viewer, read_only",
            body.role
        )));
    }

    let user_id = db_rbac::find_user_by_email(&state.pool, &body.email)
        .await?
        .ok_or_else(|| {
            crate::errors::AppError::NotFound(format!("No user with email '{}'", body.email))
        })?;

    let member = db_rbac::add_member(&state.pool, workspace_id, user_id, &body.role).await?;
    let view = MemberView::from(member);

    Ok((StatusCode::CREATED, Json(json!({ "data": view }))))
}

/// PATCH /admin/workspaces/:workspace_id/members/:user_id
#[utoipa::path(
    patch,
    path = "/admin/workspaces/{workspace_id}/members/{user_id}",
    tag = "Workspaces",
    params(
        ("workspace_id" = uuid::Uuid, Path, description = "Workspace UUID"),
        ("user_id" = uuid::Uuid, Path, description = "User UUID"),
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Member role updated", body = serde_json::Value),
        (status = 400, description = "Invalid role"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn update_member(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((workspace_id, user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateMemberRequest>,
) -> AppResult<Json<Value>> {
    require_role_in_workspace(Role::Admin, &auth.0, workspace_id, &state).await?;

    if !valid_role(&body.role) {
        return Err(crate::errors::AppError::BadRequest(format!(
            "Invalid role '{}'. Valid roles: admin, api_manager, billing_viewer, read_only",
            body.role
        )));
    }

    db_rbac::update_member_role(&state.pool, workspace_id, user_id, &body.role).await?;

    Ok(Json(json!({ "data": { "role": body.role } })))
}

/// DELETE /admin/workspaces/:workspace_id/members/:user_id
#[utoipa::path(
    delete,
    path = "/admin/workspaces/{workspace_id}/members/{user_id}",
    tag = "Workspaces",
    params(
        ("workspace_id" = uuid::Uuid, Path, description = "Workspace UUID"),
        ("user_id" = uuid::Uuid, Path, description = "User UUID"),
    ),
    responses(
        (status = 204, description = "Member removed"),
        (status = 403, description = "Forbidden"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((workspace_id, user_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_role_in_workspace(Role::Admin, &auth.0, workspace_id, &state).await?;

    db_rbac::remove_member(&state.pool, workspace_id, user_id).await?;

    Ok(StatusCode::NO_CONTENT)
}
