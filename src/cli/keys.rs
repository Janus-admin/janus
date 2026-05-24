//! `velox keys` subcommands. All go through the admin API.

use anyhow::{bail, Context};
use clap::Subcommand;
use serde_json::{json, Value};
use uuid::Uuid;

use super::{admin_client::AdminClient, CliResult};

#[derive(Subcommand, Debug)]
pub enum KeysCmd {
    /// List API keys.
    List,
    /// Create an API key. Prints the full secret to stdout once.
    Create {
        /// Human-readable name.
        #[arg(long)]
        name: String,
        /// USD budget cap. Omit for unlimited.
        #[arg(long)]
        budget: Option<f64>,
        /// Requests-per-minute cap.
        #[arg(long)]
        rpm: Option<i32>,
        /// Tokens-per-minute cap.
        #[arg(long)]
        tpm: Option<i32>,
    },
    /// Rotate a key — issues a new secret. The old secret stays valid until the grace window expires.
    Rotate {
        /// Key UUID.
        id: Uuid,
    },
    /// Revoke (deactivate) a key.
    Revoke {
        /// Key UUID.
        id: Uuid,
    },
}

pub async fn run(
    cmd: KeysCmd,
    flag_url: Option<&str>,
    flag_token: Option<&str>,
) -> CliResult {
    let client = AdminClient::resolve(flag_url, flag_token)?;
    match cmd {
        KeysCmd::List => list(&client).await,
        KeysCmd::Create {
            name,
            budget,
            rpm,
            tpm,
        } => create(&client, &name, budget, rpm, tpm).await,
        KeysCmd::Rotate { id } => rotate(&client, id).await,
        KeysCmd::Revoke { id } => revoke(&client, id).await,
    }
}

async fn list(client: &AdminClient) -> CliResult {
    let resp = client
        .request(reqwest::Method::GET, "/admin/keys")
        .send()
        .await
        .context("GET /admin/keys")?;
    check_status(&resp, "list keys")?;
    let body: Value = resp.json().await?;
    let rows = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!(
        "{:<38} {:<24} {:<14} {:<8} {:<10}",
        "ID", "NAME", "PREFIX", "ACTIVE", "STRATEGY"
    );
    for r in rows {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("");
        let name = r.get("name").and_then(Value::as_str).unwrap_or("");
        let prefix = r.get("key_prefix").and_then(Value::as_str).unwrap_or("");
        let active = r.get("is_active").and_then(Value::as_bool).unwrap_or(false);
        let strat = r
            .get("routing_strategy")
            .and_then(Value::as_str)
            .unwrap_or("");
        println!(
            "{id:<38} {name:<24} {prefix:<14} {active:<8} {strat:<10}",
            active = if active { "yes" } else { "no" }
        );
    }
    Ok(())
}

async fn create(
    client: &AdminClient,
    name: &str,
    budget: Option<f64>,
    rpm: Option<i32>,
    tpm: Option<i32>,
) -> CliResult {
    let body = json!({
        "name": name,
        "budget_limit": budget,
        "rate_limit_rpm": rpm,
        "rate_limit_tpm": tpm,
        "routing_strategy": "priority",
    });
    let resp = client
        .request(reqwest::Method::POST, "/admin/keys")
        .json(&body)
        .send()
        .await
        .context("POST /admin/keys")?;
    check_status(&resp, "create key")?;
    let body: Value = resp.json().await?;
    let data = body.get("data").cloned().unwrap_or_default();
    let full = data.get("key").and_then(Value::as_str).unwrap_or("");
    let id = data.get("id").and_then(Value::as_str).unwrap_or("");
    println!("id:       {id}");
    println!("key:      {full}");
    println!("(this secret is shown ONCE — save it now)");
    Ok(())
}

async fn rotate(client: &AdminClient, id: Uuid) -> CliResult {
    let resp = client
        .request(reqwest::Method::POST, &format!("/admin/keys/{id}/rotate"))
        .send()
        .await
        .context("POST /admin/keys/{id}/rotate")?;
    check_status(&resp, "rotate key")?;
    let body: Value = resp.json().await?;
    let data = body.get("data").cloned().unwrap_or_default();
    let new_key = data.get("key").and_then(Value::as_str).unwrap_or("");
    let expires_at = data
        .get("rotation_expires_at")
        .and_then(Value::as_str)
        .unwrap_or("");
    println!("new key:    {new_key}");
    println!("grace ends: {expires_at}");
    Ok(())
}

async fn revoke(client: &AdminClient, id: Uuid) -> CliResult {
    let resp = client
        .request(reqwest::Method::DELETE, &format!("/admin/keys/{id}"))
        .send()
        .await
        .context("DELETE /admin/keys/{id}")?;
    check_status(&resp, "revoke key")?;
    println!("revoked {id}");
    Ok(())
}

fn check_status(resp: &reqwest::Response, op: &str) -> CliResult {
    if resp.status().is_success() {
        Ok(())
    } else {
        bail!(
            "{op} failed: HTTP {} from admin API",
            resp.status().as_u16()
        );
    }
}
