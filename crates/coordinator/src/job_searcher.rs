use crate::agent::AgentJob;
use crate::constants::{JOB_POOL_SIZE, MAX_CONCURRENT_AGENTS, MAX_JOB_START_DELAY_MS, MIN_SYSTEM_MEMORY_GB};
use crate::error::{CoordinatorError, Result};
use crate::hardware::{get_available_memory_gb, get_hardware_info, get_total_memory_gb};
use crate::job_lock::{JobLockGuard, get_job_lock_manager};
use crate::jobs_cache::JobsCache;
use crate::metrics::CoordinatorMetrics;
use crate::session_id::generate_docker_session;
use crate::settlement::{can_remove_app_instance, fetch_all_pending_jobs};
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
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
    if current_agents >= MAX_CONCURRENT_AGENTS {
        debug!(
            "Maximum concurrent agents ({}) reached",
            MAX_CONCURRENT_AGENTS
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
    if total_memory_needed_gb > total_memory_gb.saturating_sub(MIN_SYSTEM_MEMORY_GB) {
        debug!(
            "Insufficient total memory: need {} GB (including {} GB for new agent), have {} GB (minus {} GB reserved)",
            total_memory_needed_gb,
            agent_method.min_memory_gb,
            total_memory_gb,
            MIN_SYSTEM_MEMORY_GB
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
                // Check for shutdown
                if monitor_state.is_shutting_down() {
                    break;
                }

                sleep(Duration::from_millis(1000)).await;
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
                            .stop_container_with_timeout(&container_id, 5)
                            .await
                        {
                            error!("Failed to stop container {}: {}", container_id, e);
                        }
                    } else {
                        warn!(
                            "Waiting for Docker container {} (job {}) to complete before shutdown...",
                            container_id, job.job_sequence
                        );
                        warn!("Press Ctrl-C again to force terminate the container");

                        // Wait for container to finish or force shutdown
                        let mut check_interval = tokio::time::interval(Duration::from_secs(1));
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
            let current_state = self.searcher_state.read().await.clone();

            match current_state {
                SearcherState::Searching => {
                    // Check for shutdown during searching phase
                    if self.state.is_shutting_down() {
                        debug!("Shutdown requested during searching phase");
                        return Ok(());
                    }

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
                    self.jobs_cache.cleanup_expired().await;

                    // Check for pending jobs and clean up app_instances without jobs
                    // This already filters out jobs in the failed cache
                    match self.check_and_clean_pending_jobs().await? {
                        Some(job) => {
                            debug!(
                                "Found pending job: {} in app_instance {}",
                                job.job_sequence, job.app_instance
                            );

                            // Note: We allow multiple instances of the same agent to run in parallel
                            // up to MAX_CONCURRENT_AGENTS limit

                            // Fetch agent method configuration first to check resource requirements
                            let agent_method = match fetch_agent_method(
                                &job.developer,
                                &job.agent,
                                &job.agent_method,
                            )
                            .await
                            {
                                Ok(method) => {
                                    debug!(
                                        "Agent method for {}/{}/{}: memory={}GB, cpu={}, tee={}",
                                        job.developer,
                                        job.agent,
                                        job.agent_method,
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
                                    // Add to failed cache to avoid retrying immediately
                                    self.jobs_cache
                                        .add_failed_job(job.app_instance.clone(), job.job_sequence)
                                        .await;
                                    continue;
                                }
                            };

                            // Check if we have sufficient resources to run this agent
                            match can_run_agent(&self.state, &agent_method).await {
                                Ok(true) => {
                                    // Resources available, continue
                                }
                                Ok(false) => {
                                    debug!(
                                        "Insufficient resources to run agent {}/{}/{}",
                                        job.developer, job.agent, job.agent_method
                                    );
                                    // Wait a bit before checking again
                                    sleep(Duration::from_secs(2)).await;
                                    continue;
                                }
                                Err(e) => {
                                    error!("Error checking resources for agent: {}", e);
                                    sleep(Duration::from_secs(1)).await;
                                    continue;
                                }
                            }

                            // Log how many containers are already loading/running
                            let (loading_count, running_count) =
                                self.docker_manager.get_container_counts().await;

                            // Update metrics
                            if let Some(ref metrics) = self.metrics {
                                metrics.set_docker_containers(loading_count, running_count);
                            }

                            debug!(
                                "📋 Picked job for Docker (currently {} loading, {} running): dev={}, agent={}/{}, job_seq={}, app={}",
                                loading_count,
                                running_count,
                                job.developer,
                                job.agent,
                                job.agent_method,
                                job.job_sequence,
                                job.app_instance
                            );

                            // Try to acquire a lock for this job
                            let lock_manager = get_job_lock_manager();
                            let job_lock = match lock_manager
                                .try_lock_job(&job.app_instance, job.job_sequence)
                            {
                                Some(lock) => lock,
                                None => {
                                    debug!(
                                        "Failed to acquire lock for job {} from app_instance {} - already locked",
                                        job.job_sequence, job.app_instance
                                    );
                                    // Another thread got the lock first, skip this job
                                    continue;
                                }
                            };

                            // Clone necessary data for the spawned task
                            let state_clone = self.state.clone();
                            let docker_manager_clone = self.docker_manager.clone();
                            let jobs_cache_clone = self.jobs_cache.clone();
                            let container_timeout_secs = self.container_timeout_secs;
                            let secrets_client_clone = self.secrets_client.clone();

                            // Spawn a task to run the Docker container in parallel
                            // The job_lock will be held until the task completes or the lock is dropped
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

                            // Continue immediately to check for more jobs
                            // This allows multiple containers to run in parallel
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
            }
        }
    }

    /// Check for pending jobs and clean up app_instances without jobs
    /// This combines job searching with cleanup that reconciliation would do
    /// Returns a randomly selected job from a pool of viable jobs
    async fn check_and_clean_pending_jobs(&self) -> Result<Option<Job>> {
        let app_instances = self.state.get_app_instances().await;

        if app_instances.is_empty() {
            return Ok(None);
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
                    warn!(
                        "No viable jobs after filtering (total={}, locked={}, failed_cached={})",
                        total_jobs, locked_count, failed_cached_count
                    );
                    return Ok(None);
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
                        let job = settlement_jobs.into_iter().next().unwrap();
                        debug!(
                            "Selected settlement job {} from app_instance {} (settle_only mode)",
                            job.job_sequence, job.app_instance
                        );
                        return Ok(Some(job));
                    } else {
                        debug!("No settlement jobs available (settle_only mode)");
                        return Ok(None);
                    }
                }

                // Normal mode: Build job pool based on priority
                // 1. Settlement jobs (highest priority - always picked if available)
                // 2. Merge jobs (collect all, up to pool size)
                // 3. Other jobs (fill remaining slots)

                // Check for settlement jobs first
                let settlement_jobs: Vec<Job> = viable_jobs
                    .iter()
                    .filter(|job| job.app_instance_method == "settle")
                    .cloned()
                    .collect();

                if !settlement_jobs.is_empty() {
                    // Always pick settlement job immediately (highest priority)
                    let job = settlement_jobs.into_iter().next().unwrap();
                    debug!(
                        "Selected settlement job {} from app_instance {}",
                        job.job_sequence, job.app_instance
                    );
                    return Ok(Some(job));
                }

                // Collect merge jobs
                let mut job_pool: Vec<Job> = viable_jobs
                    .iter()
                    .filter(|job| job.app_instance_method == "merge")
                    .cloned()
                    .collect();

                let merge_count = job_pool.len();
                debug!("Found {} merge jobs", merge_count);

                // If we have less than pool size, add other jobs
                if job_pool.len() < JOB_POOL_SIZE {
                    let mut other_jobs: Vec<Job> = viable_jobs
                        .iter()
                        .filter(|job| {
                            job.app_instance_method != "merge"
                                && job.app_instance_method != "settle"
                        })
                        .cloned()
                        .collect();

                    // Sort other jobs by sequence to get lowest ones
                    other_jobs.sort_by_key(|job| job.job_sequence);

                    // Take enough to fill the pool
                    let slots_remaining = JOB_POOL_SIZE - job_pool.len();
                    job_pool.extend(other_jobs.into_iter().take(slots_remaining));

                    debug!(
                        "Added {} other jobs to pool (total pool size: {})",
                        job_pool.len() - merge_count,
                        job_pool.len()
                    );
                }

                // Select randomly from the pool
                use rand::Rng;
                let pool_size = job_pool.len();
                let selected_index = rand::thread_rng().gen_range(0..pool_size);
                let selected_job = job_pool[selected_index].clone();

                info!(
                    "Randomly selected job {} from app_instance {} (index {}/{}, pool had {} merge jobs)",
                    selected_job.job_sequence,
                    selected_job.app_instance,
                    selected_index,
                    pool_size,
                    merge_count
                );

                Ok(Some(selected_job))
            }
            Err(e) => {
                error!("Failed to fetch pending jobs: {}", e);
                Err(CoordinatorError::Other(e))
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

/// Standalone function to run a Docker container for a job (for parallel execution)
/// The job_lock is held for the duration of this task to prevent duplicate processing
async fn run_docker_container_task(
    job: Job,
    agent_method: AgentMethod,
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    jobs_cache: JobsCache,
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
        "Job {} not tracked locally, adding random delay before start",
        job.job_sequence
    );

    // Add random delay to avoid race conditions with other coordinators
    use rand::Rng;
    let delay_ms = rand::thread_rng().gen_range(0..MAX_JOB_START_DELAY_MS);
    debug!(
        "Adding random delay of {}ms before starting job {} to avoid race conditions",
        delay_ms, job.job_sequence
    );
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;

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

    // Check if job is still in pending_jobs list using fresh data
    let job_still_pending = if let Some(jobs) = &fresh_app_instance.jobs {
        jobs.pending_jobs.contains(&job.job_sequence)
    } else {
        false
    };

    if !job_still_pending {
        debug!(
            "Job {} is no longer in pending_jobs list (fresh check) - already started by another coordinator",
            job.job_sequence
        );
        // Don't add to failed cache - the job was successfully started by another coordinator
        state.clear_current_agent(&docker_session.session_id).await;
        docker_manager
            .remove_container_tracking(&docker_session.session_id)
            .await;
        return;
    }

    debug!(
        "Job {} confirmed still pending with fresh data, attempting to start",
        job.job_sequence
    );

    // Try to start the job on blockchain - NO RETRIES since other coordinators might be handling it
    let start_time = Instant::now();

    debug!(
        "Attempting to start job {} (single attempt, no retries)",
        job.job_sequence
    );

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
            warn!(
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

    let start_elapsed = start_time.elapsed();

    // Add job to agent database as a session-specific job for Docker retrieval
    let mut agent_job = AgentJob::new(job.clone(), &state);
    agent_job.start_tx_hash = Some(tx_digest.clone());
    agent_job.start_tx_sent = true;
    let job_id = agent_job.job_id.clone();
    // Use add_session_job to link this job to the Docker session
    state.get_agent_job_db().add_session_job(docker_session.session_id.clone(), agent_job).await;

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
