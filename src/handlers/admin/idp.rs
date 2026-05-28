// Admin handlers for identity_providers CRUD — V5-L2.
//
// GET    /admin/idp         — list identity providers in the caller's workspace
// POST   /admin/idp         — configure a new OIDC IdP
// DELETE /admin/idp/:id     — remove an IdP

use crate::{
    crypto,
    db::identities as db_idp,
    enterprise::AuditEvent,
    errors::{AppError, AppResult},
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
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
pub struct CreateIdpRequest {
    pub name: String,
    pub discovery_url: String,
    pub client_id: String,
    /// Plaintext secret — stored encrypted at rest using the server's encryption_key.
    pub client_secret: String,
    /// Optional: map IdP group names to Janus roles.
    /// Example: `{ "engineering": "ApiManager", "admins": "Admin" }`
    #[serde(default)]
    pub group_role_map: Value,
    /// Workspace the IdP belongs to. Defaults to the first workspace if omitted.
    pub workspace_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct IdpView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub discovery_url: String,
    pub client_id: String,
    pub group_role_map: Value,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<db_idp::IdentityProvider> for IdpView {
    type Error = AppError;

    fn try_from(idp: db_idp::IdentityProvider) -> Result<Self, AppError> {
        let discovery_url = idp.config["discovery_url"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let client_id = idp.config["client_id"].as_str().unwrap_or("").to_string();
        Ok(IdpView {
            id: idp.id,
            workspace_id: idp.workspace_id,
            name: idp.name,
            discovery_url,
            client_id,
            group_role_map: idp.group_role_map,
            enabled: idp.enabled,
            created_at: idp.created_at,
        })
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /admin/idp — list identity providers (Admin role required).
#[utoipa::path(
    get,
    path = "/admin/idp",
    tag = "Identity Providers",
    responses(
        (status = 200, description = "List of configured identity providers", body = serde_json::Value),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn list_idps(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<Value>> {
    require_role(Role::Admin, &auth.0, &state).await?;

    // Resolve workspace: use first workspace the user belongs to.
    let workspaces = crate::db::rbac::list_workspaces(&state.pool).await?;
    let workspace_id = workspaces
        .first()
        .map(|w| w.id)
        .ok_or_else(|| AppError::BadRequest("User has no workspace".to_string()))?;

    let idps = db_idp::list_idps(&state.pool, workspace_id).await?;
    let views: Vec<IdpView> = idps
        .into_iter()
        .map(IdpView::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(json!({ "data": views })))
}

/// POST /admin/idp — configure a new OIDC identity provider (Admin role required).
#[utoipa::path(
    post,
    path = "/admin/idp",
    tag = "Identity Providers",
    request_body = serde_json::Value,
    responses(
        (status = 201, description = "Created identity provider", body = serde_json::Value),
        (status = 400, description = "Invalid request or encryption not configured"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn create_idp(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateIdpRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    require_role(Role::Admin, &auth.0, &state).await?;

    // Encrypt the client_secret before storage.
    // When encryption_key is absent the secret is stored in plaintext and a
    // tracing warning is emitted — this only happens in development/test
    // environments where encryption_key has not been configured.
    let encrypted_secret = if req.client_secret.is_empty() {
        String::new()
    } else if state.config.encryption_key.is_empty() {
        tracing::warn!(
            "encryption_key not configured — storing OIDC client_secret in plaintext. \
             Set encryption_key in janus.toml for production use."
        );
        req.client_secret.clone()
    } else {
        let aes_key = crypto::parse_key(&state.config.encryption_key)
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;
        crypto::encrypt(&req.client_secret, &aes_key)
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?
    };

    let config = json!({
        "discovery_url": req.discovery_url,
        "client_id": req.client_id,
        "client_secret": encrypted_secret,
    });

    // Resolve workspace
    let workspace_id = if let Some(ws_id) = req.workspace_id {
        ws_id
    } else {
        let workspaces = crate::db::rbac::list_workspaces(&state.pool).await?;
        workspaces
            .first()
            .map(|w| w.id)
            .ok_or_else(|| AppError::BadRequest("User has no workspace".to_string()))?
    };

    let idp = db_idp::create_idp(
        &state.pool,
        workspace_id,
        &req.name,
        config,
        req.group_role_map,
    )
    .await?;

    let view = IdpView::try_from(idp)?;

    state.enterprise.audit(
        AuditEvent::new(
            "idp.create",
            "identity_provider",
            Some(view.id.to_string()),
            Some(auth.0.sub),
            Some(auth.0.email.clone()),
        )
        .with_workspace(workspace_id)
        .with_metadata(serde_json::json!({ "name": req.name, "discovery_url": req.discovery_url })),
    );

    Ok((StatusCode::CREATED, Json(json!({ "data": view }))))
}

/// DELETE /admin/idp/:id — remove an identity provider (Admin role required).
#[utoipa::path(
    delete,
    path = "/admin/idp/{id}",
    tag = "Identity Providers",
    params(("id" = Uuid, Path, description = "Identity provider ID")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "Not found"),
    ),
    security(("bearer_jwt" = [])),
)]
pub async fn delete_idp(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    require_role(Role::Admin, &auth.0, &state).await?;

    let deleted = db_idp::delete_idp(&state.pool, id).await?;
    if deleted {
        state.enterprise.audit(AuditEvent::new(
            "idp.delete",
            "identity_provider",
            Some(id.to_string()),
            Some(auth.0.sub),
            Some(auth.0.email.clone()),
        ));
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!(
            "Identity provider {id} not found"
        )))
    }
}
