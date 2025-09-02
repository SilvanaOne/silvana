use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

/// Job status in cache
#[derive(Debug, Clone, Copy)]
enum JobCacheStatus {
    Failed,
}

/// Entry in the jobs cache
#[derive(Debug, Clone)]
struct JobCacheEntry {
    status: JobCacheStatus,
    timestamp: Instant,
}

/// Cache for tracking failed jobs with automatic expiry
/// - Failed jobs are cached for 5 minutes to prevent retry storms
#[derive(Clone)]
pub struct JobsCache {
    cache: Arc<RwLock<HashMap<(String, u64), JobCacheEntry>>>, // (app_instance, job_sequence) -> cache entry
    failed_expiry: Duration,
}

impl JobsCache {
    /// Create a new jobs cache with default expiry time
    /// - Failed jobs: 5 minutes
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            failed_expiry: Duration::from_secs(300),  // 5 minutes for failed jobs
        }
    }

    /// Create with custom expiry duration
    #[allow(dead_code)]
    pub fn with_expiry(failed_expiry: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            failed_expiry,
        }
    }

    /// Add a failed job to the cache (will be cached for 5 minutes)
    pub async fn add_failed_job(&self, app_instance: String, job_sequence: u64) {
        let mut cache = self.cache.write().await;
        let key = (app_instance.clone(), job_sequence);
        cache.insert(key, JobCacheEntry {
            status: JobCacheStatus::Failed,
            timestamp: Instant::now(),
        });
        debug!("Added failed job {} from app_instance {} to cache (5 min expiry)", job_sequence, app_instance);
        
        // Clean up expired entries while we have the write lock
        self.cleanup_expired_internal(&mut cache);
    }


    /// Check if a job is recently failed (within 5 minutes)
    #[allow(dead_code)]
    pub async fn is_recently_failed(&self, app_instance: &str, job_sequence: u64) -> bool {
        let cache = self.cache.read().await;
        let key = (app_instance.to_string(), job_sequence);
        
        if let Some(entry) = cache.get(&key) {
            if matches!(entry.status, JobCacheStatus::Failed) {
                let elapsed = entry.timestamp.elapsed();
                if elapsed < self.failed_expiry {
                    debug!(
                        "Job {} from app_instance {} was recently failed ({:.1}s ago)",
                        job_sequence, app_instance, elapsed.as_secs_f64()
                    );
                    return true;
                }
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
    fn cleanup_expired_internal(&self, cache: &mut HashMap<(String, u64), JobCacheEntry>) {
        let now = Instant::now();
        cache.retain(|key, entry| {
            let elapsed = now.duration_since(entry.timestamp);
            
            if elapsed >= self.failed_expiry {
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

    /// Get the number of cached jobs
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Clear all cached jobs
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }

    /// Remove a specific job from the cache (useful when job completes successfully)
    #[allow(dead_code)]
    pub async fn remove_job(&self, app_instance: &str, job_sequence: u64) {
        let mut cache = self.cache.write().await;
        let key = (app_instance.to_string(), job_sequence);
        if cache.remove(&key).is_some() {
            debug!("Removed job {} from app_instance {} from cache", job_sequence, app_instance);
        }
    }
}

impl Default for JobsCache {
    fn default() -> Self {
        Self::new()
    }
}

// Keep backward compatibility aliases
#[allow(dead_code)]
pub type FailedJobsCache = JobsCache;