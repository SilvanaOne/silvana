//! AppInstance type conversions between Solidity and Rust

use silvana_coordination_trait::AppInstance;
use std::collections::HashMap;

/// Note: Direct conversion from Solidity AppInstance to Rust AppInstance
/// requires matching the field structure differences between them.
///
/// Solidity AppInstance has fewer fields than the Rust AppInstance trait type.
/// We'll need to fill in missing fields with default values or fetch them separately.

/// Create a minimal AppInstance from Solidity contract data
pub fn create_app_instance_minimal(
    id: String,
    _name: String,
    owner_address: String,
    silvana_app_name: String,
    _developer_name: String,
    sequence: u64,
    block_number: u64,
    previous_block_timestamp: u64,
    previous_block_last_sequence: u64,
    is_paused: bool,
    min_time_between_blocks: u64,
    created_at: u64,
    updated_at: u64,
) -> AppInstance {
    AppInstance {
        id,
        silvana_app_name,
        description: None,
        metadata: HashMap::new(),
        kv: HashMap::new(),
        sequence,
        admin: owner_address,
        block_number,
        previous_block_timestamp,
        previous_block_last_sequence,
        last_proved_block_number: 0, // Not available from basic Solidity AppInstance
        last_settled_block_number: 0, // Must be fetched separately
        last_settled_sequence: 0,     // Must be fetched separately
        last_purged_sequence: 0,      // Must be fetched separately
        settlements: HashMap::new(),  // Must be fetched separately
        is_paused,
        min_time_between_blocks,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_app_instance_minimal() {
        let app = create_app_instance_minimal(
            "test-app".to_string(),
            "test-app".to_string(),
            "0x1234".to_string(),
            "MyApp".to_string(),
            "developer1".to_string(),
            100,
            10,
            1000,
            99,
            false,
            5000,
            12345,
            12346,
        );

        assert_eq!(app.id, "test-app");
        assert_eq!(app.silvana_app_name, "MyApp");
        assert_eq!(app.sequence, 100);
        assert!(!app.is_paused);
        assert_eq!(app.min_time_between_blocks, 5000);
    }
}
