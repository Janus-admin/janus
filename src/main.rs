mod config;
mod db;
mod errors;
mod handlers;
mod middleware;
mod models;
mod routes;
mod state;

use config::Config;
use sqlx::postgres::PgPoolOptions;
use state::AppState;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    let state = Arc::new(AppState { pool, config });
    let app = routes::create_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
