use crate::{db::DbPool, errors::AppResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub version: i32,
    pub content: String,
    pub system_prompt: Option<String>,
    pub is_active: bool,
    pub ab_weight: i32,
    pub created_at: DateTime<Utc>,
}

// ── PostgreSQL implementation ─────────────────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
mod pg {
    use super::*;

    #[derive(sqlx::FromRow)]
    pub(super) struct PromptRow {
        pub id: Uuid,
        pub name: String,
        pub description: Option<String>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    impl From<PromptRow> for Prompt {
        fn from(r: PromptRow) -> Self {
            Prompt {
                id: r.id,
                name: r.name,
                description: r.description,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        }
    }

    #[derive(sqlx::FromRow)]
    pub(super) struct VersionRow {
        pub id: Uuid,
        pub prompt_id: Uuid,
        pub version: i32,
        pub content: String,
        pub system_prompt: Option<String>,
        pub is_active: bool,
        pub ab_weight: i32,
        pub created_at: DateTime<Utc>,
    }

    impl From<VersionRow> for PromptVersion {
        fn from(r: VersionRow) -> Self {
            PromptVersion {
                id: r.id,
                prompt_id: r.prompt_id,
                version: r.version,
                content: r.content,
                system_prompt: r.system_prompt,
                is_active: r.is_active,
                ab_weight: r.ab_weight,
                created_at: r.created_at,
            }
        }
    }

    pub(super) async fn create_prompt(
        pool: &DbPool,
        id: Uuid,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<Prompt> {
        let now = Utc::now();
        let row = sqlx::query_as::<_, PromptRow>(
            "INSERT INTO prompts (id, name, description, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, name, description, created_at, updated_at",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(now)
        .bind(now)
        .fetch_one(pool)
        .await?;
        Ok(Prompt::from(row))
    }

    pub(super) async fn list_prompts(
        pool: &DbPool,
        page: i64,
        per_page: i64,
    ) -> AppResult<(Vec<Prompt>, i64)> {
        let offset = (page - 1) * per_page;
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prompts")
            .fetch_one(pool)
            .await?;
        let rows = sqlx::query_as::<_, PromptRow>(
            "SELECT id, name, description, created_at, updated_at
             FROM prompts ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(pool)
        .await?;
        Ok((rows.into_iter().map(Prompt::from).collect(), total.0))
    }

    pub(super) async fn get_prompt(pool: &DbPool, id: Uuid) -> AppResult<Option<Prompt>> {
        let row = sqlx::query_as::<_, PromptRow>(
            "SELECT id, name, description, created_at, updated_at FROM prompts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Prompt::from))
    }

    pub(super) async fn get_versions(
        pool: &DbPool,
        prompt_id: Uuid,
    ) -> AppResult<Vec<PromptVersion>> {
        let rows = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE prompt_id = $1 ORDER BY version DESC",
        )
        .bind(prompt_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(PromptVersion::from).collect())
    }

    pub(super) async fn get_active_versions(
        pool: &DbPool,
        prompt_id: Uuid,
    ) -> AppResult<Vec<PromptVersion>> {
        let rows = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE prompt_id = $1 AND is_active = TRUE ORDER BY version DESC",
        )
        .bind(prompt_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(PromptVersion::from).collect())
    }

    pub(super) async fn create_version(
        pool: &DbPool,
        id: Uuid,
        prompt_id: Uuid,
        content: &str,
        system_prompt: Option<&str>,
    ) -> AppResult<PromptVersion> {
        let now = Utc::now();
        let row = sqlx::query_as::<_, VersionRow>(
            "INSERT INTO prompt_versions
                 (id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at)
             VALUES (
                 $1, $2,
                 (SELECT COALESCE(MAX(version), 0) + 1 FROM prompt_versions WHERE prompt_id = $2),
                 $3, $4, FALSE, 100, $5
             )
             RETURNING id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at",
        )
        .bind(id)
        .bind(prompt_id)
        .bind(content)
        .bind(system_prompt)
        .bind(now)
        .fetch_one(pool)
        .await?;
        Ok(PromptVersion::from(row))
    }

    pub(super) async fn update_version(
        pool: &DbPool,
        prompt_id: Uuid,
        version: i32,
        is_active: Option<bool>,
        ab_weight: Option<i32>,
    ) -> AppResult<Option<PromptVersion>> {
        // Deactivate all other versions of this prompt when activating one.
        if is_active == Some(true) {
            sqlx::query(
                "UPDATE prompt_versions SET is_active = FALSE WHERE prompt_id = $1 AND version <> $2",
            )
            .bind(prompt_id)
            .bind(version)
            .execute(pool)
            .await?;
        }

        let mut set_parts: Vec<String> = Vec::new();
        let mut idx: i32 = 1;
        if is_active.is_some() {
            set_parts.push(format!("is_active = ${idx}"));
            idx += 1;
        }
        if ab_weight.is_some() {
            set_parts.push(format!("ab_weight = ${idx}"));
            idx += 1;
        }
        if set_parts.is_empty() {
            // Nothing to update — return current version.
            let row = sqlx::query_as::<_, VersionRow>(
                "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
                 FROM prompt_versions WHERE prompt_id = $1 AND version = $2",
            )
            .bind(prompt_id)
            .bind(version)
            .fetch_optional(pool)
            .await?;
            return Ok(row.map(PromptVersion::from));
        }

        let sql = format!(
            "UPDATE prompt_versions SET {sets}
             WHERE prompt_id = ${pid} AND version = ${ver}
             RETURNING id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at",
            sets = set_parts.join(", "),
            pid = idx,
            ver = idx + 1
        );
        let mut q = sqlx::query_as::<_, VersionRow>(&sql);
        if let Some(v) = is_active {
            q = q.bind(v);
        }
        if let Some(w) = ab_weight {
            q = q.bind(w);
        }
        q = q.bind(prompt_id).bind(version);
        let row = q.fetch_optional(pool).await?;
        Ok(row.map(PromptVersion::from))
    }
}

// ── SQLite implementation ─────────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
mod lite {
    use super::*;

    #[derive(sqlx::FromRow)]
    pub(super) struct PromptRow {
        pub id: Uuid,
        pub name: String,
        pub description: Option<String>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    impl From<PromptRow> for Prompt {
        fn from(r: PromptRow) -> Self {
            Prompt {
                id: r.id,
                name: r.name,
                description: r.description,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        }
    }

    #[derive(sqlx::FromRow)]
    pub(super) struct VersionRow {
        pub id: Uuid,
        pub prompt_id: Uuid,
        pub version: i32,
        pub content: String,
        pub system_prompt: Option<String>,
        pub is_active: bool,
        pub ab_weight: i32,
        pub created_at: DateTime<Utc>,
    }

    impl From<VersionRow> for PromptVersion {
        fn from(r: VersionRow) -> Self {
            PromptVersion {
                id: r.id,
                prompt_id: r.prompt_id,
                version: r.version,
                content: r.content,
                system_prompt: r.system_prompt,
                is_active: r.is_active,
                ab_weight: r.ab_weight,
                created_at: r.created_at,
            }
        }
    }

    pub(super) async fn create_prompt(
        pool: &DbPool,
        id: Uuid,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<Prompt> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO prompts (id, name, description, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        let row = sqlx::query_as::<_, PromptRow>(
            "SELECT id, name, description, created_at, updated_at FROM prompts WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(Prompt::from(row))
    }

    pub(super) async fn list_prompts(
        pool: &DbPool,
        page: i64,
        per_page: i64,
    ) -> AppResult<(Vec<Prompt>, i64)> {
        let offset = (page - 1) * per_page;
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prompts")
            .fetch_one(pool)
            .await?;
        let rows = sqlx::query_as::<_, PromptRow>(
            "SELECT id, name, description, created_at, updated_at
             FROM prompts ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(pool)
        .await?;
        Ok((rows.into_iter().map(Prompt::from).collect(), total.0))
    }

    pub(super) async fn get_prompt(pool: &DbPool, id: Uuid) -> AppResult<Option<Prompt>> {
        let row = sqlx::query_as::<_, PromptRow>(
            "SELECT id, name, description, created_at, updated_at FROM prompts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(Prompt::from))
    }

    pub(super) async fn get_versions(
        pool: &DbPool,
        prompt_id: Uuid,
    ) -> AppResult<Vec<PromptVersion>> {
        let rows = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE prompt_id = $1 ORDER BY version DESC",
        )
        .bind(prompt_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(PromptVersion::from).collect())
    }

    pub(super) async fn get_active_versions(
        pool: &DbPool,
        prompt_id: Uuid,
    ) -> AppResult<Vec<PromptVersion>> {
        let rows = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE prompt_id = $1 AND is_active = 1 ORDER BY version DESC",
        )
        .bind(prompt_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(PromptVersion::from).collect())
    }

    pub(super) async fn create_version(
        pool: &DbPool,
        id: Uuid,
        prompt_id: Uuid,
        content: &str,
        system_prompt: Option<&str>,
    ) -> AppResult<PromptVersion> {
        let now = Utc::now();
        // Compute next version number.
        let next_ver: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM prompt_versions WHERE prompt_id = $1",
        )
        .bind(prompt_id)
        .fetch_one(pool)
        .await?;
        let ver = next_ver.0 as i32;

        sqlx::query(
            "INSERT INTO prompt_versions
                 (id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at)
             VALUES ($1, $2, $3, $4, $5, 0, 100, $6)",
        )
        .bind(id)
        .bind(prompt_id)
        .bind(ver)
        .bind(content)
        .bind(system_prompt)
        .bind(now)
        .execute(pool)
        .await?;

        let row = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(PromptVersion::from(row))
    }

    pub(super) async fn update_version(
        pool: &DbPool,
        prompt_id: Uuid,
        version: i32,
        is_active: Option<bool>,
        ab_weight: Option<i32>,
    ) -> AppResult<Option<PromptVersion>> {
        if is_active == Some(true) {
            sqlx::query(
                "UPDATE prompt_versions SET is_active = 0 WHERE prompt_id = $1 AND version <> $2",
            )
            .bind(prompt_id)
            .bind(version)
            .execute(pool)
            .await?;
        }

        let mut set_parts: Vec<String> = Vec::new();
        let mut idx: i32 = 1;
        if is_active.is_some() {
            set_parts.push(format!("is_active = ${idx}"));
            idx += 1;
        }
        if ab_weight.is_some() {
            set_parts.push(format!("ab_weight = ${idx}"));
            idx += 1;
        }
        if set_parts.is_empty() {
            let row = sqlx::query_as::<_, VersionRow>(
                "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
                 FROM prompt_versions WHERE prompt_id = $1 AND version = $2",
            )
            .bind(prompt_id)
            .bind(version)
            .fetch_optional(pool)
            .await?;
            return Ok(row.map(PromptVersion::from));
        }

        let sql = format!(
            "UPDATE prompt_versions SET {sets} WHERE prompt_id = ${pid} AND version = ${ver}",
            sets = set_parts.join(", "),
            pid = idx,
            ver = idx + 1
        );
        let mut q = sqlx::query(&sql);
        if let Some(v) = is_active {
            q = q.bind(v as i32);
        }
        if let Some(w) = ab_weight {
            q = q.bind(w);
        }
        q = q.bind(prompt_id).bind(version);
        q.execute(pool).await?;

        let row = sqlx::query_as::<_, VersionRow>(
            "SELECT id, prompt_id, version, content, system_prompt, is_active, ab_weight, created_at
             FROM prompt_versions WHERE prompt_id = $1 AND version = $2",
        )
        .bind(prompt_id)
        .bind(version)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(PromptVersion::from))
    }
}

// ── Public API (delegates to the active backend) ──────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn create_prompt(
    pool: &DbPool,
    id: Uuid,
    name: &str,
    description: Option<&str>,
) -> AppResult<Prompt> {
    pg::create_prompt(pool, id, name, description).await
}

#[cfg(feature = "sqlite")]
pub async fn create_prompt(
    pool: &DbPool,
    id: Uuid,
    name: &str,
    description: Option<&str>,
) -> AppResult<Prompt> {
    lite::create_prompt(pool, id, name, description).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn list_prompts(
    pool: &DbPool,
    page: i64,
    per_page: i64,
) -> AppResult<(Vec<Prompt>, i64)> {
    pg::list_prompts(pool, page, per_page).await
}

#[cfg(feature = "sqlite")]
pub async fn list_prompts(
    pool: &DbPool,
    page: i64,
    per_page: i64,
) -> AppResult<(Vec<Prompt>, i64)> {
    lite::list_prompts(pool, page, per_page).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get_prompt(pool: &DbPool, id: Uuid) -> AppResult<Option<Prompt>> {
    pg::get_prompt(pool, id).await
}

#[cfg(feature = "sqlite")]
pub async fn get_prompt(pool: &DbPool, id: Uuid) -> AppResult<Option<Prompt>> {
    lite::get_prompt(pool, id).await
}

pub async fn delete_prompt(pool: &DbPool, id: Uuid) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM prompts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get_versions(pool: &DbPool, prompt_id: Uuid) -> AppResult<Vec<PromptVersion>> {
    pg::get_versions(pool, prompt_id).await
}

#[cfg(feature = "sqlite")]
pub async fn get_versions(pool: &DbPool, prompt_id: Uuid) -> AppResult<Vec<PromptVersion>> {
    lite::get_versions(pool, prompt_id).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get_active_versions(pool: &DbPool, prompt_id: Uuid) -> AppResult<Vec<PromptVersion>> {
    pg::get_active_versions(pool, prompt_id).await
}

#[cfg(feature = "sqlite")]
pub async fn get_active_versions(pool: &DbPool, prompt_id: Uuid) -> AppResult<Vec<PromptVersion>> {
    lite::get_active_versions(pool, prompt_id).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn create_version(
    pool: &DbPool,
    id: Uuid,
    prompt_id: Uuid,
    content: &str,
    system_prompt: Option<&str>,
) -> AppResult<PromptVersion> {
    pg::create_version(pool, id, prompt_id, content, system_prompt).await
}

#[cfg(feature = "sqlite")]
pub async fn create_version(
    pool: &DbPool,
    id: Uuid,
    prompt_id: Uuid,
    content: &str,
    system_prompt: Option<&str>,
) -> AppResult<PromptVersion> {
    lite::create_version(pool, id, prompt_id, content, system_prompt).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn update_version(
    pool: &DbPool,
    prompt_id: Uuid,
    version: i32,
    is_active: Option<bool>,
    ab_weight: Option<i32>,
) -> AppResult<Option<PromptVersion>> {
    pg::update_version(pool, prompt_id, version, is_active, ab_weight).await
}

#[cfg(feature = "sqlite")]
pub async fn update_version(
    pool: &DbPool,
    prompt_id: Uuid,
    version: i32,
    is_active: Option<bool>,
    ab_weight: Option<i32>,
) -> AppResult<Option<PromptVersion>> {
    lite::update_version(pool, prompt_id, version, is_active, ab_weight).await
}
