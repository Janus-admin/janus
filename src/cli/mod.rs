//! V5-1: `janus` CLI.
//!
//! Single binary, clap subcommands. Default subcommand is `serve` — running the
//! binary with no args boots the server. `janus doctor` and `janus demo` cover
//! the modes that used to live behind `--doctor` and `--demo`.
//!
//! The CLI talks to a running Janus via the admin API. It reads its endpoint
//! and admin token from `~/.config/janus/cli.toml` (or `--url` / `--token` /
//! `JANUS_URL` / `JANUS_ADMIN_TOKEN`).
//!
//! Subcommands that need direct DB access (`migrate`) speak to the DB
//! configured by `janus.toml` / `DATABASE_URL`, not the admin API.

use clap::{Parser, Subcommand};

pub mod admin_client;
pub mod backup;
pub mod config;
pub mod import;
pub mod keys;
pub mod migrate;

/// Janus — Self-hosted AI gateway.
///
/// Running with no subcommand boots the server (equivalent to `janus serve`).
#[derive(Parser, Debug)]
#[command(name = "janus", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Admin API base URL for CLI subcommands. Default: from cli.toml or `JANUS_URL`,
    /// finally `http://localhost:8080`.
    #[arg(long, global = true, env = "JANUS_URL")]
    pub url: Option<String>,

    /// Admin JWT token used by CLI subcommands. Default: from cli.toml or `JANUS_ADMIN_TOKEN`.
    #[arg(long, global = true, env = "JANUS_ADMIN_TOKEN")]
    pub token: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the Janus server (default if no subcommand is given).
    Serve(ServeArgs),

    /// Run readiness checks against the configured database and providers, then exit.
    Doctor,

    /// Start in demo mode: mock provider, seeded SQLite, no real API keys required.
    Demo,

    /// Run as an MCP stdio transport (read JSON-RPC from stdin, write to stdout).
    McpStdio,

    /// Manage API keys via the admin API.
    #[command(subcommand)]
    Keys(keys::KeysCmd),

    /// Database migrations.
    #[command(subcommand)]
    Migrate(migrate::MigrateCmd),

    /// Read or update runtime configuration via the admin API.
    #[command(subcommand)]
    Config(config::ConfigCmd),

    /// Import configuration from other gateways (LiteLLM, Portkey, OpenRouter).
    #[command(subcommand)]
    Import(import::ImportCmd),

    /// Snapshot or restore the Janus installation (DB + models + config).
    #[command(subcommand)]
    Backup(backup::BackupCmd),
}

/// Args mirroring the previous top-level flags so `janus serve` is a true drop-in
/// replacement for the old behaviour.
#[derive(clap::Args, Debug, Default)]
pub struct ServeArgs {}

/// Result type used throughout the CLI subcommands.
pub type CliResult = anyhow::Result<()>;
