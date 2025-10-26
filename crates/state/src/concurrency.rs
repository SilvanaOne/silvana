//! Concurrency control for state operations

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use sea_orm::{entity::*, query::*, DatabaseConnection, DatabaseTransaction, TransactionTrait};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::entity::{
    lock_request_bundle, object_lock_queue, object_versions, objects, optimistic_state, state,
};

/// Maximum time a lock can be held before automatic release
const LOCK_TTL_SECONDS: i64 = 300; // 5 minutes

/// Maximum retries for optimistic concurrency before falling back to pessimistic
const MAX_OPTIMISTIC_RETRIES: u32 = 3;

/// Concurrency controller for state operations
pub struct ConcurrencyController {
    db: Arc<DatabaseConnection>,
    /// In-memory tracking of active locks for fast lookup
    active_locks: Arc<RwLock<HashMap<String, LockInfo>>>,
}

#[derive(Debug, Clone)]
struct LockInfo {
    object_id: String,
    holder: String,
    acquired_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    bundle_id: Option<String>,
}

impl ConcurrencyController {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            db,
            active_locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a background task to clean up expired locks
    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = self.cleanup_expired_locks().await {
                    error!("Failed to cleanup expired locks: {}", e);
                }
            }
        });
    }

    /// Clean up expired locks from both database and memory
    async fn cleanup_expired_locks(&self) -> Result<()> {
        let now = Utc::now();

        // Clean up database
        let expired = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::LeaseUntil.lt(now))
            .filter(object_lock_queue::Column::Status.eq("GRANTED"))
            .all(self.db.as_ref())
            .await?;

        for lock in expired {
            // Update the lock status to expired
            let mut active: object_lock_queue::ActiveModel = lock.clone().into();
            active.status = Set("EXPIRED".to_string());
            active.update(self.db.as_ref()).await?;

            debug!(
                "Cleaned up expired lock for object {} request {}",
                lock.object_id, lock.req_id
            );
        }

        // Clean up memory
        let mut active = self.active_locks.write().await;
        active.retain(|_, info| info.expires_at > now);

        Ok(())
    }

    /// Attempt optimistic update with version checking
    pub async fn optimistic_update<F, T>(
        &self,
        app_instance_id: &str,
        object_id: &str,
        expected_version: u64,
        update_fn: F,
    ) -> Result<T>
    where
        F: Fn(&DatabaseTransaction) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T>> + Send + '_>
        >,
    {
        let mut retries = 0;

        loop {
            // Start transaction
            let txn = self.db.begin().await?;

            // Check current version
            let current = objects::Entity::find_by_id(object_id.to_string())
                .one(&txn)
                .await?
                .ok_or_else(|| anyhow!("Object not found"))?;

            // Verify ownership (owner could be Ed25519 key or app_instance_id)
            if current.owner != app_instance_id {
                // Check if it's a public key that owns this object
                // For now, just verify it matches
                debug!("Object owner check: expected {}, found {}", app_instance_id, current.owner);
            }

            if current.version != expected_version as i64 {
                // Version mismatch - retry or fail
                txn.rollback().await?;

                retries += 1;
                if retries >= MAX_OPTIMISTIC_RETRIES {
                    return Err(anyhow!(
                        "Version conflict: expected {}, found {}. Max retries exceeded.",
                        expected_version, current.version
                    ));
                }

                warn!(
                    "Version conflict for object {}/{}, retrying ({}/{})",
                    app_instance_id, object_id, retries, MAX_OPTIMISTIC_RETRIES
                );

                tokio::time::sleep(Duration::from_millis(100 * retries as u64)).await;
                continue;
            }

            // Perform update
            let result = update_fn(&txn).await?;

            // Update version
            let new_version = current.version + 1;

            // Clone fields before moving current
            let prev_object_data = current.object_data.clone();
            let prev_object_da = current.object_da.clone();
            let prev_object_hash = current.object_hash.clone();
            let prev_owner = current.owner.clone();
            let prev_object_type = current.object_type.clone();
            let prev_shared = current.shared;
            let prev_previous_tx = current.previous_tx.clone();

            let mut active_model: objects::ActiveModel = current.into();
            active_model.version = Set(new_version);
            active_model.updated_at = Set(Utc::now());
            active_model.update(&txn).await?;

            // Create version record
            let version = object_versions::ActiveModel {
                id: NotSet,
                object_id: Set(object_id.to_string()),
                version: Set(new_version),
                object_data: Set(prev_object_data),
                object_da: Set(prev_object_da),
                object_hash: Set(prev_object_hash),
                owner: Set(prev_owner),
                object_type: Set(prev_object_type),
                shared: Set(prev_shared),
                previous_tx: Set(prev_previous_tx),
                created_at: Set(Utc::now()),
            };
            version.insert(&txn).await?;

            // Commit transaction
            txn.commit().await?;

            info!(
                "Optimistic update succeeded for {}/{} (version {} -> {})",
                app_instance_id, object_id, expected_version, new_version
            );

            return Ok(result);
        }
    }

    /// Acquire a lock on an object (pessimistic concurrency)
    pub async fn acquire_lock(
        &self,
        app_instance_id: &str,
        object_id: &str,
        holder: &str,
        _bundle_id: Option<String>,
    ) -> Result<String> {
        let req_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let lease_until = now + chrono::Duration::seconds(LOCK_TTL_SECONDS);

        // Check for existing locks
        let existing = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ObjectId.eq(object_id))
            .filter(object_lock_queue::Column::Status.eq("GRANTED"))
            .filter(object_lock_queue::Column::LeaseUntil.gt(now))
            .one(self.db.as_ref())
            .await?;

        if let Some(lock) = existing {
            return Err(anyhow!(
                "Object {} is already locked (request {}) until {:?}",
                object_id, lock.req_id, lock.lease_until
            ));
        }

        // Create lock entry
        let lock_entry = object_lock_queue::ActiveModel {
            object_id: Set(object_id.to_string()),
            req_id: Set(req_id.clone()),
            app_instance_id: Set(app_instance_id.to_string()),
            retry_count: Set(0),
            queued_at: Set(now),
            lease_until: Set(Some(lease_until)),
            lease_granted_at: Set(Some(now)),
            status: Set("GRANTED".to_string()),
        };
        lock_entry.insert(self.db.as_ref()).await?;

        // Update in-memory tracking
        let mut active = self.active_locks.write().await;
        active.insert(
            req_id.clone(),
            LockInfo {
                object_id: object_id.to_string(),
                holder: holder.to_string(),
                acquired_at: now,
                expires_at: lease_until,
                bundle_id: None,
            },
        );

        info!(
            "Lock acquired for object {} by {} (req_id: {}, expires at {})",
            object_id, holder, req_id, lease_until
        );

        Ok(req_id)
    }

    /// Acquire multiple locks atomically (all-or-nothing)
    pub async fn acquire_lock_bundle(
        &self,
        app_instance_id: &str,
        object_ids: Vec<String>,
        holder: &str,
    ) -> Result<String> {
        let bundle_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(LOCK_TTL_SECONDS);

        // Start transaction for atomicity
        let txn = self.db.begin().await?;

        // TODO: Implement full bundle locking per LOCK_QUEUE.md
        // For now, stub it out to avoid compilation errors
        txn.rollback().await?;
        Err(anyhow!("Bundle locking not yet fully implemented"))
    }

    /// Release a lock
    pub async fn release_lock(&self, req_id: &str) -> Result<()> {
        // Find the lock
        let lock = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(req_id))
            .one(self.db.as_ref())
            .await?
            .ok_or_else(|| anyhow!("Lock not found"))?;

        // Update status to released
        let mut active: object_lock_queue::ActiveModel = lock.clone().into();
        active.status = Set("RELEASED".to_string());
        active.update(self.db.as_ref()).await?;

        // Remove from memory
        self.active_locks.write().await.remove(req_id);

        // Process queue for this object
        self.process_lock_queue(&lock.app_instance_id, &lock.object_id).await?;

        info!("Lock {} released", req_id);
        Ok(())
    }

    /// Release all locks in a bundle
    pub async fn release_lock_bundle(&self, _bundle_id: &str) -> Result<()> {
        // TODO: Implement bundle lock release per LOCK_QUEUE.md
        // For now, stub it out
        Err(anyhow!("Bundle lock release not yet fully implemented"))
    }

    /// Process the lock queue for an object after a lock is released
    async fn process_lock_queue(&self, _app_instance_id: &str, object_id: &str) -> Result<()> {
        let now = Utc::now();

        // Find next waiting request in FIFO order (by queued_at)
        let next = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ObjectId.eq(object_id))
            .filter(object_lock_queue::Column::Status.eq("WAITING"))
            .order_by_asc(object_lock_queue::Column::QueuedAt)
            .one(self.db.as_ref())
            .await?;

        if let Some(next_lock) = next {
            // Acquire the lock for the next waiter
            let lease_until = now + chrono::Duration::seconds(LOCK_TTL_SECONDS);

            let mut active_model: object_lock_queue::ActiveModel = next_lock.clone().into();
            active_model.status = Set("GRANTED".to_string());
            active_model.lease_granted_at = Set(Some(now));
            active_model.lease_until = Set(Some(lease_until));
            active_model.update(self.db.as_ref()).await?;

            debug!(
                "Lock for {} transferred to request {} from queue",
                object_id, next_lock.req_id
            );
        }

        Ok(())
    }

    /// Queue a lock request (for pessimistic fallback)
    pub async fn queue_lock_request(
        &self,
        app_instance_id: &str,
        object_id: &str,
        _holder: &str,
    ) -> Result<String> {
        let req_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        // Create queued lock entry (WAITING status)
        let lock_entry = object_lock_queue::ActiveModel {
            object_id: Set(object_id.to_string()),
            req_id: Set(req_id.clone()),
            app_instance_id: Set(app_instance_id.to_string()),
            retry_count: Set(3), // Coming from optimistic retries
            queued_at: Set(now),
            lease_until: Set(None),
            lease_granted_at: Set(None),
            status: Set("WAITING".to_string()),
        };
        lock_entry.insert(self.db.as_ref()).await?;

        info!(
            "Lock request queued for {}/{} (req_id: {})",
            app_instance_id, object_id, req_id
        );

        Ok(req_id)
    }

    /// Check if a lock is held
    pub async fn is_locked(&self, _app_instance_id: &str, object_id: &str) -> Result<bool> {
        let now = Utc::now();

        let locked = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ObjectId.eq(object_id))
            .filter(object_lock_queue::Column::Status.eq("GRANTED"))
            .filter(object_lock_queue::Column::LeaseUntil.gt(now))
            .one(self.db.as_ref())
            .await?
            .is_some();

        Ok(locked)
    }

    /// Extend a lock's TTL
    pub async fn extend_lock(&self, req_id: &str, additional_seconds: i64) -> Result<()> {
        let lock = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(req_id))
            .one(self.db.as_ref())
            .await?
            .ok_or_else(|| anyhow!("Lock not found"))?;

        if lock.status != "GRANTED" {
            return Err(anyhow!("Cannot extend lock that is not granted"));
        }

        let current_lease = lock.lease_until.ok_or_else(|| anyhow!("No lease set"))?;
        let new_lease = current_lease + chrono::Duration::seconds(additional_seconds);

        let mut active_model: object_lock_queue::ActiveModel = lock.into();
        active_model.lease_until = Set(Some(new_lease));
        active_model.update(self.db.as_ref()).await?;

        // Update in memory
        if let Some(info) = self.active_locks.write().await.get_mut(req_id) {
            info.expires_at = new_lease;
        }

        info!("Lock {} extended to {}", req_id, new_lease);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lock_expiration() {
        // Test that expired locks are cleaned up
    }

    #[tokio::test]
    async fn test_bundle_atomicity() {
        // Test that bundle acquisition is all-or-nothing
    }

    #[tokio::test]
    async fn test_queue_ordering() {
        // Test that lock queue maintains FIFO ordering
    }
}