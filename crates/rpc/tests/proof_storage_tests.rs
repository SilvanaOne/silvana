// Proof Storage Integration Tests
//
// These tests verify the proof storage functionality via gRPC.
// 
// Prerequisites:
// 1. Start the RPC server with proof storage enabled:
//    PROOFS_CACHE_BUCKET=proofs-cache cargo run -p rpc
//
// 2. Or set TEST_SERVER environment variable to point to a running server:
//    TEST_SERVER=https://rpc.silvana.dev:443 cargo test -p rpc --test proof_storage_tests
//
// The tests will gracefully skip if the server is not available.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};
use tonic::Request;

use proto::silvana_rpc_service_client::SilvanaRpcServiceClient;
use proto::*;

// Configuration
const TEST_PROOF_DATA: &str = r#"{
    "proof": "0x1234567890abcdef",
    "public_inputs": ["0x01", "0x02"],
    "verification_key": "vk_12345"
}"#;

fn get_server_addr() -> String {
    // Load .env file if it exists
    dotenvy::dotenv().ok();
    
    // Get TEST_SERVER from environment, fallback to default if not set
    std::env::var("TEST_SERVER").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string())
}

fn get_current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::test]
async fn test_submit_and_get_proof() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    let server_addr = get_server_addr();
    
    println!("🧪 Starting proof storage integration test...");
    println!("🎯 Server address: {}", server_addr);
    
    // Connect to the gRPC server
    let mut client = match SilvanaRpcServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            println!("❌ Failed to connect to RPC server at {}: {}", server_addr, e);
            println!("⚠️  Skipping proof storage tests - server not available");
            println!("   Make sure the server is running with: cargo run");
            println!("   And that PROOFS_CACHE_BUCKET is configured");
            return;
        }
    };
    
    // Test 1: Submit proof with metadata
    println!("\n📝 Test 1: Submit proof with metadata");
    
    let mut metadata = HashMap::new();
    metadata.insert("test_id".to_string(), "test_123".to_string());
    metadata.insert("environment".to_string(), "integration_test".to_string());
    metadata.insert("version".to_string(), "1.0.0".to_string());
    
    let expires_at = get_current_timestamp_ms() + (24 * 60 * 60 * 1000); // 24 hours from now
    
    let submit_request = Request::new(SubmitProofRequest {
        data: Some(proto::submit_proof_request::Data::ProofData(TEST_PROOF_DATA.to_string())),
        metadata: metadata.clone(),
        expires_at: Some(expires_at),
    });
    
    let proof_hash = match client.submit_proof(submit_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Proof submitted successfully");
            println!("  📋 Proof hash: {}", resp.proof_hash);
            assert!(!resp.proof_hash.is_empty(), "Proof hash should not be empty");
            resp.proof_hash
        }
        Err(e) => {
            if e.code() == tonic::Code::Unimplemented {
                println!("  ⚠️  Proof storage not implemented on server");
                println!("     Make sure PROOFS_CACHE_BUCKET is configured");
                return;
            }
            panic!("❌ Failed to submit proof: {}", e);
        }
    };
    
    // Test 2: Retrieve the proof
    println!("\n🔍 Test 2: Retrieve submitted proof");
    
    let get_request = Request::new(GetProofRequest {
        proof_hash: proof_hash.clone(),
        block_number: None,
    });
    
    match client.get_proof(get_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Proof retrieved successfully");
            
            // Verify the proof data matches
            assert_eq!(
                resp.proof_data, TEST_PROOF_DATA,
                "Retrieved proof data doesn't match submitted data"
            );
            
            // Verify metadata matches (note: S3 adds expires_at as a tag)
            // So we expect one extra metadata field
            assert_eq!(
                resp.metadata.len(),
                metadata.len() + 1, // +1 for expires_at tag
                "Metadata count mismatch (expected {} user fields + 1 expires_at)",
                metadata.len()
            );
            
            for (key, expected_value) in metadata.iter() {
                let actual_value = resp.metadata.get(key);
                assert!(
                    actual_value.is_some(),
                    "Missing metadata key: {}",
                    key
                );
                assert_eq!(
                    actual_value.unwrap(),
                    expected_value,
                    "Metadata value mismatch for key: {}",
                    key
                );
            }
            
            // Verify expires_at tag is present
            assert!(
                resp.metadata.contains_key("expires_at"),
                "Missing expires_at tag in metadata"
            );
            
            println!("  📊 Metadata verified: {} entries", resp.metadata.len());
            // Note: expires_at is stored in S3 tags but not returned in the response
        }
        Err(e) => {
            panic!("❌ Failed to retrieve proof: {}", e);
        }
    }
    
    // Test 3: Submit proof without metadata
    println!("\n📝 Test 3: Submit proof without metadata");
    
    let submit_request = Request::new(SubmitProofRequest {
        data: Some(proto::submit_proof_request::Data::ProofData("minimal_proof_data".to_string())),
        metadata: HashMap::new(),
        expires_at: None, // Use default expiration
    });
    
    let minimal_proof_hash = match client.submit_proof(submit_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Minimal proof submitted successfully");
            println!("  📋 Proof hash: {}", resp.proof_hash);
            resp.proof_hash
        }
        Err(e) => {
            panic!("❌ Failed to submit minimal proof: {}", e);
        }
    };
    
    // Test 4: Retrieve minimal proof
    println!("\n🔍 Test 4: Retrieve minimal proof");
    
    let get_request = Request::new(GetProofRequest {
        proof_hash: minimal_proof_hash,
        block_number: None,
    });
    
    match client.get_proof(get_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Minimal proof retrieved successfully");
            assert_eq!(resp.proof_data, "minimal_proof_data");
            // Should only have expires_at tag, no user metadata
            assert_eq!(resp.metadata.len(), 1, "Should only have expires_at tag");
            assert!(resp.metadata.contains_key("expires_at"), "Should have expires_at tag");
            
            // Note: Default expiration is applied but not returned in the response
            println!("  ✅ Minimal proof has only expires_at metadata as expected");
        }
        Err(e) => {
            panic!("❌ Failed to retrieve minimal proof: {}", e);
        }
    }
    
    // Test 5: Try to get non-existent proof
    println!("\n🔍 Test 5: Try to retrieve non-existent proof");
    
    let get_request = Request::new(GetProofRequest {
        proof_hash: "non_existent_hash_12345".to_string(),
        block_number: None,
    });
    
    match client.get_proof(get_request).await {
        Ok(_) => {
            panic!("❌ Should have failed to retrieve non-existent proof");
        }
        Err(e) => {
            println!("  ✅ Correctly failed to retrieve non-existent proof");
            println!("  📋 Error: {}", e.message());
            // Server might return Internal or NotFound for non-existent proofs
            assert!(
                e.code() == tonic::Code::NotFound || e.code() == tonic::Code::Internal,
                "Should return NotFound or Internal for non-existent proof, got: {:?}",
                e.code()
            );
        }
    }
    
    println!("\n🎉 All proof storage tests passed successfully!");
}

