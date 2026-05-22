use dashmap::DashMap;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use velox::{
    cache::{embedding::EmbeddingModel, CacheEngine},
    config::Config,
    db::api_keys as db_api_keys,
    gateway::ProviderRegistry,
    middleware::rate_limit::RateLimiter,
    providers::{anthropic::AnthropicProvider, bedrock::BedrockProvider, openai::OpenAIProvider},
    routes::create_router,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load()?;
    let addr = format!("{}:{}", config.host, config.port);

    let pool = PgPoolOptions::new()
        .max_connections(config.db_pool_max_connections)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Database migrations applied");

    // ── Build providers ───────────────────────────────────────────────────────
    let mut providers: Vec<Arc<dyn velox::providers::Provider>> = Vec::new();

    if !config.openai_api_key.is_empty() {
        providers.push(Arc::new(OpenAIProvider::new(
            config.openai_api_key.clone(),
            10,
        )));
        tracing::info!("OpenAI provider enabled");
    }

    if !config.anthropic_api_key.is_empty() {
        providers.push(Arc::new(AnthropicProvider::new(
            config.anthropic_api_key.clone(),
            20,
        )));
        tracing::info!("Anthropic provider enabled");
    }

    // Bedrock is always attempted; it reads credentials from the environment
    let bedrock = BedrockProvider::new(30).await;
    providers.push(Arc::new(bedrock));
    tracing::info!("Bedrock provider enabled");

    // ── Build in-memory API key cache ─────────────────────────────────────────
    let key_cache: Arc<DashMap<[u8; 32], _>> = Arc::new(DashMap::new());
    match db_api_keys::load_all_active(&pool).await {
        Ok(entries) => {
            let count = entries.len();
            for (hash, key) in entries {
                key_cache.insert(hash, key);
            }
            tracing::info!("Loaded {} active API keys into cache", count);
        }
        Err(e) => {
            tracing::warn!("Failed to pre-load API key cache: {e}");
        }
    }

    // ── Build embedding model + cache engine ──────────────────────────────────
    let cache = if std::path::Path::new(&config.embedding_model_path).exists() {
        match EmbeddingModel::load(
            &config.embedding_model_path,
            &config.embedding_tokenizer_path,
        ) {
            Ok(model) => {
                tracing::info!(
                    path = %config.embedding_model_path,
                    "Embedding model loaded; semantic cache enabled"
                );
                Arc::new(CacheEngine::new_with_semantic(
                    Arc::new(model),
                    config.semantic_cache_threshold as f32,
                ))
            }
            Err(e) => {
                tracing::warn!("Failed to load embedding model: {e}; semantic cache disabled");
                Arc::new(CacheEngine::new())
            }
        }
    } else {
        tracing::info!(
            "Embedding model not found at {}; semantic cache disabled",
            config.embedding_model_path
        );
        Arc::new(CacheEngine::new())
    };

    // Warm hot cache + semantic index from DB (enables restart survival).
    let warmed = cache.warm_from_db(&pool).await;
    if warmed > 0 {
        tracing::info!("Warmed cache with {} entries from database", warmed);
    }

    let registry = Arc::new(ProviderRegistry::new(providers, key_cache.clone()));
    let rate_limiter = RateLimiter::new(config.rate_limit_window_secs);

    let state = Arc::new(AppState {
        pool,
        config,
        providers: registry,
        key_cache,
        rate_limiter,
        cache,
    });

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
