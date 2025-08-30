use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

/// Cache for tracking failed job start attempts with automatic expiry
#[derive(Clone)]
pub struct FailedJobsCache {
    cache: Arc<RwLock<HashMap<(String, u64), Instant>>>, // (app_instance, job_sequence) -> last_failure_time
    expiry_duration: Duration,
}

impl FailedJobsCache {
    /// Create a new failed jobs cache with 5-minute expiry
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            expiry_duration: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Create with custom expiry duration
    #[allow(dead_code)]
    pub fn with_expiry(expiry_duration: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            expiry_duration,
        }
    }

    /// Add a failed job to the cache
    pub async fn add_failed_job(&self, app_instance: String, job_sequence: u64) {
        let mut cache = self.cache.write().await;
        let key = (app_instance.clone(), job_sequence);
        cache.insert(key, Instant::now());
        debug!("Added failed job {} from app_instance {} to cache", job_sequence, app_instance);
        
        // Clean up expired entries while we have the write lock
        self.cleanup_expired_internal(&mut cache);
    }

    /// Check if a job recently failed to start
    pub async fn is_recently_failed(&self, app_instance: &str, job_sequence: u64) -> bool {
        let cache = self.cache.read().await;
        let key = (app_instance.to_string(), job_sequence);
        
        if let Some(failure_time) = cache.get(&key) {
            let elapsed = failure_time.elapsed();
            if elapsed < self.expiry_duration {
                debug!(
                    "Job {} from app_instance {} was recently failed ({:.1}s ago)",
                    job_sequence, app_instance, elapsed.as_secs_f64()
                );
                return true;
            }
        }
        false
    }

    /// Clean up expired entries
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        self.cleanup_expired_internal(&mut cache);
    }

    /// Internal cleanup function that requires a mutable reference to the cache
    fn cleanup_expired_internal(&self, cache: &mut HashMap<(String, u64), Instant>) {
        let now = Instant::now();
        cache.retain(|key, failure_time| {
            let elapsed = now.duration_since(*failure_time);
            if elapsed >= self.expiry_duration {
                debug!(
                    "Removing expired failed job {} from app_instance {} from cache",
                    key.1, key.0
                );
                false
            } else {
                true
            }
        });
    }

    /// Get the number of cached failed jobs
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Clear all cached failed jobs
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

impl Default for FailedJobsCache {
    fn default() -> Self {
        Self::new()
    }
}