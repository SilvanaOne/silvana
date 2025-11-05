//! Sequence state types

use serde::{Deserialize, Serialize};

/// Represents the state at a specific sequence number
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceState {
    /// Sequence number
    pub sequence: u64,

    /// State data (if available)
    pub state: Option<Vec<u8>>,

    /// Data availability reference for the state
    pub data_availability: Option<String>,

    /// Optimistic state data
    pub optimistic_state: Vec<u8>,

    /// Transition data (delta from previous state)
    pub transition_data: Vec<u8>,
}

impl SequenceState {
    /// Check if state data is available locally
    pub fn has_local_state(&self) -> bool {
        self.state.is_some()
    }

    /// Check if state is stored in data availability layer
    pub fn has_da_state(&self) -> bool {
        self.data_availability.is_some()
    }

    /// Get state size in bytes
    pub fn state_size(&self) -> usize {
        self.state.as_ref().map(|s| s.len()).unwrap_or(0)
    }
}