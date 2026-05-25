//! V5-1: `velox` CLI.
//!
//! Single binary, clap subcommands. Default subcommand is `serve` — running the
//! binary with no args boots the server. `velox doctor` and `velox demo` cover
//! the modes that used to live behind `--doctor` and `--demo`.
//!
//! The CLI talks to a running Velox via the admin API. It reads its endpoint
//! and admin token from `~/.config/velox/cli.toml` (or `--url` / `--token` /
//! `VELOX_URL` / `VELOX_ADMIN_TOKEN`).
//!
//! Subcommands that need direct DB access (`migrate`) speak to the DB
//! configured by `velox.toml` / `DATABASE_URL`, not the admin API.

use clap::{Parser, Subcommand};

pub mod admin_client;
pub mod backup;
pub mod config;
pub mod import;
pub mod keys;
pub mod migrate;

/// Velox — Self-hosted AI gateway.
///
/// Running with no subcommand boots the server (equivalent to `velox serve`).
#[derive(Parser, Debug)]
#[command(name = "velox", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Admin API base URL for CLI subcommands. Default: from cli.toml or `VELOX_URL`,
    /// finally `http://localhost:8080`.
    #[arg(long, global = true, env = "VELOX_URL")]
    pub url: Option<String>,

    /// Admin JWT token used by CLI subcommands. Default: from cli.toml or `VELOX_ADMIN_TOKEN`.
    #[arg(long, global = true, env = "VELOX_ADMIN_TOKEN")]
    pub token: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the Velox server (default if no subcommand is given).
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

    /// Snapshot or restore the Velox installation (DB + models + config).
    #[command(subcommand)]
    Backup(backup::BackupCmd),
}

/// Args mirroring the previous top-level flags so `velox serve` is a true drop-in
/// replacement for the old behaviour.
#[derive(clap::Args, Debug, Default)]
pub struct ServeArgs {}

/// Result type used throughout the CLI subcommands.
pub type CliResult = anyhow::Result<()>;
