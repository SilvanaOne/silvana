use crate::job_id::generate_job_id;
use crate::state::SharedState;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use sui::fetch::Job;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Job information stored in memory for tracking agent interactions
#[derive(Debug, Clone)]
pub struct AgentJob {
    pub job_id: String,
    pub job_sequence: u64,
    pub app_instance: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub pending_job: Job,
    #[allow(dead_code)]
    pub sent_at: u64, // Unix timestamp when job was sent to agent
    pub start_tx_sent: bool, // Whether start_job transaction was sent
    pub start_tx_hash: Option<String>, // Transaction hash from start_job
}

impl AgentJob {
    pub fn new(pending_job: Job, state: &SharedState) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let job_id = generate_job_id(
            state,
            &pending_job.developer,
            &pending_job.agent,
            &pending_job.agent_method,
            &pending_job.app_instance,
            pending_job.job_sequence,
            timestamp,
        );

        Self {
            job_id,
            job_sequence: pending_job.job_sequence,
            app_instance: pending_job.app_instance.clone(),
            developer: pending_job.developer.clone(),
            agent: pending_job.agent.clone(),
            agent_method: pending_job.agent_method.clone(),
            pending_job,
            sent_at: timestamp,
            start_tx_sent: false,
            start_tx_hash: None,
        }
    }
}

/// Memory database for tracking jobs sent to agents
#[derive(Clone)]
pub struct AgentJobDatabase {
    /// Jobs ready to be sent (start_job transaction completed)
    ready_jobs: Arc<RwLock<HashMap<String, AgentJob>>>, // key: agent_key (dev/agent/method)

    /// Jobs sent to agents but not yet completed/failed
    pending_jobs: Arc<RwLock<HashMap<String, AgentJob>>>, // key: job_id

    /// Job lookup by job_id for quick access
    job_lookup: Arc<RwLock<HashMap<String, AgentJob>>>, // key: job_id
}

impl AgentJobDatabase {
    pub fn new() -> Self {
        Self {
            ready_jobs: Arc::new(RwLock::new(HashMap::new())),
            pending_jobs: Arc::new(RwLock::new(HashMap::new())),
            job_lookup: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a job that has had its start_job transaction sent successfully
    /// This job is ready to be returned by get_job
    pub async fn add_ready_job(&self, mut agent_job: AgentJob) {
        agent_job.start_tx_sent = true;
        let agent_key = self.get_agent_key(
            &agent_job.developer,
            &agent_job.agent,
            &agent_job.agent_method,
        );

        {
            let mut ready = self.ready_jobs.write().await;
            ready.insert(agent_key.clone(), agent_job.clone());
        }

        {
            let mut lookup = self.job_lookup.write().await;
            lookup.insert(agent_job.job_id.clone(), agent_job.clone());
        }

        debug!(
            "Added ready job {} for agent {}/{}:{}",
            agent_job.job_id, agent_job.developer, agent_job.agent, agent_job.agent_method
        );
    }

    /// Get and remove a ready job for the specified agent method
    /// Moves the job to pending_jobs for tracking
    pub async fn get_ready_job(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Option<AgentJob> {
        let agent_key = self.get_agent_key(developer, agent, agent_method);

        let job = {
            let mut ready = self.ready_jobs.write().await;
            ready.remove(&agent_key)
        };

        if let Some(job) = job {
            // Move to pending jobs for tracking
            {
                let mut pending = self.pending_jobs.write().await;
                pending.insert(job.job_id.clone(), job.clone());
            }

            debug!(
                "Retrieved ready job {} for agent {}/{}:{}",
                job.job_id, developer, agent, agent_method
            );

            Some(job)
        } else {
            debug!(
                "No ready jobs for agent {}/{}:{}",
                developer, agent, agent_method
            );
            None
        }
    }

    /// Mark a job as completed and remove it from tracking
    pub async fn complete_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut pending = self.pending_jobs.write().await;
            pending.remove(job_id)
        };

        if let Some(job) = job {
            {
                let mut lookup = self.job_lookup.write().await;
                lookup.remove(job_id);
            }

            debug!("Completed job {}", job_id);
            Some(job)
        } else {
            warn!("Attempted to complete non-existent job: {}", job_id);
            None
        }
    }

    /// Mark a job as failed and remove it from tracking
    pub async fn fail_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut pending = self.pending_jobs.write().await;
            pending.remove(job_id)
        };

