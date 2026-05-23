use dashmap::DashMap;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use velox::{
    cache::{embedding::EmbeddingModel, CacheEngine},
    config::Config,
    db::{self, api_keys as db_api_keys},
    gateway::ProviderRegistry,
    metrics,
    middleware::rate_limit::RateLimiter,
    providers::{
        anthropic::AnthropicProvider, bedrock::BedrockProvider, deepseek::DeepSeekProvider,
        gemini::GeminiProvider, groq::GroqProvider, openai::OpenAIProvider,
    },
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

    // Initialize Prometheus metrics exporter
    metrics::init_prometheus()?;
    tracing::info!("Prometheus metrics initialized");

    let config = Config::load()?;
    let addr = format!("{}:{}", config.host, config.port);

    let pool = db::pool::connect(&config.database_url).await?;
    tracing::info!("Database connected and migrations applied");

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

    if !config.gemini_api_key.is_empty() {
        providers.push(Arc::new(GeminiProvider::new(
            config.gemini_api_key.clone(),
            40,
        )));
        tracing::info!("Gemini provider enabled");
    }

    if !config.groq_api_key.is_empty() {
        providers.push(Arc::new(GroqProvider::new(config.groq_api_key.clone(), 50)));
        tracing::info!("Groq provider enabled");
    }

    if !config.deepseek_api_key.is_empty() {
        providers.push(Arc::new(DeepSeekProvider::new(
            config.deepseek_api_key.clone(),
            60,
        )));
        tracing::info!("DeepSeek provider enabled");
    }

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

    // Broadcast channel for the live WebSocket feed. Buffer 1 000 events.
    // Receivers are created per WebSocket connection in the stream handler.
    let (event_tx, _) = tokio::sync::broadcast::channel(1_000);

    let runtime_config = Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let state = Arc::new(AppState {
        pool,
        config,
        runtime_config,
        providers: registry,
        key_cache,
        rate_limiter,
        cache,
        event_tx,
    });

    // ── Background: alert evaluation engine ──────────────────────────────────
    {
        let alert_engine = std::sync::Arc::new(velox::alerts::AlertEngine::new(state.pool.clone()));
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = alert_engine.evaluate().await {
                    tracing::warn!("Alert evaluation error: {e}");
                }
            }
        });
    }

    // ── Background: provider health checks ───────────────────────────────────
    {
        let pool = state.pool.clone();
        let providers_for_hc = state.providers.providers().to_vec();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                for provider in &providers_for_hc {
                    let status = provider.health_check().await;
                    let _ = velox::db::providers::set_health_status(
                        &pool,
                        provider.name(),
                        status.as_str(),
                    )
                    .await;
                    tracing::debug!(provider = provider.name(), status = status.as_str(), "Health check");
                }
            }
        });
    }

    let app = create_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    // Graceful shutdown on SIGINT (Ctrl-C) or SIGTERM (systemd / Kubernetes).
    let shutdown_signal = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl-C handler");
        };

        #[cfg(unix)]
        let sigterm = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to install SIGTERM handler")
                .recv()
                .await;
        };

        // On non-Unix platforms SIGTERM does not exist; just wait on Ctrl-C.
        #[cfg(not(unix))]
        let sigterm = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c  => tracing::info!("Received SIGINT, initiating graceful shutdown"),
            _ = sigterm => tracing::info!("Received SIGTERM, initiating graceful shutdown"),
        }
    };

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    // Cleanup on shutdown
    tracing::info!("Server shutting down, flushing metrics and cache");
    drop(state); // Ensure AppState (including cache and pool) is dropped gracefully

    tracing::info!("Server shutdown complete");
    Ok(())
}
