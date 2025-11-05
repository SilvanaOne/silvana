//! Conversions from Solidity ABI types to Rust coordination trait types
//!
//! This module provides `From` implementations to convert between types generated
//! by the Alloy sol! macro and the coordination trait types.

use alloy::primitives::Bytes;

// Note: We can't implement From traits for generated types until the ABIs are loaded
// The sol! macro generates types at compile time from the JSON ABIs
//
// For now, we provide conversion functions that will be called from coordination.rs
// when the actual contract instances are available.

/// Convert Solidity uint64 array (as Bytes) to Rust Vec<u64>
pub fn decode_u64_array(bytes: &Bytes) -> Vec<u64> {
    if bytes.is_empty() {
        return Vec::new();
    }

    // Solidity uint64[] is ABI-encoded
    // For simplicity, assume packed encoding (8 bytes per u64)
    bytes
        .chunks(8)
        .filter_map(|chunk| {
            if chunk.len() == 8 {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(chunk);
                Some(u64::from_be_bytes(arr))
            } else {
                None
            }
        })
        .collect()
}

/// Encode Rust Vec<u64> to Solidity uint64 array format (as Bytes)
pub fn encode_u64_array(vec: &[u64]) -> Bytes {
    let mut bytes = Vec::new();
    for &value in vec {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    Bytes::from(bytes)
}

/// Convert Solidity JobStatus with error message to Rust JobStatus
pub fn convert_job_status_with_error(status: u8, error_message: &str) -> silvana_coordination_trait::JobStatus {
    match status {
        0 => silvana_coordination_trait::JobStatus::Pending,
        1 => silvana_coordination_trait::JobStatus::Running,
        2 => silvana_coordination_trait::JobStatus::Completed,
        3 => silvana_coordination_trait::JobStatus::Failed(error_message.to_string()),
        _ => silvana_coordination_trait::JobStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_u64_array() {
        let bytes = Bytes::from(vec![
            0, 0, 0, 0, 0, 0, 0, 1, // 1
            0, 0, 0, 0, 0, 0, 0, 2, // 2
            0, 0, 0, 0, 0, 0, 0, 3, // 3
        ]);
        assert_eq!(decode_u64_array(&bytes), vec![1, 2, 3]);
    }

    #[test]
    fn test_decode_u64_array_empty() {
        let bytes = Bytes::new();
        assert_eq!(decode_u64_array(&bytes), Vec::<u64>::new());
    }

    #[test]
    fn test_encode_u64_array() {
        let bytes = encode_u64_array(&[1, 2, 3]);
        assert_eq!(bytes.len(), 24); // 3 * 8 bytes
    }

    #[test]
    fn test_convert_job_status_with_error() {
        use silvana_coordination_trait::JobStatus;

        assert_eq!(convert_job_status_with_error(0, ""), JobStatus::Pending);
        assert_eq!(convert_job_status_with_error(1, ""), JobStatus::Running);
        assert_eq!(convert_job_status_with_error(2, ""), JobStatus::Completed);

        let failed = convert_job_status_with_error(3, "test error");
        assert!(matches!(failed, JobStatus::Failed(msg) if msg == "test error"));
    }
}
