use crate::error::{CoordinatorError, Result};
use crate::fetch::fetch_all_pending_jobs;
use crate::pending::PendingJob;
use crate::registry::fetch_agent_method;
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// State of the job searcher
#[derive(Debug, Clone, PartialEq)]
enum SearcherState {
    /// Looking for jobs
    Searching,
    /// Running a Docker container
    RunningDocker,
}

/// Job searcher that monitors shared state and runs Docker containers
pub struct JobSearcher {
    state: SharedState,
    docker_manager: DockerManager,
    container_timeout_secs: u64,
    searcher_state: Arc<RwLock<SearcherState>>,
}

impl JobSearcher {
    pub fn new(
        state: SharedState,
        use_tee: bool,
        container_timeout_secs: u64,
    ) -> Result<Self> {
        let docker_manager = DockerManager::new(use_tee)
            .map_err(|e| CoordinatorError::DockerError(e))?;

        Ok(Self {
            state,
            docker_manager,
            container_timeout_secs,
            searcher_state: Arc::new(RwLock::new(SearcherState::Searching)),
        })
    }

    /// Main loop for the job searcher
    pub async fn run(&mut self) -> Result<()> {
        info!("🔍 Job searcher started");
        
        // Use a channel to signal when state changes occur
        let (state_change_tx, mut state_change_rx) = mpsc::channel::<()>(100);
        
        // Start a task that monitors for state changes using the has_pending_jobs flag
        let monitor_state = self.state.clone();
        let state_change_tx_clone = state_change_tx.clone();
        tokio::spawn(async move {
            let mut last_has_jobs = monitor_state.has_pending_jobs_available();
            loop {
                sleep(Duration::from_millis(100)).await;
                let current_has_jobs = monitor_state.has_pending_jobs_available();
                if current_has_jobs != last_has_jobs {
                    debug!("Pending jobs availability changed: {} -> {}", last_has_jobs, current_has_jobs);
                    if current_has_jobs {
                        // Only send notification when jobs become available
                        let _ = state_change_tx_clone.send(()).await;
                    }
                    last_has_jobs = current_has_jobs;
                }
            }
        });

        loop {
            let current_state = self.searcher_state.read().await.clone();
            
            match current_state {
                SearcherState::Searching => {
                    // Fast check if there are any pending jobs
                    if !self.state.has_pending_jobs_available() {
                        // No pending jobs, wait for state change
                        tokio::select! {
                            _ = state_change_rx.recv() => {
                                debug!("Pending jobs became available, checking for jobs");
                            }
                            _ = sleep(Duration::from_secs(5)) => {
                                // Periodic check every 5 seconds as a safety measure
                            }
                        }
                        continue;
                    }
                    
                    // Check for pending jobs and clean up app_instances without jobs
                    match self.check_and_clean_pending_jobs().await? {
                        Some(job) => {
                            info!("Found pending job: {} in app_instance {}", 
                                job.job_id, job.app_instance);
                            
                            // Set state to running Docker
                            {
                                let mut state = self.searcher_state.write().await;
                                *state = SearcherState::RunningDocker;
                            }
                            
                            // Run the Docker container
                            self.run_docker_container(job).await;
                            
                            // Set state back to searching
                            {
                                let mut state = self.searcher_state.write().await;
                                *state = SearcherState::Searching;
                            }
                            
                            // Continue immediately to check for more jobs
                            continue;
                        }
                        None => {
                            // No jobs found despite flag being set - update the flag
                            self.state.update_pending_jobs_flag().await;
                            
                            // Small delay before next check
                            sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                SearcherState::RunningDocker => {
                    // This shouldn't happen as we handle Docker synchronously
                    warn!("Job searcher in RunningDocker state unexpectedly");
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Check for pending jobs and clean up app_instances without jobs
    /// This combines job searching with cleanup that reconciliation would do
    async fn check_and_clean_pending_jobs(&self) -> Result<Option<PendingJob>> {
        let app_instances = self.state.get_app_instances().await;
        
        if app_instances.is_empty() {
            return Ok(None);
        }
        
        debug!("Checking {} app_instances for pending jobs", app_instances.len());
        
        // First do a quick check to remove app_instances without pending jobs
        let mut client = self.state.get_sui_client();
        
        // Use check-only mode to quickly identify app_instances without jobs
        match fetch_all_pending_jobs(&mut client, &app_instances, &self.state, true).await {
            Ok(_) => {
                debug!("Quick check completed, removed app_instances without pending jobs");
            }
            Err(e) => {
                warn!("Quick check failed: {}", e);
            }
        }
        
        // Now fetch actual pending jobs from remaining app_instances
        let remaining_instances = self.state.get_app_instances().await;
        if remaining_instances.is_empty() {
            debug!("No app_instances with pending jobs remaining after cleanup");
            return Ok(None);
        }
        
        debug!("Fetching pending jobs from {} remaining app_instances", remaining_instances.len());
        
        let mut client = self.state.get_sui_client();
        match fetch_all_pending_jobs(&mut client, &remaining_instances, &self.state, false).await {
            Ok(pending_job) => {
                if pending_job.is_none() {
                    // No pending jobs found, but we had app_instances - they might have been cleaned up
                    debug!("No pending jobs found after detailed fetch");
                }
                Ok(pending_job)
            }
            Err(e) => {
                error!("Failed to fetch pending jobs: {}", e);
                Err(e)
            }
        }
    }

    /// Run a Docker container for a pending job
    async fn run_docker_container(&mut self, job: PendingJob) {
        let job_start = Instant::now();
        
        info!("🐳 Starting Docker container for job {}", job.job_id);
        info!("  Developer: {}", job.developer);
        info!("  Agent: {}", job.agent);
        info!("  Method: {}", job.agent_method);
        info!("  App Instance: {}", job.app_instance);
        
        // Set current agent in shared state
        self.state.set_current_agent(
            job.developer.clone(),
            job.agent.clone(),
            job.agent_method.clone(),
        ).await;
        
        // Fetch agent method configuration
        let agent_fetch_start = Instant::now();
        let mut client = self.state.get_sui_client();
        let agent_method = match fetch_agent_method(
            &mut client,
            &job.developer,
            &job.agent,
            &job.agent_method,
        ).await {
            Ok(method) => {
                let agent_fetch_duration = agent_fetch_start.elapsed();
                info!("⏱️ Agent method fetch time: {}ms", agent_fetch_duration.as_millis());
                info!(
                    "Agent Method: image={}, sha256={:?}, memory={}GB, cpu={}, tee={}",
                    method.docker_image,
                    method.docker_sha256,
                    method.min_memory_gb,
                    method.min_cpu_cores,
                    method.requires_tee
                );
                method
            }
            Err(e) => {
                error!("Failed to fetch agent method for job {}: {}", job.job_id, e);
                self.state.clear_current_agent().await;
                return;
            }
        };
        
        // Pull the Docker image
        info!("Pulling Docker image: {}", agent_method.docker_image);
        let pull_start = Instant::now();
        let _digest = match self.docker_manager.load_image(&agent_method.docker_image, false).await {
            Ok(digest) => {
                let pull_duration = pull_start.elapsed();
                info!("⏱️ Docker image pull time: {}ms", pull_duration.as_millis());
                info!("Image pulled successfully with digest: {}", digest);
                
                // Verify SHA256 if provided
                if let Some(expected_sha) = &agent_method.docker_sha256 {
                    if &digest != expected_sha {
                        error!(
                            "Image SHA mismatch for job {}: expected {}, got {}",
                            job.job_id, expected_sha, digest
                        );
                        self.state.clear_current_agent().await;
                        return;
                    }
                }
                digest
            }
            Err(e) => {
                error!("Failed to pull image for job {}: {}", job.job_id, e);
                self.state.clear_current_agent().await;
                return;
            }
        };
        
        // Prepare container configuration
        let container_config = ContainerConfig {
            image_name: agent_method.docker_image.clone(),
            image_source: agent_method.docker_image.clone(),
            command: vec![],
            env_vars: {
                let mut env = HashMap::new();
                env.insert("JOB_ID".to_string(), job.job_id.to_string());
                env.insert("JOB_DATA".to_string(), format!("0x{}", hex::encode(&job.data)));
                env.insert("DEVELOPER".to_string(), job.developer.clone());
                env.insert("AGENT".to_string(), job.agent.clone());
                env.insert("METHOD".to_string(), job.agent_method.clone());
                env.insert("APP_INSTANCE".to_string(), job.app_instance.clone());
                env
            },
            port_bindings: HashMap::new(),
            timeout_seconds: self.container_timeout_secs,
            memory_limit_mb: Some((agent_method.min_memory_gb as u64) * 1024),
            cpu_cores: Some(agent_method.min_cpu_cores as f64),
            network_mode: if agent_method.requires_tee {
                Some("host".to_string())
            } else {
                None
            },
            requires_tee: agent_method.requires_tee,
        };
        
        // Run the container
        let container_start = Instant::now();
        match self.docker_manager.run_container(&container_config).await {
            Ok(result) => {
                let container_duration = container_start.elapsed();
                info!("⏱️ Container execution time: {}ms", container_duration.as_millis());
                info!(
                    "Container completed for job {}: exit_code={}, internal_duration={}ms",
                    job.job_id, result.exit_code, result.duration_ms
                );
                if !result.logs.is_empty() {
                    info!("Container logs:\n{}", result.logs);
                }
                
                let total_duration = job_start.elapsed();
                info!("⏱️ Total job processing time: {}ms", total_duration.as_millis());
            }
            Err(e) => {
                error!("Failed to run container for job {}: {}", job.job_id, e);
            }
        }
        
        // Clear current agent after container finishes
        self.state.clear_current_agent().await;
    }
}