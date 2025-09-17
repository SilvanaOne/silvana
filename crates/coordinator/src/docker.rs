use crate::agent::AgentJob;
use crate::constants::{JOB_ACQUISITION_DELAY_PER_CONTAINER_MS, JOB_ACQUISITION_MAX_DELAY_MS};
use crate::error::Result;
use crate::job_lock::JobLockGuard;
use crate::metrics::CoordinatorMetrics;
use crate::session::{calculate_cost, generate_docker_session};
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use proto;
use secrets_client::SecretsClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sui::fetch::{AgentMethod, Job};
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

/// Guard that ensures AgentSessionFinishedEvent is sent when dropped
struct SessionFinishedGuard {
    session_id: String,
    coordinator_id: String,
    session_start_time: std::time::SystemTime,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    state: SharedState,
    logs: Arc<Mutex<String>>,
    sent: Arc<Mutex<bool>>,
}

impl SessionFinishedGuard {
    fn new(
        session_id: String,
        coordinator_id: String,
        session_start_time: std::time::SystemTime,
        agent_method: &AgentMethod,
        state: SharedState,
    ) -> Self {
        Self {
            session_id,
            coordinator_id,
            session_start_time,
            min_memory_gb: agent_method.min_memory_gb,
            min_cpu_cores: agent_method.min_cpu_cores,
            requires_tee: agent_method.requires_tee,
            state,
            logs: Arc::new(Mutex::new(String::new())),
            sent: Arc::new(Mutex::new(false)),
        }
    }

    fn set_logs(&self, logs: String) {
        if let Ok(mut guard) = self.logs.lock() {
            *guard = logs;
        }
    }

    async fn send_finished_event(&self) {
        // Check if already sent
        {
            let mut sent_guard = self.sent.lock().unwrap();
            if *sent_guard {
                return;
            }
            *sent_guard = true;
        }

        // Calculate session duration
        let duration_ms = self.session_start_time
            .elapsed()
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_millis() as u64;

        // Calculate cost using the session cost function
        let cost = calculate_cost(
            duration_ms,
            self.min_memory_gb,
            self.min_cpu_cores,
            self.requires_tee,
        );

        // Get logs
        let container_logs = self.logs.lock().unwrap().clone();

        // Truncate logs if too long (max 10KB for session_log field)
        let session_log = if container_logs.is_empty() {
            "No logs available".to_string()
        } else if container_logs.len() > 10240 {
            // Find the last valid character boundary at or before 10240
            let mut truncate_at = 10240;
            while truncate_at > 0 && !container_logs.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }
            format!("{}... (truncated)", &container_logs[..truncate_at])
        } else {
            container_logs
        };

        // Send AgentSessionFinishedEvent to RPC service
        let rpc_client = self.state.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::AgentSessionFinished(
                    proto::AgentSessionFinishedEvent {
                        coordinator_id: self.coordinator_id.clone(),
                        session_id: self.session_id.clone(),
                        session_log,
                        duration: duration_ms,
                        cost,
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
                        info!(
                            "Sent AgentSessionFinishedEvent for session {} (duration: {}ms, cost: {})",
                            self.session_id, duration_ms, cost
                        );
                    } else {
                        error!(
                            "Failed to send AgentSessionFinishedEvent for session {}: {}",
                            self.session_id, resp.message
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to send AgentSessionFinishedEvent for session {}: {}",
                        self.session_id, e
                    );
                }
            }
        } else {
            warn!(
                "No RPC client available to send AgentSessionFinishedEvent for session {}",
                self.session_id
            );
        }
    }
}

impl Drop for SessionFinishedGuard {
    fn drop(&mut self) {
        // Clone what we need for the async operation
        let session_id = self.session_id.clone();
        let coordinator_id = self.coordinator_id.clone();
        let session_start_time = self.session_start_time;
        let min_memory_gb = self.min_memory_gb;
        let min_cpu_cores = self.min_cpu_cores;
        let requires_tee = self.requires_tee;
        let state = self.state.clone();
        let logs = self.logs.clone();
        let sent = self.sent.clone();

        // Spawn a task to send the event since Drop can't be async
        tokio::spawn(async move {
            let guard = SessionFinishedGuard {
                session_id,
                coordinator_id,
                session_start_time,
                min_memory_gb,
                min_cpu_cores,
                requires_tee,
                state,
                logs,
                sent,
            };
            guard.send_finished_event().await;
        });
    }
}

