//! Cruxe sample Rust library for testing symbol extraction.
//!
//! This crate provides a minimal authentication and request handling
//! framework used as a test fixture.

pub mod auth;
pub mod config;
pub mod db;
pub mod handler;
pub mod types;

use crate::config::Config;
use crate::db::Connection;

/// Application state shared across request handlers.
pub struct AppState {
    pub config: Config,
    pub db: Connection,
}

impl AppState {
    /// Create a new application state from config.
    pub fn new(config: Config) -> Result<Self, db::DatabaseError> {
        let db = Connection::new(&config.database_url)?;
        Ok(Self { config, db })
    }

    /// Check whether the application is healthy.
    pub fn health_check(&self) -> bool {
        self.db.is_connected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check() {
        let config = Config::default();
        let state = AppState::new(config);
        assert!(state.is_ok());
    }
}
