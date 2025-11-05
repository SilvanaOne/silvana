//! App instance types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents an app instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInstance {
    /// The unique identifier of the AppInstance
    pub id: String,

    /// The name of the Silvana app
    pub silvana_app_name: String,

    /// Optional description of the app
    pub description: Option<String>,

    /// Metadata key-value store
    pub metadata: HashMap<String, String>,

    /// String key-value store
    pub kv: HashMap<String, String>,

    /// Current sequence number
    pub sequence: u64,

    /// Admin address
    pub admin: String,

    /// Current block number
    pub block_number: u64,

    /// Previous block timestamp
    pub previous_block_timestamp: u64,

    /// Previous block last sequence
    pub previous_block_last_sequence: u64,

    /// Last proved block number
    pub last_proved_block_number: u64,

    /// Last settled block number (minimum across all chains)
    pub last_settled_block_number: u64,

    /// Last settled sequence number
    pub last_settled_sequence: u64,

    /// Last purged sequence number
    pub last_purged_sequence: u64,

    /// Settlements configuration per chain
    pub settlements: HashMap<String, Settlement>,

    /// Whether the app is paused
    pub is_paused: bool,

    /// Minimum time between blocks in milliseconds
    pub min_time_between_blocks: u64,

    /// Creation timestamp
    pub created_at: u64,

    /// Last update timestamp
    pub updated_at: u64,
}

impl AppInstance {
    /// Check if the app can create a new block
    pub fn can_create_block(&self, current_time: u64) -> bool {
        if self.is_paused {
            return false;
        }

        // Check if enough time has passed since last block
        let time_since_last_block = current_time.saturating_sub(self.previous_block_timestamp);
        time_since_last_block >= self.min_time_between_blocks
    }

    /// Check if there are new sequences to include in a block
    pub fn has_new_sequences(&self) -> bool {
        self.sequence > self.previous_block_last_sequence
    }

    /// Get settlement chains
    pub fn get_settlement_chains(&self) -> Vec<String> {
        self.settlements.keys().cloned().collect()
    }
}

// Re-export Settlement type here for convenience
pub use super::settlement::Settlement;