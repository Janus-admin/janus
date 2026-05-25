//! OpenRouter → Janus model-alias report.
//!
//! OpenRouter has no equivalent of Janus's pre-seeded providers — it is itself
//! a gateway. The useful import action is therefore *discovery*: fetch the
//! OpenRouter model list and emit a Janus-friendly table mapping each
//! `<provider>/<model>` to the Janus provider id you would enable.
//!
//! `GET https://openrouter.ai/api/v1/models` →
//! ```json
//! { "data": [
//!     { "id": "openai/gpt-4o",
//!       "name": "OpenAI: GPT-4o",
//!       "context_length": 128000,
//!       "pricing": { "prompt": "5.0", "completion": "15.0" } }
//! ]}
//! ```
//!
//! No DB writes occur — this importer is read-only by design. The output
//! doubles as input for JANUS_V5_ROADMAP.md §5.4: "extend `model_pricing`
//! seeds with OpenRouter rows" (a manual migration step until V6 adds a model
//! registration endpoint).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::CliResult;

#[derive(Debug, Deserialize)]
pub struct OpenRouterListing {
    pub data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OpenRouterModel {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub pricing: Option<Pricing>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Pricing {
    /// Per-token rate. OpenRouter returns this as a string ("0.0000025").
    /// We keep it as `String` so the report shows exactly what OpenRouter said.
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelAlias {
    /// OpenRouter id, e.g. `openai/gpt-4o`.
    pub openrouter_id: String,
    /// Janus pre-seeded provider id, or `None` for OpenRouter-only providers.
    pub janus_provider: Option<String>,
    /// The model name after the `/` — what you would send as `model` to Janus.
    pub model: String,
    /// Display name from OpenRouter, if any.
    pub display_name: Option<String>,
    pub context_length: Option<i64>,
    pub prompt_price: Option<String>,
    pub completion_price: Option<String>,
}

/// Group OpenRouter models by inferred Janus provider id.
#[derive(Debug, Default, Serialize)]
pub struct AliasReport {
    pub aliases: Vec<ModelAlias>,
    /// Pre-seeded providers seen → model count, sorted alphabetically.
    pub provider_counts: BTreeMap<String, usize>,
    /// IDs of OpenRouter-only "providers" (e.g. `openrouter/auto`) that have no
    /// Janus row. Useful for capacity planning.
    pub unmapped_providers: Vec<String>,
}

impl AliasReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "openrouter import: {} models, {} mapped provider(s)\n",
            self.aliases.len(),
            self.provider_counts.len()
        ));
        for (p, n) in &self.provider_counts {
            out.push_str(&format!("  {p}: {n} models\n"));
        }
        if !self.unmapped_providers.is_empty() {
            out.push_str("  unmapped (no Janus provider row):\n");
            for p in &self.unmapped_providers {
                out.push_str(&format!("    - {p}\n"));
            }
        }
        out.push('\n');
        out.push_str(&format!(
            "{:<40} {:<10} {:<28} CONTEXT  IN/OUT $/M\n",
            "MODEL", "PROVIDER", "DISPLAY"
        ));
        for a in &self.aliases {
            let provider = a.janus_provider.as_deref().unwrap_or("—");
            let display = a.display_name.as_deref().unwrap_or("");
            let ctx = a
                .context_length
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".into());
            let p = a.prompt_price.as_deref().unwrap_or("—");
            let c = a.completion_price.as_deref().unwrap_or("—");
            out.push_str(&format!(
                "{:<40} {:<10} {:<28} {:<8} {}/{}\n",
                truncate(&a.openrouter_id, 40),
                provider,
                truncate(display, 28),
                ctx,
                p,
                c
            ));
        }
        out
    }
}

pub fn build_aliases(listing: &OpenRouterListing) -> AliasReport {
    let mut report = AliasReport::default();
    let mut unmapped_set: std::collections::BTreeSet<String> = Default::default();

    for m in &listing.data {
        let (janus_provider, model) = match m.id.split_once('/') {
            Some((lhs, rhs)) if !lhs.is_empty() && !rhs.is_empty() => {
                let normalized = normalize_openrouter_provider(lhs);
                if is_known_provider(&normalized) {
                    *report
                        .provider_counts
                        .entry(normalized.clone())
                        .or_insert(0) += 1;
                    (Some(normalized), rhs.to_string())
                } else {
                    unmapped_set.insert(lhs.to_string());
                    (None, rhs.to_string())
                }
            }
            _ => (None, m.id.clone()),
        };

        let pricing = m.pricing.as_ref();
        report.aliases.push(ModelAlias {
            openrouter_id: m.id.clone(),
            janus_provider,
            model,
            display_name: m.name.clone(),
            context_length: m.context_length,
            prompt_price: pricing.and_then(|p| p.prompt.clone()),
            completion_price: pricing.and_then(|p| p.completion.clone()),
        });
    }

    report.unmapped_providers = unmapped_set.into_iter().collect();
    report
}

/// Parse the body returned by `GET https://openrouter.ai/api/v1/models`.
pub fn parse_listing(json: &str) -> Result<OpenRouterListing> {
    serde_json::from_str::<OpenRouterListing>(json).context("parse OpenRouter listing JSON")
}

pub async fn fetch_listing(url: &str) -> Result<OpenRouterListing> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("building HTTP client")?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "openrouter listing returned HTTP {}",
            resp.status().as_u16()
        );
    }
    let text = resp.text().await.context("read openrouter listing body")?;
    parse_listing(&text)
}

pub fn report_from_path(path: &Path) -> Result<AliasReport> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("read OpenRouter listing {}", path.display()))?;
    Ok(build_aliases(&parse_listing(&json)?))
}

pub async fn run(url: String, from_file: Option<PathBuf>) -> CliResult {
    let report = match from_file {
        Some(p) => report_from_path(&p)?,
        None => build_aliases(&fetch_listing(&url).await?),
    };
    print!("{}", report.render());
    Ok(())
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn normalize_openrouter_provider(s: &str) -> String {
    match s.to_ascii_lowercase().as_str() {
        "openai" => "openai".into(),
        "anthropic" => "anthropic".into(),
        "google" => "gemini".into(),
        "amazon" | "aws" | "bedrock" => "bedrock".into(),
        "groq" => "groq".into(),
        "deepseek" => "deepseek".into(),
        other => other.to_string(),
    }
}

fn is_known_provider(id: &str) -> bool {
    matches!(
        id,
        "openai" | "anthropic" | "bedrock" | "groq" | "deepseek" | "gemini"
    )
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}
