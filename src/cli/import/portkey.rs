//! Portkey → Janus importer.
//!
//! Portkey's "Export Workspace" feature emits a JSON file shaped roughly like:
//!
//! ```json
//! {
//!   "virtual_keys": [
//!     { "name": "prod-key", "provider": "openai", "api_key": "sk-..." }
//!   ],
//!   "configs": [
//!     {
//!       "name": "prod",
//!       "strategy": {
//!         "mode": "fallback",
//!         "targets": [
//!           { "provider": "openai",    "api_key": "sk-..." },
//!           { "provider": "anthropic", "api_key": "sk-ant-..." }
//!         ]
//!       },
//!       "cache": { "mode": "semantic" }
//!     }
//!   ]
//! }
//! ```
//!
//! Mapping (matches JANUS_V5_ROADMAP.md §5.4):
//! - virtual key  → Janus api_key (new jn-sk-…, the Portkey secret is not reused)
//! - target / virtual key.provider → enable the matching Janus provider with that api_key
//! - strategy.mode → Janus routing strategy (`fallback` → `priority`, `loadbalance` → `round_robin`)
//! - cache.mode    → `PATCH /admin/config` `cache_enabled = true`

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::plan::{ApiKeySpec, ConfigPatch, MigrationPlan, ProviderPatch};
use crate::cli::{admin_client::AdminClient, CliResult};

// ── JSON schema ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct PortkeyExport {
    #[serde(default)]
    pub virtual_keys: Vec<VirtualKey>,
    #[serde(default)]
    pub configs: Vec<PortkeyConfig>,
}

#[derive(Debug, Deserialize)]
pub struct VirtualKey {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PortkeyConfig {
    pub name: String,
    #[serde(default)]
    pub strategy: Option<Strategy>,
    #[serde(default)]
    pub cache: Option<CacheBlock>,
}

#[derive(Debug, Deserialize)]
pub struct Strategy {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub weight: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct CacheBlock {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub similarity_threshold: Option<f32>,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn parse_json(json: &str) -> Result<PortkeyExport> {
    serde_json::from_str::<PortkeyExport>(json).context("parse Portkey export JSON")
}

pub fn plan_from_export(export: &PortkeyExport) -> MigrationPlan {
    let mut plan = MigrationPlan::new("portkey");

    for vk in &export.virtual_keys {
        let provider_id = normalize_provider(&vk.provider);
        if !is_known_provider(&provider_id) {
            plan.notes.push(format!(
                "virtual key `{}`: provider `{}` is not pre-seeded — skipped",
                vk.name, vk.provider
            ));
            continue;
        }
        plan.providers.push(ProviderPatch {
            id: provider_id,
            api_key: vk.api_key.clone(),
            base_url: None,
            priority: Some(1),
            is_enabled: Some(true),
        });
        plan.keys.push(ApiKeySpec {
            name: vk.name.clone(),
            routing_strategy: "priority".to_string(),
            budget_limit: None,
            rate_limit_rpm: None,
            rate_limit_tpm: None,
        });
    }

    for (cfg_idx, cfg) in export.configs.iter().enumerate() {
        let strategy = cfg.strategy.as_ref();
        let routing = strategy
            .and_then(|s| s.mode.as_deref())
            .map(map_routing_strategy)
            .unwrap_or("priority")
            .to_string();

        if let Some(s) = strategy {
            for (idx, target) in s.targets.iter().enumerate() {
                let provider_id = normalize_provider(&target.provider);
                if !is_known_provider(&provider_id) {
                    plan.notes.push(format!(
                        "config[{cfg_idx}].targets[{idx}]: provider `{}` is not pre-seeded — skipped",
                        target.provider
                    ));
                    continue;
                }
                plan.providers.push(ProviderPatch {
                    id: provider_id,
                    api_key: target.api_key.clone(),
                    base_url: target.base_url.clone(),
                    priority: Some((idx as i32) + 1),
                    is_enabled: Some(true),
                });
            }
        }

        plan.keys.push(ApiKeySpec {
            name: cfg.name.clone(),
            routing_strategy: routing,
            budget_limit: None,
            rate_limit_rpm: None,
            rate_limit_tpm: None,
        });

        if let Some(cache) = &cfg.cache {
            let mut patch = ConfigPatch::default();
            if cache.mode.is_some() {
                patch.cache_enabled = Some(true);
            }
            if let Some(thr) = cache.similarity_threshold {
                patch.semantic_cache_threshold = Some(thr);
            }
            // Multiple Portkey configs may toggle cache; last write wins, matching
            // how `PATCH /admin/config` behaves anyway.
            if !patch.is_empty() {
                plan.config = patch;
            }
        }
    }

    plan.finalize()
}

pub async fn run(
    path: std::path::PathBuf,
    apply: bool,
    flag_url: Option<&str>,
    flag_token: Option<&str>,
) -> CliResult {
    let plan = plan_from_path(&path)?;
    if !apply {
        print!("{}", plan.render_preview());
        return Ok(());
    }
    let client = AdminClient::resolve(flag_url, flag_token)?;
    let outcome = super::plan::apply_plan(&client, &plan).await?;
    println!(
        "portkey import: {} providers updated, {} keys created, config patched: {}",
        outcome.providers_updated, outcome.keys_created, outcome.config_patched
    );
    for e in &outcome.errors {
        eprintln!("  warning: {e}");
    }
    Ok(())
}

pub fn plan_from_path(path: &Path) -> Result<MigrationPlan> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("read Portkey export {}", path.display()))?;
    let export = parse_json(&json)?;
    Ok(plan_from_export(&export))
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn normalize_provider(s: &str) -> String {
    match s.to_ascii_lowercase().as_str() {
        "openai" | "azure-openai" | "azure" => "openai".to_string(),
        "anthropic" => "anthropic".to_string(),
        "bedrock" | "aws-bedrock" => "bedrock".to_string(),
        "groq" => "groq".to_string(),
        "deepseek" => "deepseek".to_string(),
        "google" | "google-ai" | "gemini" | "vertex-ai" => "gemini".to_string(),
        other => other.to_string(),
    }
}

fn is_known_provider(id: &str) -> bool {
    matches!(
        id,
        "openai" | "anthropic" | "bedrock" | "groq" | "deepseek" | "gemini"
    )
}

/// Portkey strategy modes → Janus routing strategies.
pub fn map_routing_strategy(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "loadbalance" | "load_balance" | "weighted" => "round_robin",
        "fallback" | "single" => "priority",
        "lowest-latency" | "latency" => "latency",
        "lowest-cost" | "cost" => "cost",
        _ => "priority",
    }
}
