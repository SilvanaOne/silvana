use crate::job_id::generate_job_id;
use crate::state::SharedState;
use proto;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use sui::fetch::Job;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Job information stored in memory for tracking agent interactions
#[derive(Debug, Clone)]
pub struct AgentJob {
    pub session_id: String,
    pub job_id: String,
    pub job_sequence: u64,
    pub app_instance: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub job: Job,
    pub min_memory_gb: u16,
    pub min_cpu_cores: u16,
    pub requires_tee: bool,
    pub job_start_time: std::time::SystemTime,
}

impl AgentJob {
    pub fn new(
        job: Job,
        session_id: String,
        state: &SharedState,
        min_memory_gb: u16,
        min_cpu_cores: u16,
        requires_tee: bool,
    ) -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let job_id = generate_job_id(
            state,
            &job.developer,
            &job.agent,
            &job.agent_method,
            &job.app_instance,
            job.job_sequence,
            timestamp,
        ).ok_or_else(|| format!("Cannot create agent job {}: coordinator_id not available", job.job_sequence))?;

        // Send JobCreatedEvent
        Self::send_job_created_event(
            state,
            &session_id,
            &job_id,
            &job,
        );

        Ok(Self {
            session_id: session_id.clone(),
            job_id,
            job_sequence: job.job_sequence,
            app_instance: job.app_instance.clone(),
            developer: job.developer.clone(),
            agent: job.agent.clone(),
            agent_method: job.agent_method.clone(),
            job,
            min_memory_gb,
            min_cpu_cores,
            requires_tee,
            job_start_time: SystemTime::now(),
        })
    }

    /// Send JobCreatedEvent to RPC service (fire and forget)
    fn send_job_created_event(
        state: &SharedState,
        session_id: &str,
        job_id: &str,
        job: &Job,
    ) {
        // Get coordinator ID
        let coordinator_id = match state.get_coordinator_id() {
            Some(id) => id,
            None => {
                debug!("Cannot send JobCreatedEvent: coordinator_id not available");
                return;
            }
        };

        // Clone what we need for the async task
        let state_clone = state.clone();
        let session_id = session_id.to_string();
        let job_id = job_id.to_string();
        let app_instance = job.app_instance.clone();
        let app_method = job.app_instance_method.clone();
        let job_sequence = job.job_sequence;
        let sequences = job.sequences.clone().unwrap_or_default();
        let sequences1 = job.sequences1.clone().unwrap_or_default();
        let sequences2 = job.sequences2.clone().unwrap_or_default();

        // Spawn async task to send the event
        tokio::spawn(async move {
            let rpc_client = state_clone.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::JobCreated(
                        proto::JobCreatedEvent {
                            coordinator_id,
                            session_id,
                            app_instance_id: app_instance,
                            app_method,
                            job_sequence,
                            sequences,
                            merged_sequences_1: sequences1,
                            merged_sequences_2: sequences2,
                            job_id: job_id.clone(),
                            event_timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            debug!("Sent JobCreatedEvent for job_id {}", job_id);
                        } else {
                            warn!("Failed to send JobCreatedEvent: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send JobCreatedEvent: {}", e);
                    }
                }
            } else {
                debug!("No RPC client available to send JobCreatedEvent");
            }
        });
    }
}

/// Memory database for tracking jobs sent to agents
#[derive(Clone)]
pub struct AgentJobDatabase {
    running_jobs: Arc<RwLock<HashMap<String, AgentJob>>>, // key: job_id
    session_jobs: Arc<RwLock<HashMap<String, AgentJob>>>, // key: session_id
    session_index: Arc<RwLock<HashMap<String, Vec<String>>>>, // session_id -> list of job_ids
}

