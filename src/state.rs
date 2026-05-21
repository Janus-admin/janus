use crate::config::Config;
use sqlx::PgPool;

pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
}
