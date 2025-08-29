use crate::agent::AgentJob;
use crate::error::{CoordinatorError, Result};
use crate::failed_jobs_cache::FailedJobsCache;
use crate::session_id::generate_docker_session;
use crate::settlement::fetch_all_pending_jobs;
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use secrets_client::SecretsClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use sui::fetch::Job;
use sui::fetch_agent_method;
use sui::interface::SilvanaSuiInterface;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, sleep};
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
    secrets_client: Option<SecretsClient>,
    failed_jobs_cache: FailedJobsCache,
}

impl JobSearcher {
    pub fn new(state: SharedState, use_tee: bool, container_timeout_secs: u64) -> Result<Self> {
        let docker_manager =
            DockerManager::new(use_tee).map_err(|e| CoordinatorError::DockerError(e))?;

        Ok(Self {
            state,
            docker_manager,
            container_timeout_secs,
            searcher_state: Arc::new(RwLock::new(SearcherState::Searching)),
            secrets_client: None,
            failed_jobs_cache: FailedJobsCache::new(),
        })
    }

    /// Set the secrets client for retrieving secrets during job execution
    #[allow(dead_code)]
    pub fn with_secrets_client(mut self, secrets_client: SecretsClient) -> Self {
        self.secrets_client = Some(secrets_client);
        self
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
                    debug!(
                        "Pending jobs availability changed: {} -> {}",
                        last_has_jobs, current_has_jobs
                    );
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

                    // Periodically clean up expired entries from the failed jobs cache
                    self.failed_jobs_cache.cleanup_expired().await;
                    
                    // Check for pending jobs and clean up app_instances without jobs
                    // This already filters out jobs in the failed cache
                    match self.check_and_clean_pending_jobs().await? {
                        Some(job) => {
                            info!(
                                "Found pending job: {} in app_instance {}",
                                job.job_sequence, job.app_instance
                            );

                            // Check if this agent is already running
                            if self
                                .state
                                .is_agent_running(&job.developer, &job.agent, &job.agent_method)
                                .await
                            {
                                debug!(
                                    "Agent {}/{}/{} is already running, skipping",
                                    job.developer, job.agent, job.agent_method
                                );
                                // Small delay before checking again
                                sleep(Duration::from_millis(500)).await;
                                continue;
                            }

                            // Check if we can start another container (under the limit)
                            if self.state.get_current_agent_count().await
                                >= crate::state::MAX_CONCURRENT_AGENTS
                            {
                                debug!(
                                    "Maximum concurrent agents ({}) reached, waiting",
                                    crate::state::MAX_CONCURRENT_AGENTS
                                );
                                sleep(Duration::from_secs(1)).await;
                                continue;
                            }

                            info!(
                                "🐳 Starting Docker: dev={}, agent={}/{}, job_seq={}, app={}",
                                job.developer, job.agent, job.agent_method, job.job_sequence, job.app_instance
                            );

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

                            // Delay before next check (1 second to avoid too frequent checks)
                            sleep(Duration::from_secs(1)).await;
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
    /// Returns the first job that is not in the failed cache
    async fn check_and_clean_pending_jobs(&self) -> Result<Option<Job>> {
        let app_instances = self.state.get_app_instances().await;

        if app_instances.is_empty() {
            return Ok(None);
        }

        debug!(
            "Checking {} app_instances for pending jobs",
            app_instances.len()
        );

        // First do a quick check to remove app_instances without pending jobs
        // Use check-only mode to quickly identify app_instances without jobs
        match fetch_all_pending_jobs(&app_instances, true).await {
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

        debug!(
            "Fetching pending jobs from {} remaining app_instances",
            remaining_instances.len()
        );

        match fetch_all_pending_jobs(&remaining_instances, false).await {
            Ok(pending_jobs) => {
                if pending_jobs.is_empty() {
                    // No pending jobs found
                    debug!("No pending jobs found after detailed fetch");
                    return Ok(None);
                }
                
                let total_jobs = pending_jobs.len();
                
                // Try to find a job that is not in the failed cache
                for job in pending_jobs {
                    if !self.failed_jobs_cache.is_recently_failed(&job.app_instance, job.job_sequence).await {
                        debug!("Found viable job {} from app_instance {}", job.job_sequence, job.app_instance);
                        return Ok(Some(job));
                    } else {
                        debug!("Skipping job {} from app_instance {} (in failed cache)", job.job_sequence, job.app_instance);
                    }
                }
                
                // All jobs are in the failed cache
                debug!("All {} pending jobs are in the failed cache", total_jobs);
                Ok(None)
            }
            Err(e) => {
                error!("Failed to fetch pending jobs: {}", e);
                Err(CoordinatorError::Other(e))
            }
        }
    }

    /// Run a Docker container for a pending job
    async fn run_docker_container(&mut self, job: Job) {
        let job_start = Instant::now();

        debug!(
            "🐳 Running Docker: dev={}, agent={}/{}, job_seq={}, app={}",
            job.developer, job.agent, job.agent_method, job.job_sequence, job.app_instance
        );

        // Generate Docker session keys
        let docker_session = match generate_docker_session() {
            Ok(session) => session,
            Err(e) => {
                error!(
                    "Failed to generate Docker session for job {}: {}",
                    job.job_sequence, e
                );
                return;
            }
        };

        // Set current agent in shared state with session_id
        self.state
            .set_current_agent(
                docker_session.session_id.clone(),
                job.developer.clone(),
                job.agent.clone(),
                job.agent_method.clone(),
            )
            .await;

        // Fetch agent method configuration
        let agent_fetch_start = Instant::now();
        let agent_method =
            match fetch_agent_method(&job.developer, &job.agent, &job.agent_method)
                .await
            {
                Ok(method) => {
                    let agent_fetch_duration = agent_fetch_start.elapsed();
                    info!(
                        "⏱️ Agent method fetch time: {}ms",
                        agent_fetch_duration.as_millis()
                    );
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
                    error!(
                        "Failed to fetch agent method for job {}: {}",
                        job.job_sequence, e
                    );
                    self.state
                        .clear_current_agent(&docker_session.session_id)
                        .await;
                    return;
                }
            };

        // Start the job on Sui blockchain before processing
        debug!("🔗 Starting job {} on Sui blockchain", job.job_sequence);

        // Try to start the job on blockchain with custom retry logic
        let start_time = Instant::now();
        let mut job_started = false;
        
        for attempt in 1..=3 {
            debug!("Attempting to start job {} (attempt {}/3)", job.job_sequence, attempt);
            
            // Fetch current job status before attempting to start
            if attempt == 1 {
                if let Ok(app_inst) = sui::fetch::fetch_app_instance(&job.app_instance).await {
                    if let Ok(Some((_app_instance_id, jobs_table_id))) = sui::fetch::get_jobs_info_from_app_instance(&app_inst).await {
                        if let Ok(Some(current_job)) = sui::fetch::fetch_job_by_id(&jobs_table_id, job.job_sequence).await {
                            debug!("Job {} status BEFORE start attempt: {:?}", job.job_sequence, current_job.status);
                        }
                    }
                }
            }
            
            match sui::transactions::start_job_tx(&job.app_instance, job.job_sequence).await {
                Ok(tx_digest) => {
                    debug!("Successfully started job {} on blockchain, tx: {}", job.job_sequence, tx_digest);
                    job_started = true;
                    break;
                }
                Err(e) => {
                    let error_str = e.to_string();
                    
                    // Check if the error is because the job is not in pending state
                    // This can happen if another coordinator already started it or it was completed/failed
                    if error_str.contains("Job is not in pending state") || 
                       error_str.contains("EJobNotPending") ||
                       error_str.contains("13906835364049584133") {
                        warn!("Job {} is not in pending state - likely already started/completed by another coordinator", job.job_sequence);
                        
                        // Add to failed jobs cache so we don't try again for 5 minutes
                        self.failed_jobs_cache.add_failed_job(job.app_instance.clone(), job.job_sequence).await;
                        
                        // Try to fetch comprehensive debugging information
                        if let Ok(app_inst) = sui::fetch::fetch_app_instance(&job.app_instance).await {
                            if let Ok(Some((_app_instance_id, jobs_table_id))) = sui::fetch::get_jobs_info_from_app_instance(&app_inst).await {
                                // Fetch and print the full failing job as Debug struct
                                if let Ok(Some(current_job)) = sui::fetch::fetch_job_by_id(&jobs_table_id, job.job_sequence).await {
                                    error!("FAILED JOB {} DEBUG:\n{:#?}", job.job_sequence, current_job);
                                    
                                    // Check if it's in the pending_jobs set for this specific method
                                    if let Ok(pending_jobs) = sui::fetch::fetch_pending_job_sequences_from_app_instance(
                                        &app_inst, 
                                        &job.developer, 
                                        &job.agent, 
                                        &job.agent_method
                                    ).await {
                                        let is_in_pending_set = pending_jobs.contains(&job.job_sequence);
                                        error!("Job {} in pending_jobs index for {}/{}/{}: {}", 
                                               job.job_sequence, job.developer, job.agent, job.agent_method, is_in_pending_set);
                                        if !is_in_pending_set && matches!(current_job.status, sui::fetch::JobStatus::Pending) {
                                            error!("⚠️ INCONSISTENCY: Job has Pending status but is NOT in pending_jobs index!");
                                        }
                                    }
                                }
                                
                                // Print AppInstance debug info
                                error!("APP_INSTANCE DEBUG:\n{:#?}", app_inst);
                                
                                // Print settlement job field
                                if let Ok(Some(settlement_job_id)) = sui::fetch::app_instance::get_settlement_job_id_for_instance(&app_inst).await {
                                    error!("Settlement job field: Some({})", settlement_job_id);
                                } else {
                                    error!("Settlement job field: None");
                                }
                                
                                // Fetch and print ALL jobs summary
                                if let Ok(all_jobs) = sui::fetch::fetch_all_jobs_from_app_instance(&app_inst).await {
                                    let pending_jobs: Vec<_> = all_jobs.iter()
                                        .filter(|j| matches!(j.status, sui::fetch::JobStatus::Pending))
                                        .map(|j| (j.job_sequence, &j.agent_method, j.attempts, j.next_scheduled_at))
                                        .collect();
                                    
                                    let running_jobs: Vec<_> = all_jobs.iter()
                                        .filter(|j| matches!(j.status, sui::fetch::JobStatus::Running))
                                        .map(|j| (j.job_sequence, &j.agent_method, j.attempts, j.updated_at))
                                        .collect();
                                    
                                    error!("JOBS SUMMARY: Total={}, Pending={}, Running={}", 
                                           all_jobs.len(), pending_jobs.len(), running_jobs.len());
                                    
                                    if !pending_jobs.is_empty() {
                                        error!("PENDING JOBS: {:?}", pending_jobs);
                                    }
                                    
                                    if !running_jobs.is_empty() {
                                        error!("RUNNING JOBS: {:?}", running_jobs);
                                    }
                                }
                            }
                        }
                        
                        // Clear the agent state and return
                        self.state.clear_current_agent(&docker_session.session_id).await;
                        return;
                    }
                    
                    error!("Failed to start job {} on blockchain: {}", job.job_sequence, e);
                    
                    if attempt < 3 {
                        warn!("Failed to start job {} on attempt {}, retrying...", job.job_sequence, attempt);
                        // Small delay between retries
                        sleep(Duration::from_millis(100 * attempt as u64)).await;
                    }
                }
            }
        }
        
        if !job_started {
            error!("Failed to start job {} after 3 attempts", job.job_sequence);
            // Add to failed jobs cache
            self.failed_jobs_cache.add_failed_job(job.app_instance.clone(), job.job_sequence).await;
            self.state.clear_current_agent(&docker_session.session_id).await;
            return;
        }
        
        let start_elapsed = start_time.elapsed();

        // Add job to agent database as ready for gRPC retrieval
        let agent_job = AgentJob::new(job.clone(), &self.state);
        let job_id = agent_job.job_id.clone();
        self.state.get_agent_job_db().add_ready_job(agent_job).await;
        
        info!(
            "✅ Started job: seq={}, dev={}, agent={}/{}, job_id={}, time={:?}",
            job.job_sequence, job.developer, job.agent, job.agent_method, job_id, start_elapsed
        );

        // Pull the Docker image
        info!("Pulling Docker image: {}", agent_method.docker_image);
        let pull_start = Instant::now();
        let _digest = match self
            .docker_manager
            .load_image(&agent_method.docker_image, false)
            .await
        {
            Ok(digest) => {
                let pull_duration = pull_start.elapsed();
                info!(
                    "Docker image pulled successfully with digest: {} in {:.1} sec",
                    digest,
                    pull_duration.as_secs_f64()
                );

                // Verify SHA256 if provided
                if let Some(expected_sha) = &agent_method.docker_sha256 {
                    if &digest != expected_sha {
                        error!(
                            "Image SHA mismatch for job {}: expected {}, got {}",
                            job.job_sequence, expected_sha, digest
                        );
                        self.state
                            .clear_current_agent(&docker_session.session_id)
                            .await;
                        return;
                    }
                }
                digest
            }
            Err(e) => {
                error!("Failed to pull image for job {}: {}", job.job_sequence, e);
                self.state
                    .clear_current_agent(&docker_session.session_id)
                    .await;
                return;
            }
        };

        // Docker session was already generated earlier

        // Prepare container configuration with new environment variables
        let mut env_vars = HashMap::new();
        env_vars.insert("CHAIN".to_string(), self.state.get_chain().clone());
        env_vars.insert(
            "COORDINATOR_ID".to_string(),
            self.state.get_coordinator_id().clone(),
        );
        env_vars.insert("SESSION_ID".to_string(), docker_session.session_id.clone());
        env_vars.insert(
            "SESSION_PRIVATE_KEY".to_string(),
            docker_session.session_private_key,
        );
        env_vars.insert("DEVELOPER".to_string(), job.developer.clone());
        env_vars.insert("AGENT".to_string(), job.agent.clone());
        env_vars.insert("AGENT_METHOD".to_string(), job.agent_method.clone());

        // Retrieve secrets if secrets client is available
        if let Some(ref mut secrets_client) = self.secrets_client {
            match Self::retrieve_secrets_for_job(secrets_client, &job).await {
                Ok(secrets) => {
                    for (key, value) in secrets {
                        env_vars.insert(format!("SECRET_{}", key.to_uppercase()), value);
                    }
                    info!(
                        "Retrieved {} secrets for job {}",
                        env_vars.len() - 7,
                        job.job_sequence
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to retrieve secrets for job {}: {}",
                        job.job_sequence, e
                    );
                    // Continue without secrets - let the agent decide if this is acceptable
                }
            }
        }

        let container_config = ContainerConfig {
            image_name: agent_method.docker_image.clone(),
            image_source: agent_method.docker_image.clone(),
            command: vec![],
            env_vars,
            port_bindings: HashMap::new(),
            volume_binds: vec![],
            timeout_seconds: self.container_timeout_secs,
            memory_limit_mb: Some((agent_method.min_memory_gb as u64) * 1024),
            cpu_cores: Some(agent_method.min_cpu_cores as f64),
            network_mode: if agent_method.requires_tee {
                Some("host".to_string())
            } else {
                None
            },
            requires_tee: agent_method.requires_tee,
            extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
        };

        // Run the container
        let container_start = Instant::now();
        let container_result = self.docker_manager.run_container(&container_config).await;

        match container_result {
            Ok(result) => {
                let container_duration = container_start.elapsed();
                debug!(
                    "⏱️ Container execution time: {}ms",
                    container_duration.as_millis()
                );
                debug!(
                    "Container completed for job {}: exit_code={}, time={}ms",
                    job.job_sequence, result.exit_code, result.duration_ms
                );
                if !result.logs.is_empty() {
                    info!("Container logs:\n{}", result.logs);
                }

                let total_duration = job_start.elapsed();
                info!(
                    "✅ Docker completed: dev={}, agent={}/{}, job_seq={}, time={:?}",
                    job.developer, job.agent, job.agent_method, job.job_sequence, total_duration
                );
            }
            Err(e) => {
                let total_duration = job_start.elapsed();
                error!(
                    "❌ Docker failed: dev={}, agent={}/{}, job_seq={}, error={}, time={:?}",
                    job.developer, job.agent, job.agent_method, job.job_sequence, e, total_duration
                );
            }
        }

        // Small delay to allow any pending gRPC operations to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Clean up any jobs (ready or pending) that weren't completed/failed via gRPC
        debug!(
            "Starting cleanup of uncompleted jobs for agent {}/{}:{}",
            job.developer, job.agent, job.agent_method
        );
        let jobs_to_fail = self
            .state
            .get_agent_job_db()
            .cleanup_all_jobs_for_agent(&job.developer, &job.agent, &job.agent_method)
            .await;

        if !jobs_to_fail.is_empty() {
            info!(
                "Cleaning up {} uncompleted jobs after Docker termination",
                jobs_to_fail.len()
            );

            let mut sui_interface = SilvanaSuiInterface::new();

            for uncompleted_job in jobs_to_fail {
                let error_message = "Job not completed before Docker container termination";
                info!("Auto-failing uncompleted job {}", uncompleted_job.job_id);

                if !sui_interface
                    .fail_job(
                        &uncompleted_job.app_instance,
                        uncompleted_job.job_sequence,
                        error_message,
                    )
                    .await
                {
                    error!(
                        "Failed to auto-fail uncompleted job {} on blockchain",
                        uncompleted_job.job_id
                    );
                } else {
                    info!(
                        "Successfully auto-failed uncompleted job {} on blockchain",
                        uncompleted_job.job_id
                    );
                }
            }
        }

        // Clear current agent after container finishes
        self.state
            .clear_current_agent(&docker_session.session_id)
            .await;
    }

    /// Retrieve secrets for a job from the secrets storage
    async fn retrieve_secrets_for_job(
        secrets_client: &mut SecretsClient,
        job: &Job,
    ) -> Result<HashMap<String, String>> {
        let mut secrets = HashMap::new();

        // TODO: Replace with actual signature - for now using empty signature as placeholder
        let placeholder_signature = vec![];

        // Try to retrieve secrets at different scopes, from most specific to least specific
        let secret_attempts = vec![
            // Most specific: developer + agent + app + app_instance + name
            (Some("secret"), Some(job.app_instance.as_str())),
            // Developer + agent + app + app_instance (general secret)
            (None, Some(job.app_instance.as_str())),
            // Developer + agent + app (app-level secret)
            (None, None),
        ];

        for (name, app_instance) in secret_attempts {
            match secrets_client
                .retrieve_secret(
                    &job.developer,
                    &job.agent,
                    Some(&job.agent), // Using agent as app for now
                    app_instance,
                    name,
                    &placeholder_signature,
                )
                .await
            {
                Ok(secret_value) => {
                    let key = match (name, app_instance) {
                        (Some(n), Some(_)) => format!("{}_{}", n, "instance"),
                        (None, Some(_)) => "instance".to_string(),
                        _ => "app".to_string(),
                    };
                    debug!("Retrieved secret '{}' for job {}", key, job.job_sequence);
                    secrets.insert(key, secret_value);
                }
                Err(secrets_client::SecretsClientError::SecretNotFound) => {
                    // Secret not found at this scope, try next one
                    continue;
                }
                Err(e) => {
                    warn!(
                        "Failed to retrieve secret for job {}: {}",
                        job.job_sequence, e
                    );
                    // Continue trying other secrets
                }
            }
        }

        Ok(secrets)
    }
}
