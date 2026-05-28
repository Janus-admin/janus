//! Bounded, batched audit-log writer.
//!
//! Each completed gateway request used to spawn 1–4 fire-and-forget DB writes
//! (request row, daily-cost upsert, budget add, last-used touch). Under load
//! PostgreSQL couldn't keep up and the spawned tasks accumulated until the
//! kernel OOM-killed Janus (observed at 13k+ RPS on a 4 vCPU VM).
//!
//! This module replaces that pattern with:
//!
//! 1. A bounded `mpsc::channel` — request handlers `try_send` an `AuditEvent`
//!    and return immediately. If the channel is full (writer can't keep up),
//!    the event is dropped and `janus_audit_dropped_total` increments.
//!
//! 2. A single background writer task per process. The writer buffers events,
//!    flushes when the buffer hits `BATCH_SIZE` or `FLUSH_INTERVAL` ticks, and
//!    does the inserts as multi-row statements where the schema allows it.
//!    The aggregation step de-duplicates `(api_key_id)`-keyed updates so the
//!    actual UPDATE count is much smaller than the event count.
//!
//! Memory ceiling: `channel_capacity` × `sizeof(AuditEvent)` ≈ a few MB at the
//! default 10 000 capacity. PG load: one batch INSERT every 100 ms is far
//! cheaper than per-request INSERTs at extreme rate.

use crate::db::DbPool;
use metrics::counter;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Default channel capacity (number of events buffered before drops start).
pub const DEFAULT_CAPACITY: usize = 10_000;
/// Default flush trigger size — at this buffer depth we flush immediately.
pub const BATCH_SIZE: usize = 500;
/// Default flush interval — buffered events also flush after this duration.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// One audit event produced by the gateway pipeline.
#[derive(Debug)]
pub enum AuditEvent {
    /// A full request completed (success or error). The writer turns this into
    /// `INSERT INTO requests` + `daily_costs` upsert + budget/last-used updates.
    RequestComplete(Box<RequestRecord>),
    /// A cache hit was served. The writer turns this into an UPDATE on
    /// `cache_entries` for the matching prompt_hash.
    CacheHit { hash: String, tokens: i64 },
}

/// Fields captured by the pipeline for a completed request.
/// All four downstream effects (`requests` row, `daily_costs` rollup,
/// `api_keys.budget_used` add, `api_keys.last_used_at` touch) derive from here.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub api_key_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub provider: &'static str,
    pub model: String,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cost: Option<Decimal>,
    pub latency_ms: i32,
    pub status: String,
    pub is_stream: bool,
    pub ttfb_ms: Option<i32>,
    pub prompt_version_id: Option<Uuid>,
    pub downgrade_triggered: bool,
    pub endpoint: Arc<str>,
    pub tool_calls: Option<serde_json::Value>,
    pub tags: serde_json::Value,
}

/// Cheaply-cloneable handle to the writer task. Held in `AppState`.
#[derive(Clone)]
pub struct AuditChannel {
    tx: mpsc::Sender<AuditEvent>,
}

impl AuditChannel {
    /// Submit an event for asynchronous persistence. Never blocks the caller:
    /// drops the event and increments `janus_audit_dropped_total` if the
    /// writer is behind.
    pub fn send(&self, event: AuditEvent) {
        if self.tx.try_send(event).is_err() {
            counter!("janus_audit_dropped_total").increment(1);
        }
    }

    /// Test/benchmark utility: returns true if the writer is still attached.
    /// Hot-path callers should not use this — call `send` directly.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        !self.tx.is_closed()
    }
}

/// Spawn the background writer task and return a handle to its channel.
/// The task lives for the duration of the runtime; it exits when all
/// `AuditChannel` clones are dropped.
pub fn spawn_writer(pool: DbPool, capacity: usize) -> AuditChannel {
    let (tx, rx) = mpsc::channel::<AuditEvent>(capacity.max(64));
    tokio::spawn(run_writer(pool, rx));
    AuditChannel { tx }
}

// ── Aggregation keys ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DailyCostKey {
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &'static str,
    model: String,
}

#[derive(Debug, Default)]
struct DailyCostAcc {
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost: Decimal,
}

// ── Writer task ───────────────────────────────────────────────────────────────

