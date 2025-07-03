//! TiDB-specific database entities and operations
//!
//! This crate contains Sea-ORM entities generated from proto definitions
//! and database-specific operations for TiDB.

use anyhow::Result;
use async_trait::async_trait;
use buffer::{BufferableEvent, EventBackend};
use std::sync::Arc;

pub mod entity;

// Re-export entities for convenience
pub use entity::*;

/// Generic trait for database operations on events
#[async_trait]
pub trait EventDatabase<T: BufferableEvent>: Send + Sync {
    /// Insert a batch of events into the database
    /// Returns the number of successfully inserted events
    async fn insert_events_batch(&self, events: &[T]) -> Result<usize>;
}

/// TiDB backend that uses an EventDatabase implementation to store events
pub struct TidbBackend<T: BufferableEvent> {
    database: Arc<dyn EventDatabase<T>>,
}

impl<T: BufferableEvent> TidbBackend<T> {
    pub fn new(database: Arc<dyn EventDatabase<T>>) -> Self {
        Self { database }
    }
}

#[async_trait]
impl<T: BufferableEvent> EventBackend<T> for TidbBackend<T> {
    async fn process_batch(&self, events: &[T]) -> Result<usize> {
        self.database.insert_events_batch(events).await
    }

    fn backend_name(&self) -> &'static str {
        "TiDB"
    }
}
