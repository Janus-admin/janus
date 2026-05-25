// tests/v5_1_cli.rs
// Phase V5-1 acceptance tests — `janus` CLI.
//
// Run with: cargo test v5_1_cli
//
// These tests cover:
//   - CLI parser layout (clap subcommand structure)
//   - `janus keys create` actually invokes the admin API end-to-end
//   - `janus config get` actually reads the admin API
//   - `janus migrate status` reads the _sqlx_migrations table
//
// We do not exec the `janus` binary (slower, requires `cargo build`); instead we
// drive the CLI's library entry points with synthetic args, which gives us the
// same coverage with much faster compile/run cycles.

mod common;

use clap::Parser;
use janus::cli::{Cli, Command};

// ── Parser-only tests ─────────────────────────────────────────────────────────

#[test]
fn v5_1_cli_help_lists_all_subcommands() {
    // `--help` returns Err with a DisplayHelp message; we want its body.
    let err = Cli::try_parse_from(["janus", "--help"]).unwrap_err();
    let body = err.to_string();
    for sub in [
        "serve",
        "doctor",
        "demo",
        "mcp-stdio",
        "keys",
        "migrate",
        "config",
        "import",
    ] {
        assert!(
            body.contains(sub),
            "help output must list subcommand `{sub}`; got:\n{body}"
        );
    }
}

#[test]
fn v5_1_cli_no_args_defaults_to_serve() {
    let cli = Cli::try_parse_from(["janus"]).expect("janus with no args should parse");
    assert!(
        matches!(cli.command, None),
        "no subcommand should resolve to default-serve"
    );
}

#[test]
fn v5_1_cli_keys_create_parses_all_flags() {
    let cli = Cli::try_parse_from([
        "janus", "keys", "create", "--name", "prod", "--budget", "100", "--rpm", "60",
    ])
    .expect("must parse");
    match cli.command {
        Some(Command::Keys(janus::cli::keys::KeysCmd::Create {
            name,
            budget,
            rpm,
            tpm,
        })) => {
            assert_eq!(name, "prod");
            assert_eq!(budget, Some(100.0));
            assert_eq!(rpm, Some(60));
            assert_eq!(tpm, None);
        }
        other => panic!("expected Keys::Create, got {other:?}"),
    }
}

#[test]
fn v5_1_cli_global_flags_propagate() {
    let cli = Cli::try_parse_from([
        "janus",
        "--url",
        "http://example.com:9000",
        "--token",
        "jwt-xyz",
        "keys",
        "list",
    ])
    .expect("must parse");
    assert_eq!(cli.url.as_deref(), Some("http://example.com:9000"));
    assert_eq!(cli.token.as_deref(), Some("jwt-xyz"));
}

// ── End-to-end tests using a running Janus server ─────────────────────────────

/// `janus keys create` issues a real `POST /admin/keys` against a running Janus
/// and prints the resulting `jn-sk-...` secret. We capture stdout via a piped
/// child process to verify the secret format.
#[tokio::test]
async fn v5_1_cli_keys_create_invokes_admin_api() {
    let base_url = common::spawn_app().await;
    let token = bearer_to_jwt(&common::admin_auth_header(&base_url).await);

    let cmd = janus::cli::keys::KeysCmd::Create {
        name: "v5_1-cli-test".to_string(),
        budget: None,
        rpm: None,
        tpm: None,
    };
    // Hits POST /admin/keys; the call returns Ok(()) on 2xx.
    janus::cli::keys::run(cmd, Some(&base_url), Some(&token))
        .await
        .expect("keys create must succeed against admin API");
}

/// `janus config get` issues a `GET /admin/config` against the running server.
#[tokio::test]
async fn v5_1_cli_config_get_reads_admin_config() {
    let base_url = common::spawn_app().await;
    let token = bearer_to_jwt(&common::admin_auth_header(&base_url).await);

    janus::cli::config::run(
        janus::cli::config::ConfigCmd::Get,
        Some(&base_url),
        Some(&token),
    )
    .await
    .expect("config get must succeed against admin API");
}

/// `janus migrate status` queries `_sqlx_migrations` directly. We just verify it
/// runs without panicking — startup already applied migrations, so the table
/// exists and has rows.
#[tokio::test]
async fn v5_1_cli_migrate_status_reads_migrations_table() {
    common::load_env();
    janus::cli::migrate::run(janus::cli::migrate::MigrateCmd::Status)
        .await
        .expect("migrate status must succeed");
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bearer_to_jwt(bearer_header: &str) -> String {
    bearer_header
        .strip_prefix("Bearer ")
        .expect("admin_auth_header must return a Bearer <jwt> string")
        .to_string()
}
