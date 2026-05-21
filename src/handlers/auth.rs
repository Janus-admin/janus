use crate::{
    db,
    errors::{AppError, AppResult},
    middleware::jwt::{create_token, AuthUser},
    models::user::{AuthResponse, LoginRequest, RegisterRequest, UserResponse},
    state::AppState,
};
use axum::{extract::State, Json};
use bcrypt::{hash, verify, DEFAULT_COST};
use std::sync::Arc;

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<AuthResponse>> {
    if db::users::find_by_email(&state.pool, &req.email)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict("Email already in use".to_string()));
    }

    let password_hash = hash(&req.password, DEFAULT_COST)?;
    let user = db::users::create(&state.pool, &req.email, &password_hash, &req.name).await?;

    let token = create_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_hours,
    )?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    let user = db::users::find_by_email(&state.pool, &req.email)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    if !verify(&req.password, &user.password_hash)? {
        return Err(AppError::Unauthorized("Invalid credentials".to_string()));
    }

    let token = create_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_hours,
    )?;

    Ok(Json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<UserResponse>> {
    let user = db::users::find_by_id(&state.pool, auth.0.sub)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    Ok(Json(UserResponse::from(user)))
}
