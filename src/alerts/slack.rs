use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Data passed to the Slack block-kit formatter.
pub struct SlackContext<'a> {
    pub alert_id: Uuid,
    pub alert_name: &'a str,
    pub alert_type: &'a str,
    pub value: f64,
    pub threshold: f64,
    pub triggered_at: DateTime<Utc>,
}

/// POST a Slack block-kit payload to `url`.
///
/// Uses the native Slack blocks format (not just `text:`) so the message renders
/// with a header section and structured fields in the Slack client.
pub async fn send(client: &reqwest::Client, url: &str, ctx: &SlackContext<'_>) -> Result<()> {
    let payload = build_blocks(ctx);
    client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn build_blocks(ctx: &SlackContext<'_>) -> serde_json::Value {
    use serde_json::json;

    let header_text = format!("⚠️ Velox Alert: {}", ctx.alert_name);
    let time_str = ctx.triggered_at.format("%Y-%m-%d %H:%M UTC").to_string();
    let alert_type_display = ctx.alert_type.replace('_', " ");

    json!({
        "blocks": [
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": header_text,
                    "emoji": true
                }
            },
            {
                "type": "section",
                "fields": [
                    { "type": "mrkdwn", "text": format!("*Alert:* {}", ctx.alert_name) },
                    { "type": "mrkdwn", "text": format!("*Type:* {}", alert_type_display) },
                    { "type": "mrkdwn", "text": format!("*Measured:* {:.4}", ctx.value) },
                    { "type": "mrkdwn", "text": format!("*Threshold:* {:.4}", ctx.threshold) },
                    { "type": "mrkdwn", "text": format!("*Time:* {}", time_str) },
                    { "type": "mrkdwn", "text": format!("*ID:* `{}`", ctx.alert_id) }
                ]
            },
            {
                "type": "divider"
            }
        ]
    })
}
