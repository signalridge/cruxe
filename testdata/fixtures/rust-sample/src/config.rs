//! Application configuration loaded from environment variables.

use std::env;
use std::fmt;

/// Default port the server listens on.
const DEFAULT_PORT: u16 = 8080;

/// Default database connection pool size.
const DEFAULT_POOL_SIZE: u32 = 5;

/// Application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address to bind the HTTP server to.
    pub bind_address: String,
    /// Port number for the HTTP server.
    pub port: u16,
    /// PostgreSQL connection string.
    pub database_url: String,
    /// Secret key for JWT signing and verification.
    pub jwt_secret: String,
    /// Maximum database connection pool size.
    pub pool_size: u32,
    /// Whether debug logging is enabled.
    pub debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            database_url: "postgres://localhost/cruxe_dev".into(),
            jwt_secret: "development-secret-do-not-use-in-prod".into(),
            pool_size: DEFAULT_POOL_SIZE,
            debug: true,
        }
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Config {{ bind={}:{}, db={}, pool={}, debug={} }}",
            self.bind_address, self.port, self.database_url, self.pool_size, self.debug
        )
    }
}

/// Errors that may occur when loading configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// A required environment variable is missing.
    MissingVar(String),
    /// A variable value could not be parsed.
    InvalidValue { var: String, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingVar(v) => write!(f, "missing env var: {}", v),
            ConfigError::InvalidValue { var, message } => {
                write!(f, "invalid value for {}: {}", var, message)
            }
        }
    }
}

/// Load configuration from environment variables.
///
/// Falls back to defaults for any variable that is not set.
pub fn load(strict: bool) -> Result<Config, ConfigError> {
    let mut config = Config::default();

    if let Ok(addr) = env::var("BIND_ADDRESS") {
        config.bind_address = addr;
    }

    if let Ok(port_str) = env::var("PORT") {
        config.port = port_str.parse().map_err(|_| ConfigError::InvalidValue {
            var: "PORT".into(),
            message: format!("'{}' is not a valid port", port_str),
        })?;
    }

    match env::var("DATABASE_URL") {
        Ok(url) => config.database_url = url,
        Err(_) if strict => return Err(ConfigError::MissingVar("DATABASE_URL".into())),
        Err(_) => {} // use default
    }

    if let Ok(secret) = env::var("JWT_SECRET") {
        config.jwt_secret = secret;
    } else if strict {
        return Err(ConfigError::MissingVar("JWT_SECRET".into()));
    }

    config.debug = env::var("DEBUG")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(config.debug);

    Ok(config)
}
