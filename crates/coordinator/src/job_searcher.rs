use crate::agent::AgentJob;
use crate::constants::{
    AGENT_MIN_MEMORY_REQUIREMENT_GB, CONTAINER_HEALTH_CHECK_INTERVAL_SECS,
    CONTAINER_STATUS_CHECK_INTERVAL_SECS, DOCKER_CONTAINER_FORCE_STOP_TIMEOUT_SECS,
    JOB_ACQUISITION_DELAY_PER_CONTAINER_MS, JOB_ACQUISITION_MAX_DELAY_MS, JOB_SELECTION_POOL_SIZE,
    JOB_STATUS_CHECK_DELAY_SECS, MAX_CONCURRENT_AGENT_CONTAINERS, MULTICALL_INTERVAL_SECS,
    PENDING_JOBS_CHECK_DELAY_MS,
};
use crate::error::{CoordinatorError, Result};
use crate::hardware::{get_available_memory_gb, get_hardware_info, get_total_memory_gb};
use crate::job_lock::{JobLockGuard, get_job_lock_manager};
use crate::jobs_cache::JobsCache;
use crate::metrics::CoordinatorMetrics;
use crate::session_id::generate_docker_session;
use crate::settlement::{can_remove_app_instance, fetch_all_pending_jobs};
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use futures::future;
use secrets_client::SecretsClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use sui::fetch::{AgentMethod, Job};
use sui::fetch_agent_method;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

/// Check if system has sufficient resources to run an agent
async fn can_run_agent(state: &SharedState, agent_method: &AgentMethod) -> Result<bool> {
    // Check TEE requirement
    if agent_method.requires_tee {
        debug!("Agent requires TEE but we don't run on TEE");
        return Ok(false);
    }

    // Get hardware info
    let hardware = get_hardware_info();

    // Check CPU cores
    if (hardware.cpu_cores as u16) < agent_method.min_cpu_cores {
        debug!(
            "Insufficient CPU cores: have {}, need {}",
            hardware.cpu_cores, agent_method.min_cpu_cores
        );
        return Ok(false);
    }

    // Check concurrent agent limit
    let current_agents = state.get_current_agent_count().await;
    if current_agents >= MAX_CONCURRENT_AGENT_CONTAINERS {
        debug!(
            "Maximum concurrent agents ({}) reached",
            MAX_CONCURRENT_AGENT_CONTAINERS
        );
        return Ok(false);
    }

    // Calculate total memory required by currently running agents
    let running_agents = state.get_all_current_agents().await;
    let mut total_memory_used_gb = 0u64;

    for (_session_id, agent_info) in running_agents {
        // Fetch agent method to get memory requirements
        if let Ok(method) = fetch_agent_method(
            &agent_info.developer,
            &agent_info.agent,
            &agent_info.agent_method,
        )
        .await
        {
            total_memory_used_gb += method.min_memory_gb as u64;
        }
    }

    // Add memory requirement for this new agent
    let total_memory_needed_gb = total_memory_used_gb + agent_method.min_memory_gb as u64;

    // Check against total system memory minus reserved
    let total_memory_gb = get_total_memory_gb();
    if total_memory_needed_gb > total_memory_gb.saturating_sub(AGENT_MIN_MEMORY_REQUIREMENT_GB) {
        debug!(
            "Insufficient total memory: need {} GB (including {} GB for new agent), have {} GB (minus {} GB reserved)",
            total_memory_needed_gb,
            agent_method.min_memory_gb,
            total_memory_gb,
            AGENT_MIN_MEMORY_REQUIREMENT_GB
        );
        return Ok(false);
    }

    // Check against available memory
    let available_memory_gb = get_available_memory_gb();
    if (agent_method.min_memory_gb as u64) > available_memory_gb {
        debug!(
            "Insufficient available memory: need {} GB, have {} GB available",
            agent_method.min_memory_gb, available_memory_gb
        );
        return Ok(false);
    }

    Ok(true)
}

/// State of the job searcher
#[derive(Debug, Clone, PartialEq)]
enum SearcherState {
    /// Looking for jobs
    Searching,
}

/// Job searcher that monitors shared state and runs Docker containers
pub struct JobSearcher {
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    container_timeout_secs: u64,
    searcher_state: Arc<RwLock<SearcherState>>,
    secrets_client: Option<SecretsClient>,
    jobs_cache: JobsCache,
    current_job_info: Arc<RwLock<Option<(String, Job)>>>, // (container_id, job)
    metrics: Option<Arc<CoordinatorMetrics>>,
}

impl JobSearcher {
    pub fn new(state: SharedState, use_tee: bool, container_timeout_secs: u64) -> Result<Self> {
        let docker_manager =
            DockerManager::new(use_tee).map_err(|e| CoordinatorError::DockerError(e))?;

        Ok(Self {
            state,
            docker_manager: Arc::new(docker_manager),
            container_timeout_secs,
            searcher_state: Arc::new(RwLock::new(SearcherState::Searching)),
            secrets_client: None,
            jobs_cache: JobsCache::new(),
            current_job_info: Arc::new(RwLock::new(None)),
            metrics: None,
        })
    }

    /// Set the secrets client for retrieving secrets during job execution
    #[allow(dead_code)]
    pub fn with_secrets_client(mut self, secrets_client: SecretsClient) -> Self {
        self.secrets_client = Some(secrets_client);
        self
    }

