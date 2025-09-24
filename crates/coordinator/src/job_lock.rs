use crate::constants::STUCK_JOB_TIMEOUT_SECS;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

/// Guard that automatically releases a job lock when dropped
pub struct JobLockGuard {
    manager: JobLockManager,
    job_key: (String, u64), // (app_instance, job_sequence)
}

impl JobLockGuard {
    /// Get the job key that this guard is locking
    #[allow(dead_code)]
    pub fn job_key(&self) -> &(String, u64) {
        &self.job_key
    }
}

impl Drop for JobLockGuard {
    fn drop(&mut self) {
        self.manager.release_job(&self.job_key.0, self.job_key.1);
        debug!(
            "🔓 Released lock for job {} from app_instance {} (guard dropped)",
            self.job_key.1, self.job_key.0
        );
    }
}

/// Job lock manager to prevent concurrent processing of the same job
#[derive(Clone)]
pub struct JobLockManager {
    locks: Arc<Mutex<HashMap<(String, u64), Instant>>>,
    lock_timeout: Duration,
}

impl JobLockManager {
    /// Create a new job lock manager with specified timeout
    pub fn new(lock_timeout_seconds: u64) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            lock_timeout: Duration::from_secs(lock_timeout_seconds),
        }
    }

    /// Attempts to lock a job for exclusive processing.
    /// Returns `Some(JobLockGuard)` if the job was successfully locked; `None` otherwise.
    pub fn try_lock_job(&self, app_instance: &str, job_sequence: u64) -> Option<JobLockGuard> {
        let mut locks = self.locks.lock();
        let job_key = (app_instance.to_string(), job_sequence);

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|key, lock_time| {
            let keep = now.duration_since(*lock_time) < self.lock_timeout;
            if !keep {
                debug!(
                    "⚠️ Removing EXPIRED lock for job {} from app_instance {} (locked for {:?}, timeout: {:?})",
                    key.1,
                    key.0,
                    now.duration_since(*lock_time),
                    self.lock_timeout
                );
            }
            keep
        });

        use std::collections::hash_map::Entry;
        match locks.entry(job_key.clone()) {
            Entry::Occupied(occupied_entry) => {
                let lock_time = occupied_entry.get();
                let duration = now.duration_since(*lock_time);
                debug!(
                    "🔒 Job {} from app_instance {} is already locked (locked for {:?})",
                    job_sequence, app_instance, duration
                );
                None // already locked
            }
            Entry::Vacant(entry) => {
                entry.insert(now);
                debug!(
                    "🔒 Successfully acquired lock for job {} from app_instance {}",
                    job_sequence, app_instance
                );
                Some(JobLockGuard {
                    manager: self.clone(),
                    job_key,
                })
            }
        }
    }

    /// Releases a job lock
    fn release_job(&self, app_instance: &str, job_sequence: u64) {
        let mut locks = self.locks.lock();
        let job_key = (app_instance.to_string(), job_sequence);
        if locks.remove(&job_key).is_some() {
            trace!(
                "🔓 Lock explicitly released for job {} from app_instance {}",
                job_sequence, app_instance
            );
        } else {
            trace!(
                "Attempted to release non-existent lock for job {} from app_instance {}",
                job_sequence, app_instance
            );
        }
    }

    /// Checks if a job is currently locked
    pub fn is_locked(&self, app_instance: &str, job_sequence: u64) -> bool {
        let mut locks = self.locks.lock();
        let job_key = (app_instance.to_string(), job_sequence);

        // Clean up expired locks first
        let now = Instant::now();
        locks.retain(|key, lock_time| {
            let keep = now.duration_since(*lock_time) < self.lock_timeout;
            if !keep {
                debug!(
                    "⚠️ Removing EXPIRED lock for job {} from app_instance {} (locked for {:?}, timeout: {:?})",
                    key.1,
                    key.0,
                    now.duration_since(*lock_time),
                    self.lock_timeout
                );
            }
            keep
        });

        locks.contains_key(&job_key)
    }

    /// Get the number of currently held locks (for debugging)
    #[allow(dead_code)]
    pub fn lock_count(&self) -> usize {
        let locks = self.locks.lock();
        locks.len()
    }

    /// Clear all locks (useful for testing or emergency situations)
    #[allow(dead_code)]
    pub fn clear_all_locks(&self) {
        let mut locks = self.locks.lock();
        let count = locks.len();
        locks.clear();
        if count > 0 {
            debug!("⚠️ Cleared {} job locks", count);
        }
    }
}

/// Global job lock manager instance
static JOB_LOCK_MANAGER: std::sync::OnceLock<JobLockManager> = std::sync::OnceLock::new();

/// Get the global job lock manager instance
/// The lock timeout is set to account for slow blockchain transactions
pub fn get_job_lock_manager() -> &'static JobLockManager {
    JOB_LOCK_MANAGER.get_or_init(|| JobLockManager::new((STUCK_JOB_TIMEOUT_SECS / 1000) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_lock_manager() {
        let manager = JobLockManager::new(60);

        // Should be able to lock a job
        let guard1 = manager.try_lock_job("app1", 1);
        assert!(guard1.is_some());

        // Should not be able to lock the same job again
        let guard2 = manager.try_lock_job("app1", 1);
        assert!(guard2.is_none());

        // Should be able to lock a different job
        let guard3 = manager.try_lock_job("app1", 2);
        assert!(guard3.is_some());

        // Should be able to lock same sequence from different app
        let guard4 = manager.try_lock_job("app2", 1);
        assert!(guard4.is_some());

        // Drop the first guard and verify we can lock it again
        drop(guard1);
        let guard5 = manager.try_lock_job("app1", 1);
        assert!(guard5.is_some());
    }

    #[test]
    fn test_lock_expiry() {
        let manager = JobLockManager::new(0); // 0 second timeout for testing

        // Lock a job
        let _guard = manager.try_lock_job("app1", 1);

        // Sleep to let lock expire
        std::thread::sleep(Duration::from_millis(10));

        // Should be able to lock again after expiry
        let guard2 = manager.try_lock_job("app1", 1);
        assert!(guard2.is_some());
    }
}
