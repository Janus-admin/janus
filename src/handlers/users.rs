use crate::{
    db,
    errors::{AppError, AppResult},
    middleware::jwt::AuthUser,
    models::user::{UpdateUserRequest, UserResponse},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Pagination {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Query(params): Query<Pagination>,
) -> AppResult<Json<Vec<UserResponse>>> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);
    let users = db::users::list(&state.pool, page, per_page).await?;
    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<UserResponse>> {
    let user = db::users::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))?;
    Ok(Json(UserResponse::from(user)))
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> AppResult<Json<UserResponse>> {
    if auth.0.sub != id {
        return Err(AppError::Unauthorized(
            "Cannot update another user's profile".to_string(),
        ));
    }
    let user = db::users::update(&state.pool, id, &req).await?;
    Ok(Json(UserResponse::from(user)))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if auth.0.sub != id {
        return Err(AppError::Unauthorized(
            "Cannot delete another user's profile".to_string(),
        ));
    }
    db::users::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
