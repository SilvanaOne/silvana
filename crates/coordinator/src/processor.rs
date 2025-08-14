use crate::config::Config;
use crate::error::{CoordinatorError, Result};
use crate::events::{parse_coordination_events, parse_jobs_event_with_contents, CoordinationEvent};
use crate::registry::fetch_agent_method;
use docker::{ContainerConfig, DockerManager};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sui_rpc::proto::sui::rpc::v2beta2::{
    GetTransactionRequest, SubscribeCheckpointsRequest, SubscribeCheckpointsResponse,
};
use sui_rpc::Client;
use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

const RETRY_DELAY_SECS: u64 = 5;
const MAX_RETRIES: usize = 5;
const STREAM_TIMEOUT_SECS: u64 = 30;

pub struct EventProcessor {
    config: Config,
    client: Client,
    docker_manager: DockerManager,
    processed_events: HashMap<String, bool>,
}

impl EventProcessor {
    pub async fn new(config: Config) -> Result<Self> {
        info!("Initializing event processor...");

        let client = Client::new(&config.rpc_url)
            .map_err(|e| CoordinatorError::RpcConnectionError(e.to_string()))?;

        let docker_manager =
            DockerManager::new(config.use_tee).map_err(|e| CoordinatorError::DockerError(e))?;

        Ok(Self {
            config,
            client,
            docker_manager,
            processed_events: HashMap::new(),
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

        let mut subscription_client = self.client.subscription_client();

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
                                                match fetch_agent_method(
                                                    &mut self.client,
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

                                                        // Run the Docker container immediately
                                                        info!(
                                                            "Starting Docker container for job {}",
                                                            job_id
                                                        );

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
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to pull image for job {}: {}", job_id, e);
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
                                                        match self
                                                            .docker_manager
                                                            .run_container(&container_config)
                                                            .await
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

        let checkpoint_response = self
            .client
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

            let tx_response = self
                .client
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
