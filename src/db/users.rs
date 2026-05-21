use crate::errors::{AppError, AppResult};
use crate::models::user::{UpdateUserRequest, User};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn find_by_email(pool: &PgPool, email: &str) -> AppResult<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, name, created_at, updated_at
         FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, name, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn create(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    name: &str,
) -> AppResult<User> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, password_hash, name)
         VALUES ($1, $2, $3, $4)
         RETURNING id, email, password_hash, name, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(email)
    .bind(password_hash)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn update(pool: &PgPool, id: Uuid, req: &UpdateUserRequest) -> AppResult<User> {
    let user = sqlx::query_as::<_, User>(
        "UPDATE users
         SET name = COALESCE($1, name),
             email = COALESCE($2, email),
             updated_at = NOW()
         WHERE id = $3
         RETURNING id, email, password_hash, name, created_at, updated_at",
    )
    .bind(&req.name)
    .bind(&req.email)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))?;
    Ok(user)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("User {} not found", id)));
    }
    Ok(())
}

pub async fn list(pool: &PgPool, page: i64, per_page: i64) -> AppResult<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, name, created_at, updated_at
         FROM users
         ORDER BY created_at DESC
         LIMIT $1 OFFSET $2",
    )
    .bind(per_page)
    .bind((page - 1) * per_page)
    .fetch_all(pool)
    .await?;
    Ok(users)
}
