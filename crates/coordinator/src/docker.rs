use crate::agent::AgentJob;
use crate::constants::{
    JOB_ACQUISITION_DELAY_PER_CONTAINER_MS, JOB_ACQUISITION_MAX_DELAY_MS,
    JOB_STATUS_CHECK_DELAY_SECS,
};
use crate::error::Result;
use crate::job_lock::JobLockGuard;
use crate::jobs_cache::JobsCache;
use crate::session_id::generate_docker_session;
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use secrets_client::SecretsClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use sui::fetch::{AgentMethod, Job};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

/// Standalone function to run a Docker container for a job (for parallel execution)
/// The job_lock is held for the duration of this task to prevent duplicate processing
pub async fn run_docker_container_task(
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
    let memory_requirement = (agent_method.min_memory_gb as u64) * 1024 * 1024 * 1024;
    let agent_job = AgentJob::new(job.clone(), &state, memory_requirement);
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

/// Ensure a job is failed on blockchain if it wasn't completed
/// This is called after container termination (normal, timeout, or shutdown)
pub async fn ensure_job_failed_if_not_completed(job: &Job, reason: &str, state: &SharedState) {
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