    /// Set the metrics reporter
    pub fn set_metrics(&mut self, metrics: Arc<CoordinatorMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Process jobs from the buffer by starting Docker containers
    async fn process_buffer_jobs(&self) -> Result<()> {
        // Check buffer and process jobs one by one until limits are reached
        loop {
            // If shutting down, don't start new containers
            if self.state.is_shutting_down() {
                debug!("Shutdown requested, not starting new Docker containers");
                break;
            }
            
            // Check current container counts
            let (loading_count, running_count) = self.docker_manager.get_container_counts().await;
            let total_containers = loading_count + running_count;

            // Check if we've reached container limit
            if total_containers >= MAX_CONCURRENT_AGENT_CONTAINERS {
                debug!(
                    "Container limit reached ({}/{}), waiting for containers to complete",
                    total_containers, MAX_CONCURRENT_AGENT_CONTAINERS
                );
                break;
            }

            // Get next job from buffer
            let started_job = self.state.get_next_started_job().await;
            if started_job.is_none() {
                debug!("No more jobs in buffer to process");
                break;
            }

            let started_job = started_job.unwrap();
            info!(
                "Processing buffered job: app_instance={}, sequence={}, memory={:.2} GB",
                started_job.app_instance, 
                started_job.job_sequence, 
                started_job.memory_requirement as f64 / (1024.0 * 1024.0 * 1024.0)
            );

            // First fetch the app instance to get the jobs table ID
            let job_result: std::result::Result<Option<sui::fetch::Job>, ()> =
                match sui::fetch::fetch_app_instance(&started_job.app_instance).await {
                    Ok(app_inst) => {
                        if let Some(jobs_obj) = app_inst.jobs {
                            // Try to fetch the job from the jobs table
                            match sui::fetch::fetch_job_by_id(
                                &jobs_obj.jobs_table_id,
                                started_job.job_sequence,
                            )
                            .await
                            {
                                Ok(job) => Ok(job),
                                Err(e) => {
                                    error!("Failed to fetch job by ID: {}", e);
                                    Ok(None)
                                }
                            }
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch app instance: {}", e);
                        Ok(None)
                    }
                };

            match job_result {
                Ok(Some(job)) => {
                    // Check if job is still in a state we can process
                    let can_process = matches!(
                        job.status,
                        sui::fetch::JobStatus::Pending | sui::fetch::JobStatus::Running
                    );

                    debug!(
                        "Buffered job {} status: {:?}, can_process: {}",
                        started_job.job_sequence, job.status, can_process
                    );

                    if !can_process {
                        warn!(
                            "Buffered job {} from app_instance {} is in '{:?}' status, skipping",
                            started_job.job_sequence, started_job.app_instance, job.status
                        );
                        continue;
                    }
                    // Fetch agent method configuration
                    let agent_method =
                        match fetch_agent_method(&job.developer, &job.agent, &job.agent_method)
                            .await
                        {
                            Ok(method) => method,
                            Err(e) => {
                                error!(
                                    "Failed to fetch agent method for buffered job {}: {}",
                                    job.job_sequence, e
                                );
                                // Add to failed cache
                                self.jobs_cache
                                    .add_failed_job(job.app_instance.clone(), job.job_sequence)
                                    .await;
                                continue;
                            }
                        };

                    // Check if we have sufficient resources
                    match can_run_agent(&self.state, &agent_method).await {
                        Ok(true) => {
                            // Resources available, continue
                        }
                        Ok(false) => {
                            // Put job back in buffer (at the front) and wait
                            self.state.add_started_jobs(vec![started_job]).await;
                            debug!(
                                "Insufficient resources for job {}, returned to buffer",
                                job.job_sequence
                            );
                            break;
                        }
                        Err(e) => {
                            error!("Error checking resources: {}", e);
                            // Put job back in buffer
                            self.state.add_started_jobs(vec![started_job]).await;
                            break;
                        }
                    }

                    // Try to acquire a lock for this job
                    let lock_manager = get_job_lock_manager();
                    let job_lock =
                        match lock_manager.try_lock_job(&job.app_instance, job.job_sequence) {
                            Some(lock) => lock,
                            None => {
                                debug!("Job {} already locked, skipping", job.job_sequence);
                                continue;
                            }
                        };

                    info!(
                        "🐳 Starting Docker container for buffered job {}: {}/{}/{}",
                        job.job_sequence, job.developer, job.agent, job.agent_method
                    );

                    // Update metrics
                    if let Some(ref metrics) = self.metrics {
                        metrics.set_docker_containers(loading_count + 1, running_count);
                    }

                    // Clone necessary data for the spawned task
                    let state_clone = self.state.clone();
                    let docker_manager_clone = self.docker_manager.clone();
                    let jobs_cache_clone = self.jobs_cache.clone();
                    let container_timeout_secs = self.container_timeout_secs;
                    let secrets_client_clone = self.secrets_client.clone();

                    // Spawn task to run Docker container
                    tokio::spawn(async move {
                        run_docker_container_task(
                            job,
                            agent_method,
                            state_clone,
                            docker_manager_clone,
                            jobs_cache_clone,
                            container_timeout_secs,
                            secrets_client_clone,
                            job_lock,
                        )
                        .await;
                    });
                }
                Ok(None) => {
                    warn!(
                        "Buffered job {} from app_instance {} not found in blockchain (may have been deleted or completed)",
                        started_job.job_sequence, started_job.app_instance
                    );
                    // Job doesn't exist or can't be fetched, don't put it back in buffer
                }
                _ => {
                    // This shouldn't happen with our current logic
                    error!(
                        "Unexpected error fetching buffered job {} from blockchain",
                        started_job.job_sequence
                    );
                }
            }
        }

        Ok(())
    }

    /// Main loop for the job searcher
    pub async fn run(&mut self) -> Result<()> {
        // Check initial buffer status
        let initial_buffer_count = self.state.get_started_jobs_count().await;
        info!(
            "🔍 Job searcher started (buffer: {} jobs)",
            initial_buffer_count
        );

        loop {
            // Check for shutdown request
            if self.state.is_shutting_down() {
                info!("🛑 Job searcher received shutdown signal");

                // If we have a running container, handle it based on shutdown type
                let job_info = self.current_job_info.read().await.clone();
                if let Some((container_id, job)) = job_info {
                    if self.state.is_force_shutdown() {
                        error!(
                            "Force shutdown - terminating Docker container {} for job {}",
                            container_id, job.job_sequence
                        );

                        // Try to get logs before terminating
                        if let Some(logs) = self
                            .docker_manager
                            .get_container_logs_safe(&container_id)
                            .await
                        {
                            info!("Container logs before force termination:\n{}", logs);
                        }

                        // Force stop the container with a short timeout
                        if let Err(e) = self
                            .docker_manager
                            .stop_container_with_timeout(
                                &container_id,
                                DOCKER_CONTAINER_FORCE_STOP_TIMEOUT_SECS,
                            )
                            .await
                        {
                            error!("Failed to stop container {}: {}", container_id, e);
                        }

                        // Ensure the job is failed on blockchain
                        ensure_job_failed_if_not_completed(&job, "force shutdown", &self.state)
                            .await;
                    } else {
                        warn!(
                            "Waiting for Docker container {} (job {}) to complete before shutdown...",
                            container_id, job.job_sequence
                        );
                        warn!("Press Ctrl-C again to force terminate the container");

                        // Wait for container to finish or force shutdown
                        let mut check_interval = tokio::time::interval(Duration::from_secs(
                            CONTAINER_STATUS_CHECK_INTERVAL_SECS,
                        ));
                        loop {
                            check_interval.tick().await;

                            if self.state.is_force_shutdown() {
                                error!(
                                    "Force shutdown requested - terminating container {}",
                                    container_id
                                );

                                // Get logs and terminate
                                if let Some(logs) = self
                                    .docker_manager
                                    .get_container_logs_safe(&container_id)
                                    .await
                                {
                                    info!("Container logs before force termination:\n{}", logs);
                                }

                                if let Err(e) = self
                                    .docker_manager
                                    .stop_container_with_timeout(&container_id, 5)
                                    .await
                                {
                                    error!("Failed to stop container {}: {}", container_id, e);
                                }

                                // Ensure the job is failed on blockchain
                                ensure_job_failed_if_not_completed(
                                    &job,
                                    "force shutdown during wait",
                                    &self.state,
                                )
                                .await;
                                break;
                            }

                            // Check if container is still running
                            let current_job = self.current_job_info.read().await.clone();
                            if current_job.is_none() {
                                info!("Container completed during shutdown wait");
                                break;
                            }
                        }
                    }
                }

                return Ok(());
            }

            // Job searcher cycle:
            // 1) Try to start all jobs from buffer (from previous cycle)
            // 2) Collect new jobs
            // 3) Call multicall  
            // 4) Multicall adds successfully started jobs to buffer
            // 5) Process newly buffered jobs - start Docker containers
            // 6) Sleep for MULTICALL_INTERVAL_SECS
            // 7) Repeat

            debug!("Starting job searcher cycle");
            
            // Step 1: Try to start all jobs from buffer
            debug!("Step 1: Processing jobs from buffer");
            if let Err(e) = self.process_buffer_jobs().await {
                error!("Failed to process buffer jobs: {}", e);
            }

            // Check if shutdown was requested - if so, skip collecting new jobs
            if self.state.is_shutting_down() {
                debug!("Shutdown requested, skipping new job collection and waiting for existing jobs to complete");
                // Wait a bit before checking again
                sleep(Duration::from_secs(5)).await;
                continue;
            }

            // Step 2: Collect new jobs
            debug!("Step 2: Collecting new jobs");
            
            // Periodically clean up expired entries from the failed jobs cache
            self.jobs_cache.cleanup_expired().await;
            
            // Check for pending jobs and clean up app_instances without jobs
            let jobs = self.check_and_clean_pending_jobs().await?;
            
            if !jobs.is_empty() {
                // Calculate available memory for new jobs
                let hardware_total_gb = get_total_memory_gb();
                let hardware_available_gb = get_available_memory_gb();

                // Get memory used by running containers
                let running_agents = self.state.get_all_current_agents().await;
                let mut running_memory_gb = 0.0f64;
                for (_session_id, agent_info) in &running_agents {
                    if let Ok(method) = fetch_agent_method(
                        &agent_info.developer,
                        &agent_info.agent,
                        &agent_info.agent_method,
                    )
                    .await
                    {
                        running_memory_gb += method.min_memory_gb as f64;
                    }
                }

                // Get memory reserved by jobs in buffer
                let buffer_count = self.state.get_started_jobs_count().await;
                // TODO: Track actual buffer memory in SharedSuiState
                // For now, estimate 3GB per buffered job (conservative)
                let buffer_memory_gb = (buffer_count as f64) * 3.0;

                // Calculate memory available for new jobs
                // Available = Hardware Available - Buffer Reserved
                let memory_for_new_jobs_gb = hardware_available_gb as f64 - buffer_memory_gb;

                info!(
                    "Memory status: Hardware total={:.2} GB, available={:.2} GB, running={:.2} GB, buffer={:.2} GB ({}), available for new={:.2} GB",
                    hardware_total_gb as f64,
                    hardware_available_gb as f64,
                    running_memory_gb,
                    buffer_memory_gb,
                    buffer_count,
                    memory_for_new_jobs_gb
                );
                
                // Group jobs by app_instance for multicall batching
                let mut jobs_by_app_instance: std::collections::HashMap<String, Vec<Job>> =
                    std::collections::HashMap::new();
                for job in jobs {
                    jobs_by_app_instance
                        .entry(job.app_instance.clone())
                        .or_insert_with(Vec::new)
                        .push(job);
                }

                debug!(
                    "Grouping jobs for {} app_instances",
                    jobs_by_app_instance.len()
                );

                // Process each app_instance's jobs and add to multicall queue
                let mut total_jobs_added = 0;
                let mut all_settlement_jobs = Vec::new();
                let mut all_merge_jobs = Vec::new();
                let mut all_other_jobs = Vec::new();

                for (app_instance, app_jobs) in jobs_by_app_instance {
                    debug!(
                        "Processing {} pending jobs for app_instance {}",
                        app_jobs.len(),
                        app_instance
                    );

                    // Track memory for this batch
                    let mut batch_memory_gb = 0.0f64;
                    let mut jobs_to_add = Vec::new();

                    // Collect unique agent methods to fetch
                    let mut unique_methods = std::collections::HashSet::new();
                    for job in &app_jobs {
                        unique_methods.insert((
                            job.developer.clone(),
                            job.agent.clone(),
                            job.agent_method.clone(),
                        ));
                    }

                    // Fetch all unique agent methods in parallel
                    let method_futures: Vec<_> = unique_methods
                        .iter()
                        .map(|(dev, agent, method)| {
                            let dev = dev.clone();
                            let agent = agent.clone();
                            let method = method.clone();
                            async move {
                                let result = fetch_agent_method(&dev, &agent, &method).await;
                                ((dev, agent, method), result)
                            }
                        })
                        .collect();

                    let method_results = future::join_all(method_futures).await;

                    // Build a cache of agent methods
                    let mut method_cache = std::collections::HashMap::new();
                    for ((dev, agent, method), result) in method_results {
                        if let Ok(agent_method) = result {
                            method_cache.insert((dev, agent, method), agent_method);
                        }
                    }

                    debug!(
                        "Fetched {} unique agent methods for {} jobs",
                        method_cache.len(),
                        app_jobs.len()
                    );

                    // Filter jobs based on available memory using cached agent methods
                    for job in app_jobs {
                        let cache_key = (
                            job.developer.clone(),
                            job.agent.clone(),
                            job.agent_method.clone(),
                        );

                        if let Some(agent_method) = method_cache.get(&cache_key) {
                            let job_memory_gb = agent_method.min_memory_gb as f64;

                            // Check if adding this job would exceed available memory
                            if batch_memory_gb + job_memory_gb > memory_for_new_jobs_gb {
                                continue;
                            }

                            // Job fits in memory budget
                            batch_memory_gb += job_memory_gb;
                            let memory_requirement =
                                (agent_method.min_memory_gb as u64) * 1024 * 1024 * 1024;
                            jobs_to_add.push((
                                job,
                                memory_requirement,
                                agent_method.min_memory_gb,
                            ));
                        } else {
                            error!(
                                "Failed to fetch agent method for job {} ({}/{}/{})",
                                job.job_sequence, job.developer, job.agent, job.agent_method
                            );
                            // Add to failed cache to avoid retrying immediately
                            self.jobs_cache
                                .add_failed_job(job.app_instance.clone(), job.job_sequence)
                                .await;
                        }
                    }

                    // Add the jobs that fit in memory to multicall queue
                    if !jobs_to_add.is_empty() {
                        for (job, memory_requirement, memory_gb) in jobs_to_add {
                            // Track job types globally for summary
                            match job.app_instance_method.as_str() {
                                "settle" => all_settlement_jobs.push(job.job_sequence),
                                "merge" => all_merge_jobs.push(job.job_sequence),
                                _ => all_other_jobs.push(job.job_sequence),
                            }

                            self.state
                                .add_start_job_request(
                                    job.app_instance.clone(),
                                    job.job_sequence,
                                    memory_requirement,
                                )
                                .await;

                            debug!(
                                "Added job {} ({}) to multicall queue for {} (memory: {} GB)",
                                job.job_sequence,
                                job.app_instance_method,
                                job.app_instance,
                                memory_gb
                            );
                            total_jobs_added += 1;
                        }
                    }
                }

                if total_jobs_added > 0 {
                    // Build detailed summary of job types
                    let mut job_summary = Vec::new();
                    if !all_settlement_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} settlement ({})",
                            all_settlement_jobs.len(),
                            all_settlement_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }
                    if !all_merge_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} merge ({})",
                            all_merge_jobs.len(),
                            all_merge_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }
                    if !all_other_jobs.is_empty() {
                        job_summary.push(format!(
                            "{} other ({})",
                            all_other_jobs.len(),
                            all_other_jobs
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }

                    info!(
                        "Added {} new jobs to multicall queue: {}",
                        total_jobs_added,
                        job_summary.join(", ")
                    );
                } else {
                    debug!("No new jobs to add to multicall queue");
                }
            } else {
                debug!("No new jobs found to collect");
            }
            
            // Step 3: Call multicall (only if not shutting down)
            if !self.state.is_shutting_down() {
                debug!("Step 3: Executing multicall batches");
                if let Err(e) = self.execute_multicall_batches().await {
                    error!("Failed to execute multicall batches: {}", e);
                }
            } else {
                debug!("Step 3: Skipping multicall due to shutdown");
            }
            
            // Step 4: Try to start jobs with success status from multicall
            // This is already handled in execute_multicall_batches which adds successful jobs to buffer
            debug!("Step 4: Successfully started jobs from multicall have been added to buffer");
            
            // Step 5: Process newly buffered jobs - start Docker containers
            debug!("Step 5: Processing newly buffered jobs from multicall");
            if let Err(e) = self.process_buffer_jobs().await {
                error!("Failed to process buffer jobs after multicall: {}", e);
            }
            
            // Step 6: Sleep for MULTICALL_INTERVAL_SECS
            debug!("Step 6: Sleeping for {} seconds before next cycle", MULTICALL_INTERVAL_SECS);
            sleep(Duration::from_secs(MULTICALL_INTERVAL_SECS)).await;
            
            // Step 7: Loop back to step 1
        }
    }

