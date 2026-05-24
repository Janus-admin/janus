// src/middleware/rbac.rs — Role-based access control (V4-8)
//
// Role hierarchy (higher value = more privilege):
//   admin(4) > api_manager(3) > billing_viewer(2) > read_only(1)
//
// Enforcement: call `require_role(min_role, &auth.0, &state).await?` at the
// start of any handler that needs a minimum privilege level.
//
// Bootstrap rule: a user with NO workspace memberships is treated as admin.
// This covers fresh deployments and tests where users are created after startup.

use crate::{errors::AppError, middleware::jwt::Claims, state::AppState};
use std::str::FromStr;
use std::sync::Arc;

/// Role privilege levels — comparable with Ord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    ReadOnly = 1,
    BillingViewer = 2,
    ApiManager = 3,
    Admin = 4,
}

impl FromStr for Role {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "admin" => Self::Admin,
            "api_manager" => Self::ApiManager,
            "billing_viewer" => Self::BillingViewer,
            _ => Self::ReadOnly,
        })
    }
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::ApiManager => "api_manager",
            Self::BillingViewer => "billing_viewer",
            Self::ReadOnly => "read_only",
        }
    }
}

/// Enforce that the authenticated user holds at least `min_role`.
///
/// Looks up the user's highest role across all workspace memberships.
/// Users with no memberships are treated as admin (bootstrap/compat mode).
pub async fn require_role(
    min_role: Role,
    claims: &Claims,
    state: &Arc<AppState>,
) -> Result<Role, AppError> {
    let effective = crate::db::rbac::get_user_highest_role(&state.pool, claims.sub)
        .await?
        .unwrap_or(Role::Admin); // bootstrap: no memberships → admin

    if effective >= min_role {
        Ok(effective)
    } else {
        Err(AppError::Forbidden(format!(
            "Requires role '{}' or higher — you have '{}'",
            min_role.as_str(),
            effective.as_str()
        )))
    }
}

/// Enforce that the authenticated user holds at least `min_role` in a specific workspace.
///
/// Used by member management endpoints that are workspace-scoped.
///
/// Bootstrap rule: if the user has NO workspace memberships at all (globally), they
/// are treated as admin. If they have memberships elsewhere but not in this workspace,
/// they are denied.
pub async fn require_role_in_workspace(
    min_role: Role,
    claims: &Claims,
    workspace_id: uuid::Uuid,
    state: &Arc<AppState>,
) -> Result<Role, AppError> {
    match crate::db::rbac::get_role_in_workspace(&state.pool, claims.sub, workspace_id).await? {
        Some(role) => {
            if role >= min_role {
                Ok(role)
            } else {
                Err(AppError::Forbidden(format!(
                    "Requires role '{}' or higher in this workspace — you have '{}'",
                    min_role.as_str(),
                    role.as_str()
                )))
            }
        }
        None => {
            // Not a member of this specific workspace.
            // Apply bootstrap rule only if user has NO memberships anywhere.
            let global = crate::db::rbac::get_user_highest_role(&state.pool, claims.sub).await?;
            match global {
                None => Ok(Role::Admin), // no memberships at all → bootstrap admin
                Some(_) => Err(AppError::Forbidden(
                    "You are not a member of this workspace".to_string(),
                )),
            }
        }
    }
}
