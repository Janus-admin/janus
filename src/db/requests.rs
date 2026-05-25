use crate::db::DbPool;
use crate::errors::AppResult;
use chrono::Utc;
use rust_decimal::Decimal;
use uuid::Uuid;

// ── Internal row type for pricing lookup ─────────────────────────────────────

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
#[derive(sqlx::FromRow)]
struct PricingRow {
    input_per_1m_tokens: Decimal,
    output_per_1m_tokens: Decimal,
}

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct PricingRow {
    input_per_1m_tokens: String,
    output_per_1m_tokens: String,
}

// ── SQLite-only row type for `requests` table ─────────────────────────────────

#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct RequestSqliteRow {
    id: Uuid,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: String,
    model: String,
    base_url: Option<String>,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<String>,
    latency_ms: Option<i32>,
    ttfb_ms: Option<i32>,
    status: String,
    cache_type: Option<String>,
    cache_similarity: Option<String>,
    http_status: Option<i32>,
    error_code: Option<String>,
    error_message: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
    stream: bool,
    prompt_version_id: Option<Uuid>,
    replay_of_request_id: Option<String>,
    is_playground: bool,
    tags: Option<String>,
    created_at: chrono::DateTime<Utc>,
}

#[cfg(feature = "sqlite")]
impl From<RequestSqliteRow> for crate::models::request::Request {
    fn from(r: RequestSqliteRow) -> Self {
        Self {
            id: r.id,
            api_key_id: r.api_key_id,
            workspace_id: r.workspace_id,
            provider: r.provider,
            model: r.model,
            base_url: r.base_url,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
            cost_usd: r.cost_usd.and_then(|s| s.parse().ok()),
            latency_ms: r.latency_ms,
            ttfb_ms: r.ttfb_ms,
            status: r.status,
            cache_type: r.cache_type,
            cache_similarity: r.cache_similarity.and_then(|s| s.parse().ok()),
            http_status: r.http_status,
            error_code: r.error_code,
            error_message: r.error_message,
            request_body: r.request_body,
            response_body: r.response_body,
            stream: r.stream,
            prompt_version_id: r.prompt_version_id,
            replay_of_request_id: r
                .replay_of_request_id
                .as_deref()
                .and_then(|s| s.parse().ok()),
            is_playground: r.is_playground,
            tags: r
                .tags
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
            created_at: r.created_at,
        }
    }
}

// ── Database operations ───────────────────────────────────────────────────────

/// Insert a completed request into the audit log.
/// Intentionally takes flat params to keep callers readable.
///
/// V5-0: accepts `endpoint` (e.g. `/v1/chat/completions`) and `tool_calls`
/// (extracted JSON for function-calling audit). Both are stored on the
/// `requests` row added by migration 0027.
#[allow(clippy::too_many_arguments)]
pub async fn insert_request(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    latency_ms: i32,
    status: &str,
    is_stream: bool,
    ttfb_ms: Option<i32>,
    prompt_version_id: Option<Uuid>,
    downgrade_triggered: bool,
    endpoint: &str,
    tool_calls: Option<&serde_json::Value>,
    tags: &serde_json::Value,
) -> AppResult<()> {
    // SQLite stores cost_usd as TEXT; rebind as string in sqlite builds.
    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());

    // SQLite has no JSONB — encode tool_calls and tags as JSON text blobs.
    #[cfg(feature = "sqlite")]
    let tool_calls_bind: Option<String> = tool_calls.map(|v| v.to_string());
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let tool_calls_bind: Option<serde_json::Value> = tool_calls.cloned();

    #[cfg(feature = "sqlite")]
    let tags_bind = tags.to_string();
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let tags_bind = tags.clone();

    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, completion_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, ttfb_ms, prompt_version_id,
             downgrade_triggered, endpoint, tool_calls, tags, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
    )
    .bind(Uuid::new_v4())
    .bind(api_key_id)
    .bind(workspace_id)
    .bind(provider)
    .bind(model)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(total_tokens)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(status)
    .bind(is_stream)
    .bind(ttfb_ms)
    .bind(prompt_version_id)
    .bind(downgrade_triggered)
    .bind(endpoint)
    .bind(tool_calls_bind)
    .bind(tags_bind)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// List requests with optional filters. Returns (rows, total_count).
