use crate::jobs::JobsTracker;
use std::sync::Arc;
use sui_rpc::Client;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CurrentAgent {
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
}

#[derive(Clone)]
pub struct SharedState {
    current_agent: Arc<RwLock<Option<CurrentAgent>>>,
    jobs_tracker: JobsTracker,
    sui_client: Client,  // Sui client (cloneable)
}

impl SharedState {
    pub fn new(sui_client: Client) -> Self {
        Self {
            current_agent: Arc::new(RwLock::new(None)),
            jobs_tracker: JobsTracker::new(),
            sui_client,
        }
    }

    pub async fn set_current_agent(&self, developer: String, agent: String, agent_method: String) {
        let mut current = self.current_agent.write().await;
        *current = Some(CurrentAgent {
            developer,
            agent,
            agent_method,
        });
        tracing::info!(
            "Set current agent: {}/{}/{}",
            current.as_ref().unwrap().developer,
            current.as_ref().unwrap().agent,
            current.as_ref().unwrap().agent_method
        );
    }

    pub async fn clear_current_agent(&self) {
        let mut current = self.current_agent.write().await;
        if let Some(agent) = current.as_ref() {
            tracing::info!(
                "Clearing current agent: {}/{}/{}",
                agent.developer,
                agent.agent,
                agent.agent_method
            );
        }
        *current = None;
    }

    pub async fn get_current_agent(&self) -> Option<CurrentAgent> {
        let current = self.current_agent.read().await;
        current.clone()
    }

    /// Add a new job to tracking from JobCreatedEvent
    pub async fn add_job(
        &self,
        _job_id: u64,  // No longer tracking individual job IDs
        developer: String,
        agent: String,
        agent_method: String,
        app_instance: String,
    ) {
        self.jobs_tracker.add_job(
            app_instance.clone(),
            developer.clone(),
            agent.clone(),
            agent_method.clone(),
        ).await;
        
        tracing::info!(
            "Added app_instance {} for {}/{}/{}",
            app_instance, developer, agent, agent_method
        );
    }
    
    /// Remove a completed or failed job (no longer tracking individual jobs)
    pub async fn remove_job(&self, _job_id: u64) {
        // No longer tracking individual jobs, only app_instances
        // The reconciliation process will handle removing app_instances with no pending jobs
        tracing::debug!("Job completion/failure noted (individual jobs not tracked)");
    }

    /// Remove an app_instance when it has no pending jobs
    pub async fn remove_app_instance(&self, app_instance_id: &str) {
        self.jobs_tracker.remove_app_instance(app_instance_id).await;
        tracing::info!("Removed app_instance from tracking: {}", app_instance_id);
    }

    /// Get all app_instances with pending jobs
    pub async fn get_app_instances(&self) -> Vec<String> {
        self.jobs_tracker.get_all_app_instances().await
    }

    /// Check if an app_instance is being tracked
    pub async fn has_app_instance(&self, app_instance_id: &str) -> bool {
        self.jobs_tracker.is_tracking(app_instance_id).await
    }

    /// Get the count of app_instances with pending jobs
    pub async fn app_instances_count(&self) -> usize {
        self.jobs_tracker.app_instances_count().await
    }

    /// Get app_instances with pending jobs for the current agent
    pub async fn get_current_app_instances(&self) -> Vec<String> {
        if let Some(current_agent) = self.get_current_agent().await {
            self.jobs_tracker.get_app_instances_for_agent_method(
                &current_agent.developer,
                &current_agent.agent,
                &current_agent.agent_method,
            ).await
        } else {
            Vec::new()
        }
    }

    /// Check if an app_instance has pending jobs for the current agent
    pub async fn has_current_app_instance(&self, app_instance_id: &str) -> bool {
        let current_instances = self.get_current_app_instances().await;
        current_instances.contains(&app_instance_id.to_string())
    }

    /// Get the count of app_instances with pending jobs for the current agent
    pub async fn current_app_instances_count(&self) -> usize {
        let current_instances = self.get_current_app_instances().await;
        current_instances.len()
    }

    pub fn get_sui_client(&self) -> Client {
        self.sui_client.clone()
    }
    
    /// Get reference to the JobsTracker for direct access
    pub fn get_jobs_tracker(&self) -> &JobsTracker {
        &self.jobs_tracker
    }
}