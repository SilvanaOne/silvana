//! Proof-related types

use serde::{Deserialize, Serialize};
use std::fmt;

/// Proof status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofStatus {
    /// Proof generation has started
    Started,
    /// Proof has been calculated
    Calculated,
    /// Proof was rejected
    Rejected,
    /// Proof is reserved
    Reserved,
    /// Proof has been used
    Used,
}

impl fmt::Display for ProofStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Started => write!(f, "Started"),
            Self::Calculated => write!(f, "Calculated"),
            Self::Rejected => write!(f, "Rejected"),
            Self::Reserved => write!(f, "Reserved"),
            Self::Used => write!(f, "Used"),
        }
    }
}

impl ProofStatus {
    /// Convert from u8 representation (used in Move contracts)
    pub fn from_u8(status: u8) -> Self {
        match status {
            1 => Self::Started,
            2 => Self::Calculated,
            3 => Self::Rejected,
            4 => Self::Reserved,
            5 => Self::Used,
            _ => Self::Rejected, // Default for unknown status
        }
    }

    /// Convert to u8 representation
    pub fn to_u8(&self) -> u8 {
        match self {
            Self::Started => 1,
            Self::Calculated => 2,
            Self::Rejected => 3,
            Self::Reserved => 4,
            Self::Used => 5,
        }
    }
}

/// Represents a proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    /// Proof status
    pub status: ProofStatus,

    /// Data availability hash for the proof
    pub da_hash: Option<String>,

    /// First set of sequences (for merge proofs)
    pub sequence1: Option<Vec<u64>>,

    /// Second set of sequences (for merge proofs)
    pub sequence2: Option<Vec<u64>>,

    /// Number of times this proof was rejected
    pub rejected_count: u16,

    /// Timestamp when proof was created/updated
    pub timestamp: u64,

    /// Address of the prover
    pub prover: String,

    /// Optional user address
    pub user: Option<String>,

    /// Job ID associated with this proof
    pub job_id: String,

    /// Sequences this proof covers
    pub sequences: Vec<u64>,
}

/// Proof calculation container for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofCalculation {
    /// Unique identifier
    pub id: String,

    /// Block number this calculation is for
    pub block_number: u64,

    /// Starting sequence
    pub start_sequence: u64,

    /// Ending sequence (optional)
    pub end_sequence: Option<u64>,

    /// Collection of proofs for different sequence ranges
    pub proofs: Vec<Proof>,

    /// Final block proof (after all sub-proofs are combined)
    pub block_proof: Option<String>,

    /// Whether block proof has been submitted
    pub block_proof_submitted: bool,
}

impl ProofCalculation {
    /// Check if all proofs are calculated
    pub fn all_proofs_calculated(&self) -> bool {
        self.proofs.iter().all(|p| p.status == ProofStatus::Calculated)
    }

    /// Check if block proof is ready
    pub fn is_block_proof_ready(&self) -> bool {
        self.block_proof.is_some() && !self.block_proof_submitted
    }

    /// Get total number of proofs
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// Get rejected proof count
    pub fn rejected_proof_count(&self) -> usize {
        self.proofs.iter().filter(|p| p.status == ProofStatus::Rejected).count()
    }
}