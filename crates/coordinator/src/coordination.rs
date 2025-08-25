use serde::{Deserialize, Serialize};

// Rust representation of Move's ProofCalculation struct
// Matches move/coordination/sources/prover.move
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofCalculation {
    pub block_number: u64,
    pub sequences: Vec<u64>,  // Current sequences being calculated
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    pub proofs: Vec<ProofInfo>,  // In Move: VecMap<vector<u64>, Proof>
    pub block_proof: Option<String>,
    pub is_finished: bool,
}

// Individual proof information within a ProofCalculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofInfo {
    pub sequences: Vec<u64>,
    pub status: ProofStatus,
    pub da_hash: Option<String>,
    pub timestamp: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProofStatus {
    Started,    // PROOF_STATUS_STARTED = 1
    Calculated, // PROOF_STATUS_CALCULATED = 2
    Rejected,   // PROOF_STATUS_REJECTED = 3
    Reserved,   // PROOF_STATUS_RESERVED = 4
    Used,       // PROOF_STATUS_USED = 5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofMergeData {
    pub block_number: u64,
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
}