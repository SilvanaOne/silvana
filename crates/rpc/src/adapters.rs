//! Adapters for integrating proto events with the buffer system
//!
//! This module provides the necessary implementations to make proto events
//! work with the generic buffer system.

use crate::database::EventDatabase as RpcEventDatabase;
use anyhow::Result;
use async_trait::async_trait;
use proto::Event;
use std::sync::Arc;
use tidb::{EventDatabase, TidbBackend};
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
impl EventDatabase<Event> for RpcEventDatabaseAdapter {
    async fn insert_events_batch(&self, events: &[Event]) -> Result<usize> {
        self.inner.insert_events_batch(events).await
    }
}

/// Create a TiDB backend for EventWrapper using the RPC database  
pub fn create_tidb_backend(database: Arc<RpcEventDatabase>) -> TidbBackend<Event> {
    let adapter = Arc::new(RpcEventDatabaseAdapter::new(database));
    TidbBackend::new(adapter)
}

/// Create an EventBuffer with TiDB backend and optional NATS publisher
pub async fn create_event_buffer(
    database: Arc<RpcEventDatabase>,
    batch_size: usize,
    flush_interval: std::time::Duration,
    channel_capacity: usize,
) -> buffer::EventBuffer<Event> {
    let backend = Arc::new(create_tidb_backend(database));
    buffer::EventBuffer::with_config(backend, None, batch_size, flush_interval, channel_capacity)
}
