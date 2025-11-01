//! Job-related types

use serde::{Deserialize, Serialize};
use std::fmt;

/// Job status enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is waiting to be executed
    Pending,
    /// Job is currently being executed
    Running,
    /// Job completed successfully
    Completed,
    /// Job failed with an error
    Failed(String),
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Running => write!(f, "Running"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed(err) => write!(f, "Failed: {}", err),
        }
    }
}

/// Represents a job in the coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// The unique identifier of the Job (format depends on coordination layer)
    pub id: String,

    /// Job sequence number
    pub job_sequence: u64,

    /// Optional description
    pub description: Option<String>,

    /// Developer identifier
    pub developer: String,

    /// Agent identifier
    pub agent: String,

    /// Agent method name
    pub agent_method: String,

    /// App name
    pub app: String,

    /// App instance identifier
    pub app_instance: String,

    /// App instance method name
    pub app_instance_method: String,

    /// Optional block number
    pub block_number: Option<u64>,

    /// Optional sequence numbers
    pub sequences: Option<Vec<u64>>,

    /// Optional sequences1 for merge operations
    pub sequences1: Option<Vec<u64>>,

    /// Optional sequences2 for merge operations
    pub sequences2: Option<Vec<u64>>,

    /// Job data as bytes
    pub data: Vec<u8>,

    /// Job status
    pub status: JobStatus,

    /// Number of attempts
    pub attempts: u8,

    /// Interval in milliseconds for periodic jobs
    pub interval_ms: Option<u64>,

    /// Next scheduled time for periodic jobs (absolute timestamp in ms)
    pub next_scheduled_at: Option<u64>,

    /// Creation timestamp
    pub created_at: u64,

    /// Last update timestamp
    pub updated_at: u64,
}

impl Job {
    /// Check if this job is periodic
    pub fn is_periodic(&self) -> bool {
        self.interval_ms.is_some()
    }

    /// Check if this is a settlement job
    pub fn is_settlement(&self) -> bool {
        self.app_instance_method == "settle"
    }

    /// Check if this is a merge job
    pub fn is_merge(&self) -> bool {
        self.sequences1.is_some() && self.sequences2.is_some()
    }
}