use crate::config::Config;
use crate::error::{CoordinatorError, Result};
use crate::events::{parse_coordination_events, parse_jobs_event_with_contents, CoordinationEvent};
use crate::state::SharedState;
use std::collections::HashMap;
use std::time::Duration;
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
    processed_events: HashMap<String, bool>,
    state: SharedState,
}

impl EventProcessor {
    pub async fn new(config: Config, state: SharedState) -> Result<Self> {
        info!("Initializing event processor...");

        Ok(Self {
            config,
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
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        // Parse and display the event details
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("📝 JobCreatedEvent detected: {}", job_details);

                            // Extract and track the job in shared state
                            if let Some(app_instance) = extract_field(&job_details, "app_instance: ") {
                                if let Some(job_sequence_str) = extract_field(&job_details, "job_sequence: ") {
                                    if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                        // Extract developer, agent, and agent_method for this event
                                        let developer = extract_field(&job_details, "developer: ").unwrap_or_default();
                                        let agent = extract_field(&job_details, "agent: ").unwrap_or_default();
                                        let agent_method = extract_field(&job_details, "agent_method: ").unwrap_or_default();
                                        
                                        // Add to shared state - this will trigger the job searcher
                                        self.state.add_job(
                                            job_sequence,
                                            developer.clone(),
                                            agent.clone(),
                                            agent_method.clone(),
                                            app_instance.clone(),
                                        ).await;
                                        info!("✅ Added job {} to shared state: {} ({}/{}/{})", 
                                            job_sequence, app_instance, developer, agent, agent_method);
                                    }
                                }
                            }
                        }
                    }
                } else if event.event_type.contains("JobCompletedEvent") {
                    // Handle JobCompletedEvent - job is completed and removed
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
                            
                            // Extract job_sequence and remove from tracking
                            if let Some(job_sequence_str) = extract_field(&job_details, "job_sequence: ") {
                                if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                    self.state.remove_job(job_sequence).await;
                                    info!("Removed completed job {} from tracking", job_sequence);
                                }
                            }
                        }
                    }
                } else if event.event_type.contains("JobFailedEvent") {
                    // Handle JobFailedEvent - job failed and may be retried
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("📉 JobFailedEvent: {}", job_details);
                            // Note: If job is retried, a JobUpdatedEvent will follow
                        }
                    }
                } else if event.event_type.contains("JobUpdatedEvent") {
                    // Handle JobUpdatedEvent - job status changed (e.g., failed job retried)
                    if let Some(full_event) = self
                        .fetch_event_with_contents(
                            event.checkpoint_seq,
                            event.tx_index,
                            event.event_index,
                        )
                        .await?
                    {
                        if let Some(job_details) = parse_jobs_event_with_contents(&full_event) {
                            info!("🔄 JobUpdatedEvent: {}", job_details);
                            
                            // Check if status is Pending (job was retried after failure)
                            if job_details.contains("status: JobStatus::Pending") {
                                // Extract job details to re-add to tracking
                                if let Some(job_sequence_str) = extract_field(&job_details, "job_sequence: ") {
                                    if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                        if let Some(app_instance) = extract_field(&job_details, "app_instance: ") {
                                            let developer = extract_field(&job_details, "developer: ").unwrap_or_default();
                                            let agent = extract_field(&job_details, "agent: ").unwrap_or_default();
                                            let agent_method = extract_field(&job_details, "agent_method: ").unwrap_or_default();
                                            
                                            // Re-add to tracking (job was retried and is pending again)
                                            self.state.add_job(
                                                job_sequence,
                                                developer.clone(),
                                                agent.clone(),
                                                agent_method.clone(),
                                                app_instance.clone(),
                                            ).await;
                                            info!("✅ Job {} was retried and re-added to tracking: {} ({}/{}/{})", 
                                                job_sequence, app_instance, developer, agent, agent_method);
                                        }
                                    }
                                }
                            }
                            // If status is Running, job was started - no action needed
                            // The job searcher will handle it when it completes or fails
                        }
                    }
                } else if event.event_type.contains("JobDeletedEvent") {
                    // Handle JobDeletedEvent - job is permanently removed
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
                            
                            // Extract job_sequence and remove from tracking
                            if let Some(job_sequence_str) = extract_field(&job_details, "job_sequence: ") {
                                if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                    self.state.remove_job(job_sequence).await;
                                    info!("Removed deleted job {} from tracking", job_sequence);
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

            if checkpoint_count % 10000 == 0 {
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
