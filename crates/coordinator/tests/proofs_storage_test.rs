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
            env::var("TEST_SERVER").unwrap_or_else(|_| "https://rpc.silvana.dev".to_string()),
        );
    }

    let storage = ProofStorage::new();

    // Test data
    let proof_data = "Test proof data for cache storage";
    let mut metadata = ProofMetadata::default();
    metadata
        .data
        .insert("test_key".to_string(), "test_value".to_string());
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

            // Note: expires_at comes back as a metadata field from S3
            // Check user metadata
            assert_eq!(
                retrieved_metadata.data.get("test_key"),
                Some(&"test_value".to_string())
            );
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