async fn run_writer(pool: DbPool, mut rx: mpsc::Receiver<AuditEvent>) {
    let mut requests: Vec<RequestRecord> = Vec::with_capacity(BATCH_SIZE);
    let mut cache_hits: Vec<(String, i64)> = Vec::with_capacity(BATCH_SIZE);
    let mut budget: HashMap<Uuid, Decimal> = HashMap::new();
    let mut last_used: HashSet<Uuid> = HashSet::new();
    let mut daily_costs: HashMap<DailyCostKey, DailyCostAcc> = HashMap::new();

    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            biased;
            event = rx.recv() => match event {
                None => {
                    // All senders dropped — drain anything remaining and exit.
                    flush(&pool, &mut requests, &mut cache_hits,
                          &mut budget, &mut last_used, &mut daily_costs).await;
                    return;
                }
                Some(AuditEvent::RequestComplete(rec)) => {
                    accumulate_request(*rec, &mut requests, &mut budget,
                                       &mut last_used, &mut daily_costs);
                    if requests.len() >= BATCH_SIZE {
                        flush(&pool, &mut requests, &mut cache_hits,
                              &mut budget, &mut last_used, &mut daily_costs).await;
                    }
                }
                Some(AuditEvent::CacheHit { hash, tokens }) => {
                    cache_hits.push((hash, tokens));
                    if cache_hits.len() >= BATCH_SIZE {
                        flush(&pool, &mut requests, &mut cache_hits,
                              &mut budget, &mut last_used, &mut daily_costs).await;
                    }
                }
            },
            _ = interval.tick() => {
                flush(&pool, &mut requests, &mut cache_hits,
                      &mut budget, &mut last_used, &mut daily_costs).await;
            }
        }
    }
}

fn accumulate_request(
    rec: RequestRecord,
    requests: &mut Vec<RequestRecord>,
    budget: &mut HashMap<Uuid, Decimal>,
    last_used: &mut HashSet<Uuid>,
    daily_costs: &mut HashMap<DailyCostKey, DailyCostAcc>,
) {
    if let Some(api_key_id) = rec.api_key_id {
        if let Some(cost) = rec.cost {
            if cost > Decimal::ZERO {
                *budget.entry(api_key_id).or_insert(Decimal::ZERO) += cost;
            }
        }
        last_used.insert(api_key_id);
    }

    let key = DailyCostKey {
        api_key_id: rec.api_key_id,
        workspace_id: rec.workspace_id,
        provider: rec.provider,
        model: rec.model.clone(),
    };
    let acc = daily_costs.entry(key).or_default();
    acc.request_count += 1;
    if rec.status == "success" {
        acc.prompt_tokens += rec.prompt_tokens.unwrap_or(0) as i64;
        acc.completion_tokens += rec.completion_tokens.unwrap_or(0) as i64;
        if let Some(c) = rec.cost {
            acc.cost += c;
        }
    }

    requests.push(rec);
}

