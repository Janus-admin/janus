use crate::{db::DbPool, errors::AppResult};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Alert {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub name: String,
    #[serde(rename = "type")]
    pub alert_type: String,
    pub threshold: f64,
    pub window_minutes: i32,
    pub is_active: bool,
    pub webhook_url: Option<String>,
    pub webhook_format: String,
    /// True when a webhook secret is configured (the secret itself is never returned).
    pub webhook_secret_set: bool,
    pub slack_webhook_url: Option<String>,
    /// Recipient email addresses for this alert.
    pub email_to: Vec<String>,
    pub last_triggered: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AlertHistoryEntry {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub triggered_at: DateTime<Utc>,
    pub value: Option<f64>,
    pub message: Option<String>,
    pub delivered: bool,
    pub error: Option<String>,
}

// ── Shared param types ────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CreateAlertParams<'a> {
    pub name: &'a str,
    pub alert_type: &'a str,
    pub threshold: Decimal,
    pub window_minutes: i32,
    pub webhook_url: Option<&'a str>,
    pub webhook_format: &'a str,
    pub webhook_secret: Option<&'a str>,
    pub slack_webhook_url: Option<&'a str>,
    pub email_to: &'a [String],
}

#[derive(Debug, Default)]
pub struct UpdateAlertParams {
    pub name: Option<String>,
    pub threshold: Option<Decimal>,
    pub window_minutes: Option<i32>,
    pub is_active: Option<bool>,
    pub webhook_url: Option<Option<String>>,
    pub webhook_format: Option<String>,
    pub webhook_secret: Option<Option<String>>,
    pub slack_webhook_url: Option<Option<String>>,
    pub email_to: Option<Vec<String>>,
}

// ── PostgreSQL implementation ─────────────────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
mod pg {
    use super::*;

    #[derive(sqlx::FromRow)]
    pub(super) struct AlertDbRow {
        pub id: Uuid,
        pub workspace_id: Option<Uuid>,
        pub name: String,
        pub alert_type: String,
        pub threshold: Decimal,
        pub window_minutes: i32,
        pub is_active: bool,
        pub webhook_url: Option<String>,
        pub webhook_format: String,
        pub webhook_secret: Option<String>,
        pub slack_webhook_url: Option<String>,
        /// NULL → empty vec; TEXT[] → Vec<String>.
        pub email_to: Option<Vec<String>>,
        pub last_triggered: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }

    impl From<AlertDbRow> for Alert {
        fn from(r: AlertDbRow) -> Self {
            Alert {
                id: r.id,
                workspace_id: r.workspace_id,
                name: r.name,
                alert_type: r.alert_type,
                threshold: r.threshold.try_into().unwrap_or_default(),
                window_minutes: r.window_minutes,
                is_active: r.is_active,
                webhook_url: r.webhook_url,
                webhook_format: r.webhook_format,
                webhook_secret_set: r.webhook_secret.is_some(),
                slack_webhook_url: r.slack_webhook_url,
                email_to: r.email_to.unwrap_or_default(),
                last_triggered: r.last_triggered,
                created_at: r.created_at,
            }
        }
    }

    #[derive(sqlx::FromRow)]
    pub(super) struct HistoryDbRow {
        pub id: Uuid,
        pub alert_id: Uuid,
        pub triggered_at: DateTime<Utc>,
        pub value: Option<Decimal>,
        pub message: Option<String>,
        pub delivered: bool,
        pub error: Option<String>,
    }

    impl From<HistoryDbRow> for AlertHistoryEntry {
        fn from(r: HistoryDbRow) -> Self {
            AlertHistoryEntry {
                id: r.id,
                alert_id: r.alert_id,
                triggered_at: r.triggered_at,
                value: r.value.and_then(|d| d.try_into().ok()),
                message: r.message,
                delivered: r.delivered,
                error: r.error,
            }
        }
    }

