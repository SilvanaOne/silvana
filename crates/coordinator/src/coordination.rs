use serde::{Deserialize, Serialize};

// Re-export types from sui crate to avoid duplication
pub use sui::fetch::{
    ProofCalculation,
    Proof,
    ProofStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofMergeData {
    pub block_number: u64,
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
}