/// Guard to ensure agent state is cleared from coordinator when function exits
struct AgentStateGuard {
    session_id: String,
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    cleared: Arc<Mutex<bool>>,
}

impl AgentStateGuard {
    fn new(session_id: String, state: SharedState, docker_manager: Arc<DockerManager>) -> Self {
        Self {
            session_id,
            state,
            docker_manager,
            cleared: Arc::new(Mutex::new(false)),
        }
    }

    async fn clear(&self) {
        // Check if already cleared without holding the lock across await
        {
            let cleared = self.cleared.lock().unwrap();
            if *cleared {
                return;
            }
        }

        // Do the actual cleanup
        self.state.clear_current_agent(&self.session_id).await;
        self.docker_manager.remove_container_tracking(&self.session_id).await;

        // Mark as cleared
        {
            let mut cleared = self.cleared.lock().unwrap();
            *cleared = true;
        }

        debug!("AgentStateGuard: Cleared agent state for session {}", self.session_id);
    }
}

impl Drop for AgentStateGuard {
    fn drop(&mut self) {
        // Check if already cleared
        if *self.cleared.lock().unwrap() {
            return;
        }

        // Clone for async operation
        let session_id = self.session_id.clone();
        let state = self.state.clone();
        let docker_manager = self.docker_manager.clone();

        // Spawn cleanup task
        tokio::spawn(async move {
            state.clear_current_agent(&session_id).await;
            docker_manager.remove_container_tracking(&session_id).await;
            debug!("AgentStateGuard: Cleared agent state for session {} in Drop", session_id);
        });
    }
}

