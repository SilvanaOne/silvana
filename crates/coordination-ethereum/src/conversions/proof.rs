//! Proof type conversions between Solidity and Rust

use alloy::primitives::Address;
use silvana_coordination_trait::{Proof, ProofCalculation, ProofStatus};

use super::helpers::*;

/// Convert Solidity proof status (u8) to Rust ProofStatus
pub fn convert_proof_status(status: u8) -> ProofStatus {
    match status {
        1 => ProofStatus::Started,
        2 => ProofStatus::Calculated,
        3 => ProofStatus::Rejected,
        4 => ProofStatus::Reserved,
        5 => ProofStatus::Used,
        _ => ProofStatus::Rejected, // Default for unknown
    }
}

/// Convert Rust ProofStatus to Solidity proof status (u8)
pub fn proof_status_to_u8(status: &ProofStatus) -> u8 {
    match status {
        ProofStatus::Started => 1,
        ProofStatus::Calculated => 2,
        ProofStatus::Rejected => 3,
        ProofStatus::Reserved => 4,
        ProofStatus::Used => 5,
    }
}

/// Create a Proof from Solidity contract data
pub fn create_proof(
    status: u8,
    da_hash: String,
    sequence1: Vec<u64>,
    sequence2: Vec<u64>,
    rejected_count: u16,
    timestamp: u64,
    prover: Address,
    user: Address,
    job_id: String,
    sequences: Vec<u64>,
) -> Proof {
    Proof {
        status: convert_proof_status(status),
        da_hash: optional_string(da_hash),
        sequence1: if sequence1.is_empty() {
            None
        } else {
            Some(sequence1)
        },
        sequence2: if sequence2.is_empty() {
            None
        } else {
            Some(sequence2)
        },
        rejected_count,
        timestamp,
        prover: address_to_string(prover),
        user: if user == Address::ZERO {
            None
        } else {
            Some(address_to_string(user))
        },
        job_id,
        sequences,
    }
}

/// Create a ProofCalculation from Solidity contract data
///
/// Note: Solidity ProofCalculation has a mapping which cannot be directly returned.
/// This creates a ProofCalculation with an empty proofs vector that must be populated separately.
pub fn create_proof_calculation(
    id: String,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    block_proof: String,
    is_finished: bool,
) -> ProofCalculation {
    ProofCalculation {
        id,
        block_number,
        start_sequence,
        end_sequence: if end_sequence > 0 {
            Some(end_sequence)
        } else {
            None
        },
        proofs: Vec::new(), // Must be populated by fetching individual proofs
        block_proof: optional_string(block_proof),
        block_proof_submitted: is_finished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_proof_status() {
        assert_eq!(convert_proof_status(1), ProofStatus::Started);
        assert_eq!(convert_proof_status(2), ProofStatus::Calculated);
        assert_eq!(convert_proof_status(3), ProofStatus::Rejected);
        assert_eq!(convert_proof_status(4), ProofStatus::Reserved);
        assert_eq!(convert_proof_status(5), ProofStatus::Used);
        assert_eq!(convert_proof_status(99), ProofStatus::Rejected); // Unknown
    }

    #[test]
    fn test_proof_status_to_u8() {
        assert_eq!(proof_status_to_u8(&ProofStatus::Started), 1);
        assert_eq!(proof_status_to_u8(&ProofStatus::Calculated), 2);
        assert_eq!(proof_status_to_u8(&ProofStatus::Rejected), 3);
        assert_eq!(proof_status_to_u8(&ProofStatus::Reserved), 4);
        assert_eq!(proof_status_to_u8(&ProofStatus::Used), 5);
    }

    #[test]
    fn test_create_proof() {
        let prover = Address::from([1u8; 20]);
        let user = Address::from([2u8; 20]);

        let proof = create_proof(
            2, // Calculated
            "ipfs://proof".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            0,
            12345,
            prover,
            user,
            "job-1".to_string(),
            vec![1, 2, 3, 4, 5, 6],
        );

        assert_eq!(proof.status, ProofStatus::Calculated);
        assert_eq!(proof.da_hash, Some("ipfs://proof".to_string()));
        assert_eq!(proof.sequence1, Some(vec![1, 2, 3]));
        assert_eq!(proof.sequence2, Some(vec![4, 5, 6]));
        assert_eq!(proof.rejected_count, 0);
        assert_eq!(proof.job_id, "job-1");
        assert!(proof.user.is_some());
    }

    #[test]
    fn test_create_proof_calculation() {
        let calc = create_proof_calculation(
            "calc-1".to_string(),
            100,
            0,
            99,
            "ipfs://block-proof".to_string(),
            false, // Not submitted yet
        );

        assert_eq!(calc.id, "calc-1");
        assert_eq!(calc.block_number, 100);
        assert_eq!(calc.start_sequence, 0);
        assert_eq!(calc.end_sequence, Some(99));
        assert!(calc.is_block_proof_ready()); // Has block_proof and not submitted
        assert_eq!(calc.proof_count(), 0); // Empty until populated
    }
}
