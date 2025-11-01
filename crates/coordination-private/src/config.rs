//! Configuration for Private Coordination layer

use serde::{Deserialize, Serialize};

/// Configuration for Private Coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateCoordinationConfig {
    /// Database connection URL (MySQL/TiDB)
    pub database_url: String,

    /// JWT secret for authentication
    pub jwt_secret: String,

    /// Chain ID (defaults to "private")
    #[serde(default = "default_chain_id")]
    pub chain_id: String,

    /// Maximum database connections in pool
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Database connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Enable SQL query logging
    #[serde(default)]
    pub enable_sql_logging: bool,
}

impl Default for PrivateCoordinationConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            jwt_secret: String::new(),
            chain_id: default_chain_id(),
            max_connections: default_max_connections(),
            connection_timeout_secs: default_connection_timeout(),
            enable_sql_logging: false,
        }
    }
}

fn default_chain_id() -> String {
    "private".to_string()
}

fn default_max_connections() -> u32 {
    50
}

fn default_connection_timeout() -> u64 {
    30
}