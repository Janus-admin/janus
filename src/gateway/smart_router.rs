//! Janus Smart Router — Hybrid-Speed 4-Layer Model Selection Pipeline (V5-L6)
//!
//! Only invoked when `request.model` is empty. Layer 0 (model explicitly set)
//! is handled in the gateway handler before this module is called.
//!
//! Pipeline:
//!   Layer 1 — Hard Guardrails: capability + cost filtering → candidate_set
//!   Layer 2 — Explicit Contract: client tags → admin rules (short-circuit)
//!   Layer 3 — Native Heuristic: complexity score → tier → best quality_score
//!   Layer 4 — Config default → 400 Bad Request if nothing remains
//!   Layer 4b — Meta-Classifier (opt-in per workspace): overrides Layer 3 result

use crate::{
    config::SmartRoutingConfig,
    db::{smart_routing as sr_db, DbPool},
    errors::{AppError, AppResult},
    providers::ChatCompletionRequest,
};
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

pub use sr_db::ModelCandidate;

// ── Public output types ───────────────────────────────────────────────────────

/// Why the smart router chose a model.
/// Written to `X-Janus-Routing-Reason` response header for full observability.
#[derive(Debug, Clone)]
pub enum RoutingReason {
    /// Matched a tag sent by the client (e.g. `X-Janus-Tags: quality=premium`).
    TagMatch(String),
    /// Matched an admin-defined routing rule by name.
    AdminRule(String),
    /// Selected via heuristic complexity score → tier.
    ComplexityTier { tier: String, score: u8 },
    /// Meta-classifier overrode the heuristic (opt-in, per workspace).
    MetaClassifier { score: u8, tier: String },
    /// Fell back to the configured default_model.
    ConfigDefault,
}

impl RoutingReason {
    /// Compact string written to the `X-Janus-Routing-Reason` response header.
    pub fn header_value(&self) -> String {
        match self {
            RoutingReason::TagMatch(t) => format!("tag:{t}"),
            RoutingReason::AdminRule(n) => format!("rule:{n}"),
            RoutingReason::ComplexityTier { tier, score } => {
                format!("tier:{tier}(score={score})")
            }
            RoutingReason::MetaClassifier { score, tier } => {
                format!("meta:{tier}(score={score})")
            }
            RoutingReason::ConfigDefault => "config_default".to_string(),
        }
    }
}

/// The output of the smart router: which model to use and why.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub model: String,
    pub reason: RoutingReason,
}

// ── Request profile ───────────────────────────────────────────────────────────

/// Extracted request characteristics. Computed once and reused across all layers.
#[derive(Debug)]
pub struct RequestProfile {
    /// Estimated total input tokens (chars/4 + per-message overhead + image tokens).
    pub token_estimate: u32,
    pub needs_functions: bool,
    pub needs_vision: bool,
    pub needs_streaming: bool,
    pub needs_json_mode: bool,
    /// Heuristic complexity score 0–10.
    pub complexity_score: u8,
    /// Tags extracted from `metadata.tags` and `X-Janus-Tags` header values.
    /// Populated by the gateway handler and passed in via `extra_tags`.
    pub tags: HashMap<String, String>,
}

// ── The Smart Router ──────────────────────────────────────────────────────────

pub struct SmartRouter;

