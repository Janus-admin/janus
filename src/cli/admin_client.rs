//! Thin admin-API client shared by CLI subcommands.
//!
//! Resolution order for the base URL and admin token:
//!   1. `--url` / `--token` flags on the parent `Cli`
//!   2. `VELOX_URL` / `VELOX_ADMIN_TOKEN` env vars
//!   3. `~/.config/velox/cli.toml` (`url = ...`, `admin_token = ...`)
//!   4. defaults: `http://localhost:8080`, no token

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_BASE_URL: &str = "http://localhost:8080";

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    url: Option<String>,
    admin_token: Option<String>,
}

pub struct AdminClient {
    pub base_url: String,
    pub token: Option<String>,
    pub http: reqwest::Client,
}

impl AdminClient {
    pub fn resolve(flag_url: Option<&str>, flag_token: Option<&str>) -> Result<Self> {
        let file = load_file_config().unwrap_or_default();

        let base_url = flag_url
            .map(str::to_string)
            .or(file.url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let token = flag_token.map(str::to_string).or(file.admin_token);

        Ok(Self {
            base_url,
            token,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .context("building reqwest client")?,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{path}")
            }
        )
    }

    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut rb = self.http.request(method, self.url(path));
        if let Some(t) = &self.token {
            rb = rb.bearer_auth(t);
        }
        rb
    }
}

fn cli_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/velox/cli.toml"))
}

fn load_file_config() -> Option<FileConfig> {
    let path = cli_config_path()?;
    let bytes = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&bytes).ok()
}
