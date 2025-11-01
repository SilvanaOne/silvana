//! Settlement-related types

use serde::{Deserialize, Serialize};

/// Represents settlement information for a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    /// Chain name (e.g., "ethereum", "polygon")
    pub chain: String,

    /// Last block number that was settled on this chain
    pub last_settled_block_number: u64,

    /// Settlement contract/account address on this chain
    pub settlement_address: Option<String>,

    /// ID of the active settlement job for this chain
    pub settlement_job: Option<u64>,
}

/// Represents block settlement status on a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSettlement {
    /// Block number being settled
    pub block_number: u64,

    /// Settlement transaction hash
    pub settlement_tx_hash: Option<String>,

    /// Whether the settlement transaction was included in a block
    pub settlement_tx_included_in_block: bool,

    /// Timestamp when sent to settlement
    pub sent_to_settlement_at: Option<u64>,

    /// Timestamp when settlement was confirmed
    pub settled_at: Option<u64>,
}

impl BlockSettlement {
    /// Check if settlement is pending
    pub fn is_pending(&self) -> bool {
        self.settlement_tx_hash.is_some() && !self.settlement_tx_included_in_block
    }

    /// Check if settlement is complete
    pub fn is_settled(&self) -> bool {
        self.settlement_tx_included_in_block && self.settled_at.is_some()
    }

    /// Check if settlement has been initiated
    pub fn is_initiated(&self) -> bool {
        self.settlement_tx_hash.is_some()
    }
}