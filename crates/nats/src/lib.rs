//! NATS publisher implementation for event streaming
//!
//! This publisher is used by services to publish events to NATS JetStream
//! for real-time event distribution.

use anyhow::Result;
use async_nats::jetstream::Context;
use async_trait::async_trait;
use buffer::{BufferableEvent, EventPublisher};
use futures::future::try_join_all;
use prost::Message;
use std::env;
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info};

/// NATS publisher for publishing events to NATS JetStream
pub struct EventNatsPublisher {
    jetstream: Context,
    stream_name: String,
}

impl EventNatsPublisher {
    /// Create a new NATS publisher
    pub async fn new() -> Result<Self> {
        let nats_url = env::var("NATS_URL")?;
        let stream_name = env::var("NATS_SUBJECT").unwrap_or_else(|_| "silvana".to_string());
        let subject = format!("{}.events.>", stream_name);

        info!("🔄 Connecting to NATS server at: {}", nats_url);

        let client = timeout(Duration::from_secs(5), async_nats::connect(&nats_url)).await??;
        let jetstream = async_nats::jetstream::new(client);
        info!("✅ Connected to NATS server successfully");

        let stream_config = async_nats::jetstream::stream::Config {
            name: stream_name.clone(),
            subjects: vec![subject.clone()],
            max_messages: 10_000,
            max_age: Duration::from_secs(60 * 60), // 1 hour
            ..Default::default()
        };

        // Try to get existing stream first
        match jetstream.get_stream(&stream_name).await {
            Ok(mut existing_stream) => {
                // Check if the existing stream has the correct subject pattern
                let existing_info = existing_stream.info().await?;
                if existing_info.config.subjects != vec![subject.clone()] {
                    info!(
                        "🔄 Updating NATS stream {} with new subject pattern",
                        stream_name
                    );
                    // Update the existing stream with new configuration
                    match jetstream.update_stream(stream_config).await {
                        Ok(_) => {
                            info!("✅ Updated NATS stream {} successfully", stream_name);
                        }
                        Err(e) => {
                            error!("❌ Failed to update NATS stream {} : {:?}", stream_name, e);
                            return Err(anyhow::anyhow!("Failed to update NATS stream"));
                        }
                    }
                } else {
                    info!(
                        "✅ NATS stream {} already has correct configuration",
                        stream_name
                    );
                }
            }
            Err(_) => {
                // Stream doesn't exist, create it
                match jetstream.create_stream(stream_config).await {
                    Ok(_) => {
                        info!("✅ Created NATS stream {} successfully", stream_name);
                    }
                    Err(e) => {
                        error!("❌ Failed to create NATS stream {} : {:?}", stream_name, e);
                        return Err(anyhow::anyhow!("Failed to create NATS stream"));
                    }
                }
            }
        };

        Ok(Self {
            jetstream,
            stream_name,
        })
    }

    /// Create NATS subject based on event type for better routing
    fn create_subject<T: BufferableEvent>(&self, event: &T) -> String {
        format!("{}.events.{}", self.stream_name, event.event_type_name())
    }
}

#[async_trait]
impl<T> EventPublisher<T> for EventNatsPublisher
where
    T: BufferableEvent + Message,
{
    async fn publish_batch(&self, events: &[T]) -> Result<(usize, usize)> {
        if events.is_empty() {
            return Ok((0, 0));
        }

        debug!(
            "📤 Publishing {} events to NATS JetStream stream '{}'",
            events.len(),
            self.stream_name
        );

        // Gather ack futures
        let mut ack_futures = Vec::with_capacity(events.len());
        let mut failed_sends = 0usize;

        for event in events {
            let subject = self.create_subject(event);
            let payload = event.encode_to_vec();

            match self
                .jetstream
                .publish(subject.clone(), payload.into())
                .await
            {
                Ok(ack) => {
                    debug!("✅ NATS publish success for subject {}", subject);
                    ack_futures.push(ack.into_future());
                }
                Err(e) => {
                    error!("❌ NATS publish error for subject {}: {:?}", subject, e);
                    failed_sends += 1;
                }
            }
        }
        if failed_sends > 0 {
            error!("❌ NATS publish failed for {} events", failed_sends);
        }

        // Wait for all acks concurrently (30‑second overall timeout)
        let ack_result = timeout(Duration::from_secs(30), try_join_all(ack_futures)).await;

        let successful_acks = match ack_result {
            Ok(Ok(acks)) => acks.len(),
            Ok(Err(e)) => {
                error!("❌ NATS ack error: {}", e);
                0
            }
            Err(_) => {
                error!("⏰ timeout waiting for acks");
                0
            }
        };

        let total_success = successful_acks;
        let total_fail = events.len() - total_success;

        debug!(
            "NATS publish batch result: ✅ {} ok, ❌ {} fail",
            total_success, total_fail
        );

        Ok((total_success, total_fail))
    }
}