    /// Check for pending jobs and clean up app_instances without jobs
    /// This combines job searching with cleanup that reconciliation would do
    /// Collects all viable jobs for batching instead of selecting one randomly
    async fn check_and_clean_pending_jobs(&self) -> Result<Vec<Job>> {
        let app_instances = self.state.get_app_instances().await;

        if app_instances.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Checking {} app_instances for pending jobs",
            app_instances.len()
        );

        // Check which app_instances can be removed (completely caught up with no work)
        let mut instances_to_remove = Vec::new();
        for app_instance_id in &app_instances {
            // Fetch the full AppInstance object to check removal conditions
            match sui::fetch::fetch_app_instance(app_instance_id).await {
                Ok(app_instance) => {
                    if can_remove_app_instance(&app_instance) {
                        info!(
                            "App instance {} is fully caught up and can be removed",
                            app_instance_id
                        );
                        instances_to_remove.push(app_instance_id.clone());
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch app_instance {} for removal check: {}",
                        app_instance_id, e
                    );
                }
            }
        }

        // Remove fully caught up instances
        for instance_id in &instances_to_remove {
            self.state.remove_app_instance(instance_id).await;
            debug!("Removed fully caught up app_instance: {}", instance_id);
        }

        // Now fetch actual pending jobs from remaining app_instances
        let remaining_instances = self.state.get_app_instances().await;
        if remaining_instances.is_empty() {
            debug!("No app_instances with pending jobs remaining after cleanup");
            return Ok(Vec::new());
        }

