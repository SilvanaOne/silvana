use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofCalculation {
    pub block_number: u64,
    pub sequences: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofMergeData {
    pub block_number: u64,
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
}