use crate::{db::prompts as db_prompts, errors::AppResult, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Query / request types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListPromptsQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

fn default_page() -> i64 {
    1
}
fn default_per_page() -> i64 {
    50
}

#[derive(Debug, Deserialize)]
pub struct CreatePromptRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateVersionRequest {
    pub content: String,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateVersionRequest {
    pub is_active: Option<bool>,
    pub ab_weight: Option<i32>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /admin/prompts — create a prompt.
pub async fn create_prompt(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreatePromptRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let id = Uuid::new_v4();
    let prompt =
        db_prompts::create_prompt(&state.pool, id, &body.name, body.description.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": prompt }))))
}

/// GET /admin/prompts — list all prompts (paginated).
pub async fn list_prompts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListPromptsQuery>,
) -> AppResult<Json<Value>> {
    let page = params.page.max(1);
    let per_page = params.per_page.clamp(1, 100);
    let (prompts, total) = db_prompts::list_prompts(&state.pool, page, per_page).await?;
    Ok(Json(json!({
        "data": prompts,
        "meta": { "page": page, "per_page": per_page, "total": total }
    })))
}

/// GET /admin/prompts/:id — get a prompt with all its versions.
pub async fn get_prompt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let prompt = db_prompts::get_prompt(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Prompt {id}")))?;
    let versions = db_prompts::get_versions(&state.pool, id).await?;
    Ok(Json(
        json!({ "data": { "prompt": prompt, "versions": versions } }),
    ))
}

/// POST /admin/prompts/:id/versions — create a new version for a prompt.
pub async fn create_version(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateVersionRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    // Verify prompt exists.
    db_prompts::get_prompt(&state.pool, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::NotFound(format!("Prompt {id}")))?;

    let version_id = Uuid::new_v4();
    let version = db_prompts::create_version(
        &state.pool,
        version_id,
        id,
        &body.content,
        body.system_prompt.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": version }))))
}

/// PATCH /admin/prompts/:id/versions/:version — update is_active / ab_weight.
pub async fn update_version(
    State(state): State<Arc<AppState>>,
    Path((id, version)): Path<(Uuid, i32)>,
    Json(body): Json<UpdateVersionRequest>,
) -> AppResult<Json<Value>> {
    let updated =
        db_prompts::update_version(&state.pool, id, version, body.is_active, body.ab_weight)
            .await?
            .ok_or_else(|| {
                crate::errors::AppError::NotFound(format!("Prompt {id} version {version}"))
            })?;
    Ok(Json(json!({ "data": updated })))
}

/// DELETE /admin/prompts/:id — delete a prompt and all its versions.
pub async fn delete_prompt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let deleted = db_prompts::delete_prompt(&state.pool, id).await?;
    if !deleted {
        return Err(crate::errors::AppError::NotFound(format!("Prompt {id}")));
    }
    Ok((StatusCode::OK, Json(json!({ "data": { "deleted": true } }))))
}
