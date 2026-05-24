use clap::Parser;
use dashmap::DashMap;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use velox::{
    cache::{embedding::EmbeddingModel, policy::SemanticCachePolicy, CacheEngine},
    cluster::rate_limit::DbRateLimiter,
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

/// Velox — Self-hosted AI gateway.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Run in MCP stdio mode: read JSON-RPC 2.0 from stdin, write responses to stdout.
    /// Requires the admin JWT to be passed in the `initialize` message params.token.
    #[arg(long)]
    mcp_stdio: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    dotenvy::dotenv().ok();

    // Config must be loaded before the tracing subscriber so we can add the
    // OTel layer (which needs tracing.otlp_endpoint / service_name) to the
    // registry before calling .init().
    let config = Config::load()?;

    // Initialise OTel tracer. Returns Some(provider) when tracing.enabled = true.
    // The layer is built inline below so the compiler infers the correct S type
    // (after other layers have already been composed onto the registry).
    let otel_provider = velox::telemetry::init_tracer(&config.tracing)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(otel_provider.as_ref().map(|p| {
            // Build the OTel layer at the exact subscriber composition site so
            // the S type parameter is inferred from the layered registry type.
            use opentelemetry::trace::TracerProvider as _;
            let tracer = p.tracer("velox");
            tracing_opentelemetry::layer().with_tracer(tracer)
        }))
        .init();

    // Initialize Prometheus metrics exporter
    metrics::init_prometheus()?;
    tracing::info!("Prometheus metrics initialized");
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
                let arc_model = Arc::new(model);
                let threshold = config.semantic_cache_threshold as f32;
                let engine = if config.semantic_cache_backend == "hnsw" {
                    tracing::info!(
                        path = %config.embedding_model_path,
                        "Embedding model loaded; semantic cache enabled (HNSW backend)"
                    );
                    CacheEngine::new_with_hnsw_semantic(
                        arc_model,
                        threshold,
                        config.semantic_cache_hnsw_ef,
                        config.semantic_cache_hnsw_connections,
                    )
                } else {
                    tracing::info!(
                        path = %config.embedding_model_path,
                        "Embedding model loaded; semantic cache enabled (linear backend)"
                    );
                    CacheEngine::new_with_semantic(arc_model, threshold)
                };
                Arc::new(engine)
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

    // ── Build semantic cache policy ───────────────────────────────────────────
    let semantic_policy = SemanticCachePolicy::new(
        config.semantic_cache_models.clone(),
        config.semantic_cache_exclude_routes.clone(),
        vec![],
    );

    // Warm hot cache + semantic index from DB (enables restart survival).
    let warmed = cache.warm_from_db(&pool).await;
    if warmed > 0 {
        tracing::info!("Warmed cache with {} entries from database", warmed);
    }

    let registry = Arc::new(ProviderRegistry::new(providers, key_cache.clone()));
    let rate_limiter = RateLimiter::new(config.rate_limit_window_secs);

    // Cluster mode: DB-backed rate limiter (None in single-node mode).
    let cluster_rate_limiter = if config.cluster.enabled {
        tracing::info!(
            node_id = %config.cluster.node_id,
            "Cluster mode enabled: using DB-backed rate limiting"
        );
        Some(DbRateLimiter::new(
            pool.clone(),
            config.rate_limit_window_secs,
        ))
    } else {
        None
    };

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
        cluster_rate_limiter,
        cache,
        semantic_policy,
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
                    tracing::debug!(
                        provider = provider.name(),
                        status = status.as_str(),
                        "Health check"
                    );
                }
            }
        });
    }

    // ── Background: cluster tasks (only when cluster.enabled = true) ─────────
    // In non-sqlite builds DbPool = PgPool so key_sync and pg_notify are available.
    #[cfg(not(feature = "sqlite"))]
    if state.config.cluster.enabled {
        // Key-revocation propagation via PostgreSQL LISTEN/NOTIFY.
        match velox::cluster::key_sync::start(state.pool.clone(), state.key_cache.clone()).await {
            Ok(()) => tracing::info!("Cluster key-sync listener started"),
            Err(e) => tracing::warn!("Failed to start cluster key-sync listener: {e}"),
        }

        // Rate-limit window cleanup: delete rows older than 2× the window.
        if let Some(ref cluster_rl) = state.cluster_rate_limiter {
            let rl = cluster_rl.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    if let Err(e) = rl.cleanup().await {
                        tracing::warn!("Rate-limit window cleanup error: {e}");
                    }
                }
            });
        }
    }

    // ── MCP stdio mode (velox --mcp-stdio) ───────────────────────────────────
    if args.mcp_stdio {
        tracing::info!("Starting MCP stdio transport");
        return velox::mcp::transport::stdio::run(state).await;
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

    // Flush any pending OTel spans before exiting.
    if let Some(provider) = otel_provider {
        velox::telemetry::shutdown(provider);
    }

    tracing::info!("Server shutdown complete");
    Ok(())
}
