//! Block-related types

use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a block in the coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block name/identifier
    pub name: String,

    /// Block number
    pub block_number: u64,

    /// Starting sequence for this block
    pub start_sequence: u64,

    /// Ending sequence for this block
    pub end_sequence: u64,

    /// Actions commitment (scalar element bytes)
    pub actions_commitment: Vec<u8>,

    /// State commitment (scalar element bytes)
    pub state_commitment: Vec<u8>,

    /// Time elapsed since previous block (ms)
    pub time_since_last_block: u64,

    /// Number of transactions in this block
    pub number_of_transactions: u64,

    /// Starting actions commitment
    pub start_actions_commitment: Vec<u8>,

    /// Ending actions commitment
    pub end_actions_commitment: Vec<u8>,

    /// State data availability reference
    pub state_data_availability: Option<String>,

    /// Proof data availability reference
    pub proof_data_availability: Option<String>,

    /// Block creation timestamp
    pub created_at: u64,

    /// When state was calculated
    pub state_calculated_at: Option<u64>,

    /// When block was proved
    pub proved_at: Option<u64>,
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block #{} (sequences {}-{}, txs: {})",
            self.block_number, self.start_sequence, self.end_sequence, self.number_of_transactions
        )
    }
}

impl Block {
    /// Check if this block has been proved
    pub fn is_proved(&self) -> bool {
        self.proved_at.is_some()
    }

    /// Check if state has been calculated
    pub fn is_state_calculated(&self) -> bool {
        self.state_calculated_at.is_some()
    }

    /// Get the number of sequences in this block
    pub fn sequence_count(&self) -> u64 {
        self.end_sequence - self.start_sequence + 1
    }

    /// Check if both state and proof DA are available
    pub fn has_complete_da(&self) -> bool {
        self.state_data_availability.is_some() && self.proof_data_availability.is_some()
    }
}