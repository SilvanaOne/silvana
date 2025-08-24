use serde::Serialize;
use sha2::{Sha256, Digest};
use crate::state::SharedState;

/// Structure used for generating unique job IDs
#[derive(Serialize)]
struct JobIdInput {
    coordinator_id: String,
    developer: String,
    agent: String,
    agent_method: String,
    app_instance: String,
    job_sequence: u64,
    timestamp: u64,
    chain: String,
}

/// Generate a unique job ID using the "sn" prefix and base56 encoding of SHA256 hash
pub fn generate_job_id(
    state: &SharedState,
    developer: &str,
    agent: &str,
    agent_method: &str,
    app_instance: &str,
    job_sequence: u64,
    timestamp: u64,
) -> String {
    let input = JobIdInput {
        coordinator_id: state.get_coordinator_id().clone(),
        developer: developer.to_string(),
        agent: agent.to_string(),
        agent_method: agent_method.to_string(),
        app_instance: app_instance.to_string(),
        job_sequence,
        timestamp,
        chain: state.get_chain().clone(),
    };
    
    // Serialize to JSON for consistent hashing
    let json_bytes = serde_json::to_vec(&input)
        .expect("Failed to serialize JobIdInput");
    
    // Calculate SHA256 hash
    let mut hasher = Sha256::new();
    hasher.update(&json_bytes);
    let hash_result = hasher.finalize();
    
    // Convert to base56 (using Bitcoin base58 alphabet without 0, O, I, l to avoid confusion)
    let base56_hash = encode_base56(&hash_result);
    
    // Return with "sn" prefix
    format!("sn{}", base56_hash)
}

/// Base56 encoding using Bitcoin base58 alphabet without confusing characters
fn encode_base56(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz";
    
    // Convert bytes to big integer representation
    let mut num = num_bigint::BigUint::from_bytes_be(data);
    let base = num_bigint::BigUint::from(56u32);
    let zero = num_bigint::BigUint::from(0u32);
    
    let mut result = String::new();
    
    // Handle leading zeros in input first
    let mut leading_zero_count = 0;
    for &byte in data {
        if byte == 0 {
            leading_zero_count += 1;
        } else {
            break;
        }
    }
    
    // Add leading zeros as '1' characters
    for _ in 0..leading_zero_count {
        result.push(ALPHABET[0] as char);
    }
    
    if num == zero {
        // If the entire input was zeros, we already handled it above
        if leading_zero_count == 0 {
            result.push(ALPHABET[0] as char);
        }
    } else {
        // Convert the non-zero part
        let mut temp_result = String::new();
        while num > zero {
            let remainder = &num % &base;
            let remainder_u32: u32 = remainder.try_into().unwrap();
            temp_result.push(ALPHABET[remainder_u32 as usize] as char);
            num /= &base;
        }
        // Reverse and append to result
        result.push_str(&temp_result.chars().rev().collect::<String>());
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SharedState;
    
    #[tokio::test]
    async fn test_generate_job_id() {
        // Set environment variables for test
        unsafe {
            std::env::set_var("SUI_ADDRESS", "0x123");
            std::env::set_var("SUI_CHAIN", "testnet");
        }
        
        // Create a mock sui client (we need to create this differently for tests)
        use sui_rpc::Client;
        let client = Client::new("https://test.com".to_string()).unwrap();
        
        // Create a test shared state
        let state = SharedState::new(client);
        
        // Generate a job ID
        let job_id = generate_job_id(
            &state,
            "test_dev",
            "test_agent", 
            "test_method",
            "test_app_instance",
            123,
            1692537600000, // Fixed timestamp for reproducible tests
        );
        
        // Should start with "sn" prefix
        assert!(job_id.starts_with("sn"));
        
        // Should be longer than just "sn"
        assert!(job_id.len() > 2);
        
        // Should be deterministic - same inputs should produce same output
        let job_id2 = generate_job_id(
            &state,
            "test_dev",
            "test_agent", 
            "test_method",
            "test_app_instance",
            123,
            1692537600000, // Same timestamp
        );
        assert_eq!(job_id, job_id2);
        
        // Different inputs should produce different outputs
        let job_id3 = generate_job_id(
            &state,
            "test_dev",
            "test_agent", 
            "test_method",
            "test_app_instance",
            124, // Different sequence
            1692537600000,
        );
        assert_ne!(job_id, job_id3);
    }
    
    #[test]
    fn test_base56_encoding() {
        // Test with known values
        let data = b"hello";
        let encoded = encode_base56(data);
        
        // Should not contain confusing characters
        assert!(!encoded.contains('0'));
        assert!(!encoded.contains('O'));
        assert!(!encoded.contains('I'));
        assert!(!encoded.contains('l'));
        
        // Should be non-empty
        assert!(!encoded.is_empty());
        
        // Test with zeros
        let zero_data = &[0u8; 4];
        let zero_encoded = encode_base56(zero_data);
        assert_eq!(zero_encoded, "1111"); // Leading zeros become '1's
    }
}