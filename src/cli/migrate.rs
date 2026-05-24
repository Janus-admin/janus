//! `velox migrate` — manage database migrations against the configured DATABASE_URL.
//!
//! Talks directly to the DB (not the admin API) since migrations are an operator
//! task that often runs before the server is even live.

use anyhow::Context;
use clap::Subcommand;

use super::CliResult;

#[derive(Subcommand, Debug)]
pub enum MigrateCmd {
    /// Apply all pending migrations.
    Up,
    /// Show applied/pending migrations and exit.
    Status,
}

pub async fn run(cmd: MigrateCmd) -> CliResult {
    let config = crate::config::Config::load().context("loading velox config")?;
    let pool = crate::db::pool::connect(&config.database_url)
        .await
        .context("connecting to database")?;

    match cmd {
        MigrateCmd::Up => {
            // pool::connect already runs migrations on startup; this is now a no-op
            // but exists so operators have a stable verb. Print the applied count.
            let applied: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);
            println!("Migrations applied. _sqlx_migrations row count: {applied}");
            Ok(())
        }
        MigrateCmd::Status => {
            let rows: Vec<(i64, String, bool)> = sqlx::query_as(
                "SELECT version, description, success FROM _sqlx_migrations ORDER BY version",
            )
            .fetch_all(&pool)
            .await
            .context("querying _sqlx_migrations")?;

            println!("{:<8} {:<50} OK", "VERSION", "DESCRIPTION");
            for (version, description, success) in rows {
                println!(
                    "{version:<8} {description:<50} {}",
                    if success { "yes" } else { "FAILED" }
                );
            }
            Ok(())
        }
    }
}