    const SELECT_COLS: &str =
        "id, workspace_id, name, type AS alert_type, threshold, window_minutes,
         is_active, webhook_url, webhook_format, webhook_secret,
         slack_webhook_url, email_to,
         last_triggered, created_at";

    pub(super) async fn list(pool: &DbPool) -> AppResult<Vec<Alert>> {
        let sql = format!("SELECT {SELECT_COLS} FROM alerts ORDER BY created_at DESC");
        let rows = sqlx::query_as::<_, AlertDbRow>(&sql)
            .fetch_all(pool)
            .await?;
        Ok(rows.into_iter().map(Alert::from).collect())
    }

    pub(super) async fn get(pool: &DbPool, id: Uuid) -> AppResult<Option<Alert>> {
        let sql = format!("SELECT {SELECT_COLS} FROM alerts WHERE id = $1");
        let row = sqlx::query_as::<_, AlertDbRow>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(Alert::from))
    }

    pub(super) async fn create(pool: &DbPool, p: CreateAlertParams<'_>) -> AppResult<Alert> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let email_to: Option<Vec<String>> = if p.email_to.is_empty() {
            None
        } else {
            Some(p.email_to.to_vec())
        };
        let row = sqlx::query_as::<_, AlertDbRow>(
            "INSERT INTO alerts
                 (id, name, type, threshold, window_minutes, is_active,
                  webhook_url, webhook_format, webhook_secret,
                  slack_webhook_url, email_to,
                  created_at)
             VALUES ($1, $2, $3, $4, $5, TRUE, $6, $7, $8, $9, $10, $11)
             RETURNING id, workspace_id, name, type AS alert_type, threshold, window_minutes,
                       is_active, webhook_url, webhook_format, webhook_secret,
                       slack_webhook_url, email_to,
                       last_triggered, created_at",
        )
        .bind(id)
        .bind(p.name)
        .bind(p.alert_type)
        .bind(p.threshold)
        .bind(p.window_minutes)
        .bind(p.webhook_url)
        .bind(p.webhook_format)
        .bind(p.webhook_secret)
        .bind(p.slack_webhook_url)
        .bind(email_to)
        .bind(now)
        .fetch_one(pool)
        .await?;
        Ok(Alert::from(row))
    }

    pub(super) async fn update(
        pool: &DbPool,
        id: Uuid,
        p: UpdateAlertParams,
    ) -> AppResult<Option<Alert>> {
        let mut set_parts: Vec<String> = Vec::new();
        let mut idx: i32 = 1;

        if p.name.is_some() {
            set_parts.push(format!("name = ${idx}"));
            idx += 1;
        }
        if p.threshold.is_some() {
            set_parts.push(format!("threshold = ${idx}"));
            idx += 1;
        }
        if p.window_minutes.is_some() {
            set_parts.push(format!("window_minutes = ${idx}"));
            idx += 1;
        }
        if p.is_active.is_some() {
            set_parts.push(format!("is_active = ${idx}"));
            idx += 1;
        }
        if p.webhook_url.is_some() {
            set_parts.push(format!("webhook_url = ${idx}"));
            idx += 1;
        }
        if p.webhook_format.is_some() {
            set_parts.push(format!("webhook_format = ${idx}"));
            idx += 1;
        }
        if p.webhook_secret.is_some() {
            set_parts.push(format!("webhook_secret = ${idx}"));
            idx += 1;
        }
        if p.slack_webhook_url.is_some() {
            set_parts.push(format!("slack_webhook_url = ${idx}"));
            idx += 1;
        }
        if p.email_to.is_some() {
            set_parts.push(format!("email_to = ${idx}"));
            idx += 1;
        }

        if set_parts.is_empty() {
            return get(pool, id).await;
        }

        let sql = format!(
            "UPDATE alerts SET {}
             WHERE id = ${idx}
             RETURNING {SELECT_COLS}",
            set_parts.join(", ")
        );

        let mut q = sqlx::query_as::<_, AlertDbRow>(&sql);
        if let Some(v) = p.name {
            q = q.bind(v);
        }
        if let Some(v) = p.threshold {
            q = q.bind(v);
        }
        if let Some(v) = p.window_minutes {
            q = q.bind(v);
        }
        if let Some(v) = p.is_active {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_url {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_format {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_secret {
            q = q.bind(v);
        }
        if let Some(v) = p.slack_webhook_url {
            q = q.bind(v);
        }
        if let Some(v) = p.email_to {
            let bound: Option<Vec<String>> = if v.is_empty() { None } else { Some(v) };
            q = q.bind(bound);
        }
        q = q.bind(id);

        let row = q.fetch_optional(pool).await?;
        Ok(row.map(Alert::from))
    }

    pub(super) async fn get_history(
        pool: &DbPool,
        alert_id: Uuid,
    ) -> AppResult<Vec<AlertHistoryEntry>> {
        let rows = sqlx::query_as::<_, HistoryDbRow>(
            "SELECT id, alert_id, triggered_at, value, message, delivered, error
             FROM alert_history
             WHERE alert_id = $1
             ORDER BY triggered_at DESC
             LIMIT 100",
        )
        .bind(alert_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(AlertHistoryEntry::from).collect())
    }

    pub(super) async fn record_test_delivery(
        pool: &DbPool,
        alert_id: Uuid,
        delivered: bool,
        error: Option<&str>,
    ) -> AppResult<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO alert_history (id, alert_id, triggered_at, message, delivered, error)
             VALUES ($1, $2, $3, 'Test delivery', $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(alert_id)
        .bind(now)
        .bind(delivered)
        .bind(error)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ── SQLite implementation ─────────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
mod lite {
    use super::*;

    #[derive(sqlx::FromRow)]
    pub(super) struct AlertDbRow {
        pub id: Uuid,
        pub workspace_id: Option<Uuid>,
        pub name: String,
        pub alert_type: String,
        pub threshold: String,
        pub window_minutes: i32,
        pub is_active: bool,
        pub webhook_url: Option<String>,
        pub webhook_format: String,
        pub webhook_secret: Option<String>,
        pub slack_webhook_url: Option<String>,
        /// Stored as JSON text; NULL → empty vec.
        pub email_to: Option<String>,
        pub last_triggered: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }

    impl From<AlertDbRow> for Alert {
        fn from(r: AlertDbRow) -> Self {
            let email_to = r
                .email_to
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default();
            Alert {
                id: r.id,
                workspace_id: r.workspace_id,
                name: r.name,
                alert_type: r.alert_type,
                threshold: r.threshold.parse::<f64>().unwrap_or_default(),
                window_minutes: r.window_minutes,
                is_active: r.is_active,
                webhook_url: r.webhook_url,
                webhook_format: r.webhook_format,
                webhook_secret_set: r.webhook_secret.is_some(),
                slack_webhook_url: r.slack_webhook_url,
                email_to,
                last_triggered: r.last_triggered,
                created_at: r.created_at,
            }
        }
    }

    #[derive(sqlx::FromRow)]
    pub(super) struct HistoryDbRow {
        pub id: Uuid,
        pub alert_id: Uuid,
        pub triggered_at: DateTime<Utc>,
        pub value: Option<String>,
        pub message: Option<String>,
        pub delivered: bool,
        pub error: Option<String>,
    }

    impl From<HistoryDbRow> for AlertHistoryEntry {
        fn from(r: HistoryDbRow) -> Self {
            AlertHistoryEntry {
                id: r.id,
                alert_id: r.alert_id,
                triggered_at: r.triggered_at,
                value: r.value.as_deref().and_then(|s| s.parse::<f64>().ok()),
                message: r.message,
                delivered: r.delivered,
                error: r.error,
            }
        }
    }

    const SELECT_COLS: &str =
        "id, workspace_id, name, type AS alert_type, threshold, window_minutes,
         is_active, webhook_url, webhook_format, webhook_secret,
         slack_webhook_url, email_to,
         last_triggered, created_at";

    pub(super) async fn list(pool: &DbPool) -> AppResult<Vec<Alert>> {
        let sql = format!("SELECT {SELECT_COLS} FROM alerts ORDER BY created_at DESC");
        let rows = sqlx::query_as::<_, AlertDbRow>(&sql)
            .fetch_all(pool)
            .await?;
        Ok(rows.into_iter().map(Alert::from).collect())
    }

    pub(super) async fn get(pool: &DbPool, id: Uuid) -> AppResult<Option<Alert>> {
        let sql = format!("SELECT {SELECT_COLS} FROM alerts WHERE id = $1");
        let row = sqlx::query_as::<_, AlertDbRow>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(Alert::from))
    }

    pub(super) async fn create(pool: &DbPool, p: CreateAlertParams<'_>) -> AppResult<Alert> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let threshold_str = p.threshold.to_string();
        let email_to_json = if p.email_to.is_empty() {
            None
        } else {
            Some(serde_json::to_string(p.email_to).unwrap_or_default())
        };
        sqlx::query(
            "INSERT INTO alerts
                 (id, name, type, threshold, window_minutes, is_active,
                  webhook_url, webhook_format, webhook_secret,
                  slack_webhook_url, email_to,
                  created_at)
             VALUES ($1, $2, $3, $4, $5, 1, $6, $7, $8, $9, $10, $11)",
        )
        .bind(id)
        .bind(p.name)
        .bind(p.alert_type)
        .bind(&threshold_str)
        .bind(p.window_minutes)
        .bind(p.webhook_url)
        .bind(p.webhook_format)
        .bind(p.webhook_secret)
        .bind(p.slack_webhook_url)
        .bind(email_to_json)
        .bind(now)
        .execute(pool)
        .await?;

        let sql = format!("SELECT {SELECT_COLS} FROM alerts WHERE id = $1");
        let row = sqlx::query_as::<_, AlertDbRow>(&sql)
            .bind(id)
            .fetch_one(pool)
            .await?;
        Ok(Alert::from(row))
    }

    pub(super) async fn update(
        pool: &DbPool,
        id: Uuid,
        p: UpdateAlertParams,
    ) -> AppResult<Option<Alert>> {
        let mut set_parts: Vec<String> = Vec::new();
        let mut idx: i32 = 1;

        if p.name.is_some() {
            set_parts.push(format!("name = ${idx}"));
            idx += 1;
        }
        if p.threshold.is_some() {
            set_parts.push(format!("threshold = ${idx}"));
            idx += 1;
        }
        if p.window_minutes.is_some() {
            set_parts.push(format!("window_minutes = ${idx}"));
            idx += 1;
        }
        if p.is_active.is_some() {
            set_parts.push(format!("is_active = ${idx}"));
            idx += 1;
        }
        if p.webhook_url.is_some() {
            set_parts.push(format!("webhook_url = ${idx}"));
            idx += 1;
        }
        if p.webhook_format.is_some() {
            set_parts.push(format!("webhook_format = ${idx}"));
            idx += 1;
        }
        if p.webhook_secret.is_some() {
            set_parts.push(format!("webhook_secret = ${idx}"));
            idx += 1;
        }
        if p.slack_webhook_url.is_some() {
            set_parts.push(format!("slack_webhook_url = ${idx}"));
            idx += 1;
        }
        if p.email_to.is_some() {
            set_parts.push(format!("email_to = ${idx}"));
            idx += 1;
        }

        if set_parts.is_empty() {
            return get(pool, id).await;
        }

        let sql = format!(
            "UPDATE alerts SET {} WHERE id = ${idx}",
            set_parts.join(", ")
        );

        let mut q = sqlx::query(&sql);
        if let Some(v) = p.name {
            q = q.bind(v);
        }
        if let Some(v) = p.threshold {
            q = q.bind(v.to_string());
        }
        if let Some(v) = p.window_minutes {
            q = q.bind(v);
        }
        if let Some(v) = p.is_active {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_url {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_format {
            q = q.bind(v);
        }
        if let Some(v) = p.webhook_secret {
            q = q.bind(v);
        }
        if let Some(v) = p.slack_webhook_url {
            q = q.bind(v);
        }
        if let Some(v) = p.email_to {
            let json = if v.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&v).unwrap_or_default())
            };
            q = q.bind(json);
        }
        q = q.bind(id);
        q.execute(pool).await?;

        get(pool, id).await
    }

    pub(super) async fn get_history(
        pool: &DbPool,
        alert_id: Uuid,
    ) -> AppResult<Vec<AlertHistoryEntry>> {
        let rows = sqlx::query_as::<_, HistoryDbRow>(
            "SELECT id, alert_id, triggered_at, value, message, delivered, error
             FROM alert_history
             WHERE alert_id = $1
             ORDER BY triggered_at DESC
             LIMIT 100",
        )
        .bind(alert_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(AlertHistoryEntry::from).collect())
    }

    pub(super) async fn record_test_delivery(
        pool: &DbPool,
        alert_id: Uuid,
        delivered: bool,
        error: Option<&str>,
    ) -> AppResult<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO alert_history (id, alert_id, triggered_at, message, delivered, error)
             VALUES ($1, $2, $3, 'Test delivery', $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(alert_id)
        .bind(now)
        .bind(delivered)
        .bind(error)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ── Public API (delegates to the active backend) ──────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn list(pool: &DbPool) -> AppResult<Vec<Alert>> {
    pg::list(pool).await
}

#[cfg(feature = "sqlite")]
pub async fn list(pool: &DbPool) -> AppResult<Vec<Alert>> {
    lite::list(pool).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get(pool: &DbPool, id: Uuid) -> AppResult<Option<Alert>> {
    pg::get(pool, id).await
}

#[cfg(feature = "sqlite")]
pub async fn get(pool: &DbPool, id: Uuid) -> AppResult<Option<Alert>> {
    lite::get(pool, id).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn create(pool: &DbPool, p: CreateAlertParams<'_>) -> AppResult<Alert> {
    pg::create(pool, p).await
}

#[cfg(feature = "sqlite")]
pub async fn create(pool: &DbPool, p: CreateAlertParams<'_>) -> AppResult<Alert> {
    lite::create(pool, p).await
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn update(pool: &DbPool, id: Uuid, p: UpdateAlertParams) -> AppResult<Option<Alert>> {
    pg::update(pool, id, p).await
}

#[cfg(feature = "sqlite")]
pub async fn update(pool: &DbPool, id: Uuid, p: UpdateAlertParams) -> AppResult<Option<Alert>> {
    lite::update(pool, id, p).await
}

pub async fn delete(pool: &DbPool, id: Uuid) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM alerts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get_history(pool: &DbPool, alert_id: Uuid) -> AppResult<Vec<AlertHistoryEntry>> {
    pg::get_history(pool, alert_id).await
}

#[cfg(feature = "sqlite")]
pub async fn get_history(pool: &DbPool, alert_id: Uuid) -> AppResult<Vec<AlertHistoryEntry>> {
    lite::get_history(pool, alert_id).await
}

pub async fn get_secret(pool: &DbPool, id: Uuid) -> AppResult<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT webhook_secret FROM alerts WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(s,)| s))
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn record_test_delivery(
    pool: &DbPool,
    alert_id: Uuid,
    delivered: bool,
    error: Option<&str>,
) -> AppResult<()> {
    pg::record_test_delivery(pool, alert_id, delivered, error).await
}

#[cfg(feature = "sqlite")]
pub async fn record_test_delivery(
    pool: &DbPool,
    alert_id: Uuid,
    delivered: bool,
    error: Option<&str>,
) -> AppResult<()> {
    lite::record_test_delivery(pool, alert_id, delivered, error).await
}
