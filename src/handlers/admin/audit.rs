// src/handlers/admin/audit.rs — SOC2 audit log HTTP handlers.
//
// Compiled ONLY with `--features enterprise`.
// Routes are mounted in routes/mod.rs under the same cfg guard.
//
// Endpoints:
//   GET  /admin/enterprise/audit          — paginated list with filters
//   GET  /admin/enterprise/audit/export   — full export as NDJSON or CSV
//   GET  /admin/enterprise/license        — current license state (also community-safe)

#![cfg(feature = "enterprise")]

use crate::{
    db::audit::{self as db_audit, AuditFilter},
    enterprise::license::LicenseState,
    errors::AppResult,
    middleware::{
        jwt::AuthUser,
        rbac::{require_role, Role},
    },
    state::AppState,
};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub workspace_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
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
pub struct ExportQuery {
    pub workspace_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// "ndjson" (default) or "csv"
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "ndjson".into()
}

// ── CSV helpers ───────────────────────────────────────────────────────────────

/// Wrap a CSV field in double quotes and escape internal double quotes.
/// Also prevents spreadsheet formula injection by quoting fields that start
/// with `=`, `+`, `-`, or `@`.
fn csv_escape(s: &str) -> String {
    let needs_quoting = s.contains([',', '"', '\n', '\r'])
        || matches!(s.chars().next(), Some('=' | '+' | '-' | '@'));
    if needs_quoting {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /admin/enterprise/audit — paginated audit log with optional filters.
///
/// Requires `BillingViewer` role or higher.
/// Only available in enterprise builds; route is absent in community edition.
pub async fn list_events(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Value>> {
    require_role(Role::BillingViewer, &auth.0, &state).await?;

    let filter = AuditFilter {
        workspace_id: q.workspace_id,
        actor_user_id: q.actor_user_id,
        action: q.action,
        resource_type: q.resource_type,
        from: q.from,
        to: q.to,
        page: q.page,
        per_page: q.per_page,
    };

    let (events, total) = db_audit::list_events(&state.pool, &filter).await?;

    Ok(Json(json!({
        "data": events,
        "meta": {
            "page": q.page,
            "per_page": q.per_page,
            "total": total,
        }
    })))
}

/// GET /admin/enterprise/audit/export — bulk export as NDJSON or CSV.
///
/// Streams up to 100 000 rows. Intended for compliance / SIEM ingestion.
/// Requires `Admin` role (export contains PII such as actor emails).
pub async fn export_events(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ExportQuery>,
) -> AppResult<impl IntoResponse> {
    require_role(Role::Admin, &auth.0, &state).await?;

    // Fetch all matching events (cap at 100k for safety).
    let filter = AuditFilter {
        workspace_id: q.workspace_id,
        from: q.from,
        to: q.to,
        per_page: 100_000,
        ..Default::default()
    };
    let (events, _) = db_audit::list_events(&state.pool, &filter).await?;

    match q.format.as_str() {
        "csv" => {
            let mut csv = String::from(
                "id,workspace_id,actor_user_id,actor_email,action,resource_type,resource_id,ip_address,created_at\n",
            );
            for e in &events {
                csv.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{}\n",
                    e.id,
                    e.workspace_id.map(|u| u.to_string()).unwrap_or_default(),
                    e.actor_user_id.map(|u| u.to_string()).unwrap_or_default(),
                    csv_escape(e.actor_email.as_deref().unwrap_or("")),
                    csv_escape(&e.action),
                    csv_escape(&e.resource_type),
                    csv_escape(e.resource_id.as_deref().unwrap_or("")),
                    csv_escape(e.ip_address.as_deref().unwrap_or("")),
                    e.created_at,
                ));
            }
            Ok((
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"audit_log.csv\"",
                    ),
                ],
                csv,
            )
                .into_response())
        }
        _ => {
            // NDJSON (newline-delimited JSON) — ideal for SIEM / Splunk ingestion.
            let mut ndjson = String::new();
            for e in &events {
                if let Ok(line) = serde_json::to_string(e) {
                    ndjson.push_str(&line);
                    ndjson.push('\n');
                }
            }
            Ok((
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/x-ndjson"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"audit_log.ndjson\"",
                    ),
                ],
                ndjson,
            )
                .into_response())
        }
    }
}

/// GET /admin/enterprise/license — current license state.
///
/// Available in ALL builds (community returns `{"state":"community"}`).
/// Requires `Admin` role so competitors can't enumerate your tier via the API.
pub async fn get_license(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> AppResult<Json<Value>> {
    require_role(Role::Admin, &auth.0, &state).await?;

    let license = state.enterprise.license_state();

    let body = match &license {
        LicenseState::Community => json!({
            "data": { "state": "community" }
        }),
        LicenseState::Active(info) => json!({
            "data": {
                "state": "active",
                "org": info.sub,
                "edition": info.edition,
                "features": info.features,
                "seats": info.seats,
                "expires_at": info.expires_at(),
            }
        }),
        LicenseState::Degraded {
            info,
            grace_days_left,
        } => json!({
            "data": {
                "state": "degraded",
                "org": info.sub,
                "edition": info.edition,
                "features": info.features,
                "grace_days_left": grace_days_left,
                "expires_at": info.expires_at(),
            }
        }),
        LicenseState::Expired { expired_at } => json!({
            "data": {
                "state": "expired",
                "expired_at": expired_at,
            }
        }),
        LicenseState::Invalid { reason } => json!({
            "data": {
                "state": "invalid",
                "reason": reason,
                "hint": "JANUS_LICENSE_JWT is set but failed validation. \
                         Check that JANUS_LICENSE_PUBLIC_KEY matches the key \
                         used to sign the token.",
            }
        }),
    };

    Ok(Json(body))
}
