//! NATS publisher implementation for event streaming
//!
//! This publisher is used by services to publish events to NATS JetStream
//! for real-time event distribution.

use anyhow::Result;
use async_trait::async_trait;
use buffer::{BufferableEvent, EventPublisher};
use serde::Serialize;
use std::env;
use tokio::time::{Duration, timeout};
use tracing::{debug, info, warn};

/// NATS publisher for publishing events to NATS JetStream
pub struct EventNatsPublisher {
    client: async_nats::Client,
    stream_name: String,
}

impl EventNatsPublisher {
    /// Create a new NATS publisher
    pub async fn new() -> Result<Self> {
        let nats_url = env::var("NATS_URL")?;
        let stream_name =
            env::var("NATS_STREAM_NAME").unwrap_or_else(|_| "silvana-events".to_string());

        info!("🔄 Connecting to NATS server at: {}", nats_url);

        let client = timeout(Duration::from_secs(5), async_nats::connect(&nats_url)).await??;

        info!("✅ Connected to NATS server successfully");

        Ok(Self {
            client,
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
    T: BufferableEvent + Serialize,
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

        let mut successful_publishes = 0;
        let mut failed_publishes = 0;

        for event in events {
            // Serialize event to JSON for NATS publishing
            match serde_json::to_vec(event) {
                Ok(payload) => {
                    let subject = self.create_subject(event);

                    // Publish to NATS with timeout
                    match timeout(
                        Duration::from_secs(5),
                        self.client.publish(subject.clone(), payload.into()),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {
                            successful_publishes += 1;
                            debug!(
                                "✅ Published {} event to NATS subject: {}",
                                event.event_type_name(),
                                subject
                            );
                        }
                        Ok(Err(e)) => {
                            failed_publishes += 1;
                            debug!(
                                "❌ Failed to publish {} event to NATS subject {}: {}",
                                event.event_type_name(),
                                subject,
                                e
                            );
                        }
                        Err(_) => {
                            failed_publishes += 1;
                            debug!(
                                "⏰ Timeout publishing {} event to NATS subject: {}",
                                event.event_type_name(),
                                subject
                            );
                        }
                    }
                }
                Err(e) => {
                    failed_publishes += 1;
                    debug!(
                        "🔥 Failed to serialize {} event for NATS publishing: {}",
                        event.event_type_name(),
                        e
                    );
                }
            }
        }

        if successful_publishes > 0 {
            debug!(
                "📤 Successfully published {}/{} events to NATS",
                successful_publishes,
                successful_publishes + failed_publishes
            );
        }
        if failed_publishes > 0 {
            warn!(
                "⚠️ Failed to publish {}/{} events to NATS",
                failed_publishes,
                successful_publishes + failed_publishes
            );
        }

        Ok((successful_publishes, failed_publishes))
    }
}
