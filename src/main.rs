use clap::Parser;
use dashmap::DashMap;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use velox::{
    cache::{
        embedding::EmbeddingModel, index::qdrant::QdrantIndex, policy::SemanticCachePolicy,
        CacheEngine,
    },
    cli::{Cli, Command},
    cluster::rate_limit::DbRateLimiter,
    config::Config,
    db::{self, api_keys as db_api_keys},
    gateway::ProviderRegistry,
    metrics,
    middleware::rate_limit::RateLimiter,
    plugins::{self as plugin_mod, content_length::ContentLengthPlugin, pii::PiiRedactionPlugin},
    providers::{
        anthropic::AnthropicProvider, bedrock::BedrockProvider, deepseek::DeepSeekProvider,
        gemini::GeminiProvider, groq::GroqProvider, openai::OpenAIProvider,
    },
    routes::create_router,
    state::AppState,
};

/// Resolved server mode after CLI parsing.
struct ServeMode {
    mcp_stdio: bool,
    doctor: bool,
    demo: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    dotenvy::dotenv().ok();

    // Dispatch CLI subcommands. Anything that isn't "boot the server" exits
    // here without spinning up the full app state.
    let serve_mode = match cli.command {
        None | Some(Command::Serve(_)) => ServeMode {
            mcp_stdio: false,
            doctor: false,
            demo: false,
        },
        Some(Command::Doctor) => ServeMode {
            mcp_stdio: false,
            doctor: true,
            demo: false,
        },
        Some(Command::Demo) => ServeMode {
            mcp_stdio: false,
            doctor: false,
            demo: true,
        },
        Some(Command::McpStdio) => ServeMode {
            mcp_stdio: true,
            doctor: false,
            demo: false,
        },
        Some(Command::Keys(sub)) => {
            return velox::cli::keys::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Migrate(sub)) => {
            return velox::cli::migrate::run(sub).await;
        }
        Some(Command::Config(sub)) => {
            return velox::cli::config::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Import(sub)) => {
            return velox::cli::import::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Backup(sub)) => {
            return velox::cli::backup::run(sub).await;
        }
    };

    let args = serve_mode;

    let config = Config::load()?;

    // Validate provider TLS config early; fail loudly before binding a port.
    if let Err(e) = config.provider_tls.validate() {
        return Err(anyhow::anyhow!("TLS config error: {}", e));
    }

    // Initialise OTel tracer.
    let otel_provider = velox::telemetry::init_tracer(&config.tracing)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(otel_provider.as_ref().map(|p| {
            use opentelemetry::trace::TracerProvider as _;
            let tracer = p.tracer("velox");
            tracing_opentelemetry::layer().with_tracer(tracer)
        }))
        .init();

    metrics::init_prometheus()?;
    tracing::info!("Prometheus metrics initialized");
    let addr = format!("{}:{}", config.host, config.port);

    let pool = db::pool::connect(&config.database_url).await?;
    tracing::info!("Database connected and migrations applied");

    // ── Doctor mode ───────────────────────────────────────────────────────────
    if args.doctor {
        let report = velox::doctor::run_checks(&pool, &config).await;
        velox::doctor::print_report(&report);
        std::process::exit(if report.healthy { 0 } else { 1 });
    }

    // ── Read provider base_urls from DB (V4-0) ────────────────────────────────
    // This makes custom endpoints (Ollama, vLLM, LM Studio) configurable via a
    // single DB UPDATE rather than a code change.
    let db_base_urls = velox::db::providers::load_base_urls(&pool).await;