/// Standalone function to run a Docker container for a job (for parallel execution)
/// The job_lock is held for the duration of this task to prevent duplicate processing
pub async fn run_docker_container_task(
    job: Job,
    agent_method: AgentMethod,
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    container_timeout_secs: u64,
    mut secrets_client: Option<SecretsClient>,
    _job_lock: JobLockGuard, // Hold the lock for the duration of the task
    metrics: Option<Arc<CoordinatorMetrics>>,
) {
    let job_start = Instant::now();
    let session_start_time = std::time::SystemTime::now();

    // Early check: coordinator_id must be available for running jobs
    let coordinator_id = match state.get_coordinator_id() {
        Some(id) => id,
        None => {
            error!(
                "Cannot run Docker container for job {}: coordinator_id not available. \
                 The coordinator must be properly initialized with SUI_ADDRESS.",
                job.job_sequence
            );
            return;
        }
    };

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
            // Can't send finished event without session ID
            return;
        }
    };

    // Create session finished guard to ensure event is always sent
    let session_guard = SessionFinishedGuard::new(
        docker_session.session_id.clone(),
        coordinator_id.clone(),
        session_start_time,
        &agent_method,
        state.clone(),
    );

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

    // Create guard to ensure agent state is cleaned up on exit
    let agent_state_guard = AgentStateGuard::new(
        docker_session.session_id.clone(),
        state.clone(),
        docker_manager.clone(),
    );

    // Send AgentSessionStartedEvent to RPC service
    {
        let rpc_client = state.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::AgentSessionStarted(
                    proto::AgentSessionStartedEvent {
                        coordinator_id: coordinator_id.clone(),
                        developer: job.developer.clone(),
                        agent: job.agent.clone(),
                        agent_method: job.agent_method.clone(),
                        session_id: docker_session.session_id.clone(),
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
                        debug!("Sent AgentSessionStartedEvent for session {}", docker_session.session_id);
                    } else {
                        warn!("Failed to send AgentSessionStartedEvent: {}", resp.message);
                    }
                }
                Err(e) => {
                    warn!("Failed to send AgentSessionStartedEvent: {}", e);
                }
            }
        }
    }

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
            ensure_job_failed_if_not_completed(
                &docker_session.session_id,
                "Failed to fetch fresh AppInstance",
                &state,
            )
            .await;

            // Set error log in guard before returning
            session_guard.set_logs(format!("Error: Failed to fetch AppInstance: {}", e));

            // Agent state cleanup handled by AgentStateGuard Drop
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
    let agent_job = match AgentJob::new(
        job.clone(),
        docker_session.session_id.clone(),
        &state,
        agent_method.min_memory_gb,
        agent_method.min_cpu_cores,
        agent_method.requires_tee,
    ) {
        Ok(job) => job,
        Err(e) => {
            error!(
                "Failed to create agent job for job {}: {}",
                job.job_sequence, e
            );
            // Set error log in guard before returning
            session_guard.set_logs(format!("Error: Failed to create agent job: {}", e));
            return;
        }
    };
    let job_id = agent_job.job_id.clone();
    // Use add_session_job to link this job to the Docker session
    state.get_agent_job_db().add_session_job(agent_job).await;

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
                    // Log container count after clearing
                    let (loading_count, running_count) =
                        docker_manager.get_container_counts().await;
                    info!(
                        "🏁 Docker container aborted (SHA mismatch): {} loading, {} running",
                        loading_count, running_count
                    );
                    // Set error log in guard before returning
                    session_guard.set_logs(format!("Error: Docker image SHA256 mismatch. Expected: {}, Got: {}", expected_sha, digest));
                    return;
                }
            }
            digest
        }
        Err(e) => {
            error!("Failed to pull image for job {}: {}", job.job_sequence, e);
            // Agent state cleanup handled by AgentStateGuard Drop

            ensure_job_failed_if_not_completed(
                &docker_session.session_id,
                "Failed to pull Docker image",
                &state,
            )
            .await;

            // Log container count
            let (loading_count, running_count) = docker_manager.get_container_counts().await;
            info!(
                "🏁 Docker container aborted (image pull failed): {} loading, {} running",
                loading_count, running_count
            );
            // Agent state cleanup handled by AgentStateGuard Drop
            return;
        }
    };

    // Configure the Docker container with memory limits
    let mut env_vars = HashMap::new();
    env_vars.insert("CHAIN".to_string(), state.get_chain().clone());
    // We already checked coordinator_id exists at the beginning of the function
    env_vars.insert(
        "COORDINATOR_ID".to_string(),
        state.get_coordinator_id().unwrap_or_else(|| {
            error!("Unexpected: coordinator_id not available after check");
            "error".to_string()
        }),
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

    // Collect container logs and exit result
    match docker_manager.run_container(&config).await {
        Ok(result) => {
            let docker_elapsed = docker_start.elapsed();
            let total_elapsed = job_start.elapsed();

            // Set logs in the guard
            session_guard.set_logs(result.logs.clone());

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
                // Increment container failed metric
                if let Some(ref metrics) = metrics {
                    metrics.increment_docker_containers_failed();
                }
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
            let error_msg = format!("Failed to run Docker container for job {}: {} (docker: {:?}, total: {:?})",
                job.job_sequence, e, docker_elapsed, total_elapsed);
            error!("{}", error_msg);

            // Set error message as logs in the guard
            session_guard.set_logs(format!("Error: {}", e));

            // Increment container failed metric
            if let Some(ref metrics) = metrics {
                metrics.increment_docker_containers_failed();
            }
        }
    };

    // Ensure the job is failed on blockchain if it wasn't completed
    // This handles the case where the container exits without the agent calling complete/fail
    ensure_job_failed_if_not_completed(&docker_session.session_id, "container terminated", &state)
        .await;

    // Send the finished event explicitly (also sent automatically on drop if not sent)
    session_guard.send_finished_event().await;

    // Clear the agent state after Docker completes
    // This ensures cleanup happens before function returns normally
    agent_state_guard.clear().await;

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

/// Ensure a job is failed on blockchain if it wasn't completed
/// This is called after container termination (normal, timeout, or shutdown)
pub async fn ensure_job_failed_if_not_completed(
    session_id: &str,
    reason: &str,
    state: &SharedState,
) {
    // Fail all remaining jobs for this session
    let failed_jobs = state
        .get_agent_job_db()
        .fail_all_remaining_session_jobs(&session_id)
        .await;

    // Add fail requests to multicall for any jobs that were in the database
    for failed_job in failed_jobs {
        // Send JobFinishedEvent for the failed job
        send_job_finished_event_for_failed(
            state,
            &failed_job,
            reason.to_string(),  // Not used in current proto, but kept for potential future use
        );

        state
            .add_fail_job_request(
                failed_job.app_instance.clone(),
                failed_job.job_sequence,
                format!("{}", reason),
            )
            .await;
    }
}

/// Helper function to send JobFinishedEvent for failed jobs (fire and forget)
fn send_job_finished_event_for_failed(
    state: &SharedState,
    agent_job: &crate::agent::AgentJob,
    _error_message: String,  // Keep for API compatibility but not used in proto
) {
    // Get coordinator ID
    let coordinator_id = match state.get_coordinator_id() {
        Some(id) => id,
        None => {
            debug!("Cannot send JobFinishedEvent: coordinator_id not available");
            return;
        }
    };

    // Calculate job duration and cost
    let job_end_time = std::time::SystemTime::now();
    let duration_ms = job_end_time
        .duration_since(agent_job.job_start_time)
        .unwrap_or_default()
        .as_millis() as u64;

    // Calculate cost using stored agent method info
    let cost = crate::session::calculate_cost(
        duration_ms,
        agent_job.min_memory_gb,
        agent_job.min_cpu_cores,
        agent_job.requires_tee,
    );

    // Clone what we need for the async task
    let state_clone = state.clone();
    let job_id = agent_job.job_id.clone();
    let job_end_timestamp = job_end_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Spawn async task to send the event
    tokio::spawn(async move {
        let rpc_client = state_clone.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::JobFinished(
                    proto::JobFinishedEvent {
                        coordinator_id,
                        job_id: job_id.clone(),
                        duration: duration_ms,
                        cost,
                        event_timestamp: job_end_timestamp,
                        result: proto::JobResult::Failed as i32,
                    },
                )),
            };

            let request = proto::SubmitEventRequest { event: Some(event) };

            match client.submit_event(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    if resp.success {
                        debug!("Sent JobFinishedEvent for failed job_id {}", job_id);
                    } else {
                        warn!("Failed to send JobFinishedEvent: {}", resp.message);
                    }
                }
                Err(e) => {
                    warn!("Failed to send JobFinishedEvent: {}", e);
                }
            }
        } else {
            debug!("No RPC client available to send JobFinishedEvent");
        }
    });
}

