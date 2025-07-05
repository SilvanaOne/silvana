//! # Coordinator Service
//!

use anyhow::Result;
use buffer::EventBuffer;
use monitoring::{init_logging, init_monitoring, spawn_monitoring_tasks, start_metrics_server};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

// Example coordinator event type that implements BufferableEvent
#[derive(Debug, Clone)]
pub struct CoordinatorEvent {
    pub id: String,
    pub coordinator_id: String,
    pub event_type: String,
    pub data: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// Implement BufferableEvent for our coordinator events
impl buffer::BufferableEvent for CoordinatorEvent {
    fn estimate_size(&self) -> usize {
        self.id.len() + self.coordinator_id.len() + self.event_type.len() + self.data.len() + 32 // rough estimate
    }

    fn event_type_name(&self) -> &'static str {
        "CoordinatorEvent"
    }
}

// Simple mock backend for demonstration
struct MockBackend;

#[async_trait::async_trait]
impl buffer::EventBackend<CoordinatorEvent> for MockBackend {
    async fn process_batch(&self, events: &[CoordinatorEvent]) -> Result<usize, anyhow::Error> {
        info!("📝 Coordinator processing batch of {} events", events.len());
        // Simulate some processing time
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(events.len()) // Return number of successfully processed events
    }

    fn backend_name(&self) -> &'static str {
        "MockCoordinatorBackend"
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (using monitoring crate)
    init_logging().await?;
    info!("✅ Logging initialized");

    info!("🚀 Starting Coordinator Service with Monitoring");

    // Initialize the monitoring system (same as RPC service)
    init_monitoring()?;

    // Create an event buffer for coordinator events
    let backend = Arc::new(MockBackend);
    let event_buffer = EventBuffer::with_config(
        backend,
        None,                   // No secondary publisher for this example
        50,                     // Batch size
        Duration::from_secs(5), // Flush interval
        10000,                  // Channel capacity
    );

    // Start monitoring tasks (same as RPC service)
    spawn_monitoring_tasks(event_buffer.clone());

    // Start metrics server
    let metrics_addr: SocketAddr = "0.0.0.0:9091".parse()?;
    tokio::spawn(async move {
        if let Err(e) = start_metrics_server(metrics_addr).await {
            tracing::error!("Failed to start metrics server: {}", e);
        }
    });

    info!("📊 Coordinator metrics available at http://localhost:9091/metrics");
    info!("🔧 Coordinator service running with shared monitoring system");

    // Simulate some coordinator activity
    for i in 0..100 {
        let event = CoordinatorEvent {
            id: format!("coord-event-{}", i),
            coordinator_id: "coordinator-001".to_string(),
            event_type: "task_assignment".to_string(),
            data: format!("Task data for event {}", i),
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = event_buffer.add_event(event).await {
            tracing::error!("Failed to add event: {}", e);
        }

        sleep(Duration::from_millis(100)).await;
    }

    info!("✅ Coordinator finished processing events");

    // Keep running to see metrics
    sleep(Duration::from_secs(60)).await;

    Ok(())
}