///
/// V3-5 extended filters: start_time, end_time, has_cache_hit.
#[allow(clippy::too_many_arguments)]
pub async fn list_requests(
    pool: &DbPool,
    page: i64,
    per_page: i64,
    provider: Option<&str>,
    model: Option<&str>,
    status: Option<&str>,
    api_key_id: Option<Uuid>,
    start_time: Option<chrono::DateTime<Utc>>,
    end_time: Option<chrono::DateTime<Utc>>,
    has_cache_hit: Option<bool>,
) -> AppResult<(Vec<crate::models::request::Request>, i64)> {
    // PostgreSQL: ::text / ::uuid / ::timestamptz casts are required so the planner
    // knows the parameter type when the value is NULL.
    // SQLite: no cast syntax; plain `$N IS NULL` works for both NULL and non-NULL.
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let (count_sql, list_sql) = (
        "SELECT COUNT(*) FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)
           AND ($5::timestamptz IS NULL OR created_at >= $5)
           AND ($6::timestamptz IS NULL OR created_at <= $6)
           AND ($7::boolean IS NULL
                OR ($7 = TRUE  AND cache_type IS NOT NULL)
                OR ($7 = FALSE AND cache_type IS NULL))",
        "SELECT * FROM requests
         WHERE ($1::text IS NULL OR provider = $1)
           AND ($2::text IS NULL OR model = $2)
           AND ($3::text IS NULL OR status = $3)
           AND ($4::uuid IS NULL OR api_key_id = $4)
           AND ($5::timestamptz IS NULL OR created_at >= $5)
           AND ($6::timestamptz IS NULL OR created_at <= $6)
           AND ($7::boolean IS NULL
                OR ($7 = TRUE  AND cache_type IS NOT NULL)
                OR ($7 = FALSE AND cache_type IS NULL))
         ORDER BY created_at DESC
         LIMIT $8 OFFSET $9",
    );

    #[cfg(feature = "sqlite")]
    let (count_sql, list_sql) = (
        "SELECT COUNT(*) FROM requests
         WHERE ($1 IS NULL OR provider = $1)
           AND ($2 IS NULL OR model = $2)
           AND ($3 IS NULL OR status = $3)
           AND ($4 IS NULL OR api_key_id = $4)
           AND ($5 IS NULL OR created_at >= $5)
           AND ($6 IS NULL OR created_at <= $6)
           AND ($7 IS NULL
                OR ($7 = 1 AND cache_type IS NOT NULL)
                OR ($7 = 0 AND cache_type IS NULL))",
        "SELECT * FROM requests
         WHERE ($1 IS NULL OR provider = $1)
           AND ($2 IS NULL OR model = $2)
           AND ($3 IS NULL OR status = $3)
           AND ($4 IS NULL OR api_key_id = $4)
           AND ($5 IS NULL OR created_at >= $5)
           AND ($6 IS NULL OR created_at <= $6)
           AND ($7 IS NULL
                OR ($7 = 1 AND cache_type IS NOT NULL)
                OR ($7 = 0 AND cache_type IS NULL))
         ORDER BY created_at DESC
         LIMIT $8 OFFSET $9",
    );

    // SQLite stores booleans as integers; bind None/Some(0)/Some(1).
    #[cfg(feature = "sqlite")]
    let cache_hit_bind: Option<i64> = has_cache_hit.map(|b| if b { 1 } else { 0 });
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let cache_hit_bind = has_cache_hit;

    let total: (i64,) = sqlx::query_as(count_sql)
        .bind(provider)
        .bind(model)
        .bind(status)
        .bind(api_key_id)
        .bind(start_time)
        .bind(end_time)
        .bind(cache_hit_bind)
        .fetch_one(pool)
        .await?;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let rows = sqlx::query_as::<_, crate::models::request::Request>(list_sql)
        .bind(provider)
        .bind(model)
        .bind(status)
        .bind(api_key_id)
        .bind(start_time)
        .bind(end_time)
        .bind(cache_hit_bind)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;

    #[cfg(feature = "sqlite")]
    let rows: Vec<crate::models::request::Request> =
        sqlx::query_as::<_, RequestSqliteRow>(list_sql)
            .bind(provider)
            .bind(model)
            .bind(status)
            .bind(api_key_id)
            .bind(start_time)
            .bind(end_time)
            .bind(cache_hit_bind)
            .bind(per_page)
            .bind((page - 1) * per_page)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();

    Ok((rows, total.0))
}

