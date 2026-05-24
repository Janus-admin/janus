// src/cluster/mod.rs
// Multi-node clustering support (V2-6).
//
// cluster.enabled = false  →  in-memory DashMap rate limiting (single-node default)
// cluster.enabled = true   →  DB-backed sliding windows + pg_notify key invalidation
//
// SQLite deployments are always single-node; cluster features are no-ops there.

pub mod rate_limit;

#[cfg(not(feature = "sqlite"))]
pub mod key_sync;