#[tokio::test]
async fn test_proof_storage_concurrent() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    let server_addr = get_server_addr();
    
    println!("🧪 Starting concurrent proof storage test...");
    println!("🎯 Server address: {}", server_addr);
    
    // Connect to the gRPC server
    let client = match SilvanaRpcServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            println!("❌ Failed to connect to RPC server at {}: {}", server_addr, e);
            println!("⚠️  Skipping concurrent proof storage tests");
            return;
        }
    };
    
    // Test concurrent submissions
    const NUM_CONCURRENT: usize = 10;
    println!("\n🚀 Submitting {} proofs concurrently...", NUM_CONCURRENT);
    
    let mut handles = Vec::new();
    
    for i in 0..NUM_CONCURRENT {
        let mut client_clone = client.clone();
        
        let handle = tokio::spawn(async move {
            let proof_data = format!("concurrent_proof_{}", i);
            let mut metadata = HashMap::new();
            metadata.insert("index".to_string(), i.to_string());
            metadata.insert("test".to_string(), "concurrent".to_string());
            
            let submit_request = Request::new(SubmitProofRequest {
                data: Some(proto::submit_proof_request::Data::ProofData(proof_data.clone())),
                metadata: metadata.clone(),
                expires_at: None,
            });
            
            match client_clone.submit_proof(submit_request).await {
                Ok(response) => {
                    let proof_hash = response.into_inner().proof_hash;
                    println!("  ✅ Proof {} submitted, hash: {}", i, proof_hash);
                    Ok((i, proof_hash, proof_data, metadata))
                }
                Err(e) => {
                    if e.code() == tonic::Code::Unimplemented {
                        println!("  ⚠️  Proof storage not implemented");
                        Err("not_implemented".to_string())
                    } else {
                        Err(format!("Failed to submit proof {}: {}", i, e))
                    }
                }
            }
        });
        
        handles.push(handle);
    }
    
    // Collect results
    let mut submitted_proofs = Vec::new();
    for handle in handles {
        match handle.await.unwrap() {
            Ok(proof_info) => submitted_proofs.push(proof_info),
            Err(e) if e == "not_implemented" => {
                println!("⚠️  Proof storage not implemented, skipping concurrent tests");
                return;
            }
            Err(e) => panic!("❌ {}", e),
        }
    }
    
    println!("\n✅ All {} proofs submitted successfully", NUM_CONCURRENT);
    
    // Small delay to ensure S3 consistency
    sleep(Duration::from_millis(500)).await;
    
    // Now retrieve all proofs concurrently
    println!("\n🔍 Retrieving {} proofs concurrently...", NUM_CONCURRENT);
    
    let mut retrieve_handles = Vec::new();
    
    for (index, proof_hash, expected_data, expected_metadata) in submitted_proofs {
        let mut client_clone = client.clone();
        
        let handle = tokio::spawn(async move {
            let get_request = Request::new(GetProofRequest {
                proof_hash: proof_hash.clone(),
                block_number: None,
            });
            
            match client_clone.get_proof(get_request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    
                    // Verify data matches
                    assert_eq!(
                        resp.proof_data, expected_data,
                        "Proof {} data mismatch",
                        index
                    );
                    
                    // Verify metadata matches (accounting for expires_at tag)
                    assert_eq!(
                        resp.metadata.len(),
                        expected_metadata.len() + 1, // +1 for expires_at
                        "Proof {} metadata count mismatch",
                        index
                    );
                    
                    for (key, expected_value) in expected_metadata.iter() {
                        let actual_value = resp.metadata.get(key);
                        assert!(
                            actual_value.is_some(),
                            "Proof {} missing metadata key: {}",
                            index,
                            key
                        );
                        assert_eq!(
                            actual_value.unwrap(),
                            expected_value,
                            "Proof {} metadata mismatch for key: {}",
                            index,
                            key
                        );
                    }
                    
                    println!("  ✅ Proof {} retrieved and verified", index);
                    Ok(())
                }
                Err(e) => Err(format!("Failed to retrieve proof {}: {}", index, e))
            }
        });
        
        retrieve_handles.push(handle);
    }
    
    // Wait for all retrievals to complete
    for handle in retrieve_handles {
        if let Err(e) = handle.await.unwrap() {
            panic!("❌ {}", e);
        }
    }
    
    println!("\n✅ All {} proofs retrieved and verified successfully", NUM_CONCURRENT);
    println!("🎉 Concurrent proof storage test completed successfully!");
}

