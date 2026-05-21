use anyhow::Result;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiration_hours: i64,
    pub db_pool_max_connections: u32,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            jwt_expiration_hours: env::var("JWT_EXPIRATION_HOURS")
                .unwrap_or_else(|_| "24".into())
                .parse()?,
            db_pool_max_connections: env::var("DB_POOL_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "5".into())
                .parse()?,
            port: env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?,
        })
    }
}
