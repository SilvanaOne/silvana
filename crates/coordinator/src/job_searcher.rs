use crate::agent::AgentJob;
use crate::constants::{
    AGENT_MIN_MEMORY_REQUIREMENT_GB, CONTAINER_STATUS_CHECK_INTERVAL_SECS,
    DOCKER_CONTAINER_FORCE_STOP_TIMEOUT_SECS, JOB_ACQUISITION_DELAY_PER_CONTAINER_MS,
    JOB_ACQUISITION_MAX_DELAY_MS, JOB_BUFFER_MEMORY_COEFFICIENT, JOB_SELECTION_POOL_SIZE,
    JOB_STATUS_CHECK_DELAY_SECS, MAX_CONCURRENT_AGENT_CONTAINERS, MULTICALL_INTERVAL_SECS,
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
use tokio::sync::RwLock;
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
    #[allow(dead_code)]
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
                    let can_process = matches!(job.status, sui::fetch::JobStatus::Running);

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
                            warn!(
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
                                warn!("Job {} already locked, skipping", job.job_sequence);
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
            info!("Step 1: Processing jobs from buffer");
            if let Err(e) = self.process_buffer_jobs().await {
                error!("Failed to process buffer jobs: {}", e);
            }

            // Check if shutdown was requested - if so, skip collecting new jobs
            if self.state.is_shutting_down() {
                info!(
                    "Shutdown requested, skipping new job collection and waiting for existing jobs to complete"
                );
                // Wait a bit before checking again
                sleep(Duration::from_secs(5)).await;
                continue;
            }

            // Step 2: Collect new jobs
            info!("Step 2: Collecting new jobs");

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

                // Calculate memory available for new jobs with coefficient multiplier
                // Available = (Hardware Available - Buffer Reserved) * Coefficient
                // Coefficient allows more jobs to be buffered between multicall intervals
                let memory_for_new_jobs_gb = (hardware_available_gb as f64 - buffer_memory_gb)
                    * JOB_BUFFER_MEMORY_COEFFICIENT;

                info!(
                    "Memory status: Hardware total={:.2} GB, available={:.2} GB, running={:.2} GB, buffer={:.2} GB ({}), available for new={:.2} GB (x{:.1} coefficient)",
                    hardware_total_gb as f64,
                    hardware_available_gb as f64,
                    running_memory_gb,
                    buffer_memory_gb,
                    buffer_count,
                    memory_for_new_jobs_gb,
                    JOB_BUFFER_MEMORY_COEFFICIENT
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
                            jobs_to_add.push((job, memory_requirement, agent_method.min_memory_gb));
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

            // Step 3: Smart batching - check if we should execute multicall
            if !self.state.is_shutting_down() {
                let should_execute_by_limit = self.state.should_execute_multicall_by_limit().await;
                let should_execute_by_time = self.state.should_execute_multicall_by_time().await;
                let total_operations = self.state.get_total_operations_count().await;

                if should_execute_by_limit {
                    info!(
                        "Step 3: Executing multicall due to operation limit: {} operations >= {} limit",
                        total_operations,
                        sui::get_max_operations_per_multicall()
                    );

                    if let Err(e) = self.execute_multicall_batches().await {
                        error!("Failed to execute limit-triggered multicall: {}", e);
                    }

                    // Process newly buffered jobs from multicall
                    if let Err(e) = self.process_buffer_jobs().await {
                        error!(
                            "Failed to process buffer jobs after limit-triggered multicall: {}",
                            e
                        );
                    }
                } else if should_execute_by_time && total_operations > 0 {
                    info!(
                        "Step 3: Executing multicall due to time interval: {} seconds passed with {} operations",
                        MULTICALL_INTERVAL_SECS, total_operations
                    );

                    if let Err(e) = self.execute_multicall_batches().await {
                        error!("Failed to execute time-triggered multicall: {}", e);
                    }

                    // Process newly buffered jobs from multicall
                    if let Err(e) = self.process_buffer_jobs().await {
                        error!(
                            "Failed to process buffer jobs after time-triggered multicall: {}",
                            e
                        );
                    }
                } else if should_execute_by_time {
                    // Time passed but no operations - just update timestamp to reset the timer
                    debug!("Step 3: Time interval passed but no operations to execute");
                    self.state.update_last_multicall_timestamp().await;
                } else if total_operations > 0 {
                    debug!(
                        "Step 3: {} operations pending, waiting {} more seconds ({}s elapsed since last multicall)",
                        total_operations,
                        MULTICALL_INTERVAL_SECS
                            - self.state.get_seconds_since_last_multicall().await,
                        self.state.get_seconds_since_last_multicall().await
                    );
                } else {
                    debug!("Step 3: No operations to process");
                }
            } else {
                debug!("Step 3: Skipping multicall due to shutdown");
            }

            // Step 4: Sleep for 10 seconds before next cycle
            info!("Step 4: Sleeping for 10 seconds before next cycle");
            sleep(Duration::from_secs(10)).await;

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

    /// Execute pending multicall batches
    async fn execute_multicall_batches(&self) -> Result<()> {
        // Check for app instances with pending requests
        let app_instances = self.state.has_pending_multicall_requests().await;

        if app_instances.is_empty() {
            debug!("No app instances with pending multicall requests");
            return Ok(());
        }

        debug!(
            "Found {} app instances ready for multicall execution",
            app_instances.len()
        );
        self.execute_multicall_batches_for_app_instances(app_instances)
            .await
    }

    /// Execute single multicall for all app instances (respects MULTICALL_INTERVAL_SECS)
    async fn execute_multicall_batches_for_app_instances(
        &self,
        app_instances: Vec<String>,
    ) -> Result<()> {
        if app_instances.is_empty() {
            return Ok(());
        }

        let max_operations = sui::get_max_operations_per_multicall();

        // Build only ONE batch per call - don't loop through multiple batches
        let mut current_batch_operations = Vec::new();
        let mut current_batch_started_jobs = Vec::new();
        let mut current_operation_count = 0;
        let mut has_operations = false;

        // Try to fill a single batch by taking operations from each app instance
        for app_instance in &app_instances {
            if current_operation_count >= max_operations {
                break; // Batch is full
            }

            // Try to get some operations from this app instance (without taking all)
            if let Some(operations) = self
                .take_partial_multicall_operations(
                    &app_instance,
                    max_operations - current_operation_count,
                )
                .await
            {
                let operation_count = operations.total_operations();
                if operation_count > 0 {
                    has_operations = true;
                    current_operation_count += operation_count;

                    // Collect started jobs for buffer management
                    for (i, sequence) in operations.start_job_sequences.iter().enumerate() {
                        let memory_req = operations
                            .start_job_memory_requirements
                            .get(i)
                            .copied()
                            .unwrap_or(0);
                        current_batch_started_jobs.push((
                            app_instance.clone(),
                            *sequence,
                            memory_req,
                        ));
                    }

                    info!(
                        "Added {} operations from app_instance {} to batch (batch total: {})",
                        operation_count, app_instance, current_operation_count
                    );

                    current_batch_operations.push(operations);
                }
            }
        }

        // If no operations were collected, we're done
        if !has_operations {
            debug!("No operations to process in this batch");
            return Ok(());
        }

        // Execute this single batch
        info!(
            "Executing multicall batch with {} operations from {} app instances",
            current_operation_count,
            current_batch_operations.len()
        );

        let mut sui_interface = sui::SilvanaSuiInterface::new();
        match sui_interface
            .multicall_job_operations(current_batch_operations, None)
            .await
        {
            Ok(result) => {
                info!(
                    "Successfully executed batch multicall with {} operations (tx: {})",
                    current_operation_count, result.tx_digest
                );

                // Add only successfully started jobs to buffer for container launching
                let successful_start_sequences = result.successful_start_jobs();
                let failed_start_sequences = result.failed_start_jobs();

                if !failed_start_sequences.is_empty() {
                    warn!("Some start jobs failed: {:?}", failed_start_sequences);
                }

                // Filter started jobs to only include successful ones
                let successful_started_jobs: Vec<_> = current_batch_started_jobs
                    .into_iter()
                    .filter(|(_, sequence, _)| successful_start_sequences.contains(sequence))
                    .collect();

                for (app_instance, sequence, memory_req) in successful_started_jobs.iter() {
                    self.state
                        .add_started_jobs(vec![crate::state::StartedJob {
                            app_instance: app_instance.clone(),
                            job_sequence: *sequence,
                            memory_requirement: *memory_req,
                        }])
                        .await;
                }

                info!(
                    "Added {} successfully started jobs to container launch buffer (out of {} attempted)",
                    successful_started_jobs.len(),
                    successful_start_sequences.len() + failed_start_sequences.len(),
                );

                // Update the last multicall timestamp after successful execution
                self.state.update_last_multicall_timestamp().await;
            }
            Err((error_msg, tx_digest_opt)) => {
                error!(
                    "Batch multicall failed: {} (tx: {:?})",
                    error_msg, tx_digest_opt
                );
                return Err(CoordinatorError::Other(anyhow::anyhow!(
                    "Batch multicall failed: {}",
                    error_msg
                )));
            }
        }

        Ok(())
    }

    /// Take partial operations from an app instance up to the specified limit
    async fn take_partial_multicall_operations(
        &self,
        app_instance: &str,
        max_operations: usize,
    ) -> Option<sui::MulticallOperations> {
        let app_instance = crate::state::normalize_app_instance_id(app_instance);
        let mut requests_lock = self.state.get_multicall_requests_mut().await;

        if let Some(requests) = requests_lock.get_mut(&app_instance) {
            let mut operations_count = 0;
            let mut taken_operations = sui::MulticallOperations::new(app_instance.clone(), 0);

            // Take operations one by one until we hit the limit

            // Start jobs - but sort them first: settlement jobs first, then merge jobs by sequence, then other jobs by sequence
            if operations_count < max_operations && !requests.start_jobs.is_empty() {
                // Sort start jobs: settlement first, then merge by sequence, then others by sequence
                requests.start_jobs.sort_by(|a, b| {
                    // We need to determine job types by looking up the job sequences
                    // For now, let's sort by job sequence and handle settlement validation below
                    a.job_sequence.cmp(&b.job_sequence)
                });

                // Take start jobs with validation
                let mut validated_start_jobs = Vec::new();
                let mut remaining_start_jobs = Vec::new();

                for start_job in requests.start_jobs.drain(..) {
                    if operations_count >= max_operations {
                        remaining_start_jobs.push(start_job);
                        continue;
                    }

                    // Validate settlement jobs by checking chain
                    let is_valid = true;
                    if let Ok(Some(_chain)) =
                        sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                            &app_instance,
                            start_job.job_sequence,
                        )
                        .await
                    {
                        info!(
                            "✅ Settlement start_job {} for app_instance {} - validated",
                            start_job.job_sequence, app_instance
                        );
                    } else if let Err(e) =
                        sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                            &app_instance,
                            start_job.job_sequence,
                        )
                        .await
                    {
                        warn!(
                            "Failed to lookup chain for start_job {} in app_instance {}: {} - including anyway",
                            start_job.job_sequence, app_instance, e
                        );
                    }

                    if is_valid {
                        taken_operations
                            .start_job_sequences
                            .push(start_job.job_sequence);
                        taken_operations
                            .start_job_memory_requirements
                            .push(start_job.memory_requirement);
                        operations_count += 1;
                        validated_start_jobs.push(start_job);
                    } else {
                        remaining_start_jobs.push(start_job);
                    }
                }

                // Put remaining jobs back
                requests.start_jobs = remaining_start_jobs;
            }

            // Complete jobs
            while operations_count < max_operations && !requests.complete_jobs.is_empty() {
                let complete_job = requests.complete_jobs.remove(0);
                taken_operations
                    .complete_job_sequences
                    .push(complete_job.job_sequence);
                operations_count += 1;
            }

            // Fail jobs
            while operations_count < max_operations && !requests.fail_jobs.is_empty() {
                let fail_job = requests.fail_jobs.remove(0);
                taken_operations
                    .fail_job_sequences
                    .push(fail_job.job_sequence);
                taken_operations.fail_errors.push(fail_job.error);
                operations_count += 1;
            }

            // Terminate jobs
            while operations_count < max_operations && !requests.terminate_jobs.is_empty() {
                let terminate_job = requests.terminate_jobs.remove(0);
                taken_operations
                    .terminate_job_sequences
                    .push(terminate_job.job_sequence);
                operations_count += 1;
            }

            // Update state operations
            while operations_count < max_operations
                && !requests.update_state_for_sequences.is_empty()
            {
                let update_req = requests.update_state_for_sequences.remove(0);
                taken_operations.update_state_for_sequences.push((
                    update_req.sequence,
                    update_req.new_state_data,
                    update_req.new_data_availability_hash,
                ));
                operations_count += 1;
            }

            // Submit proofs
            while operations_count < max_operations && !requests.submit_proofs.is_empty() {
                let proof_req = requests.submit_proofs.remove(0);
                taken_operations.submit_proofs.push((
                    proof_req.block_number,
                    proof_req.sequences,
                    proof_req.merged_sequences_1,
                    proof_req.merged_sequences_2,
                    proof_req.job_id,
                    proof_req.da_hash,
                    proof_req.cpu_cores,
                    proof_req.prover_architecture,
                    proof_req.prover_memory,
                    proof_req.cpu_time,
                ));
                operations_count += 1;
            }

            // Create app jobs - validate settlement jobs
            if operations_count < max_operations && !requests.create_app_jobs.is_empty() {
                let mut validated_create_jobs = Vec::new();
                let mut remaining_create_jobs = Vec::new();

                for create_req in requests.create_app_jobs.drain(..) {
                    if operations_count >= max_operations {
                        remaining_create_jobs.push(create_req);
                        continue;
                    }

                    // Check if this is a settlement job
                    if create_req.method_name == "settle" {
                        if create_req.settlement_chain.is_some() {
                            info!(
                                "✅ Settlement create_job for app_instance {} - validated",
                                app_instance
                            );
                            taken_operations.create_jobs.push((
                                create_req.method_name.clone(),
                                create_req.job_description.clone(),
                                create_req.block_number,
                                create_req.sequences.clone(),
                                create_req.sequences1.clone(),
                                create_req.sequences2.clone(),
                                create_req.data.clone(),
                                create_req.interval_ms,
                                create_req.next_scheduled_at,
                                create_req.settlement_chain.clone(),
                            ));
                            operations_count += 1;
                            validated_create_jobs.push(create_req);
                        } else {
                            error!(
                                "🚨 Settlement create_job for app_instance {} has no settlement_chain - SKIPPING",
                                app_instance
                            );
                            // Don't add this to taken operations or remaining - skip it entirely
                        }
                    } else {
                        // Non-settlement job, include it
                        taken_operations.create_jobs.push((
                            create_req.method_name.clone(),
                            create_req.job_description.clone(),
                            create_req.block_number,
                            create_req.sequences.clone(),
                            create_req.sequences1.clone(),
                            create_req.sequences2.clone(),
                            create_req.data.clone(),
                            create_req.interval_ms,
                            create_req.next_scheduled_at,
                            create_req.settlement_chain.clone(),
                        ));
                        operations_count += 1;
                        validated_create_jobs.push(create_req);
                    }
                }

                // Put remaining jobs back
                requests.create_app_jobs = remaining_create_jobs;
            }

            // Create merge jobs
            while operations_count < max_operations && !requests.create_merge_jobs.is_empty() {
                let merge_req = requests.create_merge_jobs.remove(0);
                taken_operations.create_merge_jobs.push((
                    merge_req.block_number,
                    merge_req.sequences,
                    merge_req.sequences1,
                    merge_req.sequences2,
                    merge_req.job_description,
                ));
                operations_count += 1;
            }

            // Get available memory (consistent with previous logic)
            let raw_available_memory_gb = get_available_memory_gb();
            let available_memory_with_coefficient_gb =
                raw_available_memory_gb as f64 * JOB_BUFFER_MEMORY_COEFFICIENT;
            let available_memory_bytes =
                (available_memory_with_coefficient_gb * 1024.0 * 1024.0 * 1024.0) as u64;
            taken_operations.available_memory = available_memory_bytes;

            // Remove app instance from map if no operations remain
            if requests.create_jobs.is_empty()
                && requests.start_jobs.is_empty()
                && requests.complete_jobs.is_empty()
                && requests.fail_jobs.is_empty()
                && requests.terminate_jobs.is_empty()
                && requests.update_state_for_sequences.is_empty()
                && requests.submit_proofs.is_empty()
                && requests.create_app_jobs.is_empty()
                && requests.create_merge_jobs.is_empty()
            {
                requests_lock.remove(&app_instance);
            }

            if operations_count > 0 {
                Some(taken_operations)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get the current container ID and job info for shutdown logging
    #[allow(dead_code)]
    pub async fn get_current_container_info(&self) -> Option<(String, Job)> {
        self.current_job_info.read().await.clone()
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
    if state
        .has_pending_job_request(&job.app_instance, job.job_sequence)
        .await
    {
        return; // Skip blockchain check if already handled
    }

    // Wait for multicall requests to be processed before checking blockchain
    sleep(Duration::from_secs(JOB_STATUS_CHECK_DELAY_SECS)).await;

    // Check again if a request was added while we were waiting
    if state
        .has_pending_job_request(&job.app_instance, job.job_sequence)
        .await
    {
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
                    info!(
                        "Job {} found in Running state after container terminated ({}), added fail request to multicall batch",
                        job.job_sequence, reason
                    );
                }
                sui::fetch::JobStatus::Pending => {
                    // For periodic jobs, this is expected - they go back to pending after completion
                    // For failed jobs with retries, they also go back to pending
                    if current_job.interval_ms.is_some() {
                        info!(
                            "Periodic job {} returned to Pending state after container terminated ({})",
                            job.job_sequence, reason
                        );
                    } else if current_job.attempts < 3 {
                        info!(
                            "Job {} returned to Pending state for retry (attempt {}) after container terminated ({})",
                            job.job_sequence, current_job.attempts, reason
                        );
                    } else {
                        warn!(
                            "Job {} unexpectedly in Pending state after container terminated ({})",
                            job.job_sequence, reason
                        );
                    }
                }
                sui::fetch::JobStatus::Failed(_) => {
                    info!(
                        "Job {} already in Failed state on blockchain after container terminated ({})",
                        job.job_sequence, reason
                    );
                }
            }
        }
        Ok(None) => {
            info!(
                "Job {} no longer exists on blockchain after container terminated ({})",
                job.job_sequence, reason
            );
        }
        Err(e) => {
            error!(
                "Failed to fetch job {} status after container terminated ({}): {}",
                job.job_sequence, reason, e
            );
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