    fn resolve_base_url(
        db_urls: &std::collections::HashMap<String, String>,
        id: &str,
        default: &str,
    ) -> String {
        db_urls
            .get(id)
            .filter(|u| !u.is_empty())
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    // ── Build providers ───────────────────────────────────────────────────────
    let mut providers: Vec<Arc<dyn velox::providers::Provider>> = Vec::new();

    if args.demo {
        // Demo mode: use the canned DemoProvider, skip all real LLM adapters.
        providers.push(Arc::new(velox::demo::DemoProvider));
        tracing::info!("Demo mode: using DemoProvider (no real API keys required)");
    } else {
        if !config.openai_api_key.is_empty() {
            let base_url = resolve_base_url(&db_base_urls, "openai", "https://api.openai.com/v1");
            providers.push(Arc::new(OpenAIProvider::with_base_url(
                config.openai_api_key.clone(),
                base_url.clone(),
                10,
            )));
            tracing::info!(base_url = %base_url, "OpenAI provider enabled");
        }

        if !config.anthropic_api_key.is_empty() {
            let base_url =
                resolve_base_url(&db_base_urls, "anthropic", "https://api.anthropic.com");
            providers.push(Arc::new(AnthropicProvider::with_base_url(
                config.anthropic_api_key.clone(),
                base_url.clone(),
                20,
            )));
            tracing::info!(base_url = %base_url, "Anthropic provider enabled");
        }

        let bedrock = BedrockProvider::new(30).await;
        providers.push(Arc::new(bedrock));
        tracing::info!("Bedrock provider enabled");

        if !config.gemini_api_key.is_empty() {
            let base_url = resolve_base_url(
                &db_base_urls,
                "gemini",
                "https://generativelanguage.googleapis.com",
            );
            providers.push(Arc::new(GeminiProvider::with_base_url(
                config.gemini_api_key.clone(),
                base_url.clone(),
                40,
            )));
            tracing::info!(base_url = %base_url, "Gemini provider enabled");
        }

        if !config.groq_api_key.is_empty() {
            let base_url =
                resolve_base_url(&db_base_urls, "groq", "https://api.groq.com/openai/v1");
            providers.push(Arc::new(GroqProvider::with_base_url(
                config.groq_api_key.clone(),
                base_url.clone(),
                50,
            )));
            tracing::info!(base_url = %base_url, "Groq provider enabled");
        }

        if !config.deepseek_api_key.is_empty() {
            let base_url =
                resolve_base_url(&db_base_urls, "deepseek", "https://api.deepseek.com/v1");
            providers.push(Arc::new(DeepSeekProvider::with_base_url(
                config.deepseek_api_key.clone(),
                base_url.clone(),
                60,
            )));
            tracing::info!(base_url = %base_url, "DeepSeek provider enabled");
        }
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

    // ── Demo mode: seed data ──────────────────────────────────────────────────
    if args.demo {
        if let Err(e) = velox::demo::seed_demo_data(&pool).await {
            tracing::warn!("Demo seed failed (non-fatal): {e}");
        } else {
            tracing::info!("Demo data seeded — login: admin@velox.local / demo-password");
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
                let engine = if config.semantic_cache_backend == "qdrant" {
                    match QdrantIndex::new(
                        &config.qdrant_url,
                        &config.qdrant_collection,
                        config.qdrant_vector_size,
                    )
                    .await
                    {
                        Ok(qdrant_index) => {
                            tracing::info!(
                                url = %config.qdrant_url,
                                collection = %config.qdrant_collection,
                                "Embedding model loaded; semantic cache enabled (Qdrant backend)"
                            );
                            CacheEngine::new_with_qdrant_semantic(
                                arc_model,
                                threshold,
                                qdrant_index,
                            )
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to connect to Qdrant at {}: {e}; falling back to linear backend",
                                config.qdrant_url
                            );
                            CacheEngine::new_with_semantic(arc_model, threshold)
                        }
                    }
                } else if config.semantic_cache_backend == "hnsw" {
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

    let semantic_policy = SemanticCachePolicy::new(
        config.semantic_cache_models.clone(),
        config.semantic_cache_exclude_routes.clone(),
        vec![],
    );

    let warmed = cache.warm_from_db(&pool).await;
    if warmed > 0 {
        tracing::info!("Warmed cache with {} entries from database", warmed);
    }

    let registry = Arc::new(ProviderRegistry::new(providers, key_cache.clone()));
    let rate_limiter = RateLimiter::new(config.rate_limit_window_secs);

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

    // ── Build plugin chain ────────────────────────────────────────────────────
    let mut plugin_list: Vec<Box<dyn plugin_mod::RequestPlugin>> = Vec::new();
    if config.plugins.pii_redaction {
        plugin_list.push(Box::new(PiiRedactionPlugin));
        tracing::info!("Plugin enabled: pii_redaction");
    }
    if config.plugins.max_prompt_chars > 0 {
        plugin_list.push(Box::new(ContentLengthPlugin {
            max_chars: config.plugins.max_prompt_chars,
        }));
        tracing::info!(
            limit = config.plugins.max_prompt_chars,
            "Plugin enabled: content_length"
        );
    }
    let plugins = Arc::new(plugin_list);

    let (event_tx, _) = tokio::sync::broadcast::channel(1_000);

    let runtime_config = Arc::new(tokio::sync::RwLock::new(
        velox::config::RuntimeConfig::from(&config),
    ));

    let time_guard = Arc::new(velox::cache::time_guard::TimeGuard::new(
        &config.time_sensitive_patterns,
    ));
    tracing::info!(
        patterns = config.time_sensitive_patterns.len(),
        "Time-sensitive cache guard initialized"
    );

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
        plugins,
        dedup: Arc::new(velox::gateway::dedup::InFlightDeduplicator::new()),
        time_guard,
    });

    // ── Background: cache TTL prune (V4-3) ───────────────────────────────────
    // Evicts expired entries from the DB every 5 minutes; also sweeps the hot
    // in-memory layer so lookups stay fast without iterating on cold misses.
    {
        let pool_for_prune = state.pool.clone();
        let cache_for_prune = state.cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                let evicted_hot = cache_for_prune.evict_expired();
                match velox::db::cache::prune_expired(&pool_for_prune).await {
                    Ok(n) => {
                        if n > 0 || evicted_hot > 0 {
                            tracing::info!(
                                db_pruned = n,
                                hot_evicted = evicted_hot,
                                "Cache TTL prune complete"
                            );
                        }
                    }
                    Err(e) => tracing::warn!("Cache TTL prune error: {e}"),
                }
            }
        });
    }

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
    if !args.demo {
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

    // ── Background: provider quality scores ──────────────────────────────────
    velox::analytics::quality_score::start(state.pool.clone());

    // ── Background: cluster tasks ─────────────────────────────────────────────
    #[cfg(not(feature = "sqlite"))]
    if state.config.cluster.enabled {
        match velox::cluster::key_sync::start(state.pool.clone(), state.key_cache.clone()).await {
            Ok(()) => tracing::info!("Cluster key-sync listener started"),
            Err(e) => tracing::warn!("Failed to start cluster key-sync listener: {e}"),
        }

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

    // ── MCP stdio mode ────────────────────────────────────────────────────────
    if args.mcp_stdio {
        tracing::info!("Starting MCP stdio transport");
        return velox::mcp::transport::stdio::run(state).await;
    }

    let app = create_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    if args.demo {
        tracing::info!("🎮 Velox demo mode at http://{}", addr);
        tracing::info!("   Login: admin@velox.local / demo-password");
    } else {
        tracing::info!("Server listening on {}", addr);
    }

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

        #[cfg(not(unix))]
        let sigterm = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c  => tracing::info!("Received SIGINT, initiating graceful shutdown"),
            _ = sigterm => tracing::info!("Received SIGTERM, initiating graceful shutdown"),
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    tracing::info!("Server shutting down, flushing metrics and cache");
    drop(state);

    if let Some(provider) = otel_provider {
        velox::telemetry::shutdown(provider);
    }

    tracing::info!("Server shutdown complete");
    Ok(())
}