pub async fn get_by_id(
    pool: &DbPool,
    id: Uuid,
) -> AppResult<Option<crate::models::request::Request>> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let row = sqlx::query_as::<_, crate::models::request::Request>(
        "SELECT * FROM requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    let row: Option<crate::models::request::Request> =
        sqlx::query_as::<_, RequestSqliteRow>("SELECT * FROM requests WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .map(Into::into);

    Ok(row)
}

/// Insert an embedding request into the audit log
/// (`request_type = 'embedding'`, `endpoint = '/v1/embeddings'`).
#[allow(clippy::too_many_arguments)]
pub async fn insert_embedding_request(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    prompt_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    latency_ms: i32,
    status: &str,
) -> AppResult<()> {
    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());

    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             prompt_tokens, total_tokens, cost_usd,
             latency_ms, status, stream, request_type, endpoint, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(Uuid::new_v4())
    .bind(api_key_id)
    .bind(workspace_id)
    .bind(provider)
    .bind(model)
    .bind(prompt_tokens)
    .bind(total_tokens)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(status)
    .bind(false)
    .bind("embedding")
    .bind("/v1/embeddings")
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// Insert a non-token modality request (images / audio) into the audit log.
/// Token columns stay NULL; cost reflects modality-specific pricing.
#[allow(clippy::too_many_arguments)]
pub async fn insert_modality_request(
    pool: &DbPool,
    api_key_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    provider: &str,
    model: &str,
    cost_usd: Option<Decimal>,
    latency_ms: i32,
    status: &str,
    request_type: &str,
    endpoint: &str,
) -> AppResult<()> {
    #[cfg(feature = "sqlite")]
    let cost_usd = cost_usd.map(|d| d.to_string());

    sqlx::query(
        "INSERT INTO requests (
             id, api_key_id, workspace_id, provider, model,
             cost_usd, latency_ms, status, stream, request_type, endpoint, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(Uuid::new_v4())
    .bind(api_key_id)
    .bind(workspace_id)
    .bind(provider)
    .bind(model)
    .bind(cost_usd)
    .bind(latency_ms)
    .bind(status)
    .bind(false)
    .bind(request_type)
    .bind(endpoint)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// Insert a replay or playground request and return its new UUID (V4-6).
///
/// Differs from `insert_request` in two ways:
/// - `replay_of_request_id`: links back to the original when replaying.
/// - `is_playground`: flags requests made via `POST /admin/playground`.
/// - Returns the new request's UUID so callers can include it in the response.
#[allow(clippy::too_many_arguments)]
pub async fn insert_request_for_replay(
    pool: &DbPool,
    provider: &str,
    model: &str,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    total_tokens: Option<i32>,
    cost_usd: Option<Decimal>,
    latency_ms: i64,
    status: &str,
    is_stream: bool,
    request_body: Option<&str>,
    replay_of_request_id: Option<Uuid>,
    is_playground: bool,
    cache_type: Option<&str>,
) -> AppResult<Uuid> {
    let id = Uuid::new_v4();

    #[cfg(feature = "sqlite")]
    let cost_usd_bind = cost_usd.map(|d| d.to_string());
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    let cost_usd_bind = cost_usd;

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    sqlx::query(
        "INSERT INTO requests (
             id, provider, model,
             prompt_tokens, completion_tokens, total_tokens, cost_usd,
             latency_ms, status, stream,
             request_body, replay_of_request_id, is_playground, cache_type, created_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(id)
    .bind(provider)
    .bind(model)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(total_tokens)
    .bind(cost_usd_bind)
    .bind(latency_ms as i32)
    .bind(status)
    .bind(is_stream)
    .bind(request_body)
    .bind(replay_of_request_id)
    .bind(is_playground)
    .bind(cache_type)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    #[cfg(feature = "sqlite")]
    {
        let replay_id_str = replay_of_request_id.map(|u| u.to_string());
        sqlx::query(
            "INSERT INTO requests (
                 id, provider, model,
                 prompt_tokens, completion_tokens, total_tokens, cost_usd,
                 latency_ms, status, stream,
                 request_body, replay_of_request_id, is_playground, cache_type, created_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        )
        .bind(id)
        .bind(provider)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .bind(cost_usd_bind)
        .bind(latency_ms as i32)
        .bind(status)
        .bind(is_stream)
        .bind(request_body)
        .bind(replay_id_str)
        .bind(is_playground)
        .bind(cache_type)
        .bind(Utc::now())
        .execute(pool)
        .await?;
    }

    Ok(id)
}

/// Look up per-token prices for a provider+model pair.
/// Returns `(input_per_1m, output_per_1m)` or `None` if not found.
///
/// Falls back to a date-stripped model ID when the exact ID isn't found —
/// e.g. `gpt-4o-mini-2024-07-18` → `gpt-4o-mini`. This handles the common
/// pattern where OpenAI returns a versioned model ID in the response even
/// when the client requested the canonical alias.
pub async fn find_pricing(
    pool: &DbPool,
    provider: &str,
    model: &str,
) -> AppResult<Option<(Decimal, Decimal)>> {
    // Helper: run one SQL lookup and map the row to a price pair.
    async fn query_one(
        pool: &DbPool,
        provider: &str,
        model: &str,
    ) -> AppResult<Option<(Decimal, Decimal)>> {
        let row = sqlx::query_as::<_, PricingRow>(
            "SELECT input_per_1m_tokens, output_per_1m_tokens
             FROM model_pricing
             WHERE provider = $1 AND model_id = $2 AND is_active = TRUE
             LIMIT 1",
        )
        .bind(provider)
        .bind(model)
        .fetch_optional(pool)
        .await?;

        #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
        return Ok(row.map(|r| (r.input_per_1m_tokens, r.output_per_1m_tokens)));

        #[cfg(feature = "sqlite")]
        return Ok(row.and_then(|r| {
            let input = r.input_per_1m_tokens.parse::<Decimal>().ok()?;
            let output = r.output_per_1m_tokens.parse::<Decimal>().ok()?;
            Some((input, output))
        }));
    }

    // 1. Exact match.
    if let Some(pair) = query_one(pool, provider, model).await? {
        return Ok(Some(pair));
    }

    // 2. Fallback: strip trailing date suffix (-YYYY-MM-DD) and retry.
    //    Covers versioned OpenAI IDs like `gpt-4o-mini-2024-07-18`.
    static DATE_SUFFIX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re =
        DATE_SUFFIX.get_or_init(|| regex::Regex::new(r"-\d{4}-\d{2}-\d{2}$").expect("valid regex"));
    if re.is_match(model) {
        let canonical = re.replace(model, "").to_string();
        if let Some(pair) = query_one(pool, provider, &canonical).await? {
            return Ok(Some(pair));
        }
    }

    Ok(None)
}

/// Look up modality-specific pricing for a provider+model (V5-0).
/// Returns `(price_per_image, price_per_audio_second, price_per_character)`.
/// Any column NULL in the DB comes back as `None`.
pub async fn find_modality_pricing(
    pool: &DbPool,
    provider: &str,
    model: &str,
) -> AppResult<(Option<Decimal>, Option<Decimal>, Option<Decimal>)> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    #[derive(sqlx::FromRow)]
    struct Row {
        price_per_image: Option<Decimal>,
        price_per_audio_second: Option<Decimal>,
        price_per_character: Option<Decimal>,
    }
    #[cfg(feature = "sqlite")]
    #[derive(sqlx::FromRow)]
    struct Row {
        price_per_image: Option<String>,
        price_per_audio_second: Option<String>,
        price_per_character: Option<String>,
    }

    let row = sqlx::query_as::<_, Row>(
        "SELECT price_per_image, price_per_audio_second, price_per_character
         FROM model_pricing
         WHERE provider = $1 AND model_id = $2 AND is_active = TRUE
         LIMIT 1",
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(pool)
    .await?;

    let Some(r) = row else {
        return Ok((None, None, None));
    };

    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    return Ok((
        r.price_per_image,
        r.price_per_audio_second,
        r.price_per_character,
    ));

    #[cfg(feature = "sqlite")]
    return Ok((
        r.price_per_image.and_then(|s| s.parse().ok()),
        r.price_per_audio_second.and_then(|s| s.parse().ok()),
        r.price_per_character.and_then(|s| s.parse().ok()),
    ));
}