#[tokio::test]
async fn test_proof_storage_large_data() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    let server_addr = get_server_addr();
    
    println!("🧪 Starting large proof data test...");
    println!("🎯 Server address: {}", server_addr);
    
    // Connect to the gRPC server
    let mut client = match SilvanaRpcServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            println!("❌ Failed to connect to RPC server at {}: {}", server_addr, e);
            println!("⚠️  Skipping large data tests");
            return;
        }
    };
    
    // Create a large proof (1MB of data)
    let large_proof_data = "x".repeat(1024 * 1024); // 1MB
    println!("\n📦 Submitting large proof ({}KB)...", large_proof_data.len() / 1024);
    
    let mut metadata = HashMap::new();
    metadata.insert("size".to_string(), large_proof_data.len().to_string());
    metadata.insert("type".to_string(), "large_test".to_string());
    
    let submit_request = Request::new(SubmitProofRequest {
        data: Some(proto::submit_proof_request::Data::ProofData(large_proof_data.clone())),
        metadata: metadata.clone(),
        expires_at: None,
    });
    
    let proof_hash = match client.submit_proof(submit_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Large proof submitted successfully");
            println!("  📋 Proof hash: {}", resp.proof_hash);
            resp.proof_hash
        }
        Err(e) => {
            if e.code() == tonic::Code::Unimplemented {
                println!("  ⚠️  Proof storage not implemented");
                return;
            }
            panic!("❌ Failed to submit large proof: {}", e);
        }
    };
    
    // Retrieve the large proof
    println!("\n🔍 Retrieving large proof...");
    
    let get_request = Request::new(GetProofRequest {
        proof_hash: proof_hash.clone(),
        block_number: None,
    });
    
    match client.get_proof(get_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Large proof retrieved successfully");
            
            // Verify size matches
            assert_eq!(
                resp.proof_data.len(),
                large_proof_data.len(),
                "Large proof size mismatch"
            );
            
            // Verify content matches (just check first and last parts to avoid full comparison)
            assert_eq!(
                &resp.proof_data[..100],
                &large_proof_data[..100],
                "Large proof start mismatch"
            );
            assert_eq!(
                &resp.proof_data[resp.proof_data.len() - 100..],
                &large_proof_data[large_proof_data.len() - 100..],
                "Large proof end mismatch"
            );
            
            println!("  📊 Large proof verified: {}KB", resp.proof_data.len() / 1024);
        }
        Err(e) => {
            panic!("❌ Failed to retrieve large proof: {}", e);
        }
    }
    
    println!("\n🎉 Large proof data test completed successfully!");
}

