pub mod email;
pub mod slack;
pub mod webhook;

use crate::{config::SmtpConfig, db::DbPool};
use chrono::Utc;
use email::EmailContext;
use slack::SlackContext;
use uuid::Uuid;
use webhook::{WebhookContext, WebhookFormat};

/// Internal row returned by the alert SELECT query.
#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct AlertRow {
    id: Uuid,
    name: String,
    alert_type: String,
    threshold: rust_decimal::Decimal,
    window_minutes: i32,
    last_triggered: Option<chrono::DateTime<Utc>>,
    webhook_url: Option<String>,
    webhook_format: String,
    webhook_secret: Option<String>,
    slack_webhook_url: Option<String>,
    /// NULL in the DB maps to an empty vec (no recipients configured).
    email_to: Option<Vec<String>>,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct AlertRow {
    id: Uuid,
    name: String,
    alert_type: String,
    threshold: String,
    window_minutes: i32,
    last_triggered: Option<chrono::DateTime<Utc>>,
    webhook_url: Option<String>,
    webhook_format: String,
    webhook_secret: Option<String>,
    slack_webhook_url: Option<String>,
    /// Stored as JSON text (e.g. `["a@b.com"]`). NULL maps to empty vec.
    email_to: Option<String>,
}

#[cfg(feature = "sqlite")]
impl AlertRow {
    fn email_recipients(&self) -> Vec<String> {
        self.email_to
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
impl AlertRow {
    fn email_recipients(&self) -> Vec<String> {
        self.email_to.clone().unwrap_or_default()
    }
}

/// Background alert evaluation engine.
///
/// Runs every 60 seconds and checks all active alerts against live request data.
/// On threshold breach: records history, delivers to all configured channels
/// (generic webhook, Slack, and/or email), then updates `last_triggered`.
/// Re-fire is suppressed until `window_minutes` elapses.
pub struct AlertEngine {
    pool: DbPool,
    client: reqwest::Client,
    smtp: SmtpConfig,
}

impl AlertEngine {
    pub fn new(pool: DbPool, smtp: SmtpConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build webhook HTTP client");
        Self { pool, client, smtp }
    }

    pub async fn evaluate(&self) -> anyhow::Result<()> {
        #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
        let active_filter = "WHERE is_active = TRUE";
        #[cfg(feature = "sqlite")]
        let active_filter = "WHERE is_active = 1";

        let sql = format!(
            "SELECT id, name, type AS alert_type, threshold, window_minutes, last_triggered,
                    webhook_url, webhook_format, webhook_secret,
                    slack_webhook_url, email_to
             FROM alerts {active_filter}"
        );

        let alerts = sqlx::query_as::<_, AlertRow>(&sql)
            .fetch_all(&self.pool)
            .await?;

        for alert in alerts {
            if let Some(last) = alert.last_triggered {
                let elapsed = (Utc::now() - last).num_minutes();
                if elapsed < alert.window_minutes as i64 {
                    continue;
                }
            }

            #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
            let threshold = alert.threshold;
            #[cfg(feature = "sqlite")]
            let threshold: rust_decimal::Decimal = alert.threshold.parse().unwrap_or_default();

            let result = match alert.alert_type.as_str() {
                "spend_threshold" => {
                    self.check_spend_threshold(alert.window_minutes, threshold)
                        .await
                }
                "error_rate" => self.check_error_rate(alert.window_minutes, threshold).await,
                "latency_spike" => {
                    self.check_latency_spike(alert.window_minutes, threshold)
                        .await
                }
                other => {
                    tracing::warn!("Unknown alert type: {}", other);
                    Ok(None)
                }
            };

            match result {
                Ok(Some(value)) => {
                    if let Err(e) = self.fire(&alert, threshold, value).await {
                        tracing::warn!(alert_id = %alert.id, "Alert fire error: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(alert_id = %alert.id, "Alert check error: {e}"),
            }
        }

        Ok(())
    }

    /// Fire an alert: record history, deliver to all configured channels, update last_triggered.
    async fn fire(
        &self,
        alert: &AlertRow,
        threshold: rust_decimal::Decimal,
        value: f64,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let threshold_f64: f64 = threshold.try_into().unwrap_or_default();
        let message = format!(
            "Alert '{}' fired: measured {:.4}, threshold {:.4}",
            alert.name, value, threshold_f64
        );

        let history_id = Uuid::new_v4();

        #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
        {
            let decimal_value = rust_decimal::Decimal::try_from(value).ok();
            sqlx::query(
                "INSERT INTO alert_history (id, alert_id, triggered_at, value, message, delivered)
                 VALUES ($1, $2, $3, $4, $5, FALSE)",
            )
            .bind(history_id)
            .bind(alert.id)
            .bind(now)
            .bind(decimal_value)
            .bind(&message)
            .execute(&self.pool)
            .await?;
        }

        #[cfg(feature = "sqlite")]
        {
            let value_str = format!("{value:.8}");
            sqlx::query(
                "INSERT INTO alert_history (id, alert_id, triggered_at, value, message, delivered)
                 VALUES ($1, $2, $3, $4, $5, 0)",
            )
            .bind(history_id)
            .bind(alert.id)
            .bind(now)
            .bind(value_str)
            .bind(&message)
            .execute(&self.pool)
            .await?;
        }

        // Dispatch to all configured channels; collect first error encountered.
        let mut delivery_errors: Vec<String> = Vec::new();

        // Channel 1: generic HTTP webhook.
        if let Some(ref url) = alert.webhook_url {
            let format = WebhookFormat::parse(&alert.webhook_format);
            let ctx = WebhookContext {
                alert_id: alert.id,
                alert_type: &alert.alert_type,
                alert_name: &alert.name,
                message: &message,
                value,
                threshold: threshold_f64,
                triggered_at: now,
            };
            if let Err(e) =
                webhook::deliver(&self.client, url, &format, alert.webhook_secret.as_deref(), &ctx)
                    .await
            {
                tracing::warn!(alert_id = %alert.id, "Webhook delivery failed: {e}");
                delivery_errors.push(format!("webhook: {e}"));
            }
        }

        // Channel 2: Slack block-kit.
        if let Some(ref url) = alert.slack_webhook_url {
            let ctx = SlackContext {
                alert_id: alert.id,
                alert_name: &alert.name,
                alert_type: &alert.alert_type,
                value,
                threshold: threshold_f64,
                triggered_at: now,
            };
            if let Err(e) = slack::send(&self.client, url, &ctx).await {
                tracing::warn!(alert_id = %alert.id, "Slack delivery failed: {e}");
                delivery_errors.push(format!("slack: {e}"));
            }
        }

        // Channel 3: email.
        let recipients = alert.email_recipients();
        if !recipients.is_empty() {
            let ctx = EmailContext {
                alert_name: &alert.name,
                alert_type: &alert.alert_type,
                value,
                threshold: threshold_f64,
                triggered_at: now,
            };
            if let Err(e) = email::send(&self.smtp, &recipients, &ctx).await {
                tracing::warn!(alert_id = %alert.id, "Email delivery failed: {e}");
                delivery_errors.push(format!("email: {e}"));
            }
        }

        let delivered = delivery_errors.is_empty();
        let delivery_error = if delivery_errors.is_empty() {
            None
        } else {
            Some(delivery_errors.join("; "))
        };

        sqlx::query("UPDATE alert_history SET delivered = $1, error = $2 WHERE id = $3")
            .bind(delivered)
            .bind(delivery_error.as_deref())
            .bind(history_id)
            .execute(&self.pool)
            .await?;

        sqlx::query("UPDATE alerts SET last_triggered = $1 WHERE id = $2")
            .bind(now)
            .bind(alert.id)
            .execute(&self.pool)
            .await?;

        tracing::info!(alert_id = %alert.id, value, delivered, "Alert fired");
        Ok(())
    }

    async fn check_spend_threshold(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<Option<f64>> {
        let cutoff = Utc::now() - chrono::Duration::minutes(window_minutes as i64);

        #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
        {
            let row: (Option<rust_decimal::Decimal>,) = sqlx::query_as(
                "SELECT COALESCE(SUM(cost_usd), 0) FROM requests
                 WHERE created_at >= $1 AND status = 'success'",
            )
            .bind(cutoff)
            .fetch_one(&self.pool)
            .await?;
            let total = row.0.unwrap_or_default();
            if total >= threshold {
                Ok(Some(total.try_into().unwrap_or_default()))
            } else {
                Ok(None)
            }
        }

        #[cfg(feature = "sqlite")]
        {
            let row: (Option<f64>,) = sqlx::query_as(
                "SELECT COALESCE(SUM(cost_usd), 0) FROM requests
                 WHERE created_at >= $1 AND status = 'success'",
            )
            .bind(cutoff)
            .fetch_one(&self.pool)
            .await?;
            let total = row.0.unwrap_or(0.0);
            let dec = rust_decimal::Decimal::try_from(total).unwrap_or_default();
            if dec >= threshold {
                Ok(Some(total))
            } else {
                Ok(None)
            }
        }
    }

    async fn check_error_rate(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<Option<f64>> {
        let cutoff = Utc::now() - chrono::Duration::minutes(window_minutes as i64);

        #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
        let row: (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE status = 'error'), COUNT(*)
             FROM requests WHERE created_at >= $1",
        )
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;

        #[cfg(feature = "sqlite")]
        let row: (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END), COUNT(*)
             FROM requests WHERE created_at >= $1",
        )
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;

        let errors = row.0.unwrap_or(0) as f64;
        let total = row.1.unwrap_or(0) as f64;
        if total == 0.0 {
            return Ok(None);
        }
        let rate = errors / total;
        let dec = rust_decimal::Decimal::try_from(rate).unwrap_or_default();
        if dec >= threshold {
            Ok(Some(rate))
        } else {
            Ok(None)
        }
    }

    async fn check_latency_spike(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<Option<f64>> {
        let cutoff = Utc::now() - chrono::Duration::minutes(window_minutes as i64);
        let row: (Option<i32>,) = sqlx::query_as(
            "SELECT MAX(latency_ms) FROM requests
             WHERE created_at >= $1 AND latency_ms IS NOT NULL",
        )
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;

        let max_ms = row.0.unwrap_or(0);
        let dec = rust_decimal::Decimal::from(max_ms);
        if dec >= threshold {
            Ok(Some(max_ms as f64))
        } else {
            Ok(None)
        }
    }
}