// Standalone function to run a Docker container for a job (for parallel execution)
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

use crate::constants::MAX_CONCURRENT_AGENT_CONTAINERS;
use tokio::time::sleep;

/// Docker buffer processor that monitors started_jobs_buffer and launches containers
pub struct DockerBufferProcessor {
    state: SharedState,
    docker_manager: Arc<DockerManager>,
    container_timeout_secs: u64,
    secrets_client: Option<SecretsClient>,
    metrics: Option<Arc<CoordinatorMetrics>>,
}

impl DockerBufferProcessor {
    pub fn new(state: SharedState, use_tee: bool, container_timeout_secs: u64) -> Result<Self> {
        let docker_manager = DockerManager::new(use_tee)
            .map_err(|e| crate::error::CoordinatorError::DockerError(e))?;

        Ok(Self {
            state,
            docker_manager: Arc::new(docker_manager),
            container_timeout_secs,
            secrets_client: None,
            metrics: None,
        })
    }
    
    /// Set the metrics reporter
    pub fn set_metrics(&mut self, metrics: Arc<CoordinatorMetrics>) {
        self.metrics = Some(metrics);
    }

    /// Main loop for the docker buffer processor
    pub async fn run(&mut self) -> Result<()> {
        info!("🐳 Docker buffer processor started");

        loop {
            // Check for shutdown request
            if self.state.is_shutting_down() {
                // Process all buffered jobs and wait for running containers to complete if not force shutdown
                if !self.state.is_force_shutdown() {
                    // Check if we still have buffered jobs or running containers
                    let (loading_count, running_count) =
                        self.docker_manager.get_container_counts().await;
                    let current_buffer_size = self.state.get_started_jobs_buffer_size().await;
                    
                    if current_buffer_size > 0 || loading_count > 0 || running_count > 0 {
                        if current_buffer_size > 0 {
                            debug!(
                                "Shutdown: {} jobs in buffer, {} loading and {} running containers - continuing to process",
                                current_buffer_size, loading_count, running_count
                            );
                        } else {
                            debug!(
                                "Shutdown: Waiting for {} loading and {} running containers to complete...",
                                loading_count, running_count
                            );
                        }
                        // Continue processing during shutdown - don't exit yet
                        // The loop will continue and process buffered jobs
                    } else {
                        // Everything appears done - but we need to check if multicall has pending operations
                        // that might add jobs to the buffer
                        let pending_ops = self.state.get_total_operations_count().await;
                        if pending_ops > 0 {
                            debug!(
                                "Docker buffer empty but {} multicall operations pending - waiting...",
                                pending_ops
                            );
                            sleep(Duration::from_secs(2)).await;
                            continue; // Go back to check again
                        }

                        // No pending multicall operations - wait 1 second and double-check
                        // in case multicall just picked up something
                        info!("Docker buffer appears empty, waiting 1 second to verify...");
                        sleep(Duration::from_secs(1)).await;

                        // Final check after delay
                        let (final_loading, final_running) = self.docker_manager.get_container_counts().await;
                        let final_buffer_size = self.state.get_started_jobs_buffer_size().await;
                        let final_pending_ops = self.state.get_total_operations_count().await;

                        if final_buffer_size > 0 || final_loading > 0 || final_running > 0 || final_pending_ops > 0 {
                            // Race condition detected - new work appeared
                            debug!(
                                "Race condition detected: {} new buffered jobs, {} loading, {} running, {} pending ops - continuing",
                                final_buffer_size, final_loading, final_running, final_pending_ops
                            );
                            continue; // Go back to processing
                        }

                        // Really done now
                        info!("🛑 Docker buffer processor received shutdown signal");
                        info!("✅ All buffered jobs processed and containers completed");
                        return Ok(());
                    }
                } else {
                    // Force shutdown - exit immediately
                    info!("🛑 Docker buffer processor received force shutdown signal");
                    return Ok(());
                }
            }

            // Report buffer size metric
            let buffer_size = self.state.get_started_jobs_buffer_size().await;
            if let Some(ref metrics) = self.metrics {
                metrics.set_docker_buffer_size(buffer_size);
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
                sleep(Duration::from_secs(2)).await;
                continue;
            }

            // Get next job from buffer using prioritized selection
            let started_job = self.state.get_next_prioritized_started_job().await;
            if started_job.is_none() {
                // No jobs in buffer, sleep briefly
                sleep(Duration::from_secs(1)).await;
                continue;
            }

            let started_job = started_job.unwrap();
            info!(
                "Processing buffered job: app_instance={}, sequence={}, memory={:.2} GB",
                started_job.app_instance,
                started_job.job_sequence,
                started_job.memory_requirement as f64 / (1024.0 * 1024.0 * 1024.0)
            );
            
            // Increment jobs processed metric
            if let Some(ref metrics) = self.metrics {
                metrics.increment_docker_jobs_processed();
            }

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
                                    // During shutdown, fail the job after logging the error
                                    if self.state.is_shutting_down() {
                                        warn!(
                                            "Failing job {} during shutdown due to fetch error: {}",
                                            started_job.job_sequence, e
                                        );
                                        self.state.add_fail_job_request(
                                            started_job.app_instance.clone(),
                                            started_job.job_sequence,
                                            format!("Failed to fetch job during shutdown: {}", e),
                                        ).await;
                                    }
                                    Ok(None)
                                }
                            }
                        } else {
                            // No jobs object - job likely doesn't exist
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch app instance: {}", e);
                        // During shutdown, fail the job after logging the error
                        if self.state.is_shutting_down() {
                            warn!(
                                "Failing job {} during shutdown due to app instance fetch error: {}",
                                started_job.job_sequence, e
                            );
                            self.state.add_fail_job_request(
                                started_job.app_instance.clone(),
                                started_job.job_sequence,
                                format!("Failed to fetch app instance during shutdown: {}", e),
                            ).await;
                        }
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
                        // Increment jobs skipped metric
                        if let Some(ref metrics) = self.metrics {
                            metrics.increment_docker_jobs_skipped();
                        }
                        continue;
                    }

                    // Fetch agent method configuration
                    let agent_method = match sui::fetch_agent_method(
                        &job.developer,
                        &job.agent,
                        &job.agent_method,
                    )
                    .await
                    {
                        Ok(method) => method,
                        Err(e) => {
                            error!(
                                "Failed to fetch agent method for buffered job {}: {}",
                                job.job_sequence, e
                            );
                            
                            // Return job to buffer for retry on any error
                            self.state.add_started_jobs(vec![started_job]).await;
                            info!(
                                "Returned job {} to buffer due to agent method fetch error",
                                job.job_sequence
                            );
                            
                            // Increment metrics
                            if let Some(ref metrics) = self.metrics {
                                metrics.increment_docker_agent_method_fetch_failures();
                                metrics.increment_docker_jobs_returned_to_buffer();
                            }
                            
                            // Wait before retrying
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    // Check if we have sufficient resources
                    match self.can_run_agent(&agent_method).await {
                        Ok(true) => {
                            // Resources available, continue
                        }
                        Ok(false) => {
                            // Put job back in buffer (at the front) and wait
                            self.state.add_started_jobs(vec![started_job]).await;
                            info!(
                                "Insufficient resources for job {}, returned to buffer",
                                job.job_sequence
                            );
                            // Increment jobs returned to buffer metric
                            if let Some(ref metrics) = self.metrics {
                                metrics.increment_docker_jobs_returned_to_buffer();
                            }
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                        Err(e) => {
                            error!("Error checking resources: {}", e);
                            // Put job back in buffer
                            self.state.add_started_jobs(vec![started_job]).await;
                            // Increment resource check failures metric
                            if let Some(ref metrics) = self.metrics {
                                metrics.increment_docker_resource_check_failures();
                            }
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }

                    // Try to acquire a lock for this job
                    let lock_manager = crate::job_lock::get_job_lock_manager();
                    let job_lock =
                        match lock_manager.try_lock_job(&job.app_instance, job.job_sequence) {
                            Some(lock) => lock,
                            None => {
                                warn!("Job {} already locked, skipping", job.job_sequence);
                                // Increment job lock conflicts metric
                                if let Some(ref metrics) = self.metrics {
                                    metrics.increment_docker_job_lock_conflicts();
                                }
                                continue;
                            }
                        };

                    info!(
                        "🐳 Starting Docker container for buffered job {}: {}/{}/{}",
                        job.job_sequence, job.developer, job.agent, job.agent_method
                    );
                    
                    // Increment containers started metric
                    if let Some(ref metrics) = self.metrics {
                        metrics.increment_docker_containers_started();
                    }

                    // Clone necessary data for the spawned task
                    let state_clone = self.state.clone();
                    let docker_manager_clone = self.docker_manager.clone();
                    let container_timeout_secs = self.container_timeout_secs;
                    let secrets_client_clone = self.secrets_client.clone();
                    let metrics_clone = self.metrics.clone();

                    // Spawn task to run Docker container
                    tokio::spawn(async move {
                        run_docker_container_task(
                            job,
                            agent_method,
                            state_clone,
                            docker_manager_clone,
                            container_timeout_secs,
                            secrets_client_clone,
                            job_lock,
                            metrics_clone,
                        )
                        .await;
                    });
                }
                Ok(None) => {
                    warn!(
                        "Buffered job {} from app_instance {} not found in blockchain (may have been deleted or completed)",
                        started_job.job_sequence, started_job.app_instance
                    );
                    
                    // During shutdown, fail the job to ensure clean shutdown
                    if self.state.is_shutting_down() {
                        warn!(
                            "Failing job {} during shutdown as it cannot be fetched from blockchain",
                            started_job.job_sequence
                        );
                        self.state.add_fail_job_request(
                            started_job.app_instance.clone(),
                            started_job.job_sequence,
                            "Job could not be fetched during shutdown".to_string(),
                        ).await;
                    }
                    
                    // Job doesn't exist or can't be fetched, don't put it back in buffer
                    // Increment jobs skipped metric
                    if let Some(ref metrics) = self.metrics {
                        metrics.increment_docker_jobs_skipped();
                    }
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
    }

    /// Check if system has sufficient resources to run an agent
    async fn can_run_agent(&self, agent_method: &sui::fetch::AgentMethod) -> Result<bool> {
        use crate::constants::AGENT_MIN_MEMORY_REQUIREMENT_GB;
        use crate::hardware::{get_available_memory_gb, get_hardware_info, get_total_memory_gb};

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
        let current_agents = self.state.get_current_agent_count().await;
        if current_agents >= MAX_CONCURRENT_AGENT_CONTAINERS {
            debug!(
                "Maximum concurrent agents ({}) reached",
                MAX_CONCURRENT_AGENT_CONTAINERS
            );
            return Ok(false);
        }

        // Calculate total memory required by currently running agents
        let running_agents = self.state.get_all_current_agents().await;
        let mut total_memory_used_gb = 0u64;

        for (_session_id, agent_info) in running_agents {
            // Fetch agent method to get memory requirements
            if let Ok(method) = sui::fetch_agent_method(
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
        if total_memory_needed_gb > total_memory_gb.saturating_sub(AGENT_MIN_MEMORY_REQUIREMENT_GB)
        {
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
}
