use crate::config::Config;
use crate::error::{CoordinatorError, Result};
use crate::events::{extract_docker_config, parse_coordination_events, CoordinationEvent};
use prost::Message;
use silvana_docker::{ContainerConfig, DockerManager};
use std::collections::HashMap;
use std::time::Duration;
use sui_rpc::proto::sui::rpc::v2beta2::{
    SubscribeCheckpointsRequest, SubscribeCheckpointsResponse,
};
use sui_rpc::Client;
use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

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
        
        let docker_manager = DockerManager::new(config.use_tee)
            .map_err(|e| CoordinatorError::DockerError(e))?;
        
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
                    error!("Stream error (attempt {}/{}): {}", retry_count, MAX_RETRIES, e);
                    
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
                    "summary.timestamp".to_string(),
                    "transactions.events.events.package_id".to_string(),
                    "transactions.events.events.module".to_string(),
                    "transactions.events.events.sender".to_string(),
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
                    self.process_checkpoint_response(response, checkpoint_count).await?;
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
        let response_size = response.encoded_len();
        
        if let (Some(cursor), Some(checkpoint)) = (response.cursor, response.checkpoint) {
            debug!(
                "Processing checkpoint #{} (cursor={}, size={}B)",
                checkpoint_count, cursor, response_size
            );
            
            let events = parse_coordination_events(
                &checkpoint,
                cursor,
                &self.config.package_id,
                &self.config.module,
            );
            
            for event in events {
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
            debug!("Event {} already processed, skipping", event_id);
            return Ok(());
        }
        
        info!(
            "Processing event: type={}, checkpoint={}, timestamp={}ms",
            event.event_type, event.checkpoint_seq, event.timestamp_ms
        );
        
        // Check if this event contains docker configuration
        if let Some(docker_config) = extract_docker_config(&event) {
            info!(
                "Found docker configuration: image={}, tee={}",
                docker_config.image, docker_config.requires_tee
            );
            
            // Load and run the docker container
            self.handle_docker_task(docker_config, event_id.clone()).await?;
        }
        
        self.processed_events.insert(event_id, true);
        
        // Clean up old processed events to prevent memory growth
        if self.processed_events.len() > 10000 {
            let to_remove: Vec<String> = self
                .processed_events
                .keys()
                .take(5000)
                .cloned()
                .collect();
            for key in to_remove {
                self.processed_events.remove(&key);
            }
        }
        
        Ok(())
    }

    async fn handle_docker_task(
        &self,
        docker_config: crate::events::DockerConfig,
        event_id: String,
    ) -> Result<()> {
        info!("Starting docker task for event {}", event_id);
        
        // First, load the image
        let image_digest = self
            .docker_manager
            .load_image(&docker_config.image, false)
            .await
            .map_err(|e| {
                error!("Failed to load image {}: {}", docker_config.image, e);
                CoordinatorError::DockerError(e)
            })?;
        
        info!("Loaded image with digest: {}", image_digest);
        
        // Verify SHA256 if provided
        if let Some(expected_sha) = &docker_config.sha256 {
            if &image_digest != expected_sha {
                error!(
                    "Image SHA mismatch: expected {}, got {}",
                    expected_sha, image_digest
                );
                return Err(CoordinatorError::ConfigError(
                    "Docker image SHA256 mismatch".to_string(),
                ));
            }
        }
        
        // Prepare container configuration
        let container_config = ContainerConfig {
            image_name: docker_config.image.clone(),
            image_source: docker_config.image.clone(),
            command: vec![
                "npm".to_string(),
                "run".to_string(),
                "start".to_string(),
                event_id.clone(),
            ],
            env_vars: HashMap::new(),
            port_bindings: HashMap::new(),
            timeout_seconds: self.config.container_timeout_secs,
            memory_limit_mb: Some(docker_config.memory_gb as u64 * 1024),
            cpu_cores: Some(docker_config.cpu_cores as f64),
            network_mode: if docker_config.requires_tee {
                Some("host".to_string())
            } else {
                None
            },
            requires_tee: docker_config.requires_tee,
        };
        
        // Run the container
        info!("Running container for event {}", event_id);
        let result = self
            .docker_manager
            .run_container(&container_config)
            .await
            .map_err(|e| {
                error!("Container execution failed: {}", e);
                CoordinatorError::DockerError(e)
            })?;
        
        info!(
            "Container completed: exit_code={}, duration={}ms",
            result.exit_code, result.duration_ms
        );
        
        if !result.logs.is_empty() {
            debug!("Container logs:\n{}", result.logs);
        }
        
        if result.exit_code != 0 {
            warn!("Container exited with non-zero code: {}", result.exit_code);
        }
        
        Ok(())
    }
}