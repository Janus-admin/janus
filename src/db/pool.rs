// src/db/pool.rs
// Database pool abstraction: selects PgPool or SqlitePool at compile time.
// All other modules use `DbPool` — they never import PgPool/SqlitePool directly.

#[cfg(all(feature = "postgres", not(feature = "sqlite")))]
pub type DbPool = sqlx::PgPool;

#[cfg(feature = "sqlite")]
pub type DbPool = sqlx::SqlitePool;

/// Connect to the database and return a pool.
/// Reads `database_url` from the environment / config passed in.
/// Runs migrations from the correct directory based on the active backend.
pub async fn connect(database_url: &str) -> anyhow::Result<DbPool> {
    #[cfg(all(feature = "postgres", not(feature = "sqlite")))]
    {
        let connect_options = database_url
            .parse::<sqlx::postgres::PgConnectOptions>()?
            .ssl_mode(sqlx::postgres::PgSslMode::Prefer);

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect_with(connect_options)
            .await?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| anyhow::anyhow!("Postgres migration failed: {e}"))?;

        Ok(pool)
    }

    #[cfg(feature = "sqlite")]
    {
        // Auto-create the SQLite file if it doesn't exist.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1) // SQLite writer serialisation
            .connect_with(
                database_url
                    .parse::<sqlx::sqlite::SqliteConnectOptions>()?
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .foreign_keys(true),
            )
            .await?;

        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .map_err(|e| anyhow::anyhow!("SQLite migration failed: {e}"))?;

        Ok(pool)
    }
}