        if let Some(job) = job {
            {
                let mut lookup = self.job_lookup.write().await;
                lookup.remove(job_id);
            }

            info!("Failed job {}", job_id);
            Some(job)
        } else {
            warn!("Attempted to fail non-existent job: {}", job_id);
            None
        }
    }

    /// Terminate a job and remove it from tracking
    pub async fn terminate_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut pending = self.pending_jobs.write().await;
            pending.remove(job_id)
        };

        if let Some(job) = job {
            {
                let mut lookup = self.job_lookup.write().await;
                lookup.remove(job_id);
            }

            info!("Terminated job {}", job_id);
            Some(job)
        } else {
            warn!("Attempted to terminate non-existent job: {}", job_id);
            None
        }
    }

    /// Get a job by job_id for reference (doesn't remove it)
    pub async fn get_job_by_id(&self, job_id: &str) -> Option<AgentJob> {
        let lookup = self.job_lookup.read().await;
        lookup.get(job_id).cloned()
    }

    /// Add a job directly to pending jobs (job has already been started and is being sent to agent)
    pub async fn add_to_pending(&self, agent_job: AgentJob) {
        {
            let mut pending = self.pending_jobs.write().await;
            pending.insert(agent_job.job_id.clone(), agent_job.clone());
        }

        {
            let mut lookup = self.job_lookup.write().await;
            lookup.insert(agent_job.job_id.clone(), agent_job.clone());
        }

        debug!("Added job {} to pending jobs", agent_job.job_id);
    }

    /// Get all pending jobs that haven't been completed/failed
    /// Used for cleanup when Docker containers terminate
    #[allow(dead_code)]
    pub async fn get_pending_jobs(&self) -> Vec<AgentJob> {
        let pending = self.pending_jobs.read().await;
        pending.values().cloned().collect()
    }

    /// Clean up both ready and pending jobs for a specific agent method
    /// Returns jobs that need to be failed on the blockchain
    #[allow(dead_code)]
    pub async fn cleanup_all_jobs_for_agent(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Vec<AgentJob> {
        let mut jobs_to_fail = Vec::new();
        let agent_key = self.get_agent_key(developer, agent, agent_method);

        debug!(
            "Cleanup: Looking for jobs to clean up for agent key: {}",
            agent_key
        );

        // Clean up ready jobs (jobs that had start_job called but were never retrieved by GetJob)
        {
            let mut ready = self.ready_jobs.write().await;
            if let Some(ready_job) = ready.remove(&agent_key) {
                debug!("Cleanup: Found ready job to fail: {}", ready_job.job_id);
                jobs_to_fail.push(ready_job.clone());

                // Also remove from lookup
                let mut lookup = self.job_lookup.write().await;
                lookup.remove(&ready_job.job_id);
            } else {
                debug!("Cleanup: No ready jobs found for agent key: {}", agent_key);
            }
        }

        // Clean up pending jobs (jobs that were retrieved by GetJob but not completed/failed)
        {
            let mut pending = self.pending_jobs.write().await;
            let mut lookup = self.job_lookup.write().await;

            debug!(
                "Cleanup: Checking {} pending jobs for matches",
                pending.len()
            );

            // Find jobs matching the agent method
            let matching_jobs: Vec<String> = pending
                .iter()
                .filter(|(_, job)| {
                    job.developer == developer
                        && job.agent == agent
                        && job.agent_method == agent_method
                })
                .map(|(job_id, _)| job_id.clone())
                .collect();

            debug!(
                "Cleanup: Found {} matching pending jobs to fail: {:?}",
                matching_jobs.len(),
                matching_jobs
            );

            // Remove matching jobs and collect them for failing
            for job_id in matching_jobs {
                if let Some(job) = pending.remove(&job_id) {
                    debug!("Cleanup: Removing pending job: {}", job_id);
                    lookup.remove(&job_id);
                    jobs_to_fail.push(job);
                }
            }
        }

        if !jobs_to_fail.is_empty() {
            info!(
                "Cleaned up {} total jobs (ready + pending) for agent {}/{}:{}",
                jobs_to_fail.len(),
                developer,
                agent,
                agent_method
            );
        }

        jobs_to_fail
    }

    /// Clean up pending jobs for a specific agent method
    /// Returns jobs that need to be failed on the blockchain
    #[allow(dead_code)]
    pub async fn cleanup_pending_jobs_for_agent(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Vec<AgentJob> {
        let mut jobs_to_fail = Vec::new();

        {
            let mut pending = self.pending_jobs.write().await;
            let mut lookup = self.job_lookup.write().await;

            // Find jobs matching the agent method
            let matching_jobs: Vec<String> = pending
                .iter()
                .filter(|(_, job)| {
                    job.developer == developer
                        && job.agent == agent
                        && job.agent_method == agent_method
                })
                .map(|(job_id, _)| job_id.clone())
                .collect();

            // Remove matching jobs and collect them for failing
            for job_id in matching_jobs {
                if let Some(job) = pending.remove(&job_id) {
                    lookup.remove(&job_id);
                    jobs_to_fail.push(job);
                }
            }
        }

        if !jobs_to_fail.is_empty() {
            info!(
                "Cleaned up {} pending jobs for agent {}/{}:{}",
                jobs_to_fail.len(),
                developer,
                agent,
                agent_method
            );
        }

        jobs_to_fail
    }

    /// Get statistics about the job database
    #[allow(dead_code)]
    pub async fn get_stats(&self) -> (usize, usize) {
        let ready_count = self.ready_jobs.read().await.len();
        let pending_count = self.pending_jobs.read().await.len();
        (ready_count, pending_count)
    }

    /// Generate agent key for HashMap lookups
    fn get_agent_key(&self, developer: &str, agent: &str, agent_method: &str) -> String {
        format!("{}/{}/{}", developer, agent, agent_method)
    }
}

impl Default for AgentJobDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui::fetch::{Job, JobStatus};

    fn create_test_state() -> SharedState {
        unsafe {
            std::env::set_var("SUI_ADDRESS", "0x123");
            std::env::set_var("SUI_CHAIN", "testnet");
        }
        SharedState::new()
    }

    fn create_test_pending_job() -> Job {
        Job {
            id: "test_job_id".to_string(),
            job_sequence: 1,
            description: Some("Test job".to_string()),
            developer: "TestDev".to_string(),
            agent: "TestAgent".to_string(),
            agent_method: "test_method".to_string(),
            app: "TestApp".to_string(),
            app_instance: "0x456".to_string(),
            app_instance_method: "test".to_string(),
            block_number: Some(100),
            sequences: Some(vec![1, 2, 3]),
            sequences1: None,
            sequences2: None,
            data: vec![0x01, 0x02, 0x03],
            status: JobStatus::Pending,
            attempts: 1,
            interval_ms: None,
            next_scheduled_at: None,
            created_at: 1234567890,
            updated_at: 1234567890,
        }
    }

    #[tokio::test]
    async fn test_agent_job_creation() {
        let state = create_test_state();
        let pending_job = create_test_pending_job();

        let agent_job = AgentJob::new(pending_job, &state);

        assert!(agent_job.job_id.starts_with("sn"));
        assert_eq!(agent_job.job_sequence, 1);
        assert_eq!(agent_job.developer, "TestDev");
        assert!(!agent_job.start_tx_sent);
    }

    #[tokio::test]
    async fn test_agent_job_database_flow() {
        let db = AgentJobDatabase::new();
        let state = create_test_state();
        let pending_job = create_test_pending_job();

        let agent_job = AgentJob::new(pending_job, &state);
        let job_id = agent_job.job_id.clone();

        // Add ready job
        db.add_ready_job(agent_job).await;

        // Check stats
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 1);
        assert_eq!(pending, 0);

        // Get ready job (moves to pending)
        let retrieved = db
            .get_ready_job("TestDev", "TestAgent", "test_method")
            .await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().unwrap().job_id, job_id);

        // Check stats after retrieval
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 0);
        assert_eq!(pending, 1);

        // Complete job
        let completed = db.complete_job(&job_id).await;
        assert!(completed.is_some());

        // Check final stats
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 0);
        assert_eq!(pending, 0);
    }

    #[tokio::test]
    async fn test_job_cleanup() {
        let db = AgentJobDatabase::new();
        let state = create_test_state();
        let pending_job = create_test_pending_job();

        let agent_job = AgentJob::new(pending_job, &state);

        // Add and retrieve job to make it pending
        db.add_ready_job(agent_job).await;
        db.get_ready_job("TestDev", "TestAgent", "test_method")
            .await;

        // Cleanup pending jobs
        let jobs_to_fail = db
            .cleanup_pending_jobs_for_agent("TestDev", "TestAgent", "test_method")
            .await;
        assert_eq!(jobs_to_fail.len(), 1);

        // Check stats after cleanup
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 0);
        assert_eq!(pending, 0);
    }

    #[tokio::test]
    async fn test_cleanup_all_jobs() {
        let db = AgentJobDatabase::new();
        let state = create_test_state();
        let pending_job = create_test_pending_job();

        let agent_job = AgentJob::new(pending_job, &state);

        // Add job as ready
        db.add_ready_job(agent_job).await;

        // Retrieve job (becomes pending)
        let retrieved_job = db
            .get_ready_job("TestDev", "TestAgent", "test_method")
            .await;
        assert!(retrieved_job.is_some());

        // Check stats: should have 0 ready, 1 pending
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 0);
        assert_eq!(pending, 1);

        // Add another ready job (simulating a new job arriving)
        let pending_job2 = create_test_pending_job();
        let agent_job2 = AgentJob::new(pending_job2, &state);
        db.add_ready_job(agent_job2).await;

        // Check stats: should have 1 ready, 1 pending
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 1);
        assert_eq!(pending, 1);

        // Cleanup all jobs for this agent
        let jobs_to_fail = db
            .cleanup_all_jobs_for_agent("TestDev", "TestAgent", "test_method")
            .await;
        assert_eq!(jobs_to_fail.len(), 2); // Should cleanup both ready and pending

        // Check stats after cleanup
        let (ready, pending) = db.get_stats().await;
        assert_eq!(ready, 0);
        assert_eq!(pending, 0);
    }
}
