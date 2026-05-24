// src/cluster/key_sync.rs
// PostgreSQL LISTEN/NOTIFY-based key cache invalidation for multi-node clusters.
//
// When a key is revoked on any node, that node sends:
//   NOTIFY api_key_invalidated, '<sha256_hex>'
//
// All other nodes are listening on this channel.  On receipt they remove the
// key from their local DashMap so it stops working immediately — without a
// restart or a database poll.
//
// Only compiled when the `sqlite` feature is NOT active, because pg_notify
// is a PostgreSQL-specific feature.

use crate::{db::api_keys as db_api_keys, models::api_key::ApiKey};
use dashmap::DashMap;
use std::sync::Arc;

/// Start a background task that subscribes to the `api_key_invalidated`
/// channel and removes revoked keys from `key_cache` on notification.
pub async fn start(
    pool: sqlx::PgPool,
    key_cache: Arc<DashMap<[u8; 32], ApiKey>>,
) -> anyhow::Result<()> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool).await?;
    listener.listen("api_key_invalidated").await?;

    tokio::spawn(async move {
        loop {
            match listener.recv().await {
                Ok(notification) => {
                    let sha256_hex = notification.payload();
                    if let Some(hash) = db_api_keys::parse_sha256_hex(sha256_hex) {
                        key_cache.remove(&hash);
                        tracing::debug!(
                            sha256 = sha256_hex,
                            "Cluster key-sync: evicted revoked key from local cache"
                        );
                    }
                }
                Err(e) => {
                    // PgListener reconnects automatically; just log and continue.
                    tracing::warn!("Cluster key-sync listener error: {e}");
                }
            }
        }
    });

    Ok(())
}
