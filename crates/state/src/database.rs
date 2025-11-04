//! Database connection and operations for state management

use anyhow::{anyhow, Result};
use sea_orm::{
    Database as SeaOrmDatabase, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait, ColumnTrait, Set, ActiveModelTrait, PaginatorTrait,
    Condition, ConnectionTrait,
};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::entity::*;

/// State database connection wrapper
pub struct Database {
    connection: DatabaseConnection,
}

impl Database {
    /// Get a reference to the database connection
    pub fn connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    /// Get an Arc-wrapped reference to the database connection
    pub fn connection_arc(&self) -> std::sync::Arc<DatabaseConnection> {
        std::sync::Arc::new(self.connection.clone())
    }
    /// Create a new database connection with optimized pool settings
    pub async fn new(database_url: &str) -> Result<Self> {
        info!("Connecting to TiDB for state management...");

        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 3;
        const RETRY_DELAY: Duration = Duration::from_secs(2);

        loop {
            attempts += 1;

            // Configure connection pool for high concurrency
            let mut opt = sea_orm::ConnectOptions::new(database_url.to_string());
            opt
                .max_connections(100)                          // Handle concurrent load (default ~10)
                .min_connections(10)                           // Keep connections warm
                .connect_timeout(Duration::from_secs(10))      // Connection establishment timeout
                .acquire_timeout(Duration::from_secs(60))      // Allow time for busy systems (was ~30s)
                .idle_timeout(Duration::from_secs(300))        // Close idle connections after 5min
                .max_lifetime(Duration::from_secs(3600))       // Recreate connections hourly
                .sqlx_logging(true)                            // Enable query logging
                .sqlx_slow_statements_logging_settings(
                    tracing::log::LevelFilter::Warn,
                    Duration::from_millis(500)
                );  // Only log queries that take >500ms

            match SeaOrmDatabase::connect(opt).await {
                Ok(connection) => {
                    info!("Successfully connected to TiDB with connection pool (max: 100, min: 10)");
                    return Ok(Self { connection });
                }
                Err(e) if attempts < MAX_ATTEMPTS => {
                    warn!(
                        "Failed to connect to TiDB (attempt {}/{}): {}",
                        attempts, MAX_ATTEMPTS, e
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(e) => {
                    error!("Failed to connect to TiDB after {} attempts", MAX_ATTEMPTS);
                    return Err(anyhow!("Database connection failed: {}", e));
                }
            }
        }
    }

    /// Health check
    pub async fn health_check(&self) -> Result<()> {
        use sea_orm::PaginatorTrait;
        // Try a simple query to check connection
        let _ = app_instances::Entity::find()
            .paginate(&self.connection, 1)
            .fetch_page(0)
            .await
            .map_err(|e| anyhow!("Health check failed: {}", e))?;
        Ok(())
    }

    /// Verify that an app instance exists and the caller owns it
    pub async fn verify_app_instance_ownership(
        &self,
        app_instance_id: &str,
        public_key: &str,
    ) -> Result<bool> {
        let instance = app_instances::Entity::find_by_id(app_instance_id.to_string())
            .one(&self.connection)
            .await?;

        match instance {
            Some(inst) if inst.owner == public_key => Ok(true),
            Some(_) => Ok(false), // Instance exists but different owner
            None => Err(anyhow!("App instance not found: {}", app_instance_id)),
        }
    }

    /// Get app instance by ID
    pub async fn get_app_instance(
        &self,
        app_instance_id: &str,
    ) -> Result<Option<app_instances::Model>> {
        Ok(app_instances::Entity::find_by_id(app_instance_id.to_string())
            .one(&self.connection)
            .await?)
    }

    /// List app instances owned by a public key
    pub async fn list_app_instances_by_owner(
        &self,
        owner: &str,
        limit: u64,
        offset: u64,
    ) -> Result<(Vec<app_instances::Model>, u64)> {
        let paginator = app_instances::Entity::find()
            .filter(app_instances::Column::Owner.eq(owner))
            .order_by_asc(app_instances::Column::CreatedAt)
            .paginate(&self.connection, limit);

        let total = paginator.num_items().await?;
        let items = paginator.fetch_page(offset / limit).await?;

        Ok((items, total))
    }

    /// Get the latest sequence number for an app instance
    pub async fn get_latest_sequence(
        &self,
        app_instance_id: &str,
    ) -> Result<u64> {
        // Check user_actions for the highest sequence
        let latest = user_actions::Entity::find()
            .filter(user_actions::Column::AppInstanceId.eq(app_instance_id))
            .order_by_desc(user_actions::Column::Sequence)
            .one(&self.connection)
            .await?;

        Ok(latest.map(|m| m.sequence).unwrap_or(0))
    }

    /// Check if an object is currently locked
    pub async fn is_object_locked(&self, object_id: &str) -> Result<bool> {
        use chrono::Utc;

        let now = Utc::now();
        let locked = object_lock_queue::Entity::find()
            .filter(
                Condition::all()
                    .add(object_lock_queue::Column::ObjectId.eq(object_id))
                    .add(object_lock_queue::Column::Status.eq("GRANTED"))
                    .add(object_lock_queue::Column::LeaseUntil.gt(now))
            )
            .one(&self.connection)
            .await?;

        Ok(locked.is_some())
    }

    /// Begin a database transaction
    pub async fn begin_transaction(&self) -> Result<sea_orm::DatabaseTransaction> {
        Ok(self.connection.begin().await?)
    }

    /// Execute raw SQL (for complex queries)
    pub async fn execute_raw(&self, sql: &str) -> Result<()> {
        self.connection
            .execute_unprepared(sql)
            .await?;
        Ok(())
    }
}

/// Helper function to check version for optimistic concurrency
pub async fn check_and_update_version<C>(
    txn: &C,
    object_id: &str,
    expected_version: u64,
    new_version: u64,
    update_fn: impl FnOnce() -> objects::ActiveModel,
) -> Result<bool>
where
    C: ConnectionTrait,
{
    // First check if version matches
    let current = objects::Entity::find_by_id(object_id.to_string())
        .one(txn)
        .await?;

    match current {
        Some(obj) if obj.version == expected_version => {
            // Version matches, proceed with update
            let mut active_model = update_fn();
            active_model.version = Set(new_version);
            active_model.update(txn).await?;
            Ok(true)
        }
        Some(_) => {
            // Version mismatch - concurrent update
            Ok(false)
        }
        None => {
            Err(anyhow!("Object not found: {}", object_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_database_connection() {
        // This test requires STATE_DATABASE_URL to be set
        if let Ok(url) = env::var("STATE_DATABASE_URL") {
            let db = Database::new(&url).await;
            assert!(db.is_ok());

            let db = db.unwrap();
            let health = db.health_check().await;
            assert!(health.is_ok());
        }
    }
}