impl AgentJobDatabase {
    pub fn new() -> Self {
        Self {
            running_jobs: Arc::new(RwLock::new(HashMap::new())),
            session_jobs: Arc::new(RwLock::new(HashMap::new())),
            session_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a job reserved for a specific Docker session
    pub async fn add_session_job(&self, agent_job: AgentJob) {
        let session_id = agent_job.session_id.clone();
        let job_id = agent_job.job_id.clone();

        {
            let mut session_jobs = self.session_jobs.write().await;
            session_jobs.insert(session_id.clone(), agent_job.clone());
        }

        // Update the session index
        {
            let mut session_index = self.session_index.write().await;
            session_index
                .entry(session_id.clone())
                .or_insert_with(Vec::new)
                .push(job_id.clone());
        }

        debug!(
            "Added session job {} for session {}, agent {}/{}:{}",
            agent_job.job_id,
            agent_job.session_id,
            agent_job.developer,
            agent_job.agent,
            agent_job.agent_method
        );
    }

    /// Get and remove a job for the specified session
    /// Moves the job to running_jobs for tracking
    /// Session_id is required since all execution is in Docker
    pub async fn get_ready_job(
        &self,
        developer: &str,
        agent: &str,
        agent_method: &str,
        session_id: &str,
    ) -> Option<AgentJob> {
        let job = {
            let mut session_jobs = self.session_jobs.write().await;

            // Debug: Log all session jobs before attempting removal
            if session_jobs.is_empty() {
                debug!("No session jobs available at all");
            } else {
                debug!(
                    "Available session jobs: {:?}",
                    session_jobs.keys().collect::<Vec<_>>()
                );
            }

            // Get job for this session if it exists
            if let Some(job) = session_jobs.get(session_id) {
                // Validate that this job matches the requested agent/method
                if job.developer == developer
                    && job.agent == agent
                    && job.agent_method == agent_method
                {
                    debug!(
                        "Found matching session job for session {}: dev={}, agent={}, method={}",
                        session_id, developer, agent, agent_method
                    );
                    session_jobs.remove(session_id)
                } else {
                    debug!(
                        "Session job found for {} but doesn't match agent: job(dev={}, agent={}, method={}) vs requested(dev={}, agent={}, method={})",
                        session_id,
                        job.developer,
                        job.agent,
                        job.agent_method,
                        developer,
                        agent,
                        agent_method
                    );
                    None
                }
            } else {
                debug!("No session job found for session_id: {}", session_id);
                None
            }
        };

        if let Some(job) = job {
            // Move to running jobs for tracking
            {
                let mut running = self.running_jobs.write().await;
                running.insert(job.job_id.clone(), job.clone());
            }

            // Keep the job in session_index (it's still associated with this session)
            // The index will be useful for finding all jobs for a session

            debug!(
                "Retrieved session job {} for session {}, agent {}/{}:{}",
                job.job_id, session_id, developer, agent, agent_method
            );
            Some(job)
        } else {
            debug!(
                "No session job found for session {} (agent {}/{}:{})",
                session_id, developer, agent, agent_method
            );
            None
        }
    }

    /// Mark a job as completed and remove it from tracking
    pub async fn complete_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut running = self.running_jobs.write().await;
            running.remove(job_id)
        };

        if let Some(job) = job {
            debug!("Completed job {}", job.job_id);
            Some(job)
        } else {
            warn!("Attempted to complete non-existent job: {}", job_id);
            None
        }
    }

