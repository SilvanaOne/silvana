//! Helper functions for type conversions between Solidity and Rust types

use alloy::primitives::{Address, Bytes, FixedBytes, U256};

/// Convert Alloy Address to hex string with 0x prefix
pub fn address_to_string(addr: Address) -> String {
    format!("0x{:x}", addr)
}

/// Convert bytes32 (FixedBytes<32>) to Vec<u8>
pub fn bytes32_to_vec(bytes: FixedBytes<32>) -> Vec<u8> {
    bytes.as_slice().to_vec()
}

/// Convert Alloy Bytes to Vec<u8>
pub fn bytes_to_vec(bytes: &Bytes) -> Vec<u8> {
    bytes.to_vec()
}

/// Convert Solidity string to Option<String> (empty string becomes None)
pub fn optional_string(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Convert Solidity Bytes to Option<Vec<u8>> (empty bytes becomes None)
pub fn optional_bytes(bytes: &Bytes) -> Option<Vec<u8>> {
    if bytes.is_empty() {
        None
    } else {
        Some(bytes.to_vec())
    }
}

/// Convert Solidity timestamp (U256) to u64
/// Safely handles overflow by capping at u64::MAX
pub fn timestamp_to_u64(timestamp: U256) -> u64 {
    if timestamp > U256::from(u64::MAX) {
        u64::MAX
    } else {
        timestamp.to::<u64>()
    }
}

/// Convert optional u64 field (0 means None)
pub fn optional_u64(value: u64) -> Option<u64> {
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optional_string() {
        assert_eq!(optional_string("".to_string()), None);
        assert_eq!(optional_string("test".to_string()), Some("test".to_string()));
    }

    #[test]
    fn test_optional_u64() {
        assert_eq!(optional_u64(0), None);
        assert_eq!(optional_u64(42), Some(42));
    }

    #[test]
    fn test_address_to_string() {
        let addr = Address::ZERO;
        assert_eq!(
            address_to_string(addr),
            "0x0000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn test_timestamp_to_u64() {
        assert_eq!(timestamp_to_u64(U256::from(1000)), 1000);
        assert_eq!(timestamp_to_u64(U256::from(u64::MAX)), u64::MAX);
    }
}
