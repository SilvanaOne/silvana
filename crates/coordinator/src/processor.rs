use crate::config::Config;
use crate::constants::{
    GRPC_STREAM_TIMEOUT_SECS, RETRY_INITIAL_DELAY_SECS, RETRY_MAX_ATTEMPTS, RETRY_MAX_DELAY_SECS,
};
use crate::error::{CoordinatorError, Result};
use crate::events::{CoordinationEvent, parse_coordination_events, parse_jobs_event_with_contents};
use crate::metrics::CoordinatorMetrics;
use crate::state::SharedState;
use std::collections::HashMap;
use std::time::Duration;
use sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsResponse;
use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

pub struct EventProcessor {
    config: Config,
    processed_events: HashMap<String, bool>,
    state: SharedState,
    metrics: Option<std::sync::Arc<CoordinatorMetrics>>,
}

impl EventProcessor {
    pub async fn new(config: Config, state: SharedState) -> Result<Self> {
        info!("Initializing event processor...");

        Ok(Self {
            config,
            processed_events: HashMap::new(),
            state,
            metrics: None,
        })
    }

    pub fn set_metrics(&mut self, metrics: std::sync::Arc<CoordinatorMetrics>) {
        self.metrics = Some(metrics);
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
                        retry_count, RETRY_MAX_ATTEMPTS, e
                    );

                    if retry_count >= RETRY_MAX_ATTEMPTS {
                        error!("Max retries reached, exiting");
                        return Err(e);
                    }

                    // Exponential backoff: double the delay each time, capped at RETRY_MAX_DELAY_SECS
                    let delay_secs = (RETRY_INITIAL_DELAY_SECS
                        * 2_u64.pow((retry_count - 1) as u32))
                    .min(RETRY_MAX_DELAY_SECS);
                    let delay = Duration::from_secs(delay_secs);
                    warn!(
                        "Event processor streamerror, retrying in {}s...",
                        delay_secs
                    );
                    sleep(delay).await;
                }
            }
        }
    }

    async fn stream_checkpoints(&mut self) -> Result<()> {
        info!("Starting checkpoint stream...");

        let mut stream = sui::events::create_checkpoint_stream()
            .await
            .map_err(|e| CoordinatorError::Other(e))?;

        let mut checkpoint_count = 0;

        while let Ok(Some(response)) =
            timeout(Duration::from_secs(GRPC_STREAM_TIMEOUT_SECS), stream.next()).await
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
                            // Extract and track the job in shared state
                            if let Some(app_instance) =
                                extract_field(&job_details, "app_instance: ")
                            {
                                if let Some(job_sequence_str) =
                                    extract_field(&job_details, "job_sequence: ")
                                {
                                    if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                        // Extract developer, agent, and agent_method for this event
                                        let developer = extract_field(&job_details, "developer: ")
                                            .unwrap_or_default();
                                        let agent = extract_field(&job_details, "agent: ")
                                            .unwrap_or_default();
                                        let agent_method =
                                            extract_field(&job_details, "agent_method: ")
                                                .unwrap_or_default();
                                        let app_instance_method =
                                            extract_field(&job_details, "app_instance_method: ")
                                                .unwrap_or_default();
                                        let block_number =
                                            extract_field(&job_details, "block_number: ")
                                                .and_then(|s| s.parse::<u64>().ok())
                                                .unwrap_or(0);
                                        let sequences = extract_field(&job_details, "sequences: ")
                                            .unwrap_or_default();

                                        // Add to shared state - this will trigger the job searcher
                                        self.state
                                            .add_job(
                                                developer.clone(),
                                                agent.clone(),
                                                agent_method.clone(),
                                                app_instance.clone(),
                                            )
                                            .await;

                                        info!(
                                            "📝 JobCreated: seq={}, dev={}, agent={}/{}, app_method={}, block={}, seqs={}, app={}",
                                            job_sequence,
                                            developer,
                                            agent,
                                            agent_method,
                                            app_instance_method,
                                            block_number,
                                            sequences,
                                            &app_instance[..16.min(app_instance.len())]
                                        );
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
                            info!("Jobs Event Details:\n{}", job_details)
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
                                if let Some(job_sequence_str) =
                                    extract_field(&job_details, "job_sequence: ")
                                {
                                    if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                        if let Some(app_instance) =
                                            extract_field(&job_details, "app_instance: ")
                                        {
                                            let developer =
                                                extract_field(&job_details, "developer: ")
                                                    .unwrap_or_default();
                                            let agent = extract_field(&job_details, "agent: ")
                                                .unwrap_or_default();
                                            let agent_method =
                                                extract_field(&job_details, "agent_method: ")
                                                    .unwrap_or_default();

                                            // Re-add to tracking (job was retried and is pending again)
                                            self.state
                                                .add_job(
                                                    developer.clone(),
                                                    agent.clone(),
                                                    agent_method.clone(),
                                                    app_instance.clone(),
                                                )
                                                .await;
                                            info!(
                                                "✅ Job {} was retried and re-added to tracking: {} ({}/{}/{})",
                                                job_sequence,
                                                app_instance,
                                                developer,
                                                agent,
                                                agent_method
                                            );
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
    ) -> Result<Option<sui_rpc::proto::sui::rpc::v2::Event>> {
        // Use the helper function from the sui crate's checkpoint module
        sui::fetch::checkpoint::fetch_event_with_contents(checkpoint_seq, tx_index, event_index)
            .await
            .map_err(|e| CoordinatorError::RpcConnectionError(e.to_string()))
    }
}

fn extract_field(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains(prefix))
        .and_then(|line| line.split(prefix).nth(1))
        .map(|s| s.trim().to_string())
}
