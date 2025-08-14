use crate::config::Config;
use crate::error::{CoordinatorError, Result};
use crate::events::{parse_coordination_events, parse_jobs_event_with_contents, CoordinationEvent};
use crate::pending::{fetch_all_pending_jobs, fetch_pending_jobs_from_app_instance};
use crate::registry::fetch_agent_method;
use crate::state::SharedState;
use docker::{ContainerConfig, DockerManager};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sui_rpc::proto::sui::rpc::v2beta2::{
    GetTransactionRequest, SubscribeCheckpointsRequest, SubscribeCheckpointsResponse,
};
use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

const RETRY_DELAY_SECS: u64 = 5;
const MAX_RETRIES: usize = 5;
const STREAM_TIMEOUT_SECS: u64 = 30;

pub struct EventProcessor {
    config: Config,
    docker_manager: DockerManager,
    processed_events: HashMap<String, bool>,
    state: SharedState,
}

impl EventProcessor {
    pub async fn new(config: Config, state: SharedState) -> Result<Self> {
        info!("Initializing event processor...");

        let docker_manager =
            DockerManager::new(config.use_tee).map_err(|e| CoordinatorError::DockerError(e))?;

        Ok(Self {
            config,
            docker_manager,
            processed_events: HashMap::new(),
            state,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut retry_count = 0;

        loop {
            match self.stream_checkpoints().await {
                Ok(_) => {
                    retry_count = 0;
                    info!("Checkpoint stream ended normally, restarting...");
                }
                Err(e) => {
                    retry_count += 1;
                    error!(
                        "Stream error (attempt {}/{}): {}",
                        retry_count, MAX_RETRIES, e
                    );

                    if retry_count >= MAX_RETRIES {
                        error!("Max retries reached, exiting");
                        return Err(e);
                    }

                    let delay = Duration::from_secs(RETRY_DELAY_SECS * retry_count as u64);
                    warn!("Retrying in {:?}...", delay);
                    sleep(delay).await;
                }
            }
        }
    }

    async fn stream_checkpoints(&mut self) -> Result<()> {
        info!("Starting checkpoint stream...");

        let mut client = self.state.get_sui_client();
        let mut subscription_client = client.subscription_client();

        let request = SubscribeCheckpointsRequest {
            read_mask: Some(prost_types::FieldMask {
                paths: vec![
                    //"summary.timestamp".to_string(),
                    //"transactions.events.events.package_id".to_string(),
                    //"transactions.events.events.module".to_string(),
                    //"transactions.events.events.sender".to_string(),
                    "transactions.events.events.event_type".to_string(),
                ],
            }),
        };

        let mut stream = subscription_client
            .subscribe_checkpoints(request)
            .await
            .map_err(|e| CoordinatorError::TonicError(e))?
            .into_inner();

        let mut checkpoint_count = 0;

        while let Ok(Some(response)) =
            timeout(Duration::from_secs(STREAM_TIMEOUT_SECS), stream.next()).await
        {
            match response {
                Ok(response) => {
                    checkpoint_count += 1;
                    self.process_checkpoint_response(response, checkpoint_count)
                        .await?;
                }
                Err(e) => {
                    error!("Error receiving checkpoint: {}", e);
                    return Err(CoordinatorError::StreamError(e.to_string()));
                }
            }
        }

        info!("Processed {} checkpoints", checkpoint_count);
        Ok(())
    }

    async fn process_checkpoint_response(
        &mut self,
        response: SubscribeCheckpointsResponse,
        checkpoint_count: u64,
    ) -> Result<()> {
        if let (Some(cursor), Some(checkpoint)) = (response.cursor, response.checkpoint) {
            let mut all_events = Vec::new();
            for module in &self.config.modules {
                let events =
                    parse_coordination_events(&checkpoint, cursor, &self.config.package_id, module);
                all_events.extend(events);
            }

            for event in all_events {
                // If this is a jobs event, fetch contents and display
                if event.event_type.contains("JobCreatedEvent") {
                    let job_start = Instant::now();
                    let fetch_start = Instant::now();
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        let fetch_duration = fetch_start.elapsed();
                        info!("⏱️ Event fetch time: {}ms", fetch_duration.as_millis());

                        // Parse and display the event details
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("Jobs Event Details:\n{}", job_details);

                            // Extract and track app_instance
                            // JobCreatedEvent creates a pending job, so we need to track this app_instance
                            if let Some(app_instance) = extract_field(&job_details, "app_instance: ") {
                                // Extract developer, agent, and agent_method for this event
                                let developer = extract_field(&job_details, "developer: ").unwrap_or_default();
                                let agent = extract_field(&job_details, "agent: ").unwrap_or_default();
                                let agent_method = extract_field(&job_details, "agent_method: ").unwrap_or_default();
                                
                                self.state.add_app_instance(
                                    app_instance.clone(),
                                    developer.clone(),
                                    agent.clone(),
                                    agent_method.clone(),
                                ).await;
                                info!("Tracked app_instance from JobCreatedEvent: {} ({}/{}/{})", 
                                    app_instance, developer, agent, agent_method);
                            }

                            // Extract job details from the event for fetching agent method
                            // Parse the job_details string to extract developer, agent, and method
                            if let Some(developer) = extract_field(&job_details, "developer: ") {
                                if let Some(agent) = extract_field(&job_details, "agent: ") {
                                    if let Some(method) =
                                        extract_field(&job_details, "agent_method: ")
                                    {
                                        if let Some(job_id) =
                                            extract_field(&job_details, "job_id: ")
                                        {
                                            if let Some(data) =
                                                extract_field(&job_details, "data: ")
                                            {
                                                // Fetch the agent method configuration from registry
                                                let agent_fetch_start = Instant::now();
                                                let mut client = self.state.get_sui_client();
                                                match fetch_agent_method(
                                                    &mut client,
                                                    &developer,
                                                    &agent,
                                                    &method,
                                                )
                                                .await
                                                {
                                                    Ok(agent_method) => {
                                                        let agent_fetch_duration =
                                                            agent_fetch_start.elapsed();
                                                        info!(
                                                            "⏱️ Agent method fetch time: {}ms",
                                                            agent_fetch_duration.as_millis()
                                                        );

                                                        info!(
                                                            "Agent Method: image={}, sha256={:?}, memory={}GB, cpu={}, tee={}",
                                                            agent_method.docker_image,
                                                            agent_method.docker_sha256,
                                                            agent_method.min_memory_gb,
                                                            agent_method.min_cpu_cores,
                                                            agent_method.requires_tee
                                                        );

                                                        // TEST: Fetch pending jobs from this app_instance
                                                        if let Some(app_instance) = extract_field(&job_details, "app_instance: ") {
                                                            info!("Testing fetch_pending_jobs_from_app_instance for: {}", app_instance);
                                                            // Test with full fetch (only_check = false)
                                                            let mut client = self.state.get_sui_client();
                                                            match fetch_pending_jobs_from_app_instance(&mut client, &app_instance, &self.state, false, None).await {
                                                                Ok(pending_jobs) => {
                                                                    info!("Successfully fetched {} pending jobs from app_instance {}", 
                                                                        pending_jobs.len(), app_instance);
                                                                    for job in &pending_jobs {
                                                                        info!("  Pending Job: id={}, developer={}, agent={}, method={}, status={:?}, attempts={}", 
                                                                            job.job_id, job.developer, job.agent, job.agent_method, job.status, job.attempts);
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    error!("Failed to fetch pending jobs from app_instance {}: {}", app_instance, e);
                                                                }
                                                            }
                                                            
                                                            // Test check-only mode on all tracked app_instances
                                                            let all_app_instances = self.state.get_app_instances().await;
                                                            if !all_app_instances.is_empty() {
                                                                info!("Testing check-only mode for {} app_instances", all_app_instances.len());
                                                                let app_instance_vec: Vec<String> = all_app_instances.into_iter().collect();
                                                                
                                                                // First do a fast check
                                                                let mut client = self.state.get_sui_client();
                                                                match fetch_all_pending_jobs(&mut client, &app_instance_vec, &self.state, true).await {
                                                                    Ok(_) => {
                                                                        info!("Check-only mode completed, app_instances with no pending jobs were removed");
                                                                    }
                                                                    Err(e) => {
                                                                        error!("Failed to check pending jobs: {}", e);
                                                                    }
                                                                }
                                                                
                                                                // Then fetch details from remaining app_instances
                                                                let remaining = self.state.get_app_instances().await;
                                                                if !remaining.is_empty() {
                                                                    info!("Fetching details from {} remaining app_instances", remaining.len());
                                                                    let remaining_vec: Vec<String> = remaining.into_iter().collect();
                                                                    let mut client = self.state.get_sui_client();
                                                                    match fetch_all_pending_jobs(&mut client, &remaining_vec, &self.state, false).await {
                                                                        Ok(all_pending) => {
                                                                            info!("Total pending jobs across {} app_instances: {}", 
                                                                                remaining_vec.len(), all_pending.len());
                                                                        }
                                                                        Err(e) => {
                                                                            error!("Failed to fetch all pending jobs: {}", e);
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }

                                                        // Run the Docker container immediately
                                                        info!(
                                                            "Starting Docker container for job {}",
                                                            job_id
                                                        );

                                                        // Set current agent before pulling image
                                                        self.state
                                                            .set_current_agent(
                                                                developer.clone(),
                                                                agent.clone(),
                                                                method.clone(),
                                                            )
                                                            .await;

                                                        // Pull the Docker image first
                                                        info!(
                                                            "Pulling Docker image: {}",
                                                            agent_method.docker_image
                                                        );
                                                        let pull_start = Instant::now();
                                                        match self
                                                            .docker_manager
                                                            .load_image(
                                                                &agent_method.docker_image,
                                                                false,
                                                            )
                                                            .await
                                                        {
                                                            Ok(digest) => {
                                                                let pull_duration =
                                                                    pull_start.elapsed();
                                                                info!("⏱️ Docker image pull time: {}ms", pull_duration.as_millis());
                                                                info!("Image pulled successfully with digest: {}", digest);

                                                                // Verify SHA256 if provided
                                                                if let Some(expected_sha) =
                                                                    &agent_method.docker_sha256
                                                                {
                                                                    if &digest != expected_sha {
                                                                        error!(
                                                                            "Image SHA mismatch: expected {}, got {}",
                                                                            expected_sha, digest
                                                                        );
                                                                        // Clear current agent on SHA mismatch
                                                                        self.state.clear_current_agent().await;
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to pull image for job {}: {}", job_id, e);
                                                                // Clear current agent on error
                                                                self.state.clear_current_agent().await;
                                                                continue;
                                                            }
                                                        }

                                                        // Prepare container configuration
                                                        let container_config = ContainerConfig {
                                                            image_name: agent_method
                                                                .docker_image
                                                                .clone(),
                                                            image_source: agent_method
                                                                .docker_image
                                                                .clone(),
                                                            command: vec![], // Empty command to use image default
                                                            env_vars: {
                                                                let mut env = HashMap::new();
                                                                env.insert(
                                                                    "JOB_ID".to_string(),
                                                                    job_id.clone(),
                                                                );
                                                                env.insert(
                                                                    "JOB_DATA".to_string(),
                                                                    data.clone(),
                                                                );
                                                                env.insert(
                                                                    "DEVELOPER".to_string(),
                                                                    developer.clone(),
                                                                );
                                                                env.insert(
                                                                    "AGENT".to_string(),
                                                                    agent.clone(),
                                                                );
                                                                env.insert(
                                                                    "METHOD".to_string(),
                                                                    method.clone(),
                                                                );
                                                                env
                                                            },
                                                            port_bindings: HashMap::new(),
                                                            timeout_seconds: self
                                                                .config
                                                                .container_timeout_secs,
                                                            memory_limit_mb: Some(
                                                                (agent_method.min_memory_gb as u64)
                                                                    * 1024,
                                                            ),
                                                            cpu_cores: Some(
                                                                agent_method.min_cpu_cores as f64,
                                                            ),
                                                            network_mode: if agent_method
                                                                .requires_tee
                                                            {
                                                                Some("host".to_string())
                                                            } else {
                                                                None
                                                            },
                                                            requires_tee: agent_method.requires_tee,
                                                        };

                                                        // Run the container
                                                        let container_start = Instant::now();
                                                        let container_result = self
                                                            .docker_manager
                                                            .run_container(&container_config)
                                                            .await;
                                                        
                                                        // Clear current agent after container finishes
                                                        self.state.clear_current_agent().await;
                                                        
                                                        match container_result
                                                        {
                                                            Ok(result) => {
                                                                let container_duration =
                                                                    container_start.elapsed();
                                                                info!("⏱️ Container execution time: {}ms", container_duration.as_millis());
                                                                info!(
                                                                    "Container completed for job {}: exit_code={}, internal_duration={}ms",
                                                                    job_id, result.exit_code, result.duration_ms
                                                                );
                                                                if !result.logs.is_empty() {
                                                                    info!(
                                                                        "Container logs:\n{}",
                                                                        result.logs
                                                                    );
                                                                }

                                                                let total_duration =
                                                                    job_start.elapsed();
                                                                info!("⏱️ Total job processing time: {}ms", total_duration.as_millis());
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to run container for job {}: {}", job_id, e);
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!(
                                                            "Failed to fetch agent method: {}",
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if event.event_type.contains("JobFailedEvent") {
                    // Handle JobFailedEvent - track app_instance as it creates a pending job
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("Jobs Event Details:\n{}", job_details);
                            
                            // Extract and track app_instance from JobFailedEvent
                            // JobFailedEvent creates a pending job, so we need to track this app_instance
                            if let Some(app_instance) = extract_field(&job_details, "app_instance: ") {
                                // For JobFailedEvent, we need to look up the job to get developer/agent/method
                                // Since JobFailedEvent doesn't include these fields directly
                                // For now, we'll add with empty values - these will only match current_agent if all are empty
                                // TODO: Fetch the original job details to get the correct developer/agent/method
                                self.state.add_app_instance(
                                    app_instance.clone(),
                                    String::new(),  // developer not in JobFailedEvent
                                    String::new(),  // agent not in JobFailedEvent
                                    String::new(),  // agent_method not in JobFailedEvent
                                ).await;
                                info!("Tracked app_instance from JobFailedEvent: {} (Note: agent details not available in event)", 
                                    app_instance);
                            }
                        }
                    }
                } else if event.event_type.contains("Job") {
                    // Handle other job events
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("Jobs Event Details:\n{}", job_details);
                        }
                    }
                }
                self.process_coordination_event(event).await?;
            }

            if checkpoint_count % 100 == 0 {
                info!("Processed {} checkpoints so far...", checkpoint_count);
            }
        }

        Ok(())
    }

    async fn process_coordination_event(&mut self, event: CoordinationEvent) -> Result<()> {
        let event_id = format!(
            "{}-{}-{}-{}",
            event.checkpoint_seq, event.tx_index, event.event_index, event.event_type
        );

        if self.processed_events.contains_key(&event_id) {
            return Ok(());
        }

        self.processed_events.insert(event_id, true);

        // Clean up old processed events to prevent memory growth
        if self.processed_events.len() > 10000 {
            let to_remove: Vec<String> = self.processed_events.keys().take(5000).cloned().collect();
            for key in to_remove {
                self.processed_events.remove(&key);
            }
        }

        Ok(())
    }

    async fn fetch_event_with_contents(
        &mut self,
        checkpoint_seq: u64,
        tx_index: usize,
        event_index: usize,
    ) -> Result<Option<sui_rpc::proto::sui::rpc::v2beta2::Event>> {
        // First, we need to get the transaction digest from the checkpoint
        // We'll fetch all transaction digests and select the one we need
        let checkpoint_request = sui_rpc::proto::sui::rpc::v2beta2::GetCheckpointRequest {
            checkpoint_id: Some(sui_rpc::proto::sui::rpc::v2beta2::get_checkpoint_request::CheckpointId::SequenceNumber(checkpoint_seq)),
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["transactions.digest".to_string()],
            }),
        };

        let mut client = self.state.get_sui_client();
        let checkpoint_response = client
            .ledger_client()
            .get_checkpoint(checkpoint_request)
            .await
            .map_err(|e| CoordinatorError::RpcConnectionError(e.to_string()))?;

        // Get the transaction digest
        let tx_digest = if let Some(checkpoint) = checkpoint_response.into_inner().checkpoint {
            if let Some(transaction) = checkpoint.transactions.get(tx_index) {
                transaction.digest.clone()
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        };

        // Now fetch just the transaction with event contents
        if let Some(digest) = tx_digest {
            let tx_request = GetTransactionRequest {
                digest: Some(digest),
                read_mask: Some(prost_types::FieldMask {
                    paths: vec![
                        "events.events.package_id".to_string(),
                        "events.events.module".to_string(),
                        "events.events.sender".to_string(),
                        "events.events.event_type".to_string(),
                        "events.events.contents".to_string(),
                    ],
                }),
            };

            let tx_response = client
                .ledger_client()
                .get_transaction(tx_request)
                .await
                .map_err(|e| CoordinatorError::RpcConnectionError(e.to_string()))?;

            // Extract the specific event from the transaction
            if let Some(transaction) = tx_response.into_inner().transaction {
                if let Some(tx_events) = transaction.events {
                    if let Some(event) = tx_events.events.get(event_index) {
                        return Ok(Some(event.clone()));
                    }
                }
            }
        }

        Ok(None)
    }
}

fn extract_field(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains(prefix))
        .and_then(|line| line.split(prefix).nth(1))
        .map(|s| s.trim().to_string())
}
