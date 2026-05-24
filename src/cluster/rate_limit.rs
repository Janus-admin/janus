// src/cluster/rate_limit.rs
// PostgreSQL-backed sliding-window rate limiter for multi-node deployments.
//
// Replaces the in-memory DashMap when cluster.enabled = true.
// All nodes share the same `rate_limit_windows` table, so rate limits are
// enforced globally regardless of which node handles the request.
//
// Each request inserts one row (fire-and-forget).
// A background task deletes rows older than 2× the window every 60 s.

use crate::db::DbPool;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub struct DbRateLimiter {
    pool: DbPool,
    window_ms: i64,
}

impl DbRateLimiter {
    pub fn new(pool: DbPool, window_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            pool,
            window_ms: (window_secs * 1_000) as i64,
        })
    }

    /// Check the key against its RPM limit and record the request if allowed.
    ///
    /// Returns `Ok(())` if within limits; `Err(retry_after_secs)` if exceeded.
    pub async fn check_and_record(&self, key_id: Uuid, limit: i32) -> Result<(), u64> {
        let cutoff = Utc::now() - Duration::milliseconds(self.window_ms);

        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM rate_limit_windows \
             WHERE api_key_id = $1 AND request_at > $2",
        )
        .bind(key_id)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| 60u64)?;

        if row.0 >= limit as i64 {
            let retry_after = (self.window_ms / 1_000 + 1) as u64;
            return Err(retry_after);
        }

        let _ = sqlx::query("INSERT INTO rate_limit_windows (api_key_id, tokens) VALUES ($1, 0)")
            .bind(key_id)
            .execute(&self.pool)
            .await;

        Ok(())
    }

    /// Check and record estimated tokens against the TPM limit.
    ///
    /// Returns `Ok(())` if within limits; `Err(retry_after_secs)` if exceeded.
    pub async fn check_and_record_tokens(
        &self,
        key_id: Uuid,
        tokens: i64,
        limit: i32,
    ) -> Result<(), u64> {
        let cutoff = Utc::now() - Duration::milliseconds(self.window_ms);

        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(tokens), 0) FROM rate_limit_windows \
             WHERE api_key_id = $1 AND request_at > $2",
        )
        .bind(key_id)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| 60u64)?;

        if row.1 + tokens > limit as i64 {
            let retry_after = (self.window_ms / 1_000 + 1) as u64;
            return Err(retry_after);
        }

        let _ = sqlx::query("INSERT INTO rate_limit_windows (api_key_id, tokens) VALUES ($1, $2)")
            .bind(key_id)
            .bind(tokens as i32)
            .execute(&self.pool)
            .await;

        Ok(())
    }

    /// Delete rows older than 2× the window.  Called by the background cleanup task.
    pub async fn cleanup(&self) -> anyhow::Result<()> {
        let cutoff = Utc::now() - Duration::milliseconds(self.window_ms * 2);
        sqlx::query("DELETE FROM rate_limit_windows WHERE request_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
