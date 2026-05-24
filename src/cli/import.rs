//! `velox import` — pull config from competing gateways.
//!
//! V5-1 scope: these subcommands exist and exit with a clear "not yet implemented"
//! message. The full implementation lands in V5-2 (see VELOX_V5_ROADMAP.md §5.4).

use clap::Subcommand;
use std::path::PathBuf;

use super::CliResult;

#[derive(Subcommand, Debug)]
pub enum ImportCmd {
    /// Import from a LiteLLM config.yaml.
    Litellm { path: PathBuf },
    /// Import a Portkey JSON export.
    Portkey { path: PathBuf },
    /// Import OpenRouter virtual keys (requires `OPENROUTER_API_KEY`).
    Openrouter,
}

pub async fn run(cmd: ImportCmd) -> CliResult {
    let source = match cmd {
        ImportCmd::Litellm { .. } => "litellm",
        ImportCmd::Portkey { .. } => "portkey",
        ImportCmd::Openrouter => "openrouter",
    };
    eprintln!(
        "velox import {source}: not implemented yet — lands in V5-2 (see VELOX_V5_ROADMAP.md §5.4)."
    );
    std::process::exit(2);
}
