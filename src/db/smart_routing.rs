//! DB queries for the Smart Routing Engine (V5-L6).
//!
//! Three concerns:
//!   1. Load model candidates (model_pricing + new capability columns)
//!   2. Load per-workspace smart routing config
//!   3. Match routing rules (Layer 2 explicit contract)

use crate::{db::DbPool, errors::AppResult};
use chrono::Utc;
use rust_decimal::Decimal;
use uuid::Uuid;

// ── Model candidates ──────────────────────────────────────────────────────────

/// A fully-loaded model from model_pricing, including all smart-routing fields.
/// Only `is_active = TRUE` models are returned.
#[derive(Debug, Clone)]
pub struct ModelCandidate {
    pub model_id: String,
    pub provider: String,
    pub context_window: Option<i32>,
    pub supports_functions: bool,
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_json_mode: bool,
    pub complexity_tier: String,
    pub quality_score: i32,
    pub input_per_1m: Decimal,
    pub output_per_1m: Decimal,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct CandidateRow {
    model_id: String,
    provider: String,
    context_window: Option<i32>,
    supports_functions: bool,
    supports_streaming: bool,
    supports_vision: bool,
    supports_json_mode: bool,
    complexity_tier: String,
    quality_score: i32,
    input_per_1m_tokens: Decimal,
    output_per_1m_tokens: Decimal,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct CandidateRow {
    model_id: String,
    provider: String,
    context_window: Option<i32>,
    supports_functions: bool,
    supports_streaming: bool,
    supports_vision: bool,
    supports_json_mode: bool,
    complexity_tier: String,
    quality_score: i32,
    input_per_1m_tokens: String,
    output_per_1m_tokens: String,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
impl From<CandidateRow> for ModelCandidate {
    fn from(r: CandidateRow) -> Self {
        Self {
            model_id: r.model_id,
            provider: r.provider,
            context_window: r.context_window,
            supports_functions: r.supports_functions,
            supports_streaming: r.supports_streaming,
            supports_vision: r.supports_vision,
            supports_json_mode: r.supports_json_mode,
            complexity_tier: r.complexity_tier,
            quality_score: r.quality_score,
            input_per_1m: r.input_per_1m_tokens,
            output_per_1m: r.output_per_1m_tokens,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<CandidateRow> for ModelCandidate {
    fn from(r: CandidateRow) -> Self {
        Self {
            model_id: r.model_id,
            provider: r.provider,
            context_window: r.context_window,
            supports_functions: r.supports_functions,
            supports_streaming: r.supports_streaming,
            supports_vision: r.supports_vision,
            supports_json_mode: r.supports_json_mode,
            complexity_tier: r.complexity_tier,
            quality_score: r.quality_score,
            input_per_1m: r.input_per_1m_tokens.parse().unwrap_or(Decimal::ZERO),
            output_per_1m: r.output_per_1m_tokens.parse().unwrap_or(Decimal::ZERO),
        }
    }
}

/// Load all active model candidates from model_pricing.
/// Result is sorted: best quality_score first within each tier for stable selection.
pub async fn load_model_candidates(pool: &DbPool) -> AppResult<Vec<ModelCandidate>> {
    let rows = sqlx::query_as::<_, CandidateRow>(
        "SELECT model_id, provider, context_window,
                supports_functions, supports_streaming, supports_vision, supports_json_mode,
                complexity_tier, quality_score,
                input_per_1m_tokens, output_per_1m_tokens
         FROM model_pricing
         WHERE is_active = TRUE
         ORDER BY quality_score DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(ModelCandidate::from).collect())
}

// ── Workspace smart routing config ────────────────────────────────────────────

/// Resolved per-workspace smart routing config from the DB.
#[derive(Debug, Clone)]
pub struct WorkspaceSmartConfig {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub enabled: bool,
    pub default_model: String,
    pub meta_classifier_enabled: bool,
    pub meta_classifier_provider: String,
    pub meta_classifier_model: String,
    pub meta_classifier_timeout_ms: i32,
    pub max_cost_per_request: Option<Decimal>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct SmartConfigRow {
    id: Uuid,
    workspace_id: Option<Uuid>,
    enabled: bool,
    default_model: String,
    meta_classifier_enabled: bool,
    meta_classifier_provider: String,
    meta_classifier_model: String,
    meta_classifier_timeout_ms: i32,
    max_cost_per_request: Option<Decimal>,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SmartConfigRow {
    id: Uuid,
    workspace_id: Option<Uuid>,
    enabled: bool,
    default_model: String,
    meta_classifier_enabled: bool,
    meta_classifier_provider: String,
    meta_classifier_model: String,
    meta_classifier_timeout_ms: i32,
    max_cost_per_request: Option<String>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
impl From<SmartConfigRow> for WorkspaceSmartConfig {
    fn from(r: SmartConfigRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            enabled: r.enabled,
            default_model: r.default_model,
            meta_classifier_enabled: r.meta_classifier_enabled,
            meta_classifier_provider: r.meta_classifier_provider,
            meta_classifier_model: r.meta_classifier_model,
            meta_classifier_timeout_ms: r.meta_classifier_timeout_ms,
            max_cost_per_request: r.max_cost_per_request,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<SmartConfigRow> for WorkspaceSmartConfig {
    fn from(r: SmartConfigRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            enabled: r.enabled,
            default_model: r.default_model,
            meta_classifier_enabled: r.meta_classifier_enabled,
            meta_classifier_provider: r.meta_classifier_provider,
            meta_classifier_model: r.meta_classifier_model,
            meta_classifier_timeout_ms: r.meta_classifier_timeout_ms,
            max_cost_per_request: r
                .max_cost_per_request
                .as_deref()
                .and_then(|s| s.parse().ok()),
        }
    }
}

/// Load workspace-specific smart routing config.
/// Falls back to the global row (workspace_id IS NULL) when no workspace row exists.
pub async fn get_workspace_smart_config(
    pool: &DbPool,
    workspace_id: Option<Uuid>,
) -> AppResult<Option<WorkspaceSmartConfig>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let row = sqlx::query_as::<_, SmartConfigRow>(
        "SELECT id, workspace_id, enabled, default_model,
                meta_classifier_enabled, meta_classifier_provider,
                meta_classifier_model, meta_classifier_timeout_ms,
                max_cost_per_request
         FROM smart_routing_config
         WHERE workspace_id = $1::uuid OR workspace_id IS NULL
         ORDER BY workspace_id NULLS LAST
         LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    let row = sqlx::query_as::<_, SmartConfigRow>(
        "SELECT id, workspace_id, enabled, default_model,
                meta_classifier_enabled, meta_classifier_provider,
                meta_classifier_model, meta_classifier_timeout_ms,
                max_cost_per_request
         FROM smart_routing_config
         WHERE workspace_id = $1 OR workspace_id IS NULL
         ORDER BY CASE WHEN workspace_id IS NULL THEN 1 ELSE 0 END
         LIMIT 1",
    )
    .bind(workspace_id.map(|u| u.to_string()))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(WorkspaceSmartConfig::from))
}

/// Parameters for upserting a workspace's smart routing config.
pub struct UpsertSmartConfig<'a> {
    pub enabled: bool,
    pub default_model: &'a str,
    pub meta_classifier_enabled: bool,
    pub meta_classifier_provider: &'a str,
    pub meta_classifier_model: &'a str,
    pub meta_classifier_timeout_ms: i32,
    pub max_cost_per_request: Option<Decimal>,
}

/// Upsert a workspace's smart routing config.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_workspace_smart_config(
    pool: &DbPool,
    workspace_id: Uuid,
    cfg: UpsertSmartConfig<'_>,
) -> AppResult<WorkspaceSmartConfig> {
    let enabled = cfg.enabled;
    let default_model = cfg.default_model;
    let meta_classifier_enabled = cfg.meta_classifier_enabled;
    let meta_classifier_provider = cfg.meta_classifier_provider;
    let meta_classifier_model = cfg.meta_classifier_model;
    let meta_classifier_timeout_ms = cfg.meta_classifier_timeout_ms;
    let max_cost_per_request = cfg.max_cost_per_request;
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let row = sqlx::query_as::<_, SmartConfigRow>(
            "INSERT INTO smart_routing_config
                 (id, workspace_id, enabled, default_model,
                  meta_classifier_enabled, meta_classifier_provider,
                  meta_classifier_model, meta_classifier_timeout_ms,
                  max_cost_per_request, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
             ON CONFLICT (workspace_id) DO UPDATE SET
                 enabled                  = EXCLUDED.enabled,
                 default_model            = EXCLUDED.default_model,
                 meta_classifier_enabled  = EXCLUDED.meta_classifier_enabled,
                 meta_classifier_provider = EXCLUDED.meta_classifier_provider,
                 meta_classifier_model    = EXCLUDED.meta_classifier_model,
                 meta_classifier_timeout_ms = EXCLUDED.meta_classifier_timeout_ms,
                 max_cost_per_request     = EXCLUDED.max_cost_per_request,
                 updated_at               = EXCLUDED.updated_at
             RETURNING id, workspace_id, enabled, default_model,
                       meta_classifier_enabled, meta_classifier_provider,
                       meta_classifier_model, meta_classifier_timeout_ms,
                       max_cost_per_request",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(enabled)
        .bind(default_model)
        .bind(meta_classifier_enabled)
        .bind(meta_classifier_provider)
        .bind(meta_classifier_model)
        .bind(meta_classifier_timeout_ms)
        .bind(max_cost_per_request)
        .bind(Utc::now())
        .fetch_one(pool)
        .await?;
        Ok(WorkspaceSmartConfig::from(row))
    }

    #[cfg(feature = "sqlite")]
    {
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let cost_str = max_cost_per_request.map(|d| d.to_string());
        sqlx::query(
            "INSERT INTO smart_routing_config
                 (id, workspace_id, enabled, default_model,
                  meta_classifier_enabled, meta_classifier_provider,
                  meta_classifier_model, meta_classifier_timeout_ms,
                  max_cost_per_request, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10)
             ON CONFLICT (workspace_id) DO UPDATE SET
                 enabled=$3, default_model=$4,
                 meta_classifier_enabled=$5, meta_classifier_provider=$6,
                 meta_classifier_model=$7, meta_classifier_timeout_ms=$8,
                 max_cost_per_request=$9, updated_at=$10",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(enabled)
        .bind(default_model)
        .bind(meta_classifier_enabled)
        .bind(meta_classifier_provider)
        .bind(meta_classifier_model)
        .bind(meta_classifier_timeout_ms)
        .bind(cost_str)
        .bind(&now)
        .execute(pool)
        .await?;

        get_workspace_smart_config(pool, Some(workspace_id))
            .await?
            .ok_or_else(|| {
                crate::errors::AppError::Anyhow(anyhow::anyhow!(
                    "smart_routing_config upsert failed to return row"
                ))
            })
    }
}

// ── Routing rules ─────────────────────────────────────────────────────────────

/// A single routing rule row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoutingRule {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub rule_order: i32,
    pub name: String,
    pub is_enabled: bool,
    pub tag_key: Option<String>,
    pub tag_value: Option<String>,
    pub min_token_estimate: Option<i32>,
    pub max_token_estimate: Option<i32>,
    pub requires_tools: Option<bool>,
    pub requires_vision: Option<bool>,
    pub target_model: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct RuleRow {
    id: Uuid,
    workspace_id: Uuid,
    rule_order: i32,
    name: String,
    is_enabled: bool,
    tag_key: Option<String>,
    tag_value: Option<String>,
    min_token_estimate: Option<i32>,
    max_token_estimate: Option<i32>,
    requires_tools: Option<bool>,
    requires_vision: Option<bool>,
    target_model: String,
    created_at: chrono::DateTime<Utc>,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct RuleRow {
    id: Uuid,
    workspace_id: Uuid,
    rule_order: i32,
    name: String,
    is_enabled: bool,
    tag_key: Option<String>,
    tag_value: Option<String>,
    min_token_estimate: Option<i32>,
    max_token_estimate: Option<i32>,
    requires_tools: Option<bool>,
    requires_vision: Option<bool>,
    target_model: String,
    created_at: chrono::DateTime<Utc>,
}

impl From<RuleRow> for RoutingRule {
    fn from(r: RuleRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            rule_order: r.rule_order,
            name: r.name,
            is_enabled: r.is_enabled,
            tag_key: r.tag_key,
            tag_value: r.tag_value,
            min_token_estimate: r.min_token_estimate,
            max_token_estimate: r.max_token_estimate,
            requires_tools: r.requires_tools,
            requires_vision: r.requires_vision,
            target_model: r.target_model,
            created_at: r.created_at,
        }
    }
}

/// List all routing rules for a workspace, ordered by rule_order ASC.
pub async fn list_rules(pool: &DbPool, workspace_id: Uuid) -> AppResult<Vec<RoutingRule>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let rows = sqlx::query_as::<_, RuleRow>(
        "SELECT id, workspace_id, rule_order, name, is_enabled,
                tag_key, tag_value, min_token_estimate, max_token_estimate,
                requires_tools, requires_vision, target_model, created_at
         FROM routing_rules
         WHERE workspace_id = $1
         ORDER BY rule_order ASC, created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    let rows = sqlx::query_as::<_, RuleRow>(
        "SELECT id, workspace_id, rule_order, name, is_enabled,
                tag_key, tag_value, min_token_estimate, max_token_estimate,
                requires_tools, requires_vision, target_model, created_at
         FROM routing_rules
         WHERE workspace_id = $1
         ORDER BY rule_order ASC, created_at ASC",
    )
    .bind(workspace_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(RoutingRule::from).collect())
}

/// Create a routing rule.
#[allow(clippy::too_many_arguments)]
pub async fn create_rule(
    pool: &DbPool,
    workspace_id: Uuid,
    rule_order: i32,
    name: &str,
    tag_key: Option<&str>,
    tag_value: Option<&str>,
    min_token_estimate: Option<i32>,
    max_token_estimate: Option<i32>,
    requires_tools: Option<bool>,
    requires_vision: Option<bool>,
    target_model: &str,
) -> AppResult<RoutingRule> {
    let id = Uuid::new_v4();

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let row = sqlx::query_as::<_, RuleRow>(
        "INSERT INTO routing_rules
             (id, workspace_id, rule_order, name, is_enabled,
              tag_key, tag_value, min_token_estimate, max_token_estimate,
              requires_tools, requires_vision, target_model, created_at, updated_at)
         VALUES ($1,$2,$3,$4,TRUE,$5,$6,$7,$8,$9,$10,$11,$12,$12)
         RETURNING id, workspace_id, rule_order, name, is_enabled,
                   tag_key, tag_value, min_token_estimate, max_token_estimate,
                   requires_tools, requires_vision, target_model, created_at",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(rule_order)
    .bind(name)
    .bind(tag_key)
    .bind(tag_value)
    .bind(min_token_estimate)
    .bind(max_token_estimate)
    .bind(requires_tools)
    .bind(requires_vision)
    .bind(target_model)
    .bind(Utc::now())
    .fetch_one(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO routing_rules
                 (id, workspace_id, rule_order, name, is_enabled,
                  tag_key, tag_value, min_token_estimate, max_token_estimate,
                  requires_tools, requires_vision, target_model, created_at, updated_at)
             VALUES ($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11,$12,$12)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(rule_order)
        .bind(name)
        .bind(tag_key)
        .bind(tag_value)
        .bind(min_token_estimate)
        .bind(max_token_estimate)
        .bind(requires_tools)
        .bind(requires_vision)
        .bind(target_model)
        .bind(&now)
        .execute(pool)
        .await?;

        let row = sqlx::query_as::<_, RuleRow>(
            "SELECT id, workspace_id, rule_order, name, is_enabled,
                    tag_key, tag_value, min_token_estimate, max_token_estimate,
                    requires_tools, requires_vision, target_model, created_at
             FROM routing_rules WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_one(pool)
        .await?;

        return Ok(RoutingRule::from(row));
    }

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    Ok(RoutingRule::from(row))
}

/// Toggle a rule's enabled state.
pub async fn set_rule_enabled(pool: &DbPool, rule_id: Uuid, enabled: bool) -> AppResult<()> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    sqlx::query("UPDATE routing_rules SET is_enabled=$1, updated_at=$2 WHERE id=$3")
        .bind(enabled)
        .bind(Utc::now())
        .bind(rule_id)
        .execute(pool)
        .await?;

    #[cfg(feature = "sqlite")]
    sqlx::query("UPDATE routing_rules SET is_enabled=$1, updated_at=$2 WHERE id=$3")
        .bind(enabled)
        .bind(Utc::now().to_rfc3339())
        .bind(rule_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Delete a routing rule.
pub async fn delete_rule(pool: &DbPool, rule_id: Uuid) -> AppResult<()> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    sqlx::query("DELETE FROM routing_rules WHERE id = $1")
        .bind(rule_id)
        .execute(pool)
        .await?;

    #[cfg(feature = "sqlite")]
    sqlx::query("DELETE FROM routing_rules WHERE id = $1")
        .bind(rule_id.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Evaluate workspace routing rules against a request profile.
/// Returns the target_model of the first matching enabled rule, or None.
pub async fn match_first_rule(
    pool: &DbPool,
    workspace_id: Uuid,
    token_estimate: u32,
    tags: &std::collections::HashMap<String, String>,
    needs_tools: bool,
    needs_vision: bool,
    candidate_model_ids: &[&str],
) -> AppResult<Option<(String, String)>> {
    // Load enabled rules for this workspace ordered by priority
    let rules = list_rules(pool, workspace_id).await?;

    for rule in rules.iter().filter(|r| r.is_enabled) {
        // Token range check
        if let Some(min) = rule.min_token_estimate {
            if (token_estimate as i32) < min {
                continue;
            }
        }
        if let Some(max) = rule.max_token_estimate {
            if (token_estimate as i32) > max {
                continue;
            }
        }
        // Capability checks
        if rule.requires_tools == Some(true) && !needs_tools {
            continue;
        }
        if rule.requires_vision == Some(true) && !needs_vision {
            continue;
        }
        // Tag check: both key and value must match
        if let Some(ref key) = rule.tag_key {
            match tags.get(key) {
                None => continue,
                Some(val) => {
                    if let Some(ref expected) = rule.tag_value {
                        if val != expected {
                            continue;
                        }
                    }
                }
            }
        }
        // The rule matches — verify target model is in the candidate set (Layer 1 output)
        if candidate_model_ids.contains(&rule.target_model.as_str()) {
            return Ok(Some((rule.target_model.clone(), rule.name.clone())));
        }
        // Target model was eliminated by Layer 1 → rule is skipped, not forced through
    }

    Ok(None)
}
