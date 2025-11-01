//! Database connection and pool management for Private Coordination layer

use crate::config::PrivateCoordinationConfig;
use crate::error::Result;
use sea_orm::{ConnectOptions, Database, DatabaseConnection, ConnectionTrait, TransactionTrait};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Database manager for Private Coordination layer
#[derive(Clone)]
pub struct DatabaseManager {
    pub conn: Arc<DatabaseConnection>,
}

impl DatabaseManager {
    /// Create a new database manager with connection pool
    pub async fn new(config: &PrivateCoordinationConfig) -> Result<Self> {
        info!("Connecting to database: {}", mask_connection_string(&config.database_url));

        let mut opt = ConnectOptions::new(&config.database_url);
        opt.max_connections(config.max_connections)
            .min_connections(5)
            .connect_timeout(Duration::from_secs(config.connection_timeout_secs))
            .idle_timeout(Duration::from_secs(600))
            .max_lifetime(Duration::from_secs(3600))
            .sqlx_logging(config.enable_sql_logging)
            .sqlx_logging_level(tracing::log::LevelFilter::Debug);

        let conn = Database::connect(opt).await?;

        // Test the connection
        conn.ping().await?;
        info!("Database connection established successfully");

        Ok(Self {
            conn: Arc::new(conn),
        })
    }

    /// Get a reference to the database connection
    pub fn connection(&self) -> &DatabaseConnection {
        &self.conn
    }

    /// Execute a raw SQL query (for operations not covered by SeaORM)
    pub async fn execute_raw(&self, sql: &str) -> Result<sea_orm::ExecResult> {
        debug!("Executing raw SQL: {}", sql);
        let result = self.conn.execute_unprepared(sql).await?;
        Ok(result)
    }

    /// Begin a new transaction
    pub async fn begin_transaction(&self) -> Result<sea_orm::DatabaseTransaction> {
        Ok(self.conn.begin().await?)
    }
}

/// Mask sensitive parts of connection string for logging
fn mask_connection_string(conn_str: &str) -> String {
    if let Some(at_pos) = conn_str.find('@') {
        if let Some(double_slash_pos) = conn_str.find("//") {
            let prefix = &conn_str[..double_slash_pos + 2];
            let suffix = &conn_str[at_pos..];
            return format!("{}****{}", prefix, suffix);
        }
    }
    // If parsing fails, return a generic masked string
    "mysql://****@****".to_string()
}