//! V5-1: OpenAPI 3.1 specification root.
//!
//! Every annotated handler shows up in the spec served at:
//! - `GET /admin/openapi.json` — machine-readable JSON
//! - `GET /admin/docs`         — Swagger UI
//!
//! Adding a new admin endpoint:
//! 1. Annotate the handler with `#[utoipa::path(...)]`
//! 2. Add it to the `paths(...)` list in `VeloxApiDoc` below
//! 3. If it uses a custom request/response struct, add `ToSchema` and list it under `components(schemas(...))`

use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

/// Adds two security schemes to every operation:
/// - `bearer_jwt`  → admin dashboard endpoints (JWT)
/// - `api_key`     → gateway endpoints (vx-sk-... in `Authorization: Bearer`)
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_jwt",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some(
                        "Admin dashboard JWT issued by `POST /api/v1/auth/login`.",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                "Authorization",
                "Velox gateway API key prefixed with `Bearer vx-sk-...`",
            ))),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Velox Admin & Gateway API",
        version = "0.1.0",
        description = "Velox is a self-hosted AI gateway: OpenAI-compatible proxy for chat/embeddings/images/audio, with caching, cost tracking, alerts, and per-workspace RBAC.\n\nTwo authentication systems coexist:\n- **Admin API** (`/admin/*`, `/api/v1/auth/*`): JWT bearer token from dashboard login.\n- **Gateway API** (`/v1/*`): `vx-sk-...` API key as `Authorization: Bearer <key>`.\n\nGateway endpoints are OpenAI request/response compatible — clients change only the `base_url`.",
        contact(name = "Velox", url = "https://github.com/AlizadehAFPN/Velox"),
        license(name = "MIT", url = "https://opensource.org/licenses/MIT"),
    ),
    servers(
        (url = "http://localhost:8080", description = "Local dev server"),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "Keys", description = "API key lifecycle: create, list, rotate, revoke."),
        (name = "Requests", description = "Audit log of every proxied request, with replay and export."),
        (name = "Analytics", description = "Cost, latency, cache, and routing analytics."),
        (name = "Models", description = "Pricing catalogue of models known to Velox."),
        (name = "Providers", description = "Upstream LLM providers (OpenAI, Anthropic, Bedrock, etc.)."),
        (name = "Alerts", description = "Spend/error/latency threshold alerts with webhook delivery."),
        (name = "Cache", description = "Exact + semantic cache statistics and management."),
        (name = "Prompts", description = "Prompt template registry with versioning."),
        (name = "Config", description = "Runtime-mutable subset of velox.toml."),
        (name = "System", description = "Readiness, liveness, and metrics."),
        (name = "Workspaces", description = "Tenants and member RBAC."),
        (name = "Gateway", description = "OpenAI-compatible proxy: chat, embeddings, images, audio, models."),
    ),
    paths(
        // ── Keys ─────────────────────────────────────────────────────────────
        crate::handlers::admin::keys::create_key,
        crate::handlers::admin::keys::list_keys,
        crate::handlers::admin::keys::get_key,
        crate::handlers::admin::keys::update_key,
        crate::handlers::admin::keys::revoke_key,
        crate::handlers::admin::keys::rotate_key,
        // ── Requests ─────────────────────────────────────────────────────────
        crate::handlers::admin::requests::list_requests,
        crate::handlers::admin::requests::export_requests,
        crate::handlers::admin::requests::get_request,
        crate::handlers::admin::replay::replay_request,
        crate::handlers::admin::replay::playground,
        // ── Analytics ────────────────────────────────────────────────────────
        crate::handlers::admin::analytics::overview,
        crate::handlers::admin::analytics::costs,
        crate::handlers::admin::analytics::latency,
        crate::handlers::admin::analytics::cache,
        crate::handlers::admin::analytics::simulate,
        // ── Models ───────────────────────────────────────────────────────────
        crate::handlers::admin::models::list_models,
        // ── Providers ────────────────────────────────────────────────────────
        crate::handlers::admin::providers::list_providers,
        crate::handlers::admin::providers::update_provider,
        crate::handlers::admin::providers::test_provider,
        // ── Alerts ───────────────────────────────────────────────────────────
        crate::handlers::admin::alerts::create_alert,
        crate::handlers::admin::alerts::list_alerts,
        crate::handlers::admin::alerts::get_alert,
        crate::handlers::admin::alerts::update_alert,
        crate::handlers::admin::alerts::delete_alert,
        crate::handlers::admin::alerts::test_alert,
        // ── Cache ────────────────────────────────────────────────────────────
        crate::handlers::admin::cache::get_stats,
        crate::handlers::admin::cache::flush_cache,
        crate::handlers::admin::cache::delete_entry,
        // ── Prompts ──────────────────────────────────────────────────────────
        crate::handlers::admin::prompts::create_prompt,
        crate::handlers::admin::prompts::list_prompts,
        crate::handlers::admin::prompts::get_prompt,
        crate::handlers::admin::prompts::delete_prompt,
        crate::handlers::admin::prompts::create_version,
        crate::handlers::admin::prompts::update_version,
        // ── Config ───────────────────────────────────────────────────────────
        crate::handlers::admin::velox_config::get_config,
        crate::handlers::admin::velox_config::patch_config,
        // ── System ───────────────────────────────────────────────────────────
        crate::handlers::admin::system::readiness,
        // ── Workspaces / Members ─────────────────────────────────────────────
        crate::handlers::admin::members::list_workspaces,
        crate::handlers::admin::members::list_members,
        crate::handlers::admin::members::add_member,
        crate::handlers::admin::members::update_member,
        crate::handlers::admin::members::remove_member,
        // ── Gateway (/v1/*) ──────────────────────────────────────────────────
        crate::handlers::gateway::chat_completions,
        crate::handlers::gateway::embeddings,
        crate::handlers::gateway::legacy_completions,
        crate::handlers::gateway::list_models,
        crate::handlers::gateway::images_generations,
        crate::handlers::gateway::audio_transcriptions,
        crate::handlers::gateway::audio_speech,
    ),
    components(schemas(
        crate::models::api_key::ApiKeyView,
        crate::models::api_key::CreateApiKeyRequest,
        crate::models::api_key::CreateApiKeyResponse,
    )),
)]
pub struct VeloxApiDoc;
