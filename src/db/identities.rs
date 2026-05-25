// DB queries for identity_providers and identities tables (V5-L2).
//
// Uses dynamic query_as (not compile-time query!) because the tables are
// created by migration 0028 which may not have run on all development machines.

use crate::errors::AppResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::DbPool;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct IdentityProvider {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: String,
    pub name: String,
    #[sqlx(json)]
    pub config: Value,
    #[sqlx(json)]
    pub group_role_map: Value,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Identity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub idp_id: Uuid,
    pub external_id: String,
    pub last_login: Option<DateTime<Utc>>,
}

// ── identity_providers CRUD ───────────────────────────────────────────────────

pub async fn list_idps(pool: &DbPool, workspace_id: Uuid) -> AppResult<Vec<IdentityProvider>> {
    let rows = sqlx::query_as::<_, IdentityProvider>(
        "SELECT id, workspace_id, kind, name, config, group_role_map, enabled, created_at
         FROM identity_providers
         WHERE workspace_id = $1
         ORDER BY created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_idp(pool: &DbPool, id: Uuid) -> AppResult<Option<IdentityProvider>> {
    let row = sqlx::query_as::<_, IdentityProvider>(
        "SELECT id, workspace_id, kind, name, config, group_role_map, enabled, created_at
         FROM identity_providers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_idp(
    pool: &DbPool,
    workspace_id: Uuid,
    name: &str,
    config: Value,
    group_role_map: Value,
) -> AppResult<IdentityProvider> {
    let id = Uuid::new_v4();
    let row = sqlx::query_as::<_, IdentityProvider>(
        "INSERT INTO identity_providers
             (id, workspace_id, kind, name, config, group_role_map)
         VALUES ($1, $2, 'oidc', $3, $4, $5)
         RETURNING id, workspace_id, kind, name, config, group_role_map, enabled, created_at",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(config)
    .bind(group_role_map)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_idp(pool: &DbPool, id: Uuid) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM identity_providers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── identities CRUD ───────────────────────────────────────────────────────────

pub async fn find_identity(
    pool: &DbPool,
    idp_id: Uuid,
    external_id: &str,
) -> AppResult<Option<Identity>> {
    let row = sqlx::query_as::<_, Identity>(
        "SELECT id, user_id, idp_id, external_id, last_login
         FROM identities WHERE idp_id = $1 AND external_id = $2",
    )
    .bind(idp_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_identity(
    pool: &DbPool,
    user_id: Uuid,
    idp_id: Uuid,
    external_id: &str,
) -> AppResult<Identity> {
    let id = Uuid::new_v4();
    let row = sqlx::query_as::<_, Identity>(
        "INSERT INTO identities (id, user_id, idp_id, external_id, last_login)
         VALUES ($1, $2, $3, $4, NOW())
         RETURNING id, user_id, idp_id, external_id, last_login",
    )
    .bind(id)
    .bind(user_id)
    .bind(idp_id)
    .bind(external_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn touch_last_login(pool: &DbPool, identity_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE identities SET last_login = NOW() WHERE id = $1")
        .bind(identity_id)
        .execute(pool)
        .await?;
    Ok(())
}
