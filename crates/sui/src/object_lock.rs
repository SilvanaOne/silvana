use anyhow::Result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sui_sdk_types as sui;
use tracing::debug;

/// RAII-style guard for object locking.
/// When this guard is dropped, the object lock is automatically released.
pub struct ObjectLockGuard {
    manager: ObjectLockManager,
    object_id: sui::Address,
}

impl ObjectLockGuard {
    /// Get the object ID that this guard is locking
    pub fn object_id(&self) -> sui::Address {
        self.object_id
    }
}

impl Drop for ObjectLockGuard {
    fn drop(&mut self) {
        self.manager.release_object(self.object_id);
    }
}

/// Object lock manager to prevent concurrent usage of shared objects
#[derive(Clone)]
pub struct ObjectLockManager {
    locks: Arc<Mutex<HashMap<sui::Address, Instant>>>,
    lock_timeout: Duration,
}

impl ObjectLockManager {
    pub fn new(lock_timeout_seconds: u64) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            lock_timeout: Duration::from_secs(lock_timeout_seconds),
        }
    }

    /// Attempts to lock an object for exclusive use.
    /// Returns `Some(ObjectLockGuard)` if the object was successfully locked; `None` otherwise.
    pub fn try_lock_object(&self, object_id: sui::Address) -> Option<ObjectLockGuard> {
        let mut locks = self.locks.lock();

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|_, lock_time| now.duration_since(*lock_time) < self.lock_timeout);

        use std::collections::hash_map::Entry;
        match locks.entry(object_id) {
            Entry::Occupied(_) => {
                debug!("Object {} is already locked", object_id);
                None // already locked
            }
            Entry::Vacant(entry) => {
                entry.insert(now);
                debug!("Successfully locked object {}", object_id);
                Some(ObjectLockGuard {
                    manager: self.clone(),
                    object_id,
                })
            }
        }
    }

    /// Attempts to lock an object with retries
    pub async fn lock_object_with_retry(
        &self,
        object_id: sui::Address,
        max_retries: u32,
    ) -> Result<ObjectLockGuard> {
        let retry_delay_ms = 100;
        
        for attempt in 1..=max_retries {
            if let Some(guard) = self.try_lock_object(object_id) {
                return Ok(guard);
            }
            
            if attempt < max_retries {
                debug!(
                    "Object {} is locked, retrying... (attempt {}/{})",
                    object_id, attempt, max_retries
                );
                tokio::time::sleep(Duration::from_millis(retry_delay_ms * attempt as u64)).await;
            }
        }
        
        Err(anyhow::anyhow!(
            "Failed to lock object {} after {} attempts",
            object_id,
            max_retries
        ))
    }

    /// Releases an object lock
    fn release_object(&self, object_id: sui::Address) {
        let mut locks = self.locks.lock();
        if locks.remove(&object_id).is_some() {
            debug!("Released lock on object {}", object_id);
        }
    }

    /// Checks if an object is currently locked
    #[allow(dead_code)]
    pub fn is_locked(&self, object_id: sui::Address) -> bool {
        let mut locks = self.locks.lock();

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|_, lock_time| now.duration_since(*lock_time) < self.lock_timeout);

        locks.contains_key(&object_id)
    }

    /// Get the number of currently locked objects
    #[allow(dead_code)]
    pub fn locked_count(&self) -> usize {
        let mut locks = self.locks.lock();
        
        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|_, lock_time| now.duration_since(*lock_time) < self.lock_timeout);
        
        locks.len()
    }
}

/// Global object lock manager instance
static OBJECT_LOCK_MANAGER: std::sync::OnceLock<ObjectLockManager> = std::sync::OnceLock::new();

pub fn get_object_lock_manager() -> &'static ObjectLockManager {
    OBJECT_LOCK_MANAGER.get_or_init(|| {
        debug!("Initializing global object lock manager");
        ObjectLockManager::new(120) // 2 minutes timeout for object locks
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_object_lock_manager() {
        let manager = ObjectLockManager::new(1); // 1 second timeout
        let object_id = sui::Address::from_str("0x123").unwrap();

        // Test locking
        let guard1 = manager.try_lock_object(object_id);
        assert!(guard1.is_some());

        // Test double locking fails
        let guard2 = manager.try_lock_object(object_id);
        assert!(guard2.is_none());

        // Test lock is released when guard is dropped
        drop(guard1);
        let guard3 = manager.try_lock_object(object_id);
        assert!(guard3.is_some());
    }

    #[tokio::test]
    async fn test_lock_with_retry() {
        let manager = ObjectLockManager::new(1);
        let object_id = sui::Address::from_str("0x456").unwrap();

        // Lock the object
        let _guard = manager.try_lock_object(object_id);

        // Try to lock with retry (should fail)
        let result = manager.lock_object_with_retry(object_id, 2).await;
        assert!(result.is_err());
    }
}