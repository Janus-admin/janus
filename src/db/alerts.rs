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

// ── Internal DB row (PostgreSQL) ──────────────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct AlertDbRow {
    id: Uuid,
    workspace_id: Option<Uuid>,
    name: String,
    alert_type: String,
    threshold: Decimal,
    window_minutes: i32,
    is_active: bool,
    webhook_url: Option<String>,
    webhook_format: String,
    webhook_secret: Option<String>,
    last_triggered: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
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
            last_triggered: r.last_triggered,
            created_at: r.created_at,
        }
    }
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct HistoryDbRow {
    id: Uuid,
    alert_id: Uuid,
    triggered_at: DateTime<Utc>,
    value: Option<Decimal>,
    message: Option<String>,
    delivered: bool,
    error: Option<String>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
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

// ── CRUD operations ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CreateAlertParams<'a> {
    pub name: &'a str,
    pub alert_type: &'a str,
    pub threshold: Decimal,
    pub window_minutes: i32,
    pub webhook_url: Option<&'a str>,
    pub webhook_format: &'a str,
    pub webhook_secret: Option<&'a str>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn list(pool: &DbPool) -> AppResult<Vec<Alert>> {
    let rows = sqlx::query_as::<_, AlertDbRow>(
        "SELECT id, workspace_id, name, type AS alert_type, threshold, window_minutes,
                is_active, webhook_url, webhook_format, webhook_secret, last_triggered, created_at
         FROM alerts
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Alert::from).collect())
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn get(pool: &DbPool, id: Uuid) -> AppResult<Option<Alert>> {
    let row = sqlx::query_as::<_, AlertDbRow>(
        "SELECT id, workspace_id, name, type AS alert_type, threshold, window_minutes,
                is_active, webhook_url, webhook_format, webhook_secret, last_triggered, created_at
         FROM alerts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Alert::from))
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn create(pool: &DbPool, p: CreateAlertParams<'_>) -> AppResult<Alert> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let row = sqlx::query_as::<_, AlertDbRow>(
        "INSERT INTO alerts
             (id, name, type, threshold, window_minutes, is_active,
              webhook_url, webhook_format, webhook_secret, created_at)
         VALUES ($1, $2, $3, $4, $5, TRUE, $6, $7, $8, $9)
         RETURNING id, workspace_id, name, type AS alert_type, threshold, window_minutes,
                   is_active, webhook_url, webhook_format, webhook_secret,
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
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(Alert::from(row))
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
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn update(pool: &DbPool, id: Uuid, p: UpdateAlertParams) -> AppResult<Option<Alert>> {
    // Build SET clause dynamically — only update provided fields.
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

    if set_parts.is_empty() {
        return get(pool, id).await;
    }

    let id_placeholder = format!("${idx}");
    let sql = format!(
        "UPDATE alerts SET {}
         WHERE id = {}
         RETURNING id, workspace_id, name, type AS alert_type, threshold, window_minutes,
                   is_active, webhook_url, webhook_format, webhook_secret,
                   last_triggered, created_at",
        set_parts.join(", "),
        id_placeholder
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
    q = q.bind(id);

    let row = q.fetch_optional(pool).await?;
    Ok(row.map(Alert::from))
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

/// Return the raw webhook secret for an alert (used by the test endpoint).
/// The secret is omitted from `Alert` to avoid leaking it in list/get responses.
pub async fn get_secret(pool: &DbPool, id: Uuid) -> AppResult<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT webhook_secret FROM alerts WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(s,)| s))
}

/// Record a webhook test delivery attempt in alert_history.
#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub async fn record_test_delivery(
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
