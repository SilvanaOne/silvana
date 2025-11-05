

//! Event types for coordination layer event streams

use serde::{Deserialize, Serialize};

/// Event emitted when a new job is created on a coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCreatedEvent {
    /// Job sequence number
    pub job_sequence: u64,

    /// Developer who created the job
    pub developer: String,

    /// Agent to execute the job
    pub agent: String,

    /// Agent method to call
    pub agent_method: String,

    /// App instance ID where the job was created
    pub app_instance: String,

    /// App instance method that created the job
    pub app_instance_method: String,

    /// Block number when the job was created
    pub block_number: u64,

    /// Timestamp when the job was created (milliseconds since epoch)
    pub created_at: u64,
}
