// src/demo.rs — V4-0 Demo Mode
//
// `velox --demo` starts with a mock LLM provider that returns canned responses,
// a pre-seeded demo admin user, 2 API keys, and 100 historical requests.
// No real LLM API keys or external network access required.
//
// Demo mode requires the `sqlite` feature for in-memory database support.
// To start: velox --demo
// Login: admin@velox.local / demo-password

use crate::{
    db::DbPool,
    models::provider::HealthStatus,
    providers::{
        ChatCompletionRequest, ChatCompletionResponse, ChatChoice, ChatMessage, ChunkChoice,
        ChunkDelta, ChatCompletionChunk, EmbeddingRequest, EmbeddingResponse, Provider,
        ProviderError, ProviderStream, UsageData,
    },
};
use async_trait::async_trait;
use futures_util::stream;
use std::time::{SystemTime, UNIX_EPOCH};

// ── DemoProvider ──────────────────────────────────────────────────────────────

/// Mock provider that returns pre-canned responses without any network calls.
/// Used in `velox --demo` so evaluators can experience Velox without API keys.
pub struct DemoProvider;

#[async_trait]
impl Provider for DemoProvider {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn priority(&self) -> u8 {
        1
    }

    fn is_enabled(&self) -> bool {
        true
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let user_content = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.as_str())
            .unwrap_or("hello");

        let reply = format!(
            "[Demo mode] You said: \"{}\". \
             This is a canned response from the Velox demo provider. \
             Configure a real provider to get actual LLM responses.",
            &user_content[..user_content.len().min(80)]
        );

        let prompt_tokens = (user_content.len() / 4).max(1) as u32;
        let completion_tokens = (reply.len() / 4).max(1) as u32;

        Ok(ChatCompletionResponse {
            id: format!("demo-{created}"),
            object: "chat.completion".to_string(),
            created,
            model: request.model.clone(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: serde_json::Value::String(reply),
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: UsageData {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ProviderStream, ProviderError> {
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!("demo-{created}");
        let model = request.model.clone();

        let words = vec![
            "[Demo", " mode]", " Streaming", " response", " from", " the", " Velox", " demo",
            " provider.",
        ];

        let chunks: Vec<Result<ChatCompletionChunk, ProviderError>> = words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                Ok(ChatCompletionChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: ChunkDelta {
                            role: if i == 0 {
                                Some("assistant".to_string())
                            } else {
                                None
                            },
                            content: Some(word.to_string()),
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                })
            })
            .chain(std::iter::once(Ok(ChatCompletionChunk {
                id: id.clone(),
                object: "chat.completion.chunk".to_string(),
                created,
                model: model.clone(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: ChunkDelta {
                        role: None,
                        content: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: Some(UsageData {
                    prompt_tokens: 10,
                    completion_tokens: 9,
                    total_tokens: 19,
                }),
            })))
            .collect();

        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus::Healthy
    }

    async fn embeddings(
        &self,
        request: &EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        use crate::providers::{EmbeddingData, EmbeddingUsage};
        // Return a fixed 4-dim zero vector — enough for demo purposes.
        Ok(EmbeddingResponse {
            object: "list".to_string(),
            data: vec![EmbeddingData {
                object: "embedding".to_string(),
                embedding: vec![0.0; 4],
                index: 0,
            }],
            model: request.model.clone(),
            usage: EmbeddingUsage {
                prompt_tokens: 1,
                total_tokens: 1,
            },
        })
    }
}

// ── Demo data seeding ─────────────────────────────────────────────────────────

/// Seed demo data into the given pool:
/// - demo admin user (admin@velox.local / demo-password)
/// - 2 API keys
/// - 100 historical request records with realistic timestamps and costs
pub async fn seed_demo_data(pool: &DbPool) -> anyhow::Result<()> {
    seed_demo_user(pool).await?;
    seed_demo_api_keys(pool).await?;
    seed_demo_requests(pool).await?;
    Ok(())
}

async fn seed_demo_user(pool: &DbPool) -> anyhow::Result<()> {
    let password_hash = bcrypt::hash("demo-password", 4)?;
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (email) DO NOTHING",
    )
    .bind(uuid::Uuid::new_v4())
    .bind("admin@velox.local")
    .bind(password_hash)
    .bind("Demo Admin")
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_demo_api_keys(pool: &DbPool) -> anyhow::Result<()> {
    let keys = [
        ("vx-sk-DemoKey1111111111111111111111111111111111111111", "Demo Key 1 (production)"),
        ("vx-sk-DemoKey2222222222222222222222222222222222222222", "Demo Key 2 (staging)"),
    ];

    for (key_str, name) in &keys {
        let sha256_hex = crate::db::api_keys::sha256_hex(key_str);
        let bcrypt_hash = bcrypt::hash(key_str, 4)?;
        sqlx::query(
            "INSERT INTO api_keys (id, name, key_hash, key_prefix, key_sha256, is_active, created_at)
             VALUES ($1, $2, $3, $4, $5, true, NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(name)
        .bind(bcrypt_hash)
        .bind(&key_str[..12])
        .bind(sha256_hex)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn seed_demo_requests(pool: &DbPool) -> anyhow::Result<()> {
    let models = ["gpt-4o-mini", "gpt-4o", "claude-3-5-sonnet-20241022"];
    let providers = ["openai", "anthropic"];
    let now = chrono::Utc::now();

    for i in 0..100i64 {
        let hours_ago = i * 2;
        let ts = now - chrono::Duration::hours(hours_ago);
        let model = models[(i as usize) % models.len()];
        let provider = providers[(i as usize) % providers.len()];
        let prompt_tokens = 50 + (i * 7 % 200) as i32;
        let completion_tokens = 20 + (i * 13 % 150) as i32;
        let cost = rust_decimal::Decimal::new(
            (prompt_tokens as i64 * 15 + completion_tokens as i64 * 60) / 1_000_000,
            4,
        );
        let latency_ms = 200 + (i * 37 % 800) as i32;

        sqlx::query(
            "INSERT INTO requests (
                id, provider, model, status, prompt_tokens, completion_tokens,
                total_tokens, cost_usd, latency_ms, created_at
             ) VALUES ($1, $2, $3, 'success', $4, $5, $6, $7, $8, $9)
             ON CONFLICT DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(provider)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(prompt_tokens + completion_tokens)
        .bind(cost)
        .bind(latency_ms)
        .bind(ts)
        .execute(pool)
        .await?;
    }
    Ok(())
}
