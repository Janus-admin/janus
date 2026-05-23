use crate::db::DbPool;
use chrono::Utc;
use uuid::Uuid;

/// Row type returned by the alert SELECT query.
#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct AlertRow {
    id: Uuid,
    alert_type: String,
    threshold: rust_decimal::Decimal,
    window_minutes: i32,
    last_triggered: Option<chrono::DateTime<Utc>>,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct AlertRow {
    id: Uuid,
    alert_type: String,
    // threshold stored as TEXT in SQLite; parsed to Decimal in evaluate()
    threshold: String,
    window_minutes: i32,
    last_triggered: Option<chrono::DateTime<Utc>>,
}

/// Background alert evaluation engine.
///
/// Runs every 60 seconds and checks all active alerts against live request data.
/// Fires an alert by updating `last_triggered` when a threshold is breached.
/// Webhook delivery is added in Phase V2-2.
pub struct AlertEngine {
    pool: DbPool,
}

impl AlertEngine {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn evaluate(&self) -> anyhow::Result<()> {
        let alerts = sqlx::query_as::<_, AlertRow>(
            "SELECT id, type AS alert_type, threshold, window_minutes, last_triggered
             FROM alerts
             WHERE is_active = TRUE",
        )
        .fetch_all(&self.pool)
        .await?;

        for alert in alerts {
            // Cooldown: do not re-fire within the same window.
            if let Some(last) = alert.last_triggered {
                let elapsed_minutes = (Utc::now() - last).num_minutes();
                if elapsed_minutes < alert.window_minutes as i64 {
                    continue;
                }
            }

            // Normalise threshold to Decimal regardless of backend.
            #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
            let threshold = alert.threshold;
            #[cfg(feature = "sqlite")]
            let threshold: rust_decimal::Decimal = alert.threshold.parse().unwrap_or_default();

            let fired = match alert.alert_type.as_str() {
                "spend_threshold" => self
                    .check_spend_threshold(alert.window_minutes, threshold)
                    .await
                    .unwrap_or(false),
                "error_rate" => self
                    .check_error_rate(alert.window_minutes, threshold)
                    .await
                    .unwrap_or(false),
                "latency_spike" => self
                    .check_latency_spike(alert.window_minutes, threshold)
                    .await
                    .unwrap_or(false),
                other => {
                    tracing::warn!("Unknown alert type: {}", other);
                    false
                }
            };

            if fired {
                self.fire(alert.id).await?;
            }
        }

        Ok(())
    }

    async fn fire(&self, alert_id: Uuid) -> anyhow::Result<()> {
        let now = Utc::now();
        sqlx::query("UPDATE alerts SET last_triggered = $1 WHERE id = $2")
            .bind(now)
            .bind(alert_id)
            .execute(&self.pool)
            .await?;
        tracing::info!(%alert_id, "Alert fired");
        Ok(())
    }

    async fn check_spend_threshold(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<bool> {
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
            Ok(row.0.unwrap_or_default() >= threshold)
        }

        #[cfg(feature = "sqlite")]
        {
            // SQLite: SUM over a TEXT column returns REAL.
            let row: (Option<f64>,) = sqlx::query_as(
                "SELECT COALESCE(SUM(cost_usd), 0) FROM requests
                 WHERE created_at >= $1 AND status = 'success'",
            )
            .bind(cutoff)
            .fetch_one(&self.pool)
            .await?;
            let total =
                rust_decimal::Decimal::try_from(row.0.unwrap_or(0.0)).unwrap_or_default();
            Ok(total >= threshold)
        }
    }

    async fn check_error_rate(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<bool> {
        let cutoff = Utc::now() - chrono::Duration::minutes(window_minutes as i64);
        let row: (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE status = 'error'), COUNT(*)
             FROM requests WHERE created_at >= $1",
        )
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;

        let errors = row.0.unwrap_or(0) as f64;
        let total = row.1.unwrap_or(0) as f64;
        if total == 0.0 {
            return Ok(false);
        }
        let rate = rust_decimal::Decimal::try_from(errors / total).unwrap_or_default();
        Ok(rate >= threshold)
    }

    async fn check_latency_spike(
        &self,
        window_minutes: i32,
        threshold: rust_decimal::Decimal,
    ) -> anyhow::Result<bool> {
        let cutoff = Utc::now() - chrono::Duration::minutes(window_minutes as i64);
        // latency_ms is INTEGER (INT4 in PG, INTEGER in SQLite); MAX returns the same type.
        // Use i32 — fits both PG INT4 and SQLite INTEGER for any realistic latency value.
        let row: (Option<i32>,) = sqlx::query_as(
            "SELECT MAX(latency_ms) FROM requests
             WHERE created_at >= $1 AND latency_ms IS NOT NULL",
        )
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?;

        let max_ms = rust_decimal::Decimal::from(row.0.unwrap_or(0));
        Ok(max_ms >= threshold)
    }
}
