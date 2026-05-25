//! `janus import` — pull config from competing gateways.
//!
//! V5-2 implementation. Each subcommand reads a competitor's config file (or
//! hits a public API) and emits a `MigrationPlan`. The plan can be:
//!
//! - **previewed** (`--dry-run`, the default): printed to stdout so the user
//!   can sanity-check what will change before any side-effect.
//! - **applied** (`--apply`): pushed to the configured Janus via the admin API
//!   (`PATCH /admin/providers/:id`, `POST /admin/keys`, `PATCH /admin/config`).
//!
//! The mappings are documented in JANUS_V5_ROADMAP.md §5.4. Subcommands that
//! cannot create the underlying resource (e.g. OpenRouter has no matching
//! pre-seeded provider) emit suggestions only.

use clap::Subcommand;
use std::path::PathBuf;

use super::CliResult;

pub mod litellm;
pub mod openrouter;
pub mod plan;
pub mod portkey;

pub use plan::{ApplyOutcome, MigrationPlan, ProviderPatch};

#[derive(Subcommand, Debug)]
pub enum ImportCmd {
    /// Import from a LiteLLM `proxy_config.yaml`.
    Litellm {
        /// Path to the LiteLLM YAML config.
        path: PathBuf,
        /// Apply the plan to a running Janus via the admin API.
        /// Without this flag the import runs as a dry-run preview.
        #[arg(long)]
        apply: bool,
    },
    /// Import from a Portkey JSON export.
    Portkey {
        /// Path to the Portkey export JSON.
        path: PathBuf,
        /// Apply the plan to a running Janus via the admin API.
        #[arg(long)]
        apply: bool,
    },
    /// Import the public OpenRouter model list and emit a model-alias report.
    Openrouter {
        /// Override the OpenRouter models endpoint (default: the public API).
        #[arg(long, default_value = "https://openrouter.ai/api/v1/models")]
        url: String,
        /// Read the model list from a file instead of HTTP (useful for offline runs).
        #[arg(long, conflicts_with = "url")]
        from_file: Option<PathBuf>,
    },
}

pub async fn run(cmd: ImportCmd, flag_url: Option<&str>, flag_token: Option<&str>) -> CliResult {
    match cmd {
        ImportCmd::Litellm { path, apply } => litellm::run(path, apply, flag_url, flag_token).await,
        ImportCmd::Portkey { path, apply } => portkey::run(path, apply, flag_url, flag_token).await,
        ImportCmd::Openrouter { url, from_file } => openrouter::run(url, from_file).await,
    }
}
