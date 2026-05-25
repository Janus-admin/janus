use crate::config::SmtpConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use lettre::message::header::ContentType;
use lettre::AsyncTransport;

/// Data passed to the email formatter.
pub struct EmailContext<'a> {
    pub alert_name: &'a str,
    pub alert_type: &'a str,
    pub value: f64,
    pub threshold: f64,
    pub triggered_at: DateTime<Utc>,
}

/// Send an alert email to all addresses in `to`.
///
/// Dispatch path (first match wins):
///   1. `smtp.file_dir` non-empty → write .eml files (test / CI mode)
///   2. `smtp.host` non-empty     → send via SMTP STARTTLS
///   3. Neither set               → return `Err` (caller should log and skip)
///
/// Returns `Ok(())` immediately when `to` is empty.
pub async fn send(cfg: &SmtpConfig, to: &[String], ctx: &EmailContext<'_>) -> Result<()> {
    if to.is_empty() {
        return Ok(());
    }

    let subject = format!("Janus Alert: {}", ctx.alert_name);
    let body = build_body(ctx);

    if !cfg.file_dir.is_empty() {
        send_to_file(cfg, to, &subject, &body).await
    } else if !cfg.host.is_empty() {
        send_via_smtp(cfg, to, &subject, &body).await
    } else {
        anyhow::bail!("No SMTP host or file_dir configured for email alerts")
    }
}

fn build_body(ctx: &EmailContext<'_>) -> String {
    let time_str = ctx.triggered_at.format("%Y-%m-%d %H:%M UTC");
    format!(
        "Janus Alert Triggered\n\
         ═══════════════════════════════\n\
         Alert:     {}\n\
         Type:      {}\n\
         Measured:  {:.4}\n\
         Threshold: {:.4}\n\
         Time:      {}\n\
         ═══════════════════════════════\n\
         \n\
         Visit your Janus dashboard for details and history.\n",
        ctx.alert_name, ctx.alert_type, ctx.value, ctx.threshold, time_str
    )
}

fn from_address(cfg: &SmtpConfig) -> String {
    if cfg.from_address.is_empty() {
        if cfg.host.is_empty() {
            "janus@localhost".to_string()
        } else {
            format!("janus@{}", cfg.host)
        }
    } else {
        cfg.from_address.clone()
    }
}

async fn send_to_file(cfg: &SmtpConfig, to: &[String], subject: &str, body: &str) -> Result<()> {
    use lettre::transport::file::AsyncFileTransport;
    use lettre::{Message, Tokio1Executor};

    let from_str = from_address(cfg);
    let transport = AsyncFileTransport::<Tokio1Executor>::new(&cfg.file_dir);

    for recipient in to {
        let email = Message::builder()
            .from(from_str.parse()?)
            .to(recipient.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())?;

        transport.send(email).await?;
    }
    Ok(())
}

async fn send_via_smtp(cfg: &SmtpConfig, to: &[String], subject: &str, body: &str) -> Result<()> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, Message, Tokio1Executor};

    let from_str = from_address(cfg);
    let mut builder =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?.port(cfg.port);

    if !cfg.username.is_empty() {
        builder = builder.credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()));
    }
    let transport = builder.build();

    for recipient in to {
        let email = Message::builder()
            .from(from_str.parse()?)
            .to(recipient.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())?;

        transport.send(email).await?;
    }
    Ok(())
}
