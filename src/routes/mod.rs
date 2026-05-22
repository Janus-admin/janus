use crate::{handlers, state::AppState};
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // ── Gateway API (OpenAI-compatible) ──────────────────────────────────
        .route(
            "/v1/chat/completions",
            post(handlers::gateway::chat_completions),
        )
        // ── Admin API ────────────────────────────────────────────────────────
        .route("/admin/keys", post(handlers::admin::keys::create_key))
        .route("/admin/keys", get(handlers::admin::keys::list_keys))
        .route("/admin/cache/stats", get(handlers::admin::cache::get_stats))
        .route("/admin/cache", delete(handlers::admin::cache::flush_cache))
        // ── Existing routes ──────────────────────────────────────────────────
        .route("/health", get(handlers::health::health_check))
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/me", get(handlers::auth::me))
        .route("/api/v1/users", get(handlers::users::list_users))
        .route("/api/v1/users/:id", get(handlers::users::get_user))
        .route("/api/v1/users/:id", put(handlers::users::update_user))
        .route("/api/v1/users/:id", delete(handlers::users::delete_user))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
