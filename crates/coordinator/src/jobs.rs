#[cfg(test)]
use crate::constants::JOB_PROCESSING_CHECK_DELAY_MS;
use crate::coordination_manager::CoordinationManager;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Key for agent method lookup: (developer, agent, agent_method)
type AgentMethodKey = (String, String, String);

/// Tracking info for an app_instance
#[derive(Clone, Debug)]
struct AppInstanceInfo {
    /// When this app_instance was last updated (job added or reconciliation)
    #[allow(dead_code)]
    updated_at: Instant,
    /// Which coordination layer this app_instance belongs to
    layer_id: String,
}

/// Simplified JobsTracker that tracks app_instances with pending jobs
/// and maintains an index for fast lookups by (developer, agent, agent_method)
/// The actual job details are fetched on-demand from the blockchain using indexes
#[derive(Clone)]
pub struct JobsTracker {
    /// Map of app_instance IDs to their tracking info
    app_instances_with_jobs: Arc<RwLock<HashMap<String, AppInstanceInfo>>>,

    /// Index from (developer, agent, agent_method) to set of app_instances
    /// This allows fast lookup when gRPC queries for specific agent methods
    agent_method_index: Arc<RwLock<HashMap<AgentMethodKey, HashSet<String>>>>,

    /// Track last reconciliation time
    #[allow(dead_code)]
    last_reconciliation: Arc<RwLock<Instant>>,

    /// Coordination manager for multi-layer support (optional for backward compatibility)
    #[allow(dead_code)]
    coordination_manager: Option<Arc<CoordinationManager>>,
}

impl JobsTracker {
    pub fn new() -> Self {
        Self {
            app_instances_with_jobs: Arc::new(RwLock::new(HashMap::new())),
            agent_method_index: Arc::new(RwLock::new(HashMap::new())),
            last_reconciliation: Arc::new(RwLock::new(Instant::now())),
            coordination_manager: None,
        }
    }

    /// Create a new JobsTracker with CoordinationManager for multi-layer support
    pub fn new_with_manager(manager: Arc<CoordinationManager>) -> Self {
        Self {
            app_instances_with_jobs: Arc::new(RwLock::new(HashMap::new())),
            agent_method_index: Arc::new(RwLock::new(HashMap::new())),
            last_reconciliation: Arc::new(RwLock::new(Instant::now())),
            coordination_manager: Some(manager),
        }
    }

    /// Add an app_instance that has pending jobs for a specific agent method
    /// Called when we see a JobCreatedEvent
    pub async fn add_job(
        &self,
        app_instance_id: String,
        developer: String,
        agent: String,
        agent_method: String,
        layer_id: String,
    ) {
        info!(
            "🔵 add_job called with: app_instance='{}' (len={}), dev='{}', agent='{}', method='{}', layer='{}'",
            app_instance_id,
            app_instance_id.len(),
            developer,
            agent,
            agent_method,
            layer_id
        );

        // Add to app_instances map with current timestamp and layer_id
        {
            let mut instances = self.app_instances_with_jobs.write().await;
            let is_new = !instances.contains_key(&app_instance_id);
            instances.insert(
                app_instance_id.clone(),
                AppInstanceInfo {
                    updated_at: Instant::now(),
                    layer_id: layer_id.clone(),
                },
            );
            if is_new {
                info!(
                    "✅ Added NEW app_instance {} to tracking on layer {}",
                    app_instance_id, layer_id
                );
            } else {
                debug!(
                    "Updated timestamp for app_instance {} on layer {}",
                    app_instance_id, layer_id
                );
            }
        }

        // Update agent_method_index
        {
            let mut agent_method_index = self.agent_method_index.write().await;
            let key = (developer, agent, agent_method);
            agent_method_index
                .entry(key.clone())
                .or_insert_with(HashSet::new)
                .insert(app_instance_id.clone());
            debug!(
                "Added app_instance {} to index for {:?}",
                app_instance_id, key
            );
        }
    }

    /// Remove an app_instance (e.g., when it has no more pending jobs)
    pub async fn remove_app_instance(&self, app_instance_id: &str) {
        // Remove from main map
        {
            let mut instances = self.app_instances_with_jobs.write().await;
            if instances.remove(app_instance_id).is_some() {
                debug!("Removed app_instance {} from tracking", app_instance_id);
            }
        }

        // Remove from all agent_method_index entries
        {
            let mut agent_method_index = self.agent_method_index.write().await;
            let mut empty_keys = Vec::new();

            for (key, app_instances) in agent_method_index.iter_mut() {
                app_instances.remove(app_instance_id);
                if app_instances.is_empty() {
                    empty_keys.push(key.clone());
                }
            }

            // Remove empty entries
            for key in empty_keys {
                agent_method_index.remove(&key);
                debug!("Removed empty index entry for {:?}", key);
            }
        }
    }

    /// Get all app instances with jobs (for metrics)
    pub async fn get_app_instances_with_jobs(&self) -> Vec<String> {
        let app_instances = self.app_instances_with_jobs.read().await;
        app_instances.keys().cloned().collect()
    }

