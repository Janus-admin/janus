//! LiteLLM → Janus importer.
//!
//! LiteLLM's `proxy_config.yaml` has three top-level sections we care about:
//!
//! ```yaml
//! model_list:
//!   - model_name: gpt-4o
//!     litellm_params:
//!       model: openai/gpt-4o
//!       api_key: sk-xxx
//!       api_base: https://api.openai.com/v1
//!
//! general_settings: { master_key: sk-1234 }
//!
//! litellm_settings:
//!   cache: true
//!   cache_params: { similarity_threshold: 0.85 }
//!   router_settings: { routing_strategy: simple-shuffle }
//! ```
//!
//! Mapping (matches JANUS_V5_ROADMAP.md §5.4):
//! - `litellm_params.model` prefix → Janus provider id
//! - `litellm_params.api_key`     → provider `api_key`
//! - `litellm_params.api_base`    → provider `base_url`
//! - `litellm_settings.cache`     → `PATCH /admin/config` `cache_enabled`
//! - `routing_strategy` string    → Janus routing strategy (see [`map_routing_strategy`])

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::plan::{ApiKeySpec, ConfigPatch, MigrationPlan, ProviderPatch};
use crate::cli::{admin_client::AdminClient, CliResult};

// ── YAML schema ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct LiteLLMConfig {
    #[serde(default)]
    pub model_list: Vec<ModelEntry>,
    #[serde(default)]
    pub general_settings: GeneralSettings,
    #[serde(default)]
    pub litellm_settings: LitellmSettings,
    #[serde(default)]
    pub router_settings: RouterSettings,
}

#[derive(Debug, Deserialize)]
pub struct ModelEntry {
    pub model_name: String,
    #[serde(default)]
    pub litellm_params: LitellmParams,
}

#[derive(Debug, Deserialize, Default)]
pub struct LitellmParams {
    /// Always `<provider>/<model>`. LiteLLM's source of truth for which provider to talk to.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub aws_region_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GeneralSettings {
    #[serde(default)]
    pub master_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct LitellmSettings {
    #[serde(default)]
    pub cache: Option<bool>,
    #[serde(default)]
    pub cache_params: CacheParams,
}

#[derive(Debug, Deserialize, Default)]
pub struct CacheParams {
    #[serde(default)]
    pub similarity_threshold: Option<f32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RouterSettings {
    #[serde(default)]
    pub routing_strategy: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn parse_yaml(yaml: &str) -> Result<LiteLLMConfig> {
    serde_yaml::from_str::<LiteLLMConfig>(yaml).context("parse LiteLLM YAML")
}

/// Translate a parsed LiteLLM config into a `MigrationPlan`.
pub fn plan_from_config(cfg: &LiteLLMConfig) -> MigrationPlan {
    let mut plan = MigrationPlan::new("litellm");

    for (idx, entry) in cfg.model_list.iter().enumerate() {
        let (provider_id, _) = match split_provider_model(&entry.litellm_params.model) {
            Some(v) => v,
            None => {
                plan.notes.push(format!(
                    "model_list[{idx}]: cannot infer provider from `{}` — skipped",
                    entry.litellm_params.model
                ));
                continue;
            }
        };

        if !is_known_provider(&provider_id) {
            plan.notes.push(format!(
                "model_list[{idx}]: provider `{}` has no pre-seeded Janus row \
                 — enable manually or add to migrations",
                provider_id
            ));
            continue;
        }

        plan.providers.push(ProviderPatch {
            id: provider_id,
            api_key: entry.litellm_params.api_key.clone(),
            base_url: entry.litellm_params.api_base.clone(),
            priority: Some((idx as i32) + 1),
            is_enabled: Some(true),
        });
    }

    if cfg.general_settings.master_key.is_some() {
        plan.keys.push(ApiKeySpec {
            name: "litellm-master-key".to_string(),
            routing_strategy: map_routing_strategy(cfg.router_settings.routing_strategy.as_deref())
                .to_string(),
            budget_limit: None,
            rate_limit_rpm: None,
            rate_limit_tpm: None,
        });
        plan.notes.push(
            "LiteLLM `master_key` was present in source config. \
             A fresh Janus key (jn-sk-…) will be created — the LiteLLM key is not reusable."
                .into(),
        );
    }

    let mut config = ConfigPatch::default();
    if let Some(enabled) = cfg.litellm_settings.cache {
        config.cache_enabled = Some(enabled);
    }
    if let Some(thr) = cfg.litellm_settings.cache_params.similarity_threshold {
        config.semantic_cache_threshold = Some(thr);
    }
    plan.config = config;

    if let Some(strat) = &cfg.router_settings.routing_strategy {
        if map_routing_strategy(Some(strat)) == "priority" && strat != "priority" {
            plan.notes.push(format!(
                "routing_strategy `{strat}` has no exact Janus equivalent — \
                 defaulted to `priority`. Edit the resulting key if needed."
            ));
        }
    }

    plan.finalize()
}

/// Parse a LiteLLM file and run the CLI flow (preview or apply).
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
        "litellm import: {} providers updated, {} keys created, config patched: {}",
        outcome.providers_updated, outcome.keys_created, outcome.config_patched
    );
    for e in &outcome.errors {
        eprintln!("  warning: {e}");
    }
    Ok(())
}

pub fn plan_from_path(path: &Path) -> Result<MigrationPlan> {
    let yaml = std::fs::read_to_string(path)
        .with_context(|| format!("read LiteLLM config {}", path.display()))?;
    let cfg = parse_yaml(&yaml)?;
    Ok(plan_from_config(&cfg))
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// LiteLLM model strings are always `<provider>/<rest>`. `bedrock/anthropic.claude…`
/// produces `("bedrock", "anthropic.claude…")`. Strings without `/` are rejected so
/// we never silently mis-route to the wrong provider.
fn split_provider_model(model: &str) -> Option<(String, String)> {
    let (lhs, rhs) = model.split_once('/')?;
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    let normalized = match lhs.to_ascii_lowercase().as_str() {
        "azure" => "openai".to_string(),
        "google" | "vertex_ai" | "vertexai" => "gemini".to_string(),
        other => other.to_string(),
    };
    Some((normalized, rhs.to_string()))
}

fn is_known_provider(id: &str) -> bool {
    matches!(
        id,
        "openai" | "anthropic" | "bedrock" | "groq" | "deepseek" | "gemini"
    )
}

/// LiteLLM router strategies → Janus routing strategies.
///
/// The unknown-string fallback is `priority` because that is also Janus's
/// default — see JANUS_ROADMAP.md decision #5.
pub fn map_routing_strategy(s: Option<&str>) -> &'static str {
    match s.unwrap_or("").to_ascii_lowercase().as_str() {
        "simple-shuffle" | "loadbalance" => "round_robin",
        "least-busy" | "latency-based-routing" | "lowest-latency" => "latency",
        "usage-based-routing" | "usage-based-routing-v2" | "lowest-cost" | "cost-based-routing" => {
            "cost"
        }
        _ => "priority",
    }
}
