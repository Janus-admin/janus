//! `velox config` — read and update runtime-mutable configuration via the admin API.

use anyhow::{bail, Context};
use clap::Subcommand;
use serde_json::{json, Value};

use super::{admin_client::AdminClient, CliResult};

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the current runtime configuration as JSON.
    Get,
    /// Update a config field. Accepts `key=value`. Supported keys match
    /// `PatchConfigRequest`: log_request_bodies, log_response_bodies,
    /// cache_enabled, max_retries, semantic_cache_threshold.
    Set {
        /// `key=value` pair.
        pair: String,
    },
}

pub async fn run(cmd: ConfigCmd, flag_url: Option<&str>, flag_token: Option<&str>) -> CliResult {
    let client = AdminClient::resolve(flag_url, flag_token)?;
    match cmd {
        ConfigCmd::Get => {
            let resp = client
                .request(reqwest::Method::GET, "/admin/config")
                .send()
                .await
                .context("GET /admin/config")?;
            if !resp.status().is_success() {
                bail!("admin API returned HTTP {}", resp.status().as_u16());
            }
            let body: Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
        ConfigCmd::Set { pair } => {
            let (key, value) = pair.split_once('=').context("expected `key=value` pair")?;
            let patch = build_patch(key, value)?;
            let resp = client
                .request(reqwest::Method::PATCH, "/admin/config")
                .json(&patch)
                .send()
                .await
                .context("PATCH /admin/config")?;
            if !resp.status().is_success() {
                bail!("admin API returned HTTP {}", resp.status().as_u16());
            }
            let body: Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
    }
}

fn build_patch(key: &str, value: &str) -> anyhow::Result<Value> {
    let parsed: Value = match key {
        "log_request_bodies" | "log_response_bodies" | "cache_enabled" => {
            Value::Bool(value.parse().context("expected true/false")?)
        }
        "max_retries" => Value::from(value.parse::<u32>().context("expected non-negative int")?),
        "semantic_cache_threshold" => {
            Value::from(value.parse::<f64>().context("expected float 0–1")?)
        }
        other => bail!("unknown config key: {other}"),
    };
    Ok(json!({ key: parsed }))
}