    /// Mark a job as failed and remove it from tracking
    pub async fn fail_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut running = self.running_jobs.write().await;
            running.remove(job_id)
        };

        if let Some(job) = job {
            info!("Failed job {}", job.job_id);
            Some(job)
        } else {
            warn!("Attempted to fail non-existent job: {}", job_id);
            None
        }
    }

    /// Terminate a job and remove it from tracking
    pub async fn terminate_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut pending = self.running_jobs.write().await;
            pending.remove(job_id)
        };

        if let Some(job) = job {
            info!("Terminated job {}", job.job_id);
            Some(job)
        } else {
            warn!("Attempted to terminate non-existent job: {}", job_id);
            None
        }
    }
    
    /// Release a job back to available pool (used when session can't take it due to chain mismatch)
    pub async fn release_job(&self, job_id: &str) -> Option<AgentJob> {
        let job = {
            let mut running = self.running_jobs.write().await;
            running.remove(job_id)
        };

        if let Some(job) = job {
            info!("Released job {} back to available pool", job.job_id);
            Some(job)
        } else {
            warn!("Attempted to release non-existent job: {}", job_id);
            None
        }
    }

    /// Get a job by job_id for reference (doesn't remove it)
    pub async fn get_job_by_id(&self, job_id: &str) -> Option<AgentJob> {
        let running = self.running_jobs.read().await;
        running.get(job_id).cloned()
    }

    /// Add a job directly to running jobs (job has already been started and is being sent to agent)
    pub async fn add_to_running(&self, agent_job: AgentJob) {
        let session_id = agent_job.session_id.clone();
        let job_id = agent_job.job_id.clone();

        {
            let mut running = self.running_jobs.write().await;
            running.insert(job_id.clone(), agent_job.clone());
        }

        // Update the session index
        {
            let mut session_index = self.session_index.write().await;
            session_index
                .entry(session_id.clone())
                .or_insert_with(Vec::new)
                .push(job_id.clone());
        }

        debug!(
            "Added job {} to running jobs for session {}",
            job_id, session_id
        );
    }

    /// Get statistics about jobs in the database
    pub async fn get_running_jobs(&self) -> usize {
        let running = self.running_jobs.read().await;
        running.len()
    }

    /// Get all jobs associated with a session (both in session_jobs and running_jobs)
    #[allow(dead_code)]
    pub async fn get_jobs_by_session(&self, session_id: &str) -> Vec<AgentJob> {
        let mut jobs = Vec::new();

        // First check if there's a session job waiting
        {
            let session_jobs = self.session_jobs.read().await;
            if let Some(job) = session_jobs.get(session_id) {
                jobs.push(job.clone());
            }
        }

        // Then check the session index for all job_ids associated with this session
        let job_ids = {
            let session_index = self.session_index.read().await;
            session_index.get(session_id).cloned().unwrap_or_default()
        };

        // Look up each job_id in running_jobs
        {
            let running_jobs = self.running_jobs.read().await;
            for job_id in job_ids {
                if let Some(job) = running_jobs.get(&job_id) {
                    // Only add if not already in the list (avoid duplicates with session_jobs)
                    if !jobs.iter().any(|j| j.job_id == job.job_id) {
                        jobs.push(job.clone());
                    }
                }
            }
        }

        if !jobs.is_empty() {
            debug!(
                "Found {} job(s) for session {}: {:?}",
                jobs.len(),
                session_id,
                jobs.iter().map(|j| &j.job_id).collect::<Vec<_>>()
            );
        }

        jobs
    }

    /// Clean up session index when a session is cleared
    #[allow(dead_code)]
    pub async fn cleanup_session(&self, session_id: &str) {
        let mut session_index = self.session_index.write().await;
        if session_index.remove(session_id).is_some() {
            debug!("Cleaned up session index for session {}", session_id);
        }
    }

    /// Fail all remaining jobs for a session (both waiting and running)
    /// Returns the jobs that were failed
    pub async fn fail_all_remaining_session_jobs(&self, session_id: &str) -> Vec<AgentJob> {
        let mut failed_jobs = Vec::new();

        // First, remove any waiting session job
        {
            let mut session_jobs = self.session_jobs.write().await;
            if let Some(job) = session_jobs.remove(session_id) {
                debug!(
                    "Failing waiting session job {} for session {}",
                    job.job_id, session_id
                );
                failed_jobs.push(job);
            }
        }

        // Get all job_ids associated with this session from the index
        let job_ids = {
            let session_index = self.session_index.read().await;
            session_index.get(session_id).cloned().unwrap_or_default()
        };

        // Remove all running jobs for this session
        {
            let mut running_jobs = self.running_jobs.write().await;
            for job_id in &job_ids {
                if let Some(job) = running_jobs.remove(job_id) {
                    debug!(
                        "Failing running job {} for session {}",
                        job.job_id, session_id
                    );
                    failed_jobs.push(job);
                }
            }
        }

        // Clean up the session index
        {
            let mut session_index = self.session_index.write().await;
            session_index.remove(session_id);
        }

        if !failed_jobs.is_empty() {
            warn!(
                "Failed {} remaining job(s) for session {}: {:?}",
                failed_jobs.len(),
                session_id,
                failed_jobs.iter().map(|j| &j.job_id).collect::<Vec<_>>()
            );
        } else {
            debug!("No remaining jobs to fail for session {}", session_id);
        }

        failed_jobs
    }
}

impl Default for AgentJobDatabase {
    fn default() -> Self {
        Self::new()
    }
}