        debug!(
            "Fetching pending jobs from {} remaining app_instances",
            remaining_instances.len()
        );

        match fetch_all_pending_jobs(&remaining_instances, false, self.state.is_settle_only()).await
        {
            Ok(pending_jobs) => {
                if pending_jobs.is_empty() {
                    // No pending jobs found
                    debug!("No pending jobs found after detailed fetch");
                    return Ok(Vec::new());
                }

                let total_jobs = pending_jobs.len();

                // Filter out jobs that are locked or in failed cache
                let lock_manager = get_job_lock_manager();
                let mut viable_jobs = Vec::new();
                let mut locked_count = 0usize;
                let mut failed_cached_count = 0usize;
                for job in pending_jobs {
                    // Skip if job is currently locked (being processed)
                    if lock_manager.is_locked(&job.app_instance, job.job_sequence) {
                        debug!(
                            "Skipping job {} from app_instance {} (currently locked)",
                            job.job_sequence, job.app_instance
                        );
                        locked_count += 1;
                        continue;
                    }

                    // Skip if job recently failed
                    if self
                        .jobs_cache
                        .is_recently_failed(&job.app_instance, job.job_sequence)
                        .await
                    {
                        info!(
                            "Skipping job {} from app_instance {} (recently failed)",
                            job.job_sequence, job.app_instance
                        );
                        failed_cached_count += 1;
                        continue;
                    }

                    viable_jobs.push(job);
                }

                if viable_jobs.is_empty() {
                    debug!(
                        "No viable jobs after filtering (total={}, locked={}, failed_cached={})",
                        total_jobs, locked_count, failed_cached_count
                    );
                    return Ok(Vec::new());
                }

                // Check if we're in settle_only mode
                if self.state.is_settle_only() {
                    // In settle_only mode, only process settlement jobs
                    let settlement_jobs: Vec<Job> = viable_jobs
                        .iter()
                        .filter(|job| job.app_instance_method == "settle")
                        .cloned()
                        .collect();

                    if !settlement_jobs.is_empty() {
                        debug!(
                            "Found {} settlement jobs (settle_only mode)",
                            settlement_jobs.len()
                        );
                        return Ok(settlement_jobs);
                    } else {
                        debug!("No settlement jobs available (settle_only mode)");
                        return Ok(Vec::new());
                    }
                }

                // Normal mode: Collect all viable jobs (up to pool size)
                // Priority order: settlement > merge > others
                // But we'll return all and let multicall handle execution order

                // Separate jobs by type
                let mut settlement_jobs: Vec<Job> = Vec::new();
                let mut merge_jobs: Vec<Job> = Vec::new();
                let mut other_jobs: Vec<Job> = Vec::new();

                for job in viable_jobs {
                    match job.app_instance_method.as_str() {
                        "settle" => settlement_jobs.push(job),
                        "merge" => merge_jobs.push(job),
                        _ => other_jobs.push(job),
                    }
                }

                // Build the job pool respecting pool size limit
                let mut job_pool = Vec::new();

                // Add all settlement jobs (highest priority)
                job_pool.extend(settlement_jobs.clone());

                // Add merge jobs up to remaining pool size
                let remaining = JOB_SELECTION_POOL_SIZE.saturating_sub(job_pool.len());
                job_pool.extend(merge_jobs.iter().take(remaining).cloned());

                // Add other jobs up to remaining pool size
                let remaining = JOB_SELECTION_POOL_SIZE.saturating_sub(job_pool.len());
                job_pool.extend(other_jobs.iter().take(remaining).cloned());

                info!(
                    "Collected {} jobs for batching: {} settlement, {} merge, {} other",
                    job_pool.len(),
                    settlement_jobs.len(),
                    merge_jobs
                        .len()
                        .min(JOB_SELECTION_POOL_SIZE.saturating_sub(settlement_jobs.len())),
                    other_jobs.len().min(
                        JOB_SELECTION_POOL_SIZE
                            .saturating_sub(settlement_jobs.len() + merge_jobs.len())
                    )
                );

                // Report metrics for job pool
                if let Some(ref metrics) = self.metrics {
                    if !job_pool.is_empty() {
                        metrics.set_job_selection_metrics(
                            job_pool.len(),
                            merge_jobs.len(),
                            other_jobs.len(),
                            settlement_jobs.len(),
                            locked_count,
                            failed_cached_count,
                            job_pool[0].job_sequence, // Use first job for metric
                            job_pool[0].app_instance.clone(),
                        );
                    }
                }

                Ok(job_pool)
            }
            Err(e) => {
                error!("Failed to fetch pending jobs: {}", e);
                Err(CoordinatorError::Other(e))
            }
        }
    }

    /// Process collected jobs by adding them to multicall batch
    #[allow(dead_code)]
    async fn _batch_job_starts(&self, jobs: Vec<Job>) -> Result<()> {
        for job in jobs {
            // Fetch agent method to get memory requirements
            match fetch_agent_method(&job.developer, &job.agent, &job.app_instance_method).await {
                Ok(agent_method) => {
                    let memory_requirement =
                        (agent_method.min_memory_gb as u64) * 1024 * 1024 * 1024; // Convert GB to bytes

                    // Add to multicall batch
                    self.state
                        .add_start_job_request(
                            job.app_instance.clone(),
                            job.job_sequence,
                            memory_requirement,
                        )
                        .await;

                    debug!(
                        "Added job {} to start batch (memory: {} GB)",
                        job.job_sequence, agent_method.min_memory_gb
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to fetch agent method for job {}: {}",
                        job.job_sequence, e
                    );
                    // Skip this job
                }
            }
        }
        Ok(())
    }

    /// Execute pending multicall batches
    async fn execute_multicall_batches(&self) -> Result<()> {
        use std::time::Duration;
        
        // Check for app instances with pending requests that have waited long enough
        let app_instances = self.state
            .has_pending_multicall_requests(Duration::from_secs(MULTICALL_INTERVAL_SECS))
            .await;

        if app_instances.is_empty() {
            debug!("No app instances ready for multicall execution (waiting for {} second interval)", MULTICALL_INTERVAL_SECS);
            return Ok(());
        }

        debug!("Found {} app instances ready for multicall execution", app_instances.len());
        self.execute_multicall_batches_for_app_instances(app_instances).await
    }
    
    /// Execute multicall batches for specific app instances
    async fn execute_multicall_batches_for_app_instances(&self, app_instances: Vec<String>) -> Result<()> {
        use crate::constants::MULTICALL_BATCH_DELAY_SECS;

        // Get multicall limits
        let max_operations = sui::get_max_operations_per_multicall();

        for app_instance in app_instances {
            if let Some(requests) = self.state.take_multicall_requests(&app_instance).await {
                // Get available memory similar to hardware check
                let available_memory_bytes = get_available_memory_gb() * 1024 * 1024 * 1024;

                // Prepare all operations
                let all_start_jobs = requests.start_jobs;
                let all_complete_jobs = requests.complete_jobs;
                let all_fail_jobs = requests.fail_jobs;
                let all_terminate_jobs = requests.terminate_jobs;
                let all_update_state_for_sequences = requests.update_state_for_sequences;
                let all_submit_proofs = requests.submit_proofs;
                let all_create_app_jobs = requests.create_app_jobs;
                let all_create_merge_jobs = requests.create_merge_jobs;

                // Calculate total operations (update_state_for_sequences, submit_proofs, create_app_jobs and create_merge_jobs are now included in multicall)
                let total_operations = all_start_jobs.len()
                    + all_complete_jobs.len()
                    + all_fail_jobs.len()
                    + all_terminate_jobs.len()
                    + all_update_state_for_sequences.len()
                    + all_submit_proofs.len()
                    + all_create_app_jobs.len()
                    + all_create_merge_jobs.len();

                if total_operations == 0 {
                    continue;
                }

                // Determine if we need to batch
                let needs_batching = total_operations > max_operations;

                if needs_batching {
                    info!(
                        "Total operations ({}) exceeds limit ({}), splitting into batches for app_instance {}",
                        total_operations, max_operations, app_instance
                    );

                    // Execute in batches
                    let mut batch_num = 0;
                    let mut start_idx = 0;
                    let mut complete_idx = 0;
                    let mut fail_idx = 0;
                    let mut terminate_idx = 0;

                    loop {
                        let mut batch_size = 0;
                        let mut batch_start = Vec::new();
                        let mut batch_start_memory = Vec::new();
                        let mut batch_complete = Vec::new();
                        let mut batch_fail = Vec::new();
                        let mut batch_fail_errors = Vec::new();
                        let mut batch_terminate = Vec::new();

                        // Fill batch up to max_operations, prioritizing complete, fail, terminate, then start

                        // Add complete operations
                        while complete_idx < all_complete_jobs.len() && batch_size < max_operations {
                            batch_complete.push(all_complete_jobs[complete_idx].job_sequence);
                            complete_idx += 1;
                            batch_size += 1;
                        }

                        // Add fail operations
                        while fail_idx < all_fail_jobs.len() && batch_size < max_operations {
                            batch_fail.push(all_fail_jobs[fail_idx].job_sequence);
                            batch_fail_errors.push(all_fail_jobs[fail_idx].error.clone());
                            fail_idx += 1;
                            batch_size += 1;
                        }

                        // Add terminate operations
                        while terminate_idx < all_terminate_jobs.len() && batch_size < max_operations {
                            batch_terminate.push(all_terminate_jobs[terminate_idx].job_sequence);
                            terminate_idx += 1;
                            batch_size += 1;
                        }

                        // Add start operations
                        while start_idx < all_start_jobs.len() && batch_size < max_operations {
                            batch_start.push(all_start_jobs[start_idx].job_sequence);
                            batch_start_memory.push(all_start_jobs[start_idx].memory_requirement);
                            start_idx += 1;
                            batch_size += 1;
                        }

                        if batch_size == 0 {
                            break; // No more operations to process
                        }

                        debug!(
                            "Executing batch {} for {}: {} start, {} complete, {} fail, {} terminate",
                            batch_num + 1,
                            app_instance,
                            batch_start.len(),
                            batch_complete.len(),
                            batch_fail.len(),
                            batch_terminate.len()
                        );

                        // Execute this batch (no update_state, submit_proof, create_jobs or create_merge_jobs in batched execution)
                        self.execute_single_multicall(
                            &app_instance,
                            batch_complete,
                            batch_fail,
                            batch_fail_errors,
                            batch_terminate,
                            batch_start,
                            batch_start_memory,
                            available_memory_bytes,
                            Vec::new(), // No update_state in batched execution
                            Vec::new(), // No submit_proofs in batched execution
                            Vec::new(), // No create_jobs in batched execution
                            Vec::new(), // No create_merge_jobs in batched execution
                        )
                        .await?;

                        batch_num += 1;

                        // Add delay between batches
                        if start_idx < all_start_jobs.len()
                            || complete_idx < all_complete_jobs.len()
                            || fail_idx < all_fail_jobs.len()
                            || terminate_idx < all_terminate_jobs.len()
                        {
                            debug!(
                                "Waiting {} seconds before next batch for app_instance {}",
                                MULTICALL_BATCH_DELAY_SECS, app_instance
                            );
                            sleep(Duration::from_secs(MULTICALL_BATCH_DELAY_SECS)).await;
                        }
                    }

                    // Handle update_state_for_sequences, submit_proofs, create_app_jobs and create_merge_jobs after regular job operations
                    if !all_update_state_for_sequences.is_empty()
                        || !all_submit_proofs.is_empty()
                        || !all_create_app_jobs.is_empty()
                        || !all_create_merge_jobs.is_empty()
                    {
                        // Prepare operations for the final multicall
                        let update_state_operations: Vec<(u64, Option<Vec<u8>>, Option<String>)> = all_update_state_for_sequences
                            .iter()
                            .map(|req| (
                                req.sequence,
                                req.new_state_data.clone(),
                                req.new_data_availability_hash.clone(),
                            ))
                            .collect();
                        
                        // Prepare submit_proof operations
                        let submit_proof_operations: Vec<(u64, Vec<u64>, Option<Vec<u64>>, Option<Vec<u64>>, String, String, u8, String, u64, u64)> = all_submit_proofs
                            .iter()
                            .map(|req| (
                                req.block_number,
                                req.sequences.clone(),
                                req.merged_sequences_1.clone(),
                                req.merged_sequences_2.clone(),
                                req.job_id.clone(),
                                req.da_hash.clone(),
                                req.cpu_cores,
                                req.prover_architecture.clone(),
                                req.prover_memory,
                                req.cpu_time,
                            ))
                            .collect();

                        // Prepare create_app_job operations
                        let create_job_operations: Vec<(String, Option<String>, Option<u64>, Option<Vec<u64>>, Option<Vec<u64>>, Option<Vec<u64>>, Vec<u8>, Option<u64>, Option<u64>, Option<String>)> = all_create_app_jobs
                            .iter()
                            .map(|req| (
                                req.method_name.clone(),
                                req.job_description.clone(),
                                req.block_number,
                                req.sequences.clone(),
                                req.sequences1.clone(),
                                req.sequences2.clone(),
                                req.data.clone(),
                                req.interval_ms,
                                req.next_scheduled_at,
                                req.settlement_chain.clone(),
                            ))
                            .collect();
                        
                        // Prepare create_merge_job operations
                        let merge_job_operations: Vec<(u64, Vec<u64>, Vec<u64>, Vec<u64>, Option<String>)> = all_create_merge_jobs
                            .iter()
                            .map(|req| (
                                req.block_number,
                                req.sequences.clone(),
                                req.sequences1.clone(),
                                req.sequences2.clone(),
                                req.job_description.clone(),
                            ))
                            .collect();

                        debug!(
                            "Executing multicall for {}: {} start, {} complete, {} fail, {} terminate, {} update_state, {} submit_proof, {} create_job, {} create_merge",
                            app_instance,
                            0, // No more start operations in final call
                            0, // No more complete operations in final call
                            0, // No more fail operations in final call  
                            0, // No more terminate operations in final call
                            update_state_operations.len(),
                            submit_proof_operations.len(),
                            create_job_operations.len(),
                            merge_job_operations.len()
                        );

                        // Execute the final multicall with remaining operations
                        self.execute_single_multicall(
                            &app_instance,
                            Vec::new(), // No more regular job operations
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            available_memory_bytes,
                            update_state_operations,
                            submit_proof_operations,
                            create_job_operations,
                            merge_job_operations,
                        )
                        .await?;
                    }
                } else {
                    // Single multicall can handle all operations
                    
                    // Prepare update_state_for_sequences operations
                    let update_state_operations: Vec<(u64, Option<Vec<u8>>, Option<String>)> = all_update_state_for_sequences
                        .iter()
                        .map(|req| (
                            req.sequence,
                            req.new_state_data.clone(),
                            req.new_data_availability_hash.clone(),
                        ))
                        .collect();
                    
                    // Prepare submit_proof operations
                    let submit_proof_operations: Vec<(u64, Vec<u64>, Option<Vec<u64>>, Option<Vec<u64>>, String, String, u8, String, u64, u64)> = all_submit_proofs
                        .iter()
                        .map(|req| (
                            req.block_number,
                            req.sequences.clone(),
                            req.merged_sequences_1.clone(),
                            req.merged_sequences_2.clone(),
                            req.job_id.clone(),
                            req.da_hash.clone(),
                            req.cpu_cores,
                            req.prover_architecture.clone(),
                            req.prover_memory,
                            req.cpu_time,
                        ))
                        .collect();

                    // Prepare create_app_job operations
                    let create_job_operations: Vec<(String, Option<String>, Option<u64>, Option<Vec<u64>>, Option<Vec<u64>>, Option<Vec<u64>>, Vec<u8>, Option<u64>, Option<u64>, Option<String>)> = all_create_app_jobs
                        .iter()
                        .map(|req| (
                            req.method_name.clone(),
                            req.job_description.clone(),
                            req.block_number,
                            req.sequences.clone(),
                            req.sequences1.clone(),
                            req.sequences2.clone(),
                            req.data.clone(),
                            req.interval_ms,
                            req.next_scheduled_at,
                            req.settlement_chain.clone(),
                        ))
                        .collect();
                    
                    // Prepare create_merge_job operations
                    let merge_job_operations: Vec<(u64, Vec<u64>, Vec<u64>, Vec<u64>, Option<String>)> = all_create_merge_jobs
                        .iter()
                        .map(|req| (
                            req.block_number,
                            req.sequences.clone(),
                            req.sequences1.clone(),
                            req.sequences2.clone(),
                            req.job_description.clone(),
                        ))
                        .collect();
                        

                    debug!(
                        "Executing multicall for {}: {} start, {} complete, {} fail, {} terminate, {} update_state, {} submit_proof, {} create_job, {} create_merge",
                        app_instance,
                        all_start_jobs.len(),
                        all_complete_jobs.len(),
                        all_fail_jobs.len(),
                        all_terminate_jobs.len(),
                        update_state_operations.len(),
                        submit_proof_operations.len(),
                        create_job_operations.len(),
                        merge_job_operations.len()
                    );

                    // Extract data for multicall
                    let start_sequences: Vec<u64> = all_start_jobs.iter().map(|j| j.job_sequence).collect();
                    let start_memory: Vec<u64> = all_start_jobs.iter().map(|j| j.memory_requirement).collect();
                    let complete_sequences: Vec<u64> = all_complete_jobs.iter().map(|j| j.job_sequence).collect();
                    let fail_sequences: Vec<u64> = all_fail_jobs.iter().map(|j| j.job_sequence).collect();
                    let fail_errors: Vec<String> = all_fail_jobs.iter().map(|j| j.error.clone()).collect();
                    let terminate_sequences: Vec<u64> = all_terminate_jobs.iter().map(|j| j.job_sequence).collect();

                    // Execute the single multicall with all operations
                    self.execute_single_multicall(
                        &app_instance,
                        complete_sequences,
                        fail_sequences,
                        fail_errors,
                        terminate_sequences,
                        start_sequences,
                        start_memory,
                        available_memory_bytes,
                        update_state_operations,
                        submit_proof_operations,
                        create_job_operations,
                        merge_job_operations,
                    )
                    .await?;
                }
                
                // All operations (including update_state_for_sequences and submit_proofs) are now included in the multicall above
            }
        }
        Ok(())
    }

    /// Execute pending multicall batches immediately without waiting for interval
    async fn execute_multicall_batches_immediate(&self) -> Result<()> {
        // Get all app instances with pending requests (no interval check)
        let app_instances = self.state.get_all_pending_multicall_app_instances().await;
        
        if app_instances.is_empty() {
            debug!("No app instances with pending multicall requests");
            return Ok(());
        }

        debug!("Found {} app instances with pending multicall requests (immediate execution)", app_instances.len());
        self.execute_multicall_batches_for_app_instances(app_instances).await
    }


    /// Execute a single multicall with the given operations
    async fn execute_single_multicall(
        &self,
        app_instance: &str,
        complete_sequences: Vec<u64>,
        fail_sequences: Vec<u64>,
        fail_errors: Vec<String>,
        terminate_sequences: Vec<u64>,
        start_sequences: Vec<u64>,
        start_memory: Vec<u64>,
        available_memory_bytes: u64,
        update_state_for_sequences: Vec<(u64, Option<Vec<u8>>, Option<String>)>,
        submit_proofs: Vec<(u64, Vec<u64>, Option<Vec<u64>>, Option<Vec<u64>>, String, String, u8, String, u64, u64)>,
        create_jobs: Vec<(String, Option<String>, Option<u64>, Option<Vec<u64>>, Option<Vec<u64>>, Option<Vec<u64>>, Vec<u8>, Option<u64>, Option<u64>, Option<String>)>,
        create_merge_jobs: Vec<(u64, Vec<u64>, Vec<u64>, Vec<u64>, Option<String>)>,
    ) -> Result<()> {
        // Use the provided start sequences directly
        // Shutdown check should be done by the caller if needed
        let (final_start_sequences, final_start_memory) = (start_sequences, start_memory);

        let mut sui_interface = sui::SilvanaSuiInterface::new();
        match sui_interface
            .multicall_job_operations(
                app_instance,
                complete_sequences.clone(),
                fail_sequences.clone(),
                fail_errors,
                terminate_sequences.clone(),
                final_start_sequences.clone(),
                final_start_memory.clone(),
                available_memory_bytes, // Pass actual available memory for Move contract validation
                update_state_for_sequences, // Pass update_state_for_sequences  
                submit_proofs,          // Pass submit_proofs  
                create_jobs,            // Pass create_jobs for batched execution
                create_merge_jobs,      // Pass create_merge_jobs for batched execution
                None,                   // Let gas be estimated automatically
            )
            .await
        {
            Ok(result) => {
                info!(
                    "Successfully executed multicall for {} (tx: {})",
                    app_instance, result.tx_digest
                );

                // Add successful jobs to buffer for container launching
                for (i, sequence) in final_start_sequences.iter().enumerate() {
                    let memory_req = final_start_memory.get(i).copied().unwrap_or(0);
                    self.state.add_started_jobs(vec![crate::state::StartedJob {
                        app_instance: app_instance.to_string(),
                        job_sequence: *sequence,
                        memory_requirement: memory_req,
                    }]).await;
                }

                if !final_start_sequences.is_empty() {
                    info!(
                        "✅ Adding {} successfully started jobs to buffer for app_instance {} (out of {} attempted)",
                        final_start_sequences.len(),
                        app_instance,
                        final_start_sequences.len()
                    );
                }

                Ok(())
            }
            Err((error_msg, tx_digest)) => {
                error!(
                    "Failed to execute multicall for {}: {} (tx: {:?})",
                    app_instance, error_msg, tx_digest
                );
                Err(CoordinatorError::Other(anyhow::anyhow!(
                    "Multicall failed for {}: {}",
                    app_instance, error_msg
                )))
            }
        }
    }


    /// Retrieve secrets for a job from the secrets storage (old version, no longer used)
    #[allow(dead_code)]
    async fn retrieve_secrets_for_job_old(
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

    /// Get the current container ID and job info for shutdown logging
    #[allow(dead_code)]
    pub async fn get_current_container_info(&self) -> Option<(String, Job)> {
        self.current_job_info.read().await.clone()
    }

    /// Fetch logs from current container if any
    #[allow(dead_code)]
    pub async fn fetch_current_container_logs(&self) -> Option<String> {
        if let Some((container_id, job)) = self.get_current_container_info().await {
            info!(
                "Fetching logs for container {} (job {})",
                &container_id[..12.min(container_id.len())],
                job.job_sequence
            );

            // Use the safe method that returns Option
            let logs = self
                .docker_manager
                .get_container_logs_safe(&container_id)
                .await;

            if let Some(ref log_content) = logs {
                info!(
                    "Retrieved {} bytes of logs from container",
                    log_content.len()
                );
            } else {
                warn!(
                    "Failed to retrieve logs from container {}",
                    &container_id[..12.min(container_id.len())]
                );
            }

            logs
        } else {
            None
        }
    }
}

