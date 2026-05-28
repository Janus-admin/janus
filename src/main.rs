use clap::Parser;
use dashmap::DashMap;
use janus::{
    cache::{
        embedding::EmbeddingModel, index::qdrant::QdrantIndex, policy::SemanticCachePolicy,
        CacheEngine,
    },
    cli::{Cli, Command},
    cluster::rate_limit::DbRateLimiter,
    config::Config,
    db::{self, api_keys as db_api_keys},
    enterprise::EnterpriseExt,
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
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
            return janus::cli::keys::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Migrate(sub)) => {
            return janus::cli::migrate::run(sub).await;
        }
        Some(Command::Config(sub)) => {
            return janus::cli::config::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Import(sub)) => {
            return janus::cli::import::run(sub, cli.url.as_deref(), cli.token.as_deref()).await;
        }
        Some(Command::Backup(sub)) => {
            return janus::cli::backup::run(sub).await;
        }
    };

    let args = serve_mode;

    let config = Config::load()?;

    // Validate provider TLS config early; fail loudly before binding a port.
    if let Err(e) = config.provider_tls.validate() {
        return Err(anyhow::anyhow!("TLS config error: {}", e));
    }

    // Initialise OTel tracer.
    let otel_provider = janus::telemetry::init_tracer(&config.tracing)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(otel_provider.as_ref().map(|p| {
            use opentelemetry::trace::TracerProvider as _;
            let tracer = p.tracer("janus");
            tracing_opentelemetry::layer().with_tracer(tracer)
        }))
        .init();

    metrics::init_prometheus()?;
    tracing::info!("Prometheus metrics initialized");
    let addr = format!("{}:{}", config.host, config.port);

    let pool = db::pool::connect(&config.database_url).await?;
    tracing::info!("Database connected and migrations applied");

    // ── First-run admin seeding ───────────────────────────────────────────────
    // If ADMIN_EMAIL + ADMIN_PASSWORD are set and the users table is empty,
    // create the admin account automatically so Docker users don't need to
    // call the register endpoint manually.
    if let (Some(email), Some(password)) = (
        config.admin_email.as_deref().filter(|s| !s.is_empty()),
        config.admin_password.as_deref().filter(|s| !s.is_empty()),
    ) {
        match janus::db::users::count(&pool).await {
            Ok(0) => match bcrypt::hash(password, bcrypt::DEFAULT_COST) {
                Ok(hash) => match janus::db::users::create(&pool, email, &hash, "Admin").await {
                    Ok(_) => tracing::info!(email = %email, "First-run admin account created"),
                    Err(e) => tracing::warn!("Admin seed failed (non-fatal): {e}"),
                },
                Err(e) => tracing::warn!("Admin seed: bcrypt hash failed (non-fatal): {e}"),
            },
            Ok(_) => tracing::debug!("Admin seed skipped — users already exist"),
            Err(e) => tracing::warn!("Admin seed: could not count users (non-fatal): {e}"),
        }
    }

    // ── Doctor mode ───────────────────────────────────────────────────────────
    if args.doctor {
        let report = janus::doctor::run_checks(&pool, &config).await;
        janus::doctor::print_report(&report);
        std::process::exit(if report.healthy { 0 } else { 1 });
    }

    // ── Read provider base_urls from DB (V4-0) ────────────────────────────────
    // This makes custom endpoints (Ollama, vLLM, LM Studio) configurable via a
    // single DB UPDATE rather than a code change.
    let db_base_urls = janus::db::providers::load_base_urls(&pool).await;

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
    let mut providers: Vec<Arc<dyn janus::providers::Provider>> = Vec::new();

    if args.demo {
        // Demo mode: use the canned DemoProvider, skip all real LLM adapters.
        providers.push(Arc::new(janus::demo::DemoProvider));
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

        // Only load Bedrock when AWS credentials are present. Without them the
        // adapter still loads but every request fails with a `dispatch failure`
        // taking ~50 ms — and because Bedrock is in the priority list, the
        // failover loop hits it on every model that doesn't have explicit
        // routing rules, dominating gateway latency and producing a flood of
        // 503s under load. Operators who want Bedrock just need to set
        // AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY in the environment.
        let has_aws_creds = std::env::var("AWS_ACCESS_KEY_ID")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            && std::env::var("AWS_SECRET_ACCESS_KEY")
                .map(|v| !v.is_empty())
                .unwrap_or(false);
        if has_aws_creds {
            let bedrock = BedrockProvider::new(30).await;
            providers.push(Arc::new(bedrock));
            tracing::info!("Bedrock provider enabled");
        } else {
            tracing::info!(
                "Bedrock provider skipped: AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY not set"
            );
        }

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
        if let Err(e) = janus::demo::seed_demo_data(&pool).await {
            tracing::warn!("Demo seed failed (non-fatal): {e}");
        } else {
            tracing::info!("Demo data seeded — login: admin@janus.local / demo-password");
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

    // ── Enterprise state ──────────────────────────────────────────────────────
    // Community builds get a zero-cost no-op implementation.
    // Enterprise builds get the real implementation with license validation,
    // audit DB writes, and policy engine (future).
    #[cfg(feature = "enterprise")]
    let enterprise: Arc<dyn EnterpriseExt> =
        janus::enterprise::real::EnterpriseState::new(pool.clone());

    #[cfg(not(feature = "enterprise"))]
    let enterprise: Arc<dyn EnterpriseExt> = Arc::new(janus::enterprise::CommunityEnterprise);

    let (event_tx, _) = tokio::sync::broadcast::channel(1_000);

    let runtime_config = Arc::new(arc_swap::ArcSwap::from_pointee(
        janus::config::RuntimeConfig::from(&config),
    ));

    let time_guard = Arc::new(janus::cache::time_guard::TimeGuard::new(
        &config.time_sensitive_patterns,
    ));
    tracing::info!(
        patterns = config.time_sensitive_patterns.len(),
        "Time-sensitive cache guard initialized"
    );

    let audit = janus::audit::spawn_writer(pool.clone(), config.audit_channel_capacity);

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
        dedup: Arc::new(janus::gateway::dedup::InFlightDeduplicator::new()),
        time_guard,
        models_cache: Arc::new(std::sync::Mutex::new(None)),
        oidc_states: Arc::new(dashmap::DashMap::new()),
        enterprise,
        audit,
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
                match janus::db::cache::prune_expired(&pool_for_prune).await {
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
        let alert_engine = std::sync::Arc::new(janus::alerts::AlertEngine::new(
            state.pool.clone(),
            state.config.smtp.clone(),
        ));
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

    // ── Background: license refresh (enterprise builds only) ─────────────────
    // Re-validates the JWT every 24 h so key rotation and revocation take effect
    // without a restart. The refresh is a no-op in community builds.
    {
        let enterprise_for_refresh = state.enterprise.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
            interval.tick().await; // skip the first immediate tick
            loop {
                interval.tick().await;
                enterprise_for_refresh.refresh_license().await;
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
                    let _ = janus::db::providers::set_health_status(
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
    janus::analytics::quality_score::start(state.pool.clone());

    // ── Background: cluster tasks ─────────────────────────────────────────────
    #[cfg(not(feature = "sqlite"))]
    if state.config.cluster.enabled {
        match janus::cluster::key_sync::start(state.pool.clone(), state.key_cache.clone()).await {
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
        return janus::mcp::transport::stdio::run(state).await;
    }

    let app = create_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    if args.demo {
        tracing::info!("🎮 Janus demo mode at http://{}", addr);
        tracing::info!("   Login: admin@janus.local / demo-password");
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
        janus::telemetry::shutdown(provider);
    }

    tracing::info!("Server shutdown complete");
    Ok(())
}
