// src/db/rbac.rs — RBAC database queries (V4-8)

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::rbac::Role;
use uuid::Uuid;

/// Return the highest-privilege role the user holds across all workspaces.
///
/// Returns `None` if the user has no workspace memberships at all (bootstrap
/// mode — callers treat this as admin for backward compatibility).
pub async fn get_user_highest_role(pool: &DbPool, user_id: Uuid) -> Result<Option<Role>, AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let row = sqlx::query!(
            r#"
            SELECT r.name as "role_name"
            FROM workspace_members wm
            JOIN roles r ON wm.role_id = r.id
            WHERE wm.user_id = $1
            ORDER BY
                CASE r.name
                    WHEN 'admin'          THEN 4
                    WHEN 'api_manager'    THEN 3
                    WHEN 'billing_viewer' THEN 2
                    WHEN 'read_only'      THEN 1
                    ELSE 0
                END DESC
            LIMIT 1
            "#,
            user_id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(row.map(|r| r.role_name.parse().unwrap_or(Role::ReadOnly)))
    }

    #[cfg(feature = "sqlite")]
    {
        let id_str = user_id.to_string();
        let row = sqlx::query!(
            r#"
            SELECT r.name as role_name
            FROM workspace_members wm
            JOIN roles r ON wm.role_id = r.id
            WHERE wm.user_id = ?
            ORDER BY
                CASE r.name
                    WHEN 'admin'          THEN 4
                    WHEN 'api_manager'    THEN 3
                    WHEN 'billing_viewer' THEN 2
                    WHEN 'read_only'      THEN 1
                    ELSE 0
                END DESC
            LIMIT 1
            "#,
            id_str
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(row.map(|r| r.role_name.parse().unwrap_or(Role::ReadOnly)))
    }
}

/// Return the user's role in a specific workspace, or None if not a member.
pub async fn get_role_in_workspace(
    pool: &DbPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<Role>, AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let row = sqlx::query!(
            r#"
            SELECT r.name as "role_name"
            FROM workspace_members wm
            JOIN roles r ON wm.role_id = r.id
            WHERE wm.user_id = $1 AND wm.workspace_id = $2
            "#,
            user_id,
            workspace_id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(row.map(|r| r.role_name.parse().unwrap_or(Role::ReadOnly)))
    }

    #[cfg(feature = "sqlite")]
    {
        let user_str = user_id.to_string();
        let ws_str = workspace_id.to_string();
        let row = sqlx::query!(
            r#"
            SELECT r.name as role_name
            FROM workspace_members wm
            JOIN roles r ON wm.role_id = r.id
            WHERE wm.user_id = ? AND wm.workspace_id = ?
            "#,
            user_str,
            ws_str
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(row.map(|r| r.role_name.parse().unwrap_or(Role::ReadOnly)))
    }
}

// ── Member CRUD ───────────────────────────────────────────────────────────────

pub struct MemberRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_members(pool: &DbPool, workspace_id: Uuid) -> Result<Vec<MemberRow>, AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let rows = sqlx::query!(
            r#"
            SELECT wm.id, wm.workspace_id, wm.user_id, u.email, u.name,
                   r.name as role_name, wm.created_at
            FROM workspace_members wm
            JOIN users  u ON wm.user_id  = u.id
            JOIN roles  r ON wm.role_id  = r.id
            WHERE wm.workspace_id = $1
            ORDER BY wm.created_at
            "#,
            workspace_id
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(rows
            .into_iter()
            .map(|r| MemberRow {
                id: r.id,
                workspace_id: r.workspace_id,
                user_id: r.user_id,
                email: r.email,
                name: r.name,
                role: r.role_name.parse().unwrap_or(Role::ReadOnly),
                created_at: r.created_at,
            })
            .collect())
    }

    #[cfg(feature = "sqlite")]
    {
        let ws_str = workspace_id.to_string();
        let rows = sqlx::query!(
            r#"
            SELECT wm.id, wm.workspace_id, wm.user_id, u.email, u.name,
                   r.name as role_name, wm.created_at
            FROM workspace_members wm
            JOIN users  u ON wm.user_id  = u.id
            JOIN roles  r ON wm.role_id  = r.id
            WHERE wm.workspace_id = ?
            ORDER BY wm.created_at
            "#,
            ws_str
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(rows
            .into_iter()
            .map(|r| MemberRow {
                id: Uuid::parse_str(&r.id).unwrap_or_else(|_| Uuid::new_v4()),
                workspace_id: Uuid::parse_str(&r.workspace_id).unwrap_or(workspace_id),
                user_id: Uuid::parse_str(&r.user_id).unwrap_or_else(|_| Uuid::new_v4()),
                email: r.email,
                name: r.name,
                role: r.role_name.parse().unwrap_or(Role::ReadOnly),
                created_at: chrono::DateTime::parse_from_rfc3339(&r.created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }
}

pub async fn add_member(
    pool: &DbPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role_name: &str,
) -> Result<MemberRow, AppError> {
    let member_id = Uuid::new_v4();

    #[cfg(not(feature = "sqlite"))]
    {
        sqlx::query!(
            r#"
            INSERT INTO workspace_members (id, workspace_id, user_id, role_id, created_at)
            SELECT $1, $2, $3, r.id, NOW()
            FROM roles r WHERE r.name = $4
            "#,
            member_id,
            workspace_id,
            user_id,
            role_name
        )
        .execute(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
                AppError::Conflict("User is already a member of this workspace".to_string())
            } else {
                AppError::Anyhow(anyhow::anyhow!(e))
            }
        })?;
    }

    #[cfg(feature = "sqlite")]
    {
        let id_str = member_id.to_string();
        let ws_str = workspace_id.to_string();
        let user_str = user_id.to_string();
        sqlx::query!(
            r#"
            INSERT INTO workspace_members (id, workspace_id, user_id, role_id, created_at)
            SELECT ?, ?, ?, r.id, datetime('now')
            FROM roles r WHERE r.name = ?
            "#,
            id_str,
            ws_str,
            user_str,
            role_name
        )
        .execute(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::Conflict("User is already a member of this workspace".to_string())
            } else {
                AppError::Anyhow(anyhow::anyhow!(e))
            }
        })?;
    }

    let rows = list_members(pool, workspace_id).await?;
    rows.into_iter()
        .find(|m| m.member_id_eq(member_id))
        .ok_or_else(|| AppError::Anyhow(anyhow::anyhow!("Member not found after insert")))
}

impl MemberRow {
    fn member_id_eq(&self, id: Uuid) -> bool {
        self.id == id
    }
}

pub async fn update_member_role(
    pool: &DbPool,
    workspace_id: Uuid,
    user_id: Uuid,
    new_role_name: &str,
) -> Result<(), AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let result = sqlx::query!(
            r#"
            UPDATE workspace_members wm
            SET role_id = r.id
            FROM roles r
            WHERE r.name = $1
              AND wm.workspace_id = $2
              AND wm.user_id = $3
            "#,
            new_role_name,
            workspace_id,
            user_id
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Member not found".to_string()));
        }
    }

    #[cfg(feature = "sqlite")]
    {
        let ws_str = workspace_id.to_string();
        let user_str = user_id.to_string();
        let result = sqlx::query!(
            r#"
            UPDATE workspace_members
            SET role_id = (SELECT id FROM roles WHERE name = ?)
            WHERE workspace_id = ? AND user_id = ?
            "#,
            new_role_name,
            ws_str,
            user_str
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Member not found".to_string()));
        }
    }

    Ok(())
}