/// Standalone function to run a Docker container for a job (for parallel execution)
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

/// Ensure a job is failed on blockchain if it wasn't completed
/// This is called after container termination (normal, timeout, or shutdown)
async fn ensure_job_failed_if_not_completed(job: &Job, reason: &str, state: &SharedState) {
    // First check if there's already a complete/fail request for this job in the multicall queue
    if state.has_pending_job_request(&job.app_instance, job.job_sequence).await {
        return; // Skip blockchain check if already handled
    }

    // Wait for multicall requests to be processed before checking blockchain
    sleep(Duration::from_secs(JOB_STATUS_CHECK_DELAY_SECS)).await;

    // Check again if a request was added while we were waiting
    if state.has_pending_job_request(&job.app_instance, job.job_sequence).await {
        return; // Skip blockchain check if now handled
    }

    // First fetch the app instance to get the jobs table ID
    let app_instance = match sui::fetch::fetch_app_instance(&job.app_instance).await {
        Ok(app_inst) => app_inst,
        Err(e) => {
            error!(
                "Failed to fetch app instance for job {} status check: {}",
                job.job_sequence, e
            );
            return;
        }
    };

    // Get the jobs table ID from the app instance
    let jobs_table_id = match app_instance.jobs {
        Some(jobs) => jobs.jobs_table_id,
        None => {
            error!(
                "No jobs table found in app instance {} for job {}",
                job.app_instance, job.job_sequence
            );
            return;
        }
    };

    // Now check if the job is still in a running state on blockchain
    match sui::fetch::fetch_job_by_id(&jobs_table_id, job.job_sequence).await {
        Ok(Some(current_job)) => {
            match current_job.status {
                sui::fetch::JobStatus::Running => {
                    // Job is still running, we need to fail it
                    let error_message = format!("Container terminated: {}", reason);
                    state
                        .add_fail_job_request(
                            job.app_instance.clone(),
                            job.job_sequence,
                            error_message.clone(),
                        )
                        .await;
                    info!("Job {} found in Running state after container terminated ({}), added fail request to multicall batch", job.job_sequence, reason);
                }
                sui::fetch::JobStatus::Pending => {
                    // For periodic jobs, this is expected - they go back to pending after completion
                    // For failed jobs with retries, they also go back to pending
                    if current_job.interval_ms.is_some() {
                        info!("Periodic job {} returned to Pending state after container terminated ({})", job.job_sequence, reason);
                    } else if current_job.attempts < 3 {
                        info!("Job {} returned to Pending state for retry (attempt {}) after container terminated ({})", job.job_sequence, current_job.attempts, reason);
                    } else {
                        warn!("Job {} unexpectedly in Pending state after container terminated ({})", job.job_sequence, reason);
                    }
                }
                sui::fetch::JobStatus::Failed(_) => {
                    info!("Job {} already in Failed state on blockchain after container terminated ({})", job.job_sequence, reason);
                }
            }
        }
        Ok(None) => {
            info!("Job {} no longer exists on blockchain after container terminated ({})", job.job_sequence, reason);
        }
        Err(e) => {
            error!("Failed to fetch job {} status after container terminated ({}): {}", job.job_sequence, reason, e);
        }
    }
}

