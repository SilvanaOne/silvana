//! Multi-layer event processor
//!
//! This module provides event stream processing for all coordination layers (Sui, Ethereum, Private).
//! It replaces the single-layer EventProcessor with a unified multi-layer approach.

use crate::coordination_layer::CoordinationLayer;
use crate::coordination_manager::CoordinationManager;
use crate::error::Result;
use crate::state::SharedState;
use futures::StreamExt;
use silvana_coordination_trait::{Coordination, JobCreatedEvent};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

pub struct MultiLayerEventProcessor {
    manager: Arc<CoordinationManager>,
    state: SharedState,
}

impl MultiLayerEventProcessor {
    pub fn new(manager: Arc<CoordinationManager>, state: SharedState) -> Self {
        info!("Initializing multi-layer event processor...");
        Self { manager, state }
    }

    pub async fn run(&mut self) -> Result<()> {
        let layers = self.manager.get_all_layers().await;
        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        // Spawn a task for each coordination layer's event stream
        for (layer_id, layer) in layers {
            let layer_id_clone = layer_id.clone();
            let state_clone = self.state.clone();
            let layer_clone = layer.clone();

            let handle = tokio::spawn(async move {
                Self::process_layer_events(layer_id_clone, layer_clone, state_clone).await;
            });

            handles.push(handle);
            info!("Started event stream processor for layer: {}", layer_id);
        }

        // Wait for all streams (should run indefinitely)
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Event stream task panicked: {}", e);
            }
        }

        Ok(())
    }

    async fn process_layer_events(
        layer_id: String,
        layer: Arc<CoordinationLayer>,
        state: SharedState,
    ) {
        loop {
            match layer.event_stream().await {
                Ok(mut stream) => {
                    info!("✅ {}: Event stream connected", layer_id);

                    while let Some(event_result) = stream.next().await {
                        match event_result {
                            Ok(event) => {
                                Self::handle_job_created_event(&layer_id, event, &state).await;
                            }
                            Err(e) => {
                                error!("{}: Event stream error: {}", layer_id, e);
                                break; // Reconnect
                            }
                        }
                    }

                    warn!("{}: Event stream ended, reconnecting in 5s...", layer_id);
                }
                Err(e) => {
                    error!("{}: Failed to create event stream: {}", layer_id, e);
                    warn!("{}: Retrying in 10s...", layer_id);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn handle_job_created_event(
        layer_id: &str,
        event: JobCreatedEvent,
        state: &SharedState,
    ) {
        info!(
            "{}: JobCreated - seq={}, app={}, agent={}/{}",
            layer_id,
            event.job_sequence,
            event.app_instance,
            event.agent,
            event.agent_method
        );

        // Add job to tracker so job_searcher can find it
        // Note: SharedState::add_job() has different parameter order than JobsTracker::add_job()
        state.add_job(
            event.developer.clone(),      // 1st: developer
            event.agent.clone(),          // 2nd: agent
            event.agent_method.clone(),   // 3rd: agent_method
            event.app_instance.clone(),   // 4th: app_instance
            layer_id.to_string(),         // 5th: layer_id
        ).await;
    }
}