#[tokio::test]
async fn test_proof_with_special_characters() {
    // Initialize Rustls crypto provider for HTTPS connections
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    
    let server_addr = get_server_addr();
    
    println!("🧪 Starting special characters proof test...");
    println!("🎯 Server address: {}", server_addr);
    
    // Connect to the gRPC server
    let mut client = match SilvanaRpcServiceClient::connect(server_addr.clone()).await {
        Ok(client) => {
            println!("✅ Connected to RPC server successfully");
            client
        }
        Err(e) => {
            println!("❌ Failed to connect to RPC server at {}: {}", server_addr, e);
            println!("⚠️  Skipping special characters tests");
            return;
        }
    };
    
    // Test with various special characters in proof data
    // Note: S3 tags have restrictions, so we keep metadata simple
    let special_proof_data = r#"{
        "unicode": "Hello 世界 🚀 Émojis",
        "symbols": "!@#$%^&*()_+-=[]{}|;':\",./<>?",
        "newlines": "Line1\nLine2\rLine3\r\nLine4",
        "tabs": "Col1\tCol2\tCol3",
        "escaped": "Quote: \" Backslash: \\ Null: \0"
    }"#;
    
    println!("\n📝 Submitting proof with special characters in data...");
    
    let mut metadata = HashMap::new();
    metadata.insert("test_type".to_string(), "special_chars".to_string());
    // S3 tags don't support unicode well, use simple ASCII for metadata
    metadata.insert("test_note".to_string(), "proof_with_special_chars_in_data".to_string());
    
    let submit_request = Request::new(SubmitProofRequest {
        data: Some(proto::submit_proof_request::Data::ProofData(special_proof_data.to_string())),
        metadata: metadata.clone(),
        expires_at: None,
    });
    
    let proof_hash = match client.submit_proof(submit_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Special characters proof submitted successfully");
            println!("  📋 Proof hash: {}", resp.proof_hash);
            resp.proof_hash
        }
        Err(e) => {
            if e.code() == tonic::Code::Unimplemented {
                println!("  ⚠️  Proof storage not implemented");
                return;
            }
            panic!("❌ Failed to submit special characters proof: {}", e);
        }
    };
    
    // Retrieve and verify
    println!("\n🔍 Retrieving proof with special characters...");
    
    let get_request = Request::new(GetProofRequest {
        proof_hash,
        block_number: None,
    });
    
    match client.get_proof(get_request).await {
        Ok(response) => {
            let resp = response.into_inner();
            println!("  ✅ Special characters proof retrieved successfully");
            
            // Verify exact match including all special characters
            assert_eq!(
                resp.proof_data, special_proof_data,
                "Special characters proof data mismatch"
            );
            
            // Verify metadata
            assert_eq!(
                resp.metadata.get("test_note").unwrap(),
                "proof_with_special_chars_in_data",
                "Metadata mismatch"
            );
            
            println!("  ✅ All special characters preserved correctly");
        }
        Err(e) => {
            panic!("❌ Failed to retrieve special characters proof: {}", e);
        }
    }
    
    println!("\n🎉 Special characters test completed successfully!");
}