/// Standalone function to run a Docker container for a job (for parallel execution)
/// The job_lock is held for the duration of this task to prevent duplicate processing
async fn run_docker_container_task(
    job: Job,
    agent_method: AgentMethod,
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    _jobs_cache: JobsCache,
    container_timeout_secs: u64,
    mut secrets_client: Option<SecretsClient>,
    _job_lock: JobLockGuard, // Hold the lock for the duration of the task
) {
    let job_start = Instant::now();

    debug!(
        "🐳 Running Docker task: dev={}, agent={}/{}, job_seq={}, app={}",
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

    // Start the job on Sui blockchain before processing
    debug!(
        "🔗 Preparing to start job {} on Sui blockchain",
        job.job_sequence
    );

    // We already have the lock for this job, so no other task can process it
    // The lock will be automatically released when this function returns (via Drop trait)

    // Track container as loading in docker state
    docker_manager
        .track_container_loading(
            docker_session.session_id.clone(),
            job.job_sequence.to_string(),
        )
        .await;

    // Set current agent in coordinator state (for job tracking)
    state
        .set_current_agent(
            docker_session.session_id.clone(),
            job.developer.clone(),
            job.agent.clone(),
            job.agent_method.clone(),
        )
        .await;

    // Log how many containers are loading/running
    let (loading_count, running_count) = docker_manager.get_container_counts().await;
    debug!(
        "🔄 Preparing Docker container: {} loading, {} running",
        loading_count, running_count
    );

    debug!(
        "Job {} not tracked locally, calculating proportional delay before start",
        job.job_sequence
    );

    // Calculate proportional delay based on number of running containers
    // This helps balance load across coordinators - those with fewer containers get jobs first
    let total_containers = (loading_count + running_count) as u64;
    let proportional_delay = std::cmp::min(
        total_containers * JOB_ACQUISITION_DELAY_PER_CONTAINER_MS,
        JOB_ACQUISITION_MAX_DELAY_MS,
    );

    // Add small random jitter on top to prevent exact synchronization
    use rand::Rng;
    let jitter_ms = rand::thread_rng().gen_range(0..proportional_delay);
    let total_delay_ms = proportional_delay + jitter_ms;

    debug!(
        "Adding delay of {}ms ({}ms proportional + {}ms jitter) before starting job {} \
         based on {} running containers",
        total_delay_ms, proportional_delay, jitter_ms, job.job_sequence, total_containers
    );
    tokio::time::sleep(Duration::from_millis(total_delay_ms)).await;

    // Fetch fresh AppInstance data to ensure job is still pending
    // This is critical to avoid EJobNotPending errors
    debug!(
        "Fetching fresh AppInstance data for job {} after delay",
        job.job_sequence
    );
    let fresh_app_instance = match sui::fetch::fetch_app_instance(&job.app_instance).await {
        Ok(app_inst) => app_inst,
        Err(e) => {
            warn!(
                "Failed to fetch fresh AppInstance for job {}: {} - skipping",
                job.job_sequence, e
            );
            // Don't add to failed cache - this is a network/fetch error, not a job failure
            // We'll retry this job on the next iteration
            state.clear_current_agent(&docker_session.session_id).await;
            docker_manager
                .remove_container_tracking(&docker_session.session_id)
                .await;
            return;
        }
    };

    // For buffered jobs that were started via multicall, they won't be in pending_jobs anymore
    // We've already verified the job exists and is in a processable state earlier
    // So we just log and continue
    let job_in_pending = if let Some(jobs) = &fresh_app_instance.jobs {
        jobs.pending_jobs.contains(&job.job_sequence)
    } else {
        false
    };

    if job_in_pending {
        // This shouldn't happen for buffered jobs that were started via multicall
        warn!(
            "Buffered job {} is still in pending_jobs list - multicall may have failed to start it",
            job.job_sequence
        );
        // Continue anyway since we need to process it
    } else {
        debug!(
            "Job {} is not in pending_jobs (expected for buffered job started via multicall)",
            job.job_sequence
        );
    }

    debug!(
        "Job {} proceeding with Docker container execution",
        job.job_sequence
    );

    // Try to start the job on blockchain - NO RETRIES since other coordinators might be handling it
    let start_time = Instant::now();

    debug!(
        "Job {} should already be started via multicall",
        job.job_sequence
    );

    // Jobs are now started via multicall, not individually
    // We assume the job was already started when added to the buffer
    let tx_digest = "started_via_multicall".to_string();

    // Old individual start_job_tx code removed - jobs now started via multicall
    /*
    let tx_digest = match sui::transactions::start_job_tx(&job.app_instance, job.job_sequence).await
    {
        Ok(tx_digest) => {
            debug!(
                "Successfully started job {} on blockchain, tx: {}",
                job.job_sequence, tx_digest
            );
            tx_digest
        }
        Err(e) => {
            let error_str = e.to_string();

            // Check if the error is because the job is not in pending state or not found
            // This can happen if another coordinator already started it
            if error_str.contains("Job is not in pending state") ||
               error_str.contains("EJobNotPending") ||
               error_str.contains("13906835325394878469") || // EJobNotPending abort code
               error_str.contains("Job not found") ||
               error_str.contains("EJobNotFound") ||
               error_str.contains("13906835322940243973")
            {
                // EJobNotFound abort code
                warn!(
                    "Job {} cannot be started - likely already handled by another coordinator: {}",
                    job.job_sequence, error_str
                );

                // Add to failed jobs cache so we don't try again for 5 minutes
                jobs_cache
                    .add_failed_job(job.app_instance.clone(), job.job_sequence)
                    .await;

                // Clear the agent state and return
                state.clear_current_agent(&docker_session.session_id).await;
                docker_manager
                    .remove_container_tracking(&docker_session.session_id)
                    .await;
                return;
            }

            // This can happen due to race conditions even with our random delay
            // Log as warning since it's expected in a multi-coordinator environment
            info!(
                "Failed to start job {} on blockchain: {}",
                job.job_sequence, e
            );

            // For other blockchain errors, add to failed cache to avoid immediate retry
            // This could be due to gas issues, network problems, etc.
            jobs_cache
                .add_failed_job(job.app_instance.clone(), job.job_sequence)
                .await;
            state.clear_current_agent(&docker_session.session_id).await;
            docker_manager
                .remove_container_tracking(&docker_session.session_id)
                .await;
            return;
        }
    };
    */

    let start_elapsed = start_time.elapsed();

    // Add job to agent database as a session-specific job for Docker retrieval
    let mut agent_job = AgentJob::new(job.clone(), &state);
    agent_job.start_tx_hash = Some(tx_digest.clone());
    agent_job.start_tx_sent = true;
    let job_id = agent_job.job_id.clone();
    // Use add_session_job to link this job to the Docker session
    state
        .get_agent_job_db()
        .add_session_job(docker_session.session_id.clone(), agent_job)
        .await;

    info!(
        "✅ Job {} started and reserved for Docker session {}, job_id: {}, tx: {}, start_time: {:?}",
        job.job_sequence, docker_session.session_id, job_id, tx_digest, start_elapsed
    );

    // Pull the Docker image if needed
    info!("Pulling Docker image: {}", agent_method.docker_image);
    let pull_start = Instant::now();
    let _digest = match docker_manager
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
                        "Docker image SHA256 mismatch for job {}: expected {}, got {}",
                        job.job_sequence, expected_sha, digest
                    );
                    state.clear_current_agent(&docker_session.session_id).await;
                    docker_manager
                        .remove_container_tracking(&docker_session.session_id)
                        .await;

                    // Log container count after clearing
                    let (loading_count, running_count) =
                        docker_manager.get_container_counts().await;
                    info!(
                        "🏁 Docker container aborted (SHA mismatch): {} loading, {} running",
                        loading_count, running_count
                    );
                    return;
                }
            }
            digest
        }
        Err(e) => {
            error!("Failed to pull image for job {}: {}", job.job_sequence, e);
            state.clear_current_agent(&docker_session.session_id).await;
            docker_manager
                .remove_container_tracking(&docker_session.session_id)
                .await;

            // Log container count after clearing
            let (loading_count, running_count) = docker_manager.get_container_counts().await;
            info!(
                "🏁 Docker container aborted (image pull failed): {} loading, {} running",
                loading_count, running_count
            );
            return;
        }
    };

    // Configure the Docker container with memory limits
    let mut env_vars = HashMap::new();
    env_vars.insert("CHAIN".to_string(), state.get_chain().clone());
    env_vars.insert(
        "COORDINATOR_ID".to_string(),
        state.get_coordinator_id().clone(),
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
    if let Some(ref mut secrets_client) = secrets_client {
        match retrieve_secrets_for_job(secrets_client, &job).await {
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

    let config = ContainerConfig {
        image_name: agent_method.docker_image.clone(),
        image_source: agent_method.docker_image.clone(),
        command: vec![],
        env_vars,
        port_bindings: HashMap::new(),
        volume_binds: vec![],
        timeout_seconds: container_timeout_secs,
        // Set memory limit based on agent method requirements (convert GB to MB)
        memory_limit_mb: Some((agent_method.min_memory_gb as u64) * 1024),
        // Set CPU cores limit
        cpu_cores: Some(agent_method.min_cpu_cores as f64),
        network_mode: if agent_method.requires_tee {
            Some("host".to_string())
        } else {
            None
        },
        requires_tee: agent_method.requires_tee,
        extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
    };

    // Update state to Running and run the Docker container
    docker_manager
        .mark_container_running(&docker_session.session_id)
        .await;
    let (loading_count, running_count) = docker_manager.get_container_counts().await;
    info!(
        "🐳 Starting Docker container for job {} ({} loading, {} running)",
        job.job_sequence, loading_count, running_count
    );
    let docker_start = Instant::now();
    match docker_manager.run_container(&config).await {
        Ok(result) => {
            let docker_elapsed = docker_start.elapsed();
            let total_elapsed = job_start.elapsed();

            if result.exit_code == 0 {
                info!(
                    "✅ Docker container succeeded for job {} (docker: {:?}, total: {:?})",
                    job.job_sequence, docker_elapsed, total_elapsed
                );
            } else {
                error!(
                    "❌ Docker container failed for job {} with exit code {} (docker: {:?}, total: {:?})",
                    job.job_sequence, result.exit_code, docker_elapsed, total_elapsed
                );
            }

            // Always display Docker logs after container exits
            if !result.logs.is_empty() {
                info!(
                    "📋 Docker container logs for job {}:\n{}",
                    job.job_sequence, result.logs
                );
            }
        }
        Err(e) => {
            let docker_elapsed = docker_start.elapsed();
            let total_elapsed = job_start.elapsed();
            error!(
                "Failed to run Docker container for job {}: {} (docker: {:?}, total: {:?})",
                job.job_sequence, e, docker_elapsed, total_elapsed
            );
        }
    }

    // Ensure the job is failed on blockchain if it wasn't completed
    // This handles the case where the container exits without the agent calling complete/fail
    ensure_job_failed_if_not_completed(&job, "container terminated", &state).await;

    // Clear the agent state after Docker completes
    state.clear_current_agent(&docker_session.session_id).await;
    docker_manager
        .remove_container_tracking(&docker_session.session_id)
        .await;

    // Log how many containers are still loading/running
    let (loading_count, running_count) = docker_manager.get_container_counts().await;
    info!(
        "🏁 Docker container finished: {} loading, {} running",
        loading_count, running_count
    );

    // Job will be automatically cleaned up from the database when retrieved or replaced

    info!(
        "🏁 Job {} completed, total time: {:?}",
        job.job_sequence,
        job_start.elapsed()
    );
}
