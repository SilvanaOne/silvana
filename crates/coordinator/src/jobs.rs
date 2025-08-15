use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use sui_rpc::Client;
use crate::error::Result;

/// Key for agent method lookup: (developer, agent, agent_method)
type AgentMethodKey = (String, String, String);

/// Tracking info for an app_instance
#[derive(Clone, Debug)]
struct AppInstanceInfo {
    /// When this app_instance was last updated (job added or reconciliation)
    updated_at: Instant,
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
    last_reconciliation: Arc<RwLock<Instant>>,
}

impl JobsTracker {
    pub fn new() -> Self {
        Self {
            app_instances_with_jobs: Arc::new(RwLock::new(HashMap::new())),
            agent_method_index: Arc::new(RwLock::new(HashMap::new())),
            last_reconciliation: Arc::new(RwLock::new(Instant::now())),
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
    ) {
        // Add to app_instances map with current timestamp
        {
            let mut instances = self.app_instances_with_jobs.write().await;
            let is_new = !instances.contains_key(&app_instance_id);
            instances.insert(
                app_instance_id.clone(),
                AppInstanceInfo {
                    updated_at: Instant::now(),
                },
            );
            if is_new {
                debug!("Added app_instance {} to tracking", app_instance_id);
            } else {
                debug!("Updated timestamp for app_instance {}", app_instance_id);
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
            debug!("Added app_instance {} to index for {:?}", app_instance_id, key);
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

    /// Get app_instances that have pending jobs for a specific (developer, agent, agent_method)
    /// This is used by gRPC to quickly find relevant app_instances
    pub async fn get_app_instances_for_agent_method(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Vec<String> {
        let agent_method_index = self.agent_method_index.read().await;
        let key = (developer.to_string(), agent.to_string(), agent_method.to_string());
        
        agent_method_index
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all app_instances that potentially have pending jobs
    pub async fn get_all_app_instances(&self) -> Vec<String> {
        let instances = self.app_instances_with_jobs.read().await;
        instances.keys().cloned().collect()
    }


    /// Get the count of tracked app_instances
    pub async fn app_instances_count(&self) -> usize {
        let instances = self.app_instances_with_jobs.read().await;
        instances.len()
    }


    /// Reconcile with on-chain state by checking pending_jobs_count for each tracked app_instance
    /// Only removes app_instances that haven't been updated during the reconciliation
    /// Returns true if there are still pending jobs after reconciliation
    pub async fn reconcile_with_chain(&self, client: &mut Client) -> Result<bool> {
        let initial_count = self.app_instances_count().await;
        info!("Starting reconciliation with on-chain state ({} app_instances tracked)", initial_count);
        
        // Get a snapshot of instances to check with their timestamps
        let instances_to_check: Vec<(String, Instant)> = {
            let instances = self.app_instances_with_jobs.read().await;
            instances
                .iter()
                .map(|(id, info)| (id.clone(), info.updated_at))
                .collect()
        };
        
        let mut removed_count = 0;
        let mut instances_with_jobs = 0;
        let mut instances_with_errors = 0;
        let mut skipped_updated = 0;
        
        for (app_instance_id, original_timestamp) in &instances_to_check {
            // Fetch pending_jobs_count directly from the embedded Jobs in AppInstance
            match fetch_pending_jobs_count_from_app_instance(client, app_instance_id).await {
                Ok(count) => {
                    if count == 0 {
                        // Check if the timestamp has changed (new events arrived)
                        let should_remove = {
                            let instances = self.app_instances_with_jobs.read().await;
                            instances
                                .get(app_instance_id)
                                .map(|info| info.updated_at == *original_timestamp)
                                .unwrap_or(false)
                        };
                        
                        if should_remove {
                            debug!("App_instance {} has 0 pending jobs and wasn't updated, removing", app_instance_id);
                            self.remove_app_instance(app_instance_id).await;
                            removed_count += 1;
                        } else {
                            debug!("App_instance {} has 0 pending jobs but was updated during reconciliation, keeping", app_instance_id);
                            skipped_updated += 1;
                        }
                    } else {
                        debug!("App_instance {} has {} pending jobs", app_instance_id, count);
                        instances_with_jobs += 1;
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch pending_jobs_count for {}: {}", app_instance_id, e);
                    instances_with_errors += 1;
                }
            }
        }
        
        // Update last reconciliation time
        let mut last_rec = self.last_reconciliation.write().await;
        *last_rec = Instant::now();
        
        let final_count = self.app_instances_count().await;
        
        info!(
            "Reconciliation completed: {} checked, {} removed, {} have jobs, {} errors, {} skipped (updated during reconciliation). Now tracking {} app_instances",
            instances_to_check.len(),
            removed_count,
            instances_with_jobs,
            instances_with_errors,
            skipped_updated,
            final_count
        );
        
        // Return whether we still have pending jobs
        Ok(final_count > 0)
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
async fn fetch_pending_jobs_count_from_app_instance(client: &mut Client, app_instance_id: &str) -> Result<u64> {
    use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
    use crate::error::CoordinatorError;
    
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch AppInstance {}: {}", formatted_id, e)
        ))?;

    if let Some(proto_object) = response.into_inner().object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Extract pending_jobs_count from the embedded Jobs
                        if let Some(count_field) = jobs_struct.fields.get("pending_jobs_count") {
                            if let Some(prost_types::value::Kind::StringValue(count_str)) = &count_field.kind {
                                if let Ok(count) = count_str.parse::<u64>() {
                                    return Ok(count);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    Err(CoordinatorError::ConfigError(
        format!("Could not extract pending_jobs_count from AppInstance {}", app_instance_id)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_add_and_remove_with_index() {
        let tracker = JobsTracker::new();
        
        // Add a job
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method1".to_string(),
        ).await;
        
        // Check it was added
        assert!(tracker.is_tracking("app_instance1").await);
        assert_eq!(tracker.app_instances_count().await, 1);
        
        // Check the index
        let instances = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method1").await;
        assert_eq!(instances.len(), 1);
        assert!(instances.contains(&"app_instance1".to_string()));
        
        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;
        
        // Check it was removed from both main map and index
        assert!(!tracker.is_tracking("app_instance1").await);
        let instances = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method1").await;
        assert_eq!(instances.len(), 0);
    }
    
    #[tokio::test]
    async fn test_multiple_agent_methods_same_instance() {
        let tracker = JobsTracker::new();
        
        // Add same app_instance for different agent methods
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method1".to_string(),
        ).await;
        
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method2".to_string(),
        ).await;
        
        // Check app_instance is tracked only once
        assert_eq!(tracker.app_instances_count().await, 1);
        
        // But appears in both indexes
        let instances1 = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method1").await;
        assert_eq!(instances1.len(), 1);
        
        let instances2 = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method2").await;
        assert_eq!(instances2.len(), 1);
        
        // Remove the app_instance
        tracker.remove_app_instance("app_instance1").await;
        
        // Check it's removed from all indexes
        let instances1 = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method1").await;
        assert_eq!(instances1.len(), 0);
        
        let instances2 = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method2").await;
        assert_eq!(instances2.len(), 0);
    }
    
    #[tokio::test]
    async fn test_agent_method_index() {
        let tracker = JobsTracker::new();
        
        // Add jobs for same agent method but different app instances
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method1".to_string(),
        ).await;
        
        tracker.add_job(
            "app_instance2".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method1".to_string(),
        ).await;
        
        // Check the index returns both app_instances
        let instances = tracker.get_app_instances_for_agent_method("dev1", "agent1", "method1").await;
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
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method1".to_string(),
        ).await;
        
        // Get the initial timestamp
        let initial_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };
        
        // Wait a bit
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        // Add another job to the same instance
        tracker.add_job(
            "app_instance1".to_string(),
            "dev1".to_string(),
            "agent1".to_string(),
            "method2".to_string(),
        ).await;
        
        // Check timestamp was updated
        let updated_timestamp = {
            let instances = tracker.app_instances_with_jobs.read().await;
            instances.get("app_instance1").unwrap().updated_at
        };
        
        assert!(updated_timestamp > initial_timestamp);
    }
}