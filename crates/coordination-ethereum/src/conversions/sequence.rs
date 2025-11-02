//! SequenceState type conversions between Solidity and Rust

use alloy::primitives::{Bytes, FixedBytes};
use silvana_coordination_trait::SequenceState;

use super::helpers::*;

/// Create a SequenceState from Solidity contract data
///
/// Note: Solidity SequenceState structure differs from Rust trait type.
/// The Solidity version stores commitments (bytes32) but not full state data.
pub fn create_sequence_state(
    sequence: u64,
    state_data: Bytes,
    data_availability: String,
    _timestamp: u64,
) -> SequenceState {
    SequenceState {
        sequence,
        state: optional_bytes(&state_data),
        data_availability: optional_string(data_availability),
        optimistic_state: Vec::new(), // Not stored in Solidity version
        transition_data: Vec::new(),  // Not stored in Solidity version
    }
}

/// Create a SequenceState with commitments from Solidity
///
/// This version is for when we only have commitment hashes, not full state
pub fn create_sequence_state_from_commitments(
    sequence: u64,
    actions_commitment: FixedBytes<32>,
    state_commitment: FixedBytes<32>,
    _block_number: u64,
    _timestamp: u64,
) -> SequenceState {
    // We don't have full state data, just commitments
    // Store commitments in transition_data for now
    let mut transition_data = Vec::new();
    transition_data.extend_from_slice(actions_commitment.as_slice());
    transition_data.extend_from_slice(state_commitment.as_slice());

    SequenceState {
        sequence,
        state: None,
        data_availability: None,
        optimistic_state: Vec::new(),
        transition_data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sequence_state() {
        let state_data = Bytes::from(vec![1, 2, 3, 4]);
        let seq = create_sequence_state(
            42,
            state_data,
            "ipfs://state-hash".to_string(),
            12345,
        );

        assert_eq!(seq.sequence, 42);
        assert!(seq.has_local_state());
        assert!(seq.has_da_state());
        assert_eq!(seq.state_size(), 4);
    }

    #[test]
    fn test_create_sequence_state_empty() {
        let seq = create_sequence_state(42, Bytes::new(), String::new(), 12345);

        assert_eq!(seq.sequence, 42);
        assert!(!seq.has_local_state());
        assert!(!seq.has_da_state());
        assert_eq!(seq.state_size(), 0);
    }

    #[test]
    fn test_create_sequence_state_from_commitments() {
        let actions = FixedBytes::<32>::from([1u8; 32]);
        let state = FixedBytes::<32>::from([2u8; 32]);

        let seq = create_sequence_state_from_commitments(42, actions, state, 10, 12345);

        assert_eq!(seq.sequence, 42);
        assert!(!seq.has_local_state());
        assert_eq!(seq.transition_data.len(), 64); // 32 + 32 bytes
    }
}
