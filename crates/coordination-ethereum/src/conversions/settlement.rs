//! Settlement type conversions between Solidity and Rust

use silvana_coordination_trait::{BlockSettlement, Settlement};

use super::helpers::*;

/// Create a Settlement from Solidity contract data
pub fn create_settlement(
    chain: String,
    last_settled_block_number: u64,
    settlement_address: String,
    settlement_job: u64,
) -> Settlement {
    Settlement {
        chain,
        last_settled_block_number,
        settlement_address: optional_string(settlement_address),
        settlement_job: optional_u64(settlement_job),
    }
}

/// Create a BlockSettlement from Solidity contract data
pub fn create_block_settlement(
    block_number: u64,
    settlement_tx_hash: String,
    settlement_tx_included_in_block: bool,
    sent_to_settlement_at: u64,
    settled_at: u64,
) -> BlockSettlement {
    BlockSettlement {
        block_number,
        settlement_tx_hash: optional_string(settlement_tx_hash),
        settlement_tx_included_in_block,
        sent_to_settlement_at: optional_u64(sent_to_settlement_at),
        settled_at: optional_u64(settled_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_settlement() {
        let settlement = create_settlement(
            "ethereum".to_string(),
            100,
            "0x1234".to_string(),
            42,
        );

        assert_eq!(settlement.chain, "ethereum");
        assert_eq!(settlement.last_settled_block_number, 100);
        assert_eq!(settlement.settlement_address, Some("0x1234".to_string()));
        assert_eq!(settlement.settlement_job, Some(42));
    }

    #[test]
    fn test_create_settlement_minimal() {
        let settlement = create_settlement("polygon".to_string(), 50, String::new(), 0);

        assert_eq!(settlement.chain, "polygon");
        assert_eq!(settlement.last_settled_block_number, 50);
        assert_eq!(settlement.settlement_address, None);
        assert_eq!(settlement.settlement_job, None);
    }

    #[test]
    fn test_create_block_settlement() {
        let settlement = create_block_settlement(
            100,
            "0xabc123".to_string(),
            true,
            12345,
            12350,
        );

        assert_eq!(settlement.block_number, 100);
        assert!(settlement.is_settled());
        assert!(settlement.is_initiated());
    }

    #[test]
    fn test_create_block_settlement_pending() {
        let settlement = create_block_settlement(
            101,
            "0xdef456".to_string(),
            false,
            12345,
            0,
        );

        assert_eq!(settlement.block_number, 101);
        assert!(settlement.is_pending());
        assert!(!settlement.is_settled());
    }
}