async fn flush(
    pool: &DbPool,
    requests: &mut Vec<RequestRecord>,
    cache_hits: &mut Vec<(String, i64)>,
    budget: &mut HashMap<Uuid, Decimal>,
    last_used: &mut HashSet<Uuid>,
    daily_costs: &mut HashMap<DailyCostKey, DailyCostAcc>,
) {
    if requests.is_empty()
        && cache_hits.is_empty()
        && budget.is_empty()
        && last_used.is_empty()
        && daily_costs.is_empty()
    {
        return;
    }

    // 1. Multi-row INSERT into requests. PostgreSQL only — SQLite path falls
    //    back to per-row inserts because the bind-count optimisation isn't
    //    worth the complexity for that backend.
    if !requests.is_empty() {
        if let Err(e) = batch_insert_requests(pool, requests).await {
            tracing::warn!(error = %e, count = requests.len(), "audit: batch insert_requests failed");
        }
        requests.clear();
    }

    // Each stage below emits a `tracing::warn!` and bumps the
    // `janus_audit_flush_errors_total{stage=...}` counter on failure so a
    // chronically failing DB shows up in Prometheus rather than silently
    // dropping audit rows.

    // 2. cache_entries hit-counter updates (one UPDATE per distinct hash).
    if !cache_hits.is_empty() {
        let mut grouped: HashMap<String, i64> = HashMap::new();
        for (h, t) in cache_hits.drain(..) {
            *grouped.entry(h).or_insert(0) += t;
        }
        for (hash, tokens) in grouped {
            if let Err(e) =
                crate::db::cache::record_hit(pool, &hash, tokens, Decimal::ZERO).await
            {
                tracing::warn!(error = %e, hash = %hash, "audit: record_hit failed");
                counter!("janus_audit_flush_errors_total", "stage" => "cache_hit")
                    .increment(1);
            }
        }
    }

    // 3. daily_costs upsert (one row per (date, key, ws, provider, model)).
    for (key, acc) in daily_costs.drain() {
        if let Err(e) = crate::db::analytics::upsert_daily_cost(
            pool,
            key.api_key_id,
            key.workspace_id,
            key.provider,
            &key.model,
            acc.request_count,
            0, // cache_hit aggregation handled separately
            acc.prompt_tokens,
            acc.completion_tokens,
            if acc.cost > Decimal::ZERO { Some(acc.cost) } else { None },
        )
        .await
        {
            tracing::warn!(error = %e, provider = key.provider, model = %key.model, "audit: upsert_daily_cost failed");
            counter!("janus_audit_flush_errors_total", "stage" => "daily_costs").increment(1);
        }
    }

    // 4. budget add (one UPDATE per api_key with summed cost).
    for (api_key_id, amount) in budget.drain() {
        if let Err(e) = crate::db::api_keys::add_budget_used(pool, api_key_id, amount).await {
            tracing::warn!(error = %e, api_key_id = %api_key_id, "audit: add_budget_used failed");
            counter!("janus_audit_flush_errors_total", "stage" => "budget").increment(1);
        }
    }

    // 5. last_used touch (one UPDATE per api_key).
    for api_key_id in last_used.drain() {
        if let Err(e) = crate::db::api_keys::update_last_used(pool, api_key_id).await {
            tracing::warn!(error = %e, api_key_id = %api_key_id, "audit: update_last_used failed");
            counter!("janus_audit_flush_errors_total", "stage" => "last_used").increment(1);
        }
    }
}

// ── Multi-row INSERT into requests (PostgreSQL) ───────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
async fn batch_insert_requests(
    pool: &DbPool,
    records: &[RequestRecord],
) -> Result<(), sqlx::Error> {
    if records.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now();
    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "INSERT INTO requests (
            id, api_key_id, workspace_id, provider, model,
            prompt_tokens, completion_tokens, total_tokens, cost_usd,
            latency_ms, status, stream, ttfb_ms, prompt_version_id,
            downgrade_triggered, endpoint, tool_calls, tags, created_at
        ) ",
    );

    builder.push_values(records.iter(), |mut b, r| {
        b.push_bind(Uuid::new_v4())
            .push_bind(r.api_key_id)
            .push_bind(r.workspace_id)
            .push_bind(r.provider)
            .push_bind(&r.model)
            .push_bind(r.prompt_tokens)
            .push_bind(r.completion_tokens)
            .push_bind(r.total_tokens)
            .push_bind(r.cost)
            .push_bind(r.latency_ms)
            .push_bind(&r.status)
            .push_bind(r.is_stream)
            .push_bind(r.ttfb_ms)
            .push_bind(r.prompt_version_id)
            .push_bind(r.downgrade_triggered)
            .push_bind(r.endpoint.as_ref())
            .push_bind(r.tool_calls.clone())
            .push_bind(r.tags.clone())
            .push_bind(now);
    });

    builder.build().execute(pool).await?;
    Ok(())
}

// ── SQLite fallback (per-row INSERT, same path as pre-batch behaviour) ────────

#[cfg(feature = "sqlite")]
async fn batch_insert_requests(
    pool: &DbPool,
    records: &[RequestRecord],
) -> Result<(), crate::errors::AppError> {
    for r in records {
        crate::db::requests::insert_request(
            pool,
            r.api_key_id,
            r.workspace_id,
            r.provider,
            &r.model,
            r.prompt_tokens,
            r.completion_tokens,
            r.total_tokens,
            r.cost,
            r.latency_ms,
            &r.status,
            r.is_stream,
            r.ttfb_ms,
            r.prompt_version_id,
            r.downgrade_triggered,
            r.endpoint.as_ref(),
            r.tool_calls.as_ref(),
            &r.tags,
        )
        .await?;
    }
    Ok(())
}
