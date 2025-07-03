//! Adapters for integrating proto events with the buffer system
//!
//! This module provides the necessary implementations to make proto events
//! work with the generic buffer system.

use crate::database::EventDatabase as RpcEventDatabase;
use anyhow::Result;
use async_trait::async_trait;
use buffer::BufferableEvent;
use proto::Event;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tidb::{EventDatabase, TidbBackend};

/// Wrapper type for proto Event to implement BufferableEvent and Serialize for NATS publishing
#[derive(Clone, Serialize, Deserialize)]
pub struct EventWrapper(pub Event);

impl From<Event> for EventWrapper {
    fn from(event: Event) -> Self {
        EventWrapper(event)
    }
}

impl From<EventWrapper> for Event {
    fn from(wrapper: EventWrapper) -> Self {
        wrapper.0
    }
}

/// Implementation of BufferableEvent for EventWrapper
impl BufferableEvent for EventWrapper {
    fn estimate_size(&self) -> usize {
        // More accurate size estimation
        let base_size = std::mem::size_of::<Event>();

        let payload_size = match &self.0.event_type {
            Some(event_type) => match event_type {
                proto::events::event::EventType::Coordinator(coord_event) => {
                    estimate_coordinator_event_size(coord_event)
                }
                proto::events::event::EventType::Agent(agent_event) => {
                    estimate_agent_event_size(agent_event)
                }
            },
            None => 0,
        };

        base_size + payload_size
    }

    fn event_type_name(&self) -> &'static str {
        match &self.0.event_type {
            Some(event_type) => match event_type {
                proto::events::event::EventType::Coordinator(coord_event) => {
                    match &coord_event.event {
                        Some(event) => match event {
                            proto::events::coordinator_event::Event::CoordinatorStarted(_) => {
                                "coordinator.started"
                            }
                            proto::events::coordinator_event::Event::AgentStartedJob(_) => {
                                "coordinator.agent_started_job"
                            }
                            proto::events::coordinator_event::Event::AgentFinishedJob(_) => {
                                "coordinator.agent_finished_job"
                            }
                            proto::events::coordinator_event::Event::CoordinationTx(_) => {
                                "coordinator.coordination_tx"
                            }
                            proto::events::coordinator_event::Event::CoordinatorError(_) => {
                                "coordinator.error"
                            }
                            proto::events::coordinator_event::Event::ClientTransaction(_) => {
                                "coordinator.client_transaction"
                            }
                        },
                        None => "coordinator.unknown",
                    }
                }
                proto::events::event::EventType::Agent(agent_event) => match &agent_event.event {
                    Some(event) => match event {
                        proto::events::agent_event::Event::Message(_) => "agent.message",
                        proto::events::agent_event::Event::Transaction(_) => "agent.transaction",
                    },
                    None => "agent.unknown",
                },
            },
            None => "unknown",
        }
    }
}

/// Adapter to make RPC EventDatabase work with the buffer EventDatabase trait
pub struct RpcEventDatabaseAdapter {
    inner: Arc<RpcEventDatabase>,
}

impl RpcEventDatabaseAdapter {
    pub fn new(database: Arc<RpcEventDatabase>) -> Self {
        Self { inner: database }
    }
}

#[async_trait]
impl EventDatabase<EventWrapper> for RpcEventDatabaseAdapter {
    async fn insert_events_batch(&self, events: &[EventWrapper]) -> Result<usize> {
        let unwrapped_events: Vec<Event> = events.iter().map(|w| w.0.clone()).collect();
        self.inner.insert_events_batch(&unwrapped_events).await
    }
}

/// Create a TiDB backend for EventWrapper using the RPC database  
pub fn create_tidb_backend(database: Arc<RpcEventDatabase>) -> TidbBackend<EventWrapper> {
    let adapter = Arc::new(RpcEventDatabaseAdapter::new(database));
    TidbBackend::new(adapter)
}

/// Create an EventBuffer with TiDB backend and optional NATS publisher
pub async fn create_event_buffer(
    database: Arc<RpcEventDatabase>,
    batch_size: usize,
    flush_interval: std::time::Duration,
    channel_capacity: usize,
) -> buffer::EventBuffer<EventWrapper> {
    let backend = Arc::new(create_tidb_backend(database));

    // For now, let's skip NATS publisher since EventWrapper doesn't implement Serialize
    // TODO: Implement NATS publishing for EventWrapper if needed
    buffer::EventBuffer::with_config(backend, None, batch_size, flush_interval, channel_capacity)
}

// Helper functions for size estimation (moved from original buffer.rs)

fn estimate_coordinator_event_size(coord_event: &proto::events::CoordinatorEvent) -> usize {
    match &coord_event.event {
        Some(event) => match event {
            proto::events::coordinator_event::Event::CoordinatorStarted(e) => {
                e.coordinator_id.len() + e.ethereum_address.len() + e.sui_ed25519_address.len() + 64
            }
            proto::events::coordinator_event::Event::AgentStartedJob(e) => {
                e.coordinator_id.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.job_id.len()
                    + 64
            }
            proto::events::coordinator_event::Event::AgentFinishedJob(e) => {
                e.coordinator_id.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.job_id.len()
                    + 72
            }
            proto::events::coordinator_event::Event::CoordinationTx(e) => {
                e.coordinator_id.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.job_id.len()
                    + e.memo.len()
                    + e.tx_hash.len()
                    + 64
            }
            proto::events::coordinator_event::Event::CoordinatorError(e) => {
                e.coordinator_id.len() + e.message.len() + 64
            }
            proto::events::coordinator_event::Event::ClientTransaction(e) => {
                e.coordinator_id.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.client_ip_address.len()
                    + e.method.len()
                    + e.data.len()
                    + e.tx_hash.len()
                    + 128
            }
        },
        None => 0,
    }
}

fn estimate_agent_event_size(agent_event: &proto::events::AgentEvent) -> usize {
    match &agent_event.event {
        Some(event) => match event {
            proto::events::agent_event::Event::Message(e) => {
                let base_size = e.coordinator_id.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.job_id.len()
                    + e.message.len()
                    + 64;

                // Add memory for child table records
                let sequence_records_size = e.sequences.len()
                    * std::mem::size_of::<tidb::agent_message_event_sequences::Model>();

                base_size + sequence_records_size
            }
            proto::events::agent_event::Event::Transaction(e) => {
                let base_size = e.coordinator_id.len()
                    + e.tx_type.len()
                    + e.developer.len()
                    + e.agent.len()
                    + e.app.len()
                    + e.job_id.len()
                    + e.tx_hash.len()
                    + e.chain.len()
                    + e.network.len()
                    + e.memo.len()
                    + e.metadata.len()
                    + 128;

                // Add memory for child table records
                let sequence_records_size = e.sequences.len()
                    * std::mem::size_of::<tidb::agent_transaction_event_sequences::Model>();

                base_size + sequence_records_size
            }
        },
        None => 0,
    }
}