impl SmartRouter {
    /// Run the Hybrid-Speed 4-layer pipeline.
    ///
    /// `extra_tags` contains tags from the `X-Janus-Tags` HTTP header (already
    /// parsed by the gateway handler). Tags from `request.metadata` are also
    /// extracted here so both sources are available to Layer 2.
    ///
    /// Returns `Err(AppError::BadRequest)` with a clear diagnostic when no model
    /// can be safely selected — never silently returns a wrong model.
    pub async fn select(
        pool: &DbPool,
        request: &ChatCompletionRequest,
        workspace_id: Option<Uuid>,
        extra_tags: &HashMap<String, String>,
        global_config: &SmartRoutingConfig,
        allowed_models: Option<&[String]>,
    ) -> AppResult<RoutingDecision> {
        // ── Build request profile (used by all layers) ────────────────────────
        let profile = Self::profile_request(request, extra_tags);

        // ── Load per-workspace config (may be None when DB has no row yet) ────
        let ws_config = sr_db::get_workspace_smart_config(pool, workspace_id).await?;
        let ws_cfg = ws_config.as_ref();

        // ── Load all active model candidates ──────────────────────────────────
        let all_candidates = sr_db::load_model_candidates(pool).await?;
        if all_candidates.is_empty() {
            return Err(AppError::BadRequest(
                "Smart routing: no active models in model_pricing. \
                 Add at least one model or set 'model' explicitly."
                    .to_string(),
            ));
        }

        // ── Pre-filter: respect per-key allowed_models allowlist ──────────────
        // When an API key has a non-empty allowed_models list the router must
        // only consider those models. This runs before Layer 1 so capability
        // filtering still applies within the allowed set.
        let all_candidates: Vec<ModelCandidate> = match allowed_models {
            Some(allowed) if !allowed.is_empty() => {
                let filtered: Vec<ModelCandidate> = all_candidates
                    .into_iter()
                    .filter(|m| allowed.contains(&m.model_id))
                    .collect();
                if filtered.is_empty() {
                    return Err(AppError::BadRequest(
                        "Smart routing: none of the models allowed by this API key \
                         are available in the model catalogue. Check the key's \
                         allowed_models list or set 'model' explicitly."
                            .to_string(),
                    ));
                }
                filtered
            }
            _ => all_candidates,
        };

        // ── Layer 1: Hard Guardrails + Budget Shield ──────────────────────────
        // Compute effective cost cap: workspace DB value > global config.
        let max_cost: Option<Decimal> = ws_cfg
            .and_then(|c| c.max_cost_per_request)
            .or(global_config.max_cost_per_request);

        let candidate_set: Vec<&ModelCandidate> = all_candidates
            .iter()
            .filter(|m| Self::passes_layer1(m, &profile, max_cost))
            .collect();

        if candidate_set.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Smart routing: no model satisfies this request \
                 (needs_tools={}, needs_vision={}, needs_json_mode={}, \
                  token_estimate={}, max_cost_per_request={:?}). \
                 Specify 'model' explicitly or relax budget/capability constraints.",
                profile.needs_functions,
                profile.needs_vision,
                profile.needs_json_mode,
                profile.token_estimate,
                max_cost,
            )));
        }

        // Build a fast lookup slice for rule matching
        let candidate_ids: Vec<&str> = candidate_set.iter().map(|m| m.model_id.as_str()).collect();

        // ── Layer 2a: Client tag routing ──────────────────────────────────────
        if let Some(decision) = Self::check_quality_tag(&profile, &candidate_set) {
            return Ok(decision);
        }

        // ── Layer 2b: Admin routing rules ─────────────────────────────────────
        if let Some(ws_id) = workspace_id {
            if let Some((model, rule_name)) = sr_db::match_first_rule(
                pool,
                ws_id,
                profile.token_estimate,
                &profile.tags,
                profile.needs_functions,
                profile.needs_vision,
                &candidate_ids,
            )
            .await?
            {
                return Ok(RoutingDecision {
                    model,
                    reason: RoutingReason::AdminRule(rule_name),
                });
            }
        }

        // ── Layer 3: Native Heuristic ─────────────────────────────────────────
        let heuristic_decision = Self::heuristic_select(&profile, &candidate_set);

        // ── Layer 4b: Meta-Classifier (opt-in) ────────────────────────────────
        let meta_enabled = ws_cfg.map(|c| c.meta_classifier_enabled).unwrap_or(false);
        if meta_enabled {
            let timeout_ms = ws_cfg
                .map(|c| c.meta_classifier_timeout_ms as u64)
                .unwrap_or(300);

            let meta_provider = ws_cfg
                .map(|c| c.meta_classifier_provider.as_str())
                .unwrap_or("groq");
            let meta_model = ws_cfg
                .map(|c| c.meta_classifier_model.as_str())
                .unwrap_or("llama-3.1-8b-instant");

            match Self::meta_classify(
                pool,
                request,
                &candidate_set,
                meta_provider,
                meta_model,
                timeout_ms,
            )
            .await
            {
                Ok(Some(meta_decision)) => return Ok(meta_decision),
                Ok(None) | Err(_) => {
                    tracing::debug!("Meta-classifier timed out or failed — using heuristic result");
                }
            }
        }

        // ── Return heuristic result ───────────────────────────────────────────
        if let Some(decision) = heuristic_decision {
            return Ok(decision);
        }

        // ── Layer 4: Config default ───────────────────────────────────────────
        let default_model = ws_cfg
            .and_then(|c| {
                if c.default_model.is_empty() {
                    None
                } else {
                    Some(c.default_model.as_str())
                }
            })
            .unwrap_or(global_config.default_model.as_str());

        if !default_model.is_empty() {
            return Ok(RoutingDecision {
                model: default_model.to_string(),
                reason: RoutingReason::ConfigDefault,
            });
        }

        Err(AppError::BadRequest(
            "Smart routing: no model could be selected. \
             Set 'smart_routing.default_model' in config or specify 'model' in the request."
                .to_string(),
        ))
    }

    // ── Request profiling ─────────────────────────────────────────────────────

    pub fn profile_request(
        request: &ChatCompletionRequest,
        extra_tags: &HashMap<String, String>,
    ) -> RequestProfile {
        // ── Token estimation ──────────────────────────────────────────────────
        // 1 token ≈ 4 chars for English text. Images ≈ 765 tokens each (OpenAI).
        // Per-message overhead accounts for role + separator tokens.
        let char_count: usize = request
            .messages
            .iter()
            .map(|m| {
                if let Some(s) = m.content.as_str() {
                    s.len()
                } else if let Some(arr) = m.content.as_array() {
                    arr.iter()
                        .map(|item| match item["type"].as_str() {
                            Some("text") => item["text"].as_str().map(str::len).unwrap_or(0),
                            Some("image_url") => 765 * 4, // ~765 tokens per image → chars
                            _ => 30,
                        })
                        .sum()
                } else {
                    30
                }
            })
            .sum::<usize>()
            + (request.messages.len() * 8) // per-message token overhead
            + 50; // system overhead

        let token_estimate = (char_count / 4).max(1) as u32;

        // ── Capability detection ──────────────────────────────────────────────
        let needs_functions = request.tools.is_some();

        let needs_vision = request.messages.iter().any(|m| {
            m.content
                .as_array()
                .map(|arr| arr.iter().any(|i| i["type"].as_str() == Some("image_url")))
                .unwrap_or(false)
        });

        let needs_streaming = request.stream == Some(true);

        let needs_json_mode = request
            .response_format
            .as_ref()
            .and_then(|rf| rf["type"].as_str())
            .map(|t| t == "json_object" || t == "json_schema")
            .unwrap_or(false);

        // ── Complexity scoring (0–10) ─────────────────────────────────────────
        let mut score: u8 = 0;

        // Signal 1: Token payload (0–4 pts)
        score += match token_estimate {
            0..=200 => 0,
            201..=1_000 => 1,
            1_001..=4_000 => 2,
            4_001..=8_000 => 3,
            _ => 4,
        };

        // Signal 2: Conversation depth — longer history = harder task (0–2 pts)
        score = score.saturating_add(match request.messages.len() {
            0..=2 => 0,
            3..=8 => 1,
            _ => 2,
        });

        // Signal 3: Tool use → complex reasoning almost always needed (0–2 pts)
        if needs_functions {
            score = score.saturating_add(2);
        }

        // Signal 4: Structural indicators — code blocks, SQL, function defs (0–1 pt)
        // Signal 5: High-complexity verbs in any message (0–2 pts)
        const COMPLEX_VERBS: &[&str] = &[
            "analyze",
            "analyse",
            "reason",
            "compare",
            "critique",
            "evaluate",
            "summarize",
            "summarise",
            "research",
            "architect",
            "optimize",
            "optimise",
            "debug",
            "refactor",
            "explain in detail",
            "review",
            "investigate",
            "synthesize",
        ];
        const STRUCTURAL_PATTERNS: &[&str] = &[
            "```",
            "def ",
            "fn ",
            "class ",
            "SELECT ",
            "function ",
            "import ",
        ];

        let all_text: String = request
            .messages
            .iter()
            .filter_map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();

        let verb_pts = COMPLEX_VERBS
            .iter()
            .filter(|k| all_text.contains(*k))
            .count()
            .min(2) as u8;

        let struct_pts = STRUCTURAL_PATTERNS.iter().any(|p| all_text.contains(p)) as u8;

        score = score
            .saturating_add(verb_pts)
            .saturating_add(struct_pts)
            .min(10);

        // ── Tag extraction ────────────────────────────────────────────────────
        // Merge metadata.tags (body) and extra_tags (header). Header wins on conflict.
        let mut tags: HashMap<String, String> = HashMap::new();

        if let Some(meta) = &request.metadata {
            if let Some(obj) = meta.get("tags").and_then(|v| v.as_object()) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        tags.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        for (k, v) in extra_tags {
            tags.insert(k.clone(), v.clone()); // header wins
        }

        RequestProfile {
            token_estimate,
            needs_functions,
            needs_vision,
            needs_streaming,
            needs_json_mode,
            complexity_score: score,
            tags,
        }
    }

    // ── Layer 1: Hard Guardrails ──────────────────────────────────────────────

    fn passes_layer1(
        model: &ModelCandidate,
        profile: &RequestProfile,
        max_cost: Option<Decimal>,
    ) -> bool {
        // Context window: leave 20% headroom for response tokens.
        if let Some(ctx) = model.context_window {
            if ctx > 0 && profile.token_estimate > (ctx as f64 * 0.80) as u32 {
                return false;
            }
        }
        if profile.needs_functions && !model.supports_functions {
            return false;
        }
        if profile.needs_vision && !model.supports_vision {
            return false;
        }
        if profile.needs_streaming && !model.supports_streaming {
            return false;
        }
        if profile.needs_json_mode && !model.supports_json_mode {
            return false;
        }
        // Cost envelope: rough estimate = input cost + 50% of input as output
        if let Some(max) = max_cost {
            let est_in = Decimal::from(profile.token_estimate);
            let est_out = Decimal::from(profile.token_estimate / 2 + 100);
            let est_cost = (est_in * model.input_per_1m + est_out * model.output_per_1m)
                / Decimal::from(1_000_000);
            if est_cost > max {
                return false;
            }
        }
        true
    }

    // ── Layer 2a: Client quality tag ──────────────────────────────────────────

    fn check_quality_tag(
        profile: &RequestProfile,
        candidate_set: &[&ModelCandidate],
    ) -> Option<RoutingDecision> {
        let quality = profile.tags.get("quality")?;
        let target_tier = match quality.as_str() {
            "premium" | "high" => "premium",
            "standard" | "medium" => "standard",
            "cheap" | "low" | "micro" => "micro",
            _ => return None,
        };
        Self::best_in_tier(candidate_set, target_tier).map(|m| RoutingDecision {
            model: m.model_id.clone(),
            reason: RoutingReason::TagMatch(format!("quality={quality}")),
        })
    }

    // ── Layer 3: Native Heuristic ─────────────────────────────────────────────

    fn heuristic_select(
        profile: &RequestProfile,
        candidate_set: &[&ModelCandidate],
    ) -> Option<RoutingDecision> {
        let tier = Self::tier_from_score(profile.complexity_score);
        let score = profile.complexity_score;

        // Try exact tier first, then fall through to adjacent tiers.
        // For micro: try standard before premium (avoid overkill).
        // For premium: try standard before micro (avoid under-serving).
        let fallback_order: &[&str] = match tier {
            "micro" => &["micro", "standard", "premium"],
            "standard" => &["standard", "premium", "micro"],
            "premium" => &["premium", "standard", "micro"],
            _ => &["standard", "micro", "premium"],
        };

        for &t in fallback_order {
            if let Some(best) = Self::best_in_tier(candidate_set, t) {
                return Some(RoutingDecision {
                    model: best.model_id.clone(),
                    reason: RoutingReason::ComplexityTier {
                        tier: tier.to_string(),
                        score,
                    },
                });
            }
        }
        None
    }

    pub fn tier_from_score(score: u8) -> &'static str {
        match score {
            0..=3 => "micro",
            4..=7 => "standard",
            _ => "premium",
        }
    }

    fn best_in_tier<'a>(
        candidate_set: &[&'a ModelCandidate],
        tier: &str,
    ) -> Option<&'a ModelCandidate> {
        candidate_set
            .iter()
            .filter(|m| m.complexity_tier == tier)
            .max_by_key(|m| m.quality_score)
            .copied()
    }

    // ── Layer 4b: Meta-Classifier (opt-in stub) ───────────────────────────────
    //
    // Full implementation requires access to the ProviderRegistry to call the
    // meta-classifier model. The stub always returns Ok(None), causing the
    // pipeline to fall through to the heuristic result — completely safe.
    //
    // To implement: call the configured meta_classifier_model via the provider
    // registry with a 1-shot classification prompt, parse the integer response,
    // map to tier, select best_in_tier. Wrap in tokio::time::timeout(timeout_ms).
    async fn meta_classify(
        _pool: &DbPool,
        _request: &ChatCompletionRequest,
        _candidate_set: &[&ModelCandidate],
        _meta_provider: &str,
        _meta_model: &str,
        _timeout_ms: u64,
    ) -> AppResult<Option<RoutingDecision>> {
        // Stub: returns None so the pipeline falls through to heuristic.
        // Replace with actual provider call when wiring the full meta-classifier.
        Ok(None)
    }
}

// ── Tag parsing helper ────────────────────────────────────────────────────────

/// Parse `X-Janus-Tags: key=val,key2=val2` header into a HashMap.
/// Used by the gateway handler to build `extra_tags` before calling the router.
pub fn parse_tag_header(header_value: &str) -> HashMap<String, String> {
    header_value
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.trim().splitn(2, '=');
            let key = parts.next()?.trim().to_string();
            let val = parts.next()?.trim().to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, val))
            }
        })
        .collect()
}
