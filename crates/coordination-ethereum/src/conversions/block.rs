//! Block type conversions between Solidity and Rust

use alloy::primitives::FixedBytes;
use silvana_coordination_trait::Block;

use super::helpers::*;

/// Create a Block from Solidity contract data
///
/// Note: Solidity Block structure is different from the Rust Block trait type.
/// Solidity stores commitments as bytes32, while Rust uses Vec<u8>.
pub fn create_block(
    name: String,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    actions_commitment: FixedBytes<32>,
    state_commitment: FixedBytes<32>,
    time_since_last_block: u64,
    number_of_transactions: u64,
    start_actions_commitment: FixedBytes<32>,
    end_actions_commitment: FixedBytes<32>,
    state_data_availability: String,
    proof_data_availability: String,
    created_at: u64,
    state_calculated_at: u64,
    proved_at: u64,
) -> Block {
    Block {
        name,
        block_number,
        start_sequence,
        end_sequence,
        actions_commitment: bytes32_to_vec(actions_commitment),
        state_commitment: bytes32_to_vec(state_commitment),
        time_since_last_block,
        number_of_transactions,
        start_actions_commitment: bytes32_to_vec(start_actions_commitment),
        end_actions_commitment: bytes32_to_vec(end_actions_commitment),
        state_data_availability: optional_string(state_data_availability),
        proof_data_availability: optional_string(proof_data_availability),
        created_at,
        state_calculated_at: optional_u64(state_calculated_at),
        proved_at: optional_u64(proved_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_block() {
        let actions_commitment = FixedBytes::<32>::from([1u8; 32]);
        let state_commitment = FixedBytes::<32>::from([2u8; 32]);
        let start_actions = FixedBytes::<32>::from([3u8; 32]);
        let end_actions = FixedBytes::<32>::from([4u8; 32]);

        let block = create_block(
            "block-1".to_string(),
            1,
            0,
            99,
            actions_commitment,
            state_commitment,
            5000,
            100,
            start_actions,
            end_actions,
            "ipfs://state".to_string(),
            "ipfs://proof".to_string(),
            12345,
            12350,
            12360,
        );

        assert_eq!(block.name, "block-1");
        assert_eq!(block.block_number, 1);
        assert_eq!(block.sequence_count(), 100);
        assert!(block.is_state_calculated());
        assert!(block.is_proved());
        assert!(block.has_complete_da());
    }

    #[test]
    fn test_create_block_without_optional_fields() {
        let zero_commitment = FixedBytes::<32>::from([0u8; 32]);

        let block = create_block(
            "block-2".to_string(),
            2,
            100,
            199,
            zero_commitment,
            zero_commitment,
            5000,
            100,
            zero_commitment,
            zero_commitment,
            String::new(), // Empty DA
            String::new(), // Empty DA
            12345,
            0,  // No state calculated
            0,  // Not proved
        );

        assert_eq!(block.name, "block-2");
        assert!(!block.is_state_calculated());
        assert!(!block.is_proved());
        assert!(!block.has_complete_da());
    }
}
