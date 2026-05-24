//! V5-1: OpenAPI spec + Swagger UI integration.
//!
//! Mounted via [`router`] into the main router. Adds two routes:
//! - `GET /admin/openapi.json` — machine-readable OpenAPI 3.1 spec
//! - `GET /admin/docs` (+ asset paths under `/admin/docs/*`) — Swagger UI
//!
//! Both endpoints are unauthenticated by design — the spec describes which
//! endpoints require auth, and consumers need to read it to know what to call.

use std::sync::Arc;

use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{openapi::VeloxApiDoc, state::AppState};

/// Build the sub-router for OpenAPI + Swagger UI.
///
/// Mount with `.merge(docs::router())` in the main router. No state is needed —
/// the spec is generated at compile time from `VeloxApiDoc::openapi()`.
pub fn router() -> Router<Arc<AppState>> {
    SwaggerUi::new("/admin/docs")
        .url("/admin/openapi.json", VeloxApiDoc::openapi())
        .into()
}
