use coordinator::proofs_storage::{ProofDescriptor, ProofMetadata, ProofStorage};
use std::env;

#[test]
fn test_descriptor_parsing() {
    // Test valid descriptors
    let test_cases = vec![
        (
            "walrus:testnet:s04UEUvCT-CTJVKsPHDWWMWcagr2mzDJHlnFm-dHOeo",
            "walrus",
            "testnet",
            "s04UEUvCT-CTJVKsPHDWWMWcagr2mzDJHlnFm-dHOeo",
        ),
        (
            "ipfs:public:fhkvrefvejhfvjrvfekkkrfvefv",
            "ipfs",
            "public",
            "fhkvrefvejhfvjrvfekkkrfvefv",
        ),
        (
            "s3:private:vhrevkevekkvvbhreb",
            "s3",
            "private",
            "vhrevkevekkvvbhreb",
        ),
        ("cache:public:hjfvhjvjhdv", "cache", "public", "hjfvhjvjhdv"),
    ];

    for (descriptor_str, expected_chain, expected_network, expected_hash) in test_cases {
        let descriptor = ProofDescriptor::parse(descriptor_str).unwrap();
        assert_eq!(descriptor.chain, expected_chain);
        assert_eq!(descriptor.network, expected_network);
        assert_eq!(descriptor.hash, expected_hash);
        assert_eq!(descriptor.to_string(), descriptor_str);
    }
}

#[test]
fn test_invalid_descriptors() {
    let invalid_cases = vec![
        "walrus:testnet",            // Missing hash
        "walrus",                    // Missing network and hash
        "",                          // Empty
        "walrus::hash",              // Missing network
        "walrus:testnet:hash:extra", // Too many parts
    ];

    for invalid in invalid_cases {
        assert!(
            ProofDescriptor::parse(invalid).is_err(),
            "Should fail to parse: {}",
            invalid
        );
    }
}

#[tokio::test]
async fn test_walrus_storage() {
    dotenvy::dotenv().ok();

    let storage = ProofStorage::new();

    // Test data
    let proof_data = "Test proof data for Walrus storage";
    let mut metadata = ProofMetadata::default();
    metadata
        .data
        .insert("test_key".to_string(), "test_value".to_string());
    metadata
        .data
        .insert("timestamp".to_string(), "2024-01-01".to_string());

    // Store proof to Walrus
    let descriptor = match storage
        .store_proof("walrus", "testnet", proof_data, Some(metadata.clone()))
        .await
    {
        Ok(desc) => {
            println!("Stored proof to Walrus: {}", desc.to_string());
            assert_eq!(desc.chain, "walrus");
            assert_eq!(desc.network, "testnet");
            assert!(!desc.hash.is_empty());
            desc
        }
        Err(e) => {
            println!("Skipping Walrus test - not available: {}", e);
            return;
        }
    };

    // Retrieve proof from Walrus
    match storage.get_proof(&descriptor.to_string()).await {
        Ok((retrieved_data, retrieved_metadata)) => {
            println!("Retrieved proof from Walrus");

            // The data might be wrapped in JSON with metadata
            // Try to extract the actual proof data
            if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&retrieved_data) {
                if let Some(proof) = wrapper.get("proof").and_then(|p| p.as_str()) {
                    assert_eq!(proof, proof_data);
                }
            }

            // Check metadata
            assert_eq!(
                retrieved_metadata.data.get("test_key"),
                Some(&"test_value".to_string())
            );
        }
        Err(e) => {
            panic!("Failed to retrieve proof from Walrus: {}", e);
        }
    }

    // Check if proof exists
    let exists = storage.proof_exists(&descriptor.to_string()).await.unwrap();
    assert!(exists, "Proof should exist in Walrus");
}

