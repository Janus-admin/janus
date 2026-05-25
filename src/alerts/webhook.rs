use anyhow::Result;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

/// Which payload shape to use when POSTing to the webhook URL.
#[derive(Debug, Clone, PartialEq)]
pub enum WebhookFormat {
    Slack,
    Discord,
    Generic,
}

impl WebhookFormat {
    pub fn parse(s: &str) -> Self {
        match s {
            "slack" => Self::Slack,
            "discord" => Self::Discord,
            _ => Self::Generic,
        }
    }
}

/// Data about the firing alert passed to webhook delivery.
pub struct WebhookContext<'a> {
    pub alert_id: Uuid,
    pub alert_type: &'a str,
    pub alert_name: &'a str,
    pub message: &'a str,
    /// Measured metric value that exceeded the threshold.
    pub value: f64,
    pub threshold: f64,
    pub triggered_at: DateTime<Utc>,
}

/// POST the alert payload to `url` using the given format.
///
/// When `secret` is provided, a `X-Janus-Signature` HMAC-SHA256 header is added.
/// Returns `Err` if the HTTP request fails or the server returns a non-2xx status.
pub async fn deliver(
    client: &reqwest::Client,
    url: &str,
    format: &WebhookFormat,
    secret: Option<&str>,
    ctx: &WebhookContext<'_>,
) -> Result<()> {
    let body = build_payload(format, ctx);
    let body_str = serde_json::to_string(&body)?;

    let mut builder = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body_str.clone());

    if let Some(s) = secret {
        builder = builder.header("X-Janus-Signature", sign(s, &body_str));
    }

    builder.send().await?.error_for_status()?;
    Ok(())
}

fn build_payload(format: &WebhookFormat, ctx: &WebhookContext<'_>) -> serde_json::Value {
    use serde_json::json;
    let summary = format!(
        "Janus Alert: {} exceeded. Value: {:.4} / threshold: {:.4}",
        ctx.alert_type, ctx.value, ctx.threshold
    );
    match format {
        WebhookFormat::Slack => json!({ "text": format!("🚨 {}", summary) }),
        WebhookFormat::Discord => json!({ "content": format!("🚨 {}", summary) }),
        WebhookFormat::Generic => json!({
            "alert_id":     ctx.alert_id,
            "type":         ctx.alert_type,
            "name":         ctx.alert_name,
            "message":      ctx.message,
            "value":        ctx.value,
            "threshold":    ctx.threshold,
            "triggered_at": ctx.triggered_at.to_rfc3339(),
        }),
    }
}

/// Compute HMAC-SHA256 over `body` using `secret`, returned as lowercase hex.
pub fn sign(secret: &str, body: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body.as_bytes());
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
