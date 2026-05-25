// OIDC SSO handlers — V5-L2
//
// GET  /auth/oidc/:idp_id/start    → redirects browser to the IdP authorization URL
// GET  /auth/oidc/:idp_id/callback → receives code from IdP, validates, mints JWT

use crate::{
    auth::oidc as oidc_proto,
    crypto,
    db::{identities as db_idp, rbac as db_rbac, users as db_users},
    errors::{AppError, AppResult},
    middleware::jwt::create_token,
    models::user::{AuthResponse, UserResponse},
    state::{AppState, OidcState},
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

// ── Query types ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build the callback redirect URI from request Host header.
fn build_redirect_uri(headers: &HeaderMap, idp_id: Uuid) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{scheme}://{host}/auth/oidc/{idp_id}/callback")
}

/// Resolve the highest Velox role from IdP group claims and the IdP's group_role_map.
/// Returns "ReadOnly" when no group matches.
fn resolve_role(groups: &[String], group_role_map: &serde_json::Value) -> &'static str {
    let Some(map) = group_role_map.as_object() else {
        return "ReadOnly";
    };
    let priority = |role: &str| match role {
        "Admin" => 4,
        "ApiManager" => 3,
        "BillingViewer" => 2,
        _ => 1,
    };
    let mut best = "ReadOnly";
    let mut best_p = 1usize;
    for g in groups {
        if let Some(role) = map.get(g).and_then(|v| v.as_str()) {
            let p = priority(role);
            if p > best_p {
                best = match role {
                    "Admin" => "Admin",
                    "ApiManager" => "ApiManager",
                    "BillingViewer" => "BillingViewer",
                    _ => "ReadOnly",
                };
                best_p = p;
            }
        }
    }
    best
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /auth/oidc/:idp_id/start — redirect to the IdP authorization URL.
///
/// Generates a PKCE code verifier and a CSRF state token, stores them in the
/// in-memory `oidc_states` map, then issues a 302 redirect to the IdP.
pub async fn oidc_start(
    State(state): State<Arc<AppState>>,
    Path(idp_id): Path<Uuid>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let idp = db_idp::get_idp(&state.pool, idp_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Identity provider not found".to_string()))?;

    if !idp.enabled {
        return Err(AppError::NotFound(
            "Identity provider is disabled".to_string(),
        ));
    }

    let discovery_url = idp.config["discovery_url"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("IdP config missing discovery_url".to_string()))?;
    let client_id = idp.config["client_id"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("IdP config missing client_id".to_string()))?;

    let discovery = oidc_proto::fetch_discovery(discovery_url)
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery failed: {e}")))?;

    let (code_verifier, code_challenge) = oidc_proto::generate_pkce();
    let csrf_state = oidc_proto::random_token();
    let nonce = oidc_proto::random_token();

    let redirect_uri = build_redirect_uri(&headers, idp_id);

    let auth_url = oidc_proto::build_auth_url(
        &discovery,
        client_id,
        &redirect_uri,
        &code_challenge,
        &csrf_state,
        &nonce,
    );

    state.oidc_states.insert(
        csrf_state,
        OidcState {
            code_verifier,
            nonce,
            idp_id,
            created_at: std::time::Instant::now(),
        },
    );

    Ok(Redirect::to(&auth_url).into_response())
}

/// GET /auth/oidc/:idp_id/callback — receive code from IdP, mint Velox JWT.
///
/// On success, returns JSON `{ "token": "...", "user": { ... } }` — same shape
/// as `POST /api/v1/auth/login` so the dashboard can handle both identically.
pub async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    Path(idp_id): Path<Uuid>,
    Query(q): Query<CallbackQuery>,
    headers: HeaderMap,
) -> AppResult<Json<AuthResponse>> {
    // IdP sent an error (user denied, misconfiguration, etc.)
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        return Err(AppError::BadRequest(format!(
            "IdP error: {err} — {desc}"
        )));
    }

    let code = q
        .code
        .ok_or_else(|| AppError::BadRequest("Missing 'code' in callback".to_string()))?;
    let state_token = q
        .state
        .ok_or_else(|| AppError::BadRequest("Missing 'state' in callback".to_string()))?;

    // ── CSRF + PKCE state lookup ─────────────────────────────────────────────
    let oidc_state = state
        .oidc_states
        .remove(&state_token)
        .map(|(_, v)| v)
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired OIDC state".to_string()))?;

    // Reject states older than 10 minutes
    if oidc_state.created_at.elapsed().as_secs() > 600 {
        return Err(AppError::Unauthorized(
            "OIDC state has expired — please try logging in again".to_string(),
        ));
    }

    // Verify the idp_id from the path matches what was stored with the state
    if oidc_state.idp_id != idp_id {
        return Err(AppError::Unauthorized(
            "OIDC state IdP mismatch".to_string(),
        ));
    }

    // ── Load IdP + decrypt client_secret ────────────────────────────────────
    let idp = db_idp::get_idp(&state.pool, idp_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Identity provider not found".to_string()))?;

    if !idp.enabled {
        return Err(AppError::NotFound(
            "Identity provider is disabled".to_string(),
        ));
    }

    let discovery_url = idp.config["discovery_url"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("IdP config missing discovery_url".to_string()))?;
    let client_id = idp.config["client_id"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("IdP config missing client_id".to_string()))?;

    let client_secret_enc = idp.config["client_secret"].as_str().unwrap_or("");
    let client_secret = if client_secret_enc.is_empty() {
        String::new()
    } else if state.config.encryption_key.is_empty() {
        client_secret_enc.to_string() // plaintext fallback (dev only)
    } else {
        let aes_key = crypto::parse_key(&state.config.encryption_key)
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;
        crypto::decrypt(client_secret_enc, &aes_key)
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?
    };

    let redirect_uri = build_redirect_uri(&headers, idp_id);

    // ── Fetch discovery doc + exchange code ──────────────────────────────────
    let discovery = oidc_proto::fetch_discovery(discovery_url)
        .await
        .map_err(|e| AppError::BadRequest(format!("OIDC discovery failed: {e}")))?;

    let claims = oidc_proto::exchange_code(
        &discovery,
        client_id,
        &client_secret,
        &code,
        &oidc_state.code_verifier,
        &redirect_uri,
        &oidc_state.nonce,
    )
    .await
    .map_err(|e| AppError::Unauthorized(format!("OIDC token validation failed: {e}")))?;

    // ── JIT user creation / lookup ───────────────────────────────────────────
    let email = claims
        .email
        .as_deref()
        .unwrap_or(claims.sub.as_str());
    let display_name = claims
        .name
        .as_deref()
        .unwrap_or(email);

    let existing_identity = db_idp::find_identity(&state.pool, idp_id, &claims.sub).await?;

    let user = if let Some(identity) = existing_identity {
        db_idp::touch_last_login(&state.pool, identity.id).await?;
        db_users::find_by_id(&state.pool, identity.user_id)
            .await?
            .ok_or_else(|| AppError::Anyhow(anyhow::anyhow!("Orphaned identity row")))?
    } else {
        // First-time login — JIT create the user
        let new_user = if db_users::find_by_email(&state.pool, email).await?.is_some() {
            // Email already exists from a password-based account — link to it
            db_users::find_by_email(&state.pool, email)
                .await?
                .ok_or_else(|| AppError::Anyhow(anyhow::anyhow!("Race condition on user lookup")))?
        } else {
            db_users::create(&state.pool, email, "", display_name).await?
        };
        db_idp::create_identity(&state.pool, new_user.id, idp_id, &claims.sub).await?;

        // Add user to the IdP's workspace with the resolved role
        let role = resolve_role(&claims.groups, &idp.group_role_map);
        let _ = db_rbac::add_member(&state.pool, idp.workspace_id, new_user.id, role).await;

        new_user
    };

    // ── Mint Velox JWT ───────────────────────────────────────────────────────
    let token = create_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_hours,
    )
    .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}
