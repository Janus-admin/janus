//! Shared types for `janus import`.
//!
//! Importers translate competitor configs into a `MigrationPlan`, which is the
//! sole DTO that drives both `--dry-run` printing and `--apply` execution. Each
//! importer is responsible for parsing — the plan is the only thing that talks
//! to the admin API.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::admin_client::AdminClient;

/// Per-provider patch destined for `PATCH /admin/providers/:id`.
///
/// Only fields the importer wants to change are populated. The admin API
/// leaves every other field on the pre-seeded provider untouched.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProviderPatch {
    /// Pre-seeded provider id: `openai`, `anthropic`, `bedrock`, `groq`, `deepseek`, `gemini`.
    pub id: String,
    /// Provider API key (plaintext on the wire; encrypted at rest by the admin handler).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Override base URL (Azure-style endpoints, Bedrock VPC endpoints, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Lower integer = higher priority in the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Whether the provider is selectable by the router.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_enabled: Option<bool>,
}

/// API key to be created via `POST /admin/keys`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiKeySpec {
    pub name: String,
    /// One of `priority`, `cost`, `latency`, `round_robin` — Janus routing strategies.
    pub routing_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_rpm: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_tpm: Option<i32>,
}

/// Subset of `PATCH /admin/config` reachable from a competitor config.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ConfigPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_cache_threshold: Option<f32>,
}

impl ConfigPatch {
    pub fn is_empty(&self) -> bool {
        self.cache_enabled.is_none() && self.semantic_cache_threshold.is_none()
    }
}

/// The structured plan an importer produces.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MigrationPlan {
    /// Human-readable importer name, e.g. "litellm".
    pub source: String,
    /// Each pre-seeded provider that the source touched.
    pub providers: Vec<ProviderPatch>,
    /// API keys to create.
    pub keys: Vec<ApiKeySpec>,
    /// Runtime config changes (cache toggle etc.).
    pub config: ConfigPatch,
    /// Human-readable notes that don't map cleanly (e.g. unknown LiteLLM strategies).
    pub notes: Vec<String>,
}

impl MigrationPlan {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            ..Default::default()
        }
    }

    /// Sorted-by-id, deduplicated provider list. Importers can call this after
    /// emitting one patch per source-row.
    pub fn finalize(mut self) -> Self {
        self.providers.sort_by(|a, b| a.id.cmp(&b.id));
        self.providers.dedup_by(|b, a| {
            if a.id != b.id {
                return false;
            }
            // Merge b into a, then dedup removes b.
            if a.api_key.is_none() {
                a.api_key = b.api_key.take();
            }
            if a.base_url.is_none() {
                a.base_url = b.base_url.take();
            }
            if a.priority.is_none() {
                a.priority = b.priority;
            }
            if a.is_enabled.is_none() {
                a.is_enabled = b.is_enabled;
            }
            true
        });
        self
    }

    /// Render a human-friendly preview for `--dry-run`.
    pub fn render_preview(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("janus import {}: dry run\n", self.source));
        out.push_str(&format!(
            "  providers to update: {}\n",
            self.providers.len()
        ));
        for p in &self.providers {
            let mut bits: Vec<String> = vec![];
            if p.is_enabled == Some(true) {
                bits.push("enable".into());
            }
            if let Some(prio) = p.priority {
                bits.push(format!("priority={prio}"));
            }
            if p.api_key.is_some() {
                bits.push("api_key=<redacted>".into());
            }
            if let Some(url) = &p.base_url {
                bits.push(format!("base_url={url}"));
            }
            out.push_str(&format!("    - {}: {}\n", p.id, bits.join(", ")));
        }
        out.push_str(&format!("  api keys to create: {}\n", self.keys.len()));
        for k in &self.keys {
            out.push_str(&format!(
                "    - {} (strategy={})\n",
                k.name, k.routing_strategy
            ));
        }
        if !self.config.is_empty() {
            out.push_str("  config patch:\n");
            if let Some(v) = self.config.cache_enabled {
                out.push_str(&format!("    cache_enabled = {v}\n"));
            }
            if let Some(v) = self.config.semantic_cache_threshold {
                out.push_str(&format!("    semantic_cache_threshold = {v}\n"));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("  notes:\n");
            for n in &self.notes {
                out.push_str(&format!("    - {n}\n"));
            }
        }
        out
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct ApplyOutcome {
    pub providers_updated: usize,
    pub keys_created: usize,
    pub config_patched: bool,
    pub errors: Vec<String>,
}

/// Push a plan to a running Janus via the admin API.
///
/// On HTTP errors the function continues (collects each into `errors`) so a
/// partially-successful import still creates as much as it can. The caller
/// decides whether to fail hard.
pub async fn apply_plan(client: &AdminClient, plan: &MigrationPlan) -> Result<ApplyOutcome> {
    let mut out = ApplyOutcome::default();

    for p in &plan.providers {
        let body = serde_json::to_value(p).context("serialise provider patch")?;
        let resp = client
            .request(
                reqwest::Method::PATCH,
                &format!("/admin/providers/{}", p.id),
            )
            .json(&body)
            .send()
            .await
            .with_context(|| format!("PATCH /admin/providers/{}", p.id))?;
        if resp.status().is_success() {
            out.providers_updated += 1;
        } else {
            out.errors.push(format!(
                "provider {}: HTTP {}",
                p.id,
                resp.status().as_u16()
            ));
        }
    }

    for k in &plan.keys {
        let body = json!({
            "name": k.name,
            "routing_strategy": k.routing_strategy,
            "budget_limit": k.budget_limit,
            "rate_limit_rpm": k.rate_limit_rpm,
            "rate_limit_tpm": k.rate_limit_tpm,
        });
        let resp = client
            .request(reqwest::Method::POST, "/admin/keys")
            .json(&body)
            .send()
            .await
            .context("POST /admin/keys")?;
        if resp.status().is_success() {
            out.keys_created += 1;
        } else {
            out.errors
                .push(format!("key {}: HTTP {}", k.name, resp.status().as_u16()));
        }
    }

    if !plan.config.is_empty() {
        let body: Value = serde_json::to_value(&plan.config).context("serialise config patch")?;
        let resp = client
            .request(reqwest::Method::PATCH, "/admin/config")
            .json(&body)
            .send()
            .await
            .context("PATCH /admin/config")?;
        if resp.status().is_success() {
            out.config_patched = true;
        } else {
            out.errors
                .push(format!("config patch: HTTP {}", resp.status().as_u16()));
        }
    }

    if !out.errors.is_empty() && out.providers_updated == 0 && out.keys_created == 0 {
        bail!("import failed: every admin call returned an error");
    }
    Ok(out)
}