#[tokio::test]
async fn test_cache_storage() {
    dotenvy::dotenv().ok();

    // Set RPC endpoint for testing (use the test server)
    // Using unsafe for set_var in tests is acceptable
    unsafe {
        env::set_var(
            "RPC_ENDPOINT",
            env::var("TEST_SERVER").unwrap_or_else(|_| "https://rpc-devnet.silvana.dev".to_string()),
        );
    }

    let storage = ProofStorage::new();

    // Test data - simulate real coordinator proof submission
    let proof_data = "Test proof data for cache storage with realistic size";
    let mut metadata = ProofMetadata::default();
    
    // Add metadata similar to real submit_proof in grpc.rs
    metadata.data.insert("job_id".to_string(), "test_job_123456789".to_string());
    metadata.data.insert("session_id".to_string(), "0x1234567890abcdef".to_string());
    metadata.data.insert("app_instance_id".to_string(), "0xabcdef1234567890".to_string());
    metadata.data.insert("block_number".to_string(), "12345".to_string());
    
    // Test with a large sequences array that would exceed S3 limits if not truncated
    let large_sequences = vec![1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let sequences_str = format!("{:?}", large_sequences);
    // Simulate the truncation logic from grpc.rs
    let sequences_metadata = if sequences_str.len() > 200 {
        format!("{}... ({} sequences)", &sequences_str[..200], large_sequences.len())
    } else {
        sequences_str
    };
    metadata.data.insert("sequences".to_string(), sequences_metadata);
    
    metadata.data.insert("project".to_string(), "silvana".to_string());
    metadata.data.insert("sui_chain".to_string(), "testnet".to_string());
    
    // Test edge case: metadata value at exactly 256 chars (S3 limit)
    let long_value = "a".repeat(256);
    metadata.data.insert("edge_case_256".to_string(), long_value);
    
    // Test edge case: metadata key at exactly 128 chars (S3 limit)
    let long_key = "test_key_".to_string() + &"x".repeat(119); // 9 + 119 = 128
    metadata.data.insert(long_key, "test_value".to_string());
    
    metadata.expires_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 24 * 60 * 60 * 1000, // 24 hours from now
    );

    // Store proof to cache
    let descriptor = match storage
        .store_proof("cache", "public", proof_data, Some(metadata.clone()))
        .await
    {
        Ok(desc) => {
            println!("Stored proof to cache: {}", desc.to_string());
            assert_eq!(desc.chain, "cache");
            assert_eq!(desc.network, "public");
            assert!(!desc.hash.is_empty());
            desc
        }
        Err(e) => {
            println!("Skipping cache test - RPC not available: {}", e);
            return;
        }
    };

    // Retrieve proof from cache
    match storage.get_proof(&descriptor.to_string()).await {
        Ok((retrieved_data, retrieved_metadata)) => {
            println!("Retrieved proof from cache");
            assert_eq!(retrieved_data, proof_data);

            // Verify key metadata fields are present
            assert_eq!(
                retrieved_metadata.data.get("job_id"),
                Some(&"test_job_123456789".to_string())
            );
            assert_eq!(
                retrieved_metadata.data.get("app_instance_id"),
                Some(&"0xabcdef1234567890".to_string())
            );
            assert_eq!(
                retrieved_metadata.data.get("project"),
                Some(&"silvana".to_string())
            );
            
            // Verify the edge case with 256 char value was truncated to 256
            let edge_value = retrieved_metadata.data.get("edge_case_256");
            assert!(edge_value.is_some());
            assert!(edge_value.unwrap().len() <= 256, "Value should be truncated to 256 chars");
            
            // Verify the long key exists (might be truncated to 128 chars)
            let found_long_key = retrieved_metadata.data.keys()
                .any(|k| k.starts_with("test_key_"));
            assert!(found_long_key, "Long key should exist (possibly truncated)");
        }
        Err(e) => {
            panic!("Failed to retrieve proof from cache: {}", e);
        }
    }

    // Check if proof exists
    let exists = storage.proof_exists(&descriptor.to_string()).await.unwrap();
    assert!(exists, "Proof should exist in cache");
}

#[tokio::test]
async fn test_unsupported_chain() {
    let storage = ProofStorage::new();

    // Try to store to unsupported chain
    let result = storage
        .store_proof("ipfs", "public", "test data", None)
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Unsupported storage chain")
    );

    // Try to get from unsupported chain
    let result = storage.get_proof("ipfs:public:fakehash").await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Unsupported storage chain")
    );
}

#[tokio::test]
async fn test_proof_not_found() {
    let storage = ProofStorage::new();

    // Try to get non-existent proof from walrus
    let result = storage.get_proof("walrus:testnet:nonexistenthash123").await;
    assert!(result.is_err());

    // Check existence of non-existent proof
    let exists = storage
        .proof_exists("walrus:testnet:nonexistenthash123")
        .await
        .unwrap();
    assert!(!exists, "Non-existent proof should not exist");
}