pub async fn remove_member(
    pool: &DbPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let result = sqlx::query!(
            "DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
            workspace_id,
            user_id
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Member not found".to_string()));
        }
    }

    #[cfg(feature = "sqlite")]
    {
        let ws_str = workspace_id.to_string();
        let user_str = user_id.to_string();
        let result = sqlx::query!(
            "DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
            ws_str,
            user_str
        )
        .execute(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Member not found".to_string()));
        }
    }

    Ok(())
}

// ── Workspace listing ─────────────────────────────────────────────────────────

pub struct WorkspaceRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub member_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_workspaces(pool: &DbPool) -> Result<Vec<WorkspaceRow>, AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let rows = sqlx::query!(
            r#"
            SELECT w.id, w.name, w.slug, w.created_at,
                   COUNT(wm.id) as "member_count!"
            FROM workspaces w
            LEFT JOIN workspace_members wm ON wm.workspace_id = w.id
            GROUP BY w.id, w.name, w.slug, w.created_at
            ORDER BY w.created_at
            "#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(rows
            .into_iter()
            .map(|r| WorkspaceRow {
                id: r.id,
                name: r.name,
                slug: r.slug,
                member_count: r.member_count,
                created_at: r.created_at,
            })
            .collect())
    }

    #[cfg(feature = "sqlite")]
    {
        let rows = sqlx::query!(
            r#"
            SELECT w.id, w.name, w.slug, w.created_at,
                   COUNT(wm.id) as member_count
            FROM workspaces w
            LEFT JOIN workspace_members wm ON wm.workspace_id = w.id
            GROUP BY w.id, w.name, w.slug, w.created_at
            ORDER BY w.created_at
            "#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;

        Ok(rows
            .into_iter()
            .map(|r| WorkspaceRow {
                id: Uuid::parse_str(&r.id).unwrap_or_else(|_| Uuid::new_v4()),
                name: r.name,
                slug: r.slug,
                member_count: r.member_count.unwrap_or(0),
                created_at: chrono::DateTime::parse_from_rfc3339(&r.created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }
}

/// Look up a user by email — used when adding a member by email address.
pub async fn find_user_by_email(pool: &DbPool, email: &str) -> Result<Option<Uuid>, AppError> {
    #[cfg(not(feature = "sqlite"))]
    {
        let row = sqlx::query!("SELECT id FROM users WHERE email = $1", email)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;
        Ok(row.map(|r| r.id))
    }

    #[cfg(feature = "sqlite")]
    {
        let row = sqlx::query!("SELECT id FROM users WHERE email = ?", email)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Anyhow(anyhow::anyhow!(e)))?;
        Ok(row.map(|r| Uuid::parse_str(&r.id).unwrap_or_else(|_| Uuid::new_v4())))
    }
}
