use crate::{handlers, middleware::jwt::AuthUser, state::AppState};
use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
    trace::TraceLayer,
};

pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Gateway routes with 1MB size limit
    let gateway_routes = Router::new()
        .route(
            "/v1/chat/completions",
            post(handlers::gateway::chat_completions),
        )
        .route("/v1/embeddings", post(handlers::gateway::embeddings))
        .route(
            "/v1/completions",
            post(handlers::gateway::legacy_completions),
        )
        .route("/v1/models", get(handlers::gateway::list_models))
        .layer(RequestBodyLimitLayer::new(1024 * 1024)); // 1MB

    // Admin routes — all require a valid JWT (dashboard user, not gateway key)
    let admin_routes = Router::new()
        // ── Admin — Keys ─────────────────────────────────────────────────────
        .route("/admin/keys", post(handlers::admin::keys::create_key))
        .route("/admin/keys", get(handlers::admin::keys::list_keys))
        .route("/admin/keys/:id", get(handlers::admin::keys::get_key))
        .route("/admin/keys/:id", patch(handlers::admin::keys::update_key))
        .route("/admin/keys/:id", delete(handlers::admin::keys::revoke_key))
        .route(
            "/admin/keys/:id/rotate",
            post(handlers::admin::keys::rotate_key),
        )
        // ── Admin — Requests ─────────────────────────────────────────────────
        .route(
            "/admin/requests",
            get(handlers::admin::requests::list_requests),
        )
        .route(
            "/admin/requests/export",
            get(handlers::admin::requests::export_requests),
        )
        .route(
            "/admin/requests/:id",
            get(handlers::admin::requests::get_request),
        )
        .route(
            "/admin/requests/:id/replay",
            post(handlers::admin::replay::replay_request),
        )
        // ── Admin — Playground (V4-6) ────────────────────────────────────────
        .route(
            "/admin/playground",
            post(handlers::admin::replay::playground),
        )
        // ── Admin — Analytics ────────────────────────────────────────────────
        .route(
            "/admin/analytics/overview",
            get(handlers::admin::analytics::overview),
        )
        .route(
            "/admin/analytics/costs",
            get(handlers::admin::analytics::costs),
        )
        .route(
            "/admin/analytics/latency",
            get(handlers::admin::analytics::latency),
        )
        .route(
            "/admin/analytics/cache",
            get(handlers::admin::analytics::cache),
        )
        .route(
            "/admin/analytics/simulate",
            get(handlers::admin::analytics::simulate),
        )
        // ── Admin — Providers ────────────────────────────────────────────────
        .route(
            "/admin/providers",
            get(handlers::admin::providers::list_providers),
        )
        .route(
            "/admin/providers/:id",
            patch(handlers::admin::providers::update_provider),
        )
        .route(
            "/admin/providers/:id/test",
            post(handlers::admin::providers::test_provider),
        )
        // ── Admin — Alerts ───────────────────────────────────────────────────
        .route("/admin/alerts", post(handlers::admin::alerts::create_alert))
        .route("/admin/alerts", get(handlers::admin::alerts::list_alerts))
        .route("/admin/alerts/:id", get(handlers::admin::alerts::get_alert))
        .route(
            "/admin/alerts/:id",
            patch(handlers::admin::alerts::update_alert),
        )
        .route(
            "/admin/alerts/:id",
            delete(handlers::admin::alerts::delete_alert),
        )
        .route(
            "/admin/alerts/:id/test",
            post(handlers::admin::alerts::test_alert),
        )
        // ── Admin — Cache ────────────────────────────────────────────────────
        .route("/admin/cache/stats", get(handlers::admin::cache::get_stats))
        .route("/admin/cache", delete(handlers::admin::cache::flush_cache))
        .route(
            "/admin/cache/entries/:id",
            delete(handlers::admin::cache::delete_entry),
        )
        // ── Admin — Prompts ──────────────────────────────────────────────────
        .route(
            "/admin/prompts",
            post(handlers::admin::prompts::create_prompt)
                .get(handlers::admin::prompts::list_prompts),
        )
        .route(
            "/admin/prompts/:id",
            get(handlers::admin::prompts::get_prompt)
                .delete(handlers::admin::prompts::delete_prompt),
        )
        .route(
            "/admin/prompts/:id/versions",
            post(handlers::admin::prompts::create_version),
        )
        .route(
            "/admin/prompts/:id/versions/:version",
            patch(handlers::admin::prompts::update_version),
        )
        // ── Admin — Config ───────────────────────────────────────────────────
        .route(
            "/admin/config",
            get(handlers::admin::velox_config::get_config)
                .patch(handlers::admin::velox_config::patch_config),
        )
        // ── Admin — System Readiness (V4-0) ──────────────────────────────────
        .route(
            "/admin/system/readiness",
            get(handlers::admin::system::readiness),
        )
        // ── Admin — Live Stream (WebSocket) ──────────────────────────────────
        .route(
            "/admin/stream",
            get(handlers::admin::stream::stream_handler),
        )
        // ── Admin — Workspaces + Members (V4-8) ──────────────────────────────
        .route(
            "/admin/workspaces",
            get(handlers::admin::members::list_workspaces),
        )
        .route(
            "/admin/workspaces/:workspace_id/members",
            get(handlers::admin::members::list_members).post(handlers::admin::members::add_member),
        )
        .route(
            "/admin/workspaces/:workspace_id/members/:user_id",
            patch(handlers::admin::members::update_member)
                .delete(handlers::admin::members::remove_member),
        )
        .route_layer(axum::middleware::from_extractor_with_state::<
            AuthUser,
            Arc<AppState>,
        >(state.clone()));

    // MCP routes — JWT authentication handled inside the handlers (token in header
    // or params.token for initialize), not via the admin middleware layer.
    let mcp_routes = Router::new()
        .route("/mcp/rpc", post(handlers::mcp::rpc_handler))
        .route("/mcp/sse", get(handlers::mcp::sse_handler));

    Router::new()
        .merge(gateway_routes)
        .merge(admin_routes)
        .merge(mcp_routes)
        // ── Existing routes ──────────────────────────────────────────────────
        .route("/health", get(handlers::health::health_check))
        .route("/metrics", get(handlers::metrics::prometheus_handler))
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/me", get(handlers::auth::me))
        .route("/api/v1/users", get(handlers::users::list_users))
        .route("/api/v1/users/:id", get(handlers::users::get_user))
        .route("/api/v1/users/:id", put(handlers::users::update_user))
        .route("/api/v1/users/:id", delete(handlers::users::delete_user))
        .fallback(crate::dashboard::serve)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