    /// Get app_instances that have pending jobs for a specific (developer, agent, agent_method)
    /// This is used by gRPC to quickly find relevant app_instances
    #[allow(dead_code)]
    pub async fn get_app_instances_for_agent_method(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Vec<String> {
        let agent_method_index = self.agent_method_index.read().await;
        let key = (
            developer.to_string(),
            agent.to_string(),
            agent_method.to_string(),
        );

        agent_method_index
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all app_instances that potentially have pending jobs
    pub async fn get_all_app_instances(&self) -> Vec<String> {
        let instances = self.app_instances_with_jobs.read().await;
        let result: Vec<String> = instances.keys().cloned().collect();
        info!(
            "🟢 get_all_app_instances returning {} instances: {:?}",
            result.len(),
            result
        );
        result
    }

    /// Get app_instances for a specific coordination layer
    pub async fn get_app_instances_for_layer(&self, layer_id: &str) -> Vec<String> {
        let instances = self.app_instances_with_jobs.read().await;
        let result: Vec<String> = instances
            .iter()
            .filter(|(_, info)| info.layer_id == layer_id)
            .map(|(id, _)| id.clone())
            .collect();

        result
    }

    /// Get the count of tracked app_instances
    pub async fn app_instances_count(&self) -> usize {
        let instances = self.app_instances_with_jobs.read().await;
        instances.len()
    }

    /// Get the layer_id for a specific app_instance
    #[allow(dead_code)]
    pub async fn get_layer_id_for_app_instance(&self, app_instance: &str) -> Option<String> {
        let instances = self.app_instances_with_jobs.read().await;
        instances
            .get(app_instance)
            .map(|info| info.layer_id.clone())
    }

    /// Get all app_instances with their layer_ids
    pub async fn get_all_app_instances_with_layer(&self) -> Vec<(String, String)> {
        let instances = self.app_instances_with_jobs.read().await;
        instances
            .iter()
            .map(|(app_id, info)| (app_id.clone(), info.layer_id.clone()))
            .collect()
    }

    /// Get statistics about the tracker
    pub async fn get_stats(&self) -> TrackerStats {
        let instances = self.app_instances_with_jobs.read().await;
        let agent_method_index = self.agent_method_index.read().await;

        TrackerStats {
            app_instances_count: instances.len(),
            agent_methods_count: agent_method_index.len(),
        }
    }
}

impl Default for JobsTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the tracker state
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub app_instances_count: usize,
    pub agent_methods_count: usize,
}

/// Helper function to fetch pending_jobs_count from embedded Jobs in AppInstance
#[allow(dead_code)]
async fn fetch_pending_jobs_count_from_app_instance(app_instance_id: &str) -> Result<u64> {
    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;

    // Get pending_jobs_count from the Jobs struct if it exists
    if let Some(jobs) = app_instance.jobs {
        Ok(jobs.pending_jobs_count)
    } else {
        Ok(0) // No jobs struct means no pending jobs
    }
}

/// Helper function to check if there are any running jobs for an app instance
#[allow(dead_code)]
async fn check_for_running_jobs(app_instance_id: &str) -> Result<bool> {
    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;

    // Fetch all jobs and check for Running status
    match sui::fetch::fetch_all_jobs_from_app_instance(&app_instance).await {
        Ok(jobs) => {
            let running_count = jobs
                .iter()
                .filter(|job| matches!(job.status, sui::fetch::JobStatus::Running))
                .count();

            if running_count > 0 {
                debug!(
                    "App instance {} has {} running job(s)",
                    app_instance_id, running_count
                );
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Err(e) => {
            // If we can't fetch jobs, assume there might be running jobs to be safe
            debug!("Failed to fetch jobs for {}: {}", app_instance_id, e);
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_remove_with_index() {
        let tracker = JobsTracker::new();

        // Add a job
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Check it was added
        assert_eq!(tracker.app_instances_count().await, 1);
        assert_eq!(tracker.app_instances_count().await, 1);

        // Check the index
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 1);
        assert!(instances.contains(&"app_instance1".to_string()));

        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;

        // Check it was removed from both main map and index
        assert_eq!(tracker.app_instances_count().await, 0);
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_agent_methods_same_instance() {
        let tracker = JobsTracker::new();

        // Add same app_instance for different agent methods
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method2".to_string(),
            )
            .await;

        // Check app_instance is tracked only once
        assert_eq!(tracker.app_instances_count().await, 1);

        // But appears in both indexes
        let instances1 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances1.len(), 1);

        let instances2 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method2")
            .await;
        assert_eq!(instances2.len(), 1);

        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;

        // Check it's removed from all indexes
        let instances1 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances1.len(), 0);

        let instances2 = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method2")
            .await;
        assert_eq!(instances2.len(), 0);
    }

    #[tokio::test]
    async fn test_agent_method_index() {
        let tracker = JobsTracker::new();

        // Add jobs for same agent method but different app instances
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        tracker
            .add_job(
                "app_instance2".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Check the index returns both app_instances
        let instances = tracker
            .get_app_instances_for_agent_method("dev1", "agent1", "method1")
            .await;
        assert_eq!(instances.len(), 2);
        assert!(instances.contains(&"app_instance1".to_string()));
        assert!(instances.contains(&"app_instance2".to_string()));

        // Check total count
        assert_eq!(tracker.app_instances_count().await, 2);
    }

    #[tokio::test]
    async fn test_timestamp_updates() {
        let tracker = JobsTracker::new();

        // Add a job
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method1".to_string(),
            )
            .await;

        // Get the initial timestamp
        let initial_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(
            JOB_PROCESSING_CHECK_DELAY_MS,
        ))
        .await;

        // Add another job to the same instance
        tracker
            .add_job(
                "app_instance1".to_string(),
                "dev1".to_string(),
                "agent1".to_string(),
                "method2".to_string(),
            )
            .await;

        // Check timestamp was updated
        let updated_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };

        assert!(updated_timestamp > initial_timestamp);
    }
}
