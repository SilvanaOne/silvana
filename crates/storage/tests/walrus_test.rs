use dotenvy::dotenv;
use std::env;
use storage::walrus::*;

#[tokio::test]
async fn test_save_to_walrus() {
    dotenv().ok();
    let client = WalrusClient::new();

    let test_message = "This is a test message for Walrus data availability";

    let params = SaveToWalrusParams {
        data: WalrusData::Blob(test_message.to_string()),
        address: None,
        num_epochs: Some(53),
    };

    let result = client.save_to_walrus(params).await;
    println!("save_to_walrus result: {:?}", result);

    match result {
        Ok(Some(store_result)) => {
            assert!(!store_result.blob_id.is_empty(), "blob_id should not be empty");
            println!("BlobId: {}", store_result.blob_id);
            if let Some(tx) = store_result.tx_digest {
                println!("TxDigest: {}", tx);
            }
        }
        Ok(None) => panic!("Expected store result but got None"),
        Err(e) => panic!("Failed to save to Walrus: {}", e),
    }
}

#[tokio::test]
async fn test_save_and_read_string() {
    dotenv().ok();
    let client = WalrusClient::new();

    let test_message = "This is a test message for Walrus data availability";

    // Save the string
    let save_params = SaveToWalrusParams {
        data: WalrusData::Blob(test_message.to_string()),
        address: None,
        num_epochs: Some(53),
    };

    let store_result = match client.save_to_walrus(save_params).await {
        Ok(Some(result)) => result,
        Ok(None) => panic!("Failed to get store result"),
        Err(e) => panic!("Save failed: {}", e),
    };
    println!("blob_id: {}", store_result.blob_id);
    if let Some(tx) = &store_result.tx_digest {
        println!("tx_digest: {}", tx);
    }

    // Saved successfully

    // Read back the string
    let read_params = ReadFromWalrusParams { 
        blob_id: store_result.blob_id,
        quilt_identifier: None,
    };

    let result = client.read_from_walrus(read_params).await;
    println!("read_from_walrus result: {:?}", result);

    match result {
        Ok(Some(retrieved_data)) => {
            assert_eq!(
                retrieved_data, test_message,
                "Retrieved data should match original"
            );
        }
        Ok(None) => panic!("Expected data but got None"),
        Err(e) => panic!("Failed to read from Walrus: {}", e),
    }
}

#[tokio::test]
async fn test_get_walrus_url() {
    dotenv().ok();
    let client = WalrusClient::new();

    let test_blob_id = "test_blob_id_123";

    let params = GetWalrusUrlParams {
        blob_id: test_blob_id.to_string(),
    };

    let result = client.get_walrus_url(params);

    match result {
        Ok(url) => {
            assert!(url.contains(test_blob_id), "URL should contain blob_id");
            assert!(
                url.starts_with("https://") || url.starts_with("http://"),
                "URL should be valid"
            );
        }
        Err(e) => panic!("Failed to generate URL: {}", e),
    }
}

#[test]
fn test_empty_blob_id_error() {
    dotenv().ok();
    let client = WalrusClient::new();

    let params = GetWalrusUrlParams {
        blob_id: String::new(),
    };

    let result = client.get_walrus_url(params);
    assert!(result.is_err(), "Should return error for empty blob_id");
}

#[test]
fn test_config_urls() {
    dotenv().ok();
    let testnet_config = WalrusConfig {
        daemon: Daemon::Testnet,
        ..Default::default()
    };
    println!(
        "testnet_config: {:?} {}",
        testnet_config,
        testnet_config.base_publisher_url()
    );

    let local_config = WalrusConfig {
        daemon: Daemon::Local,
        ..Default::default()
    };
    println!("local_config: {:?}", local_config);

    assert_eq!(
        testnet_config.base_publisher_url(),
        env::var("WALRUS_PUBLISHER")
            .unwrap_or_else(|_| "https://wal-publisher-testnet.staketab.org".to_string())
    );
    assert_eq!(
        testnet_config.reader_url(),
        "https://wal-aggregator-testnet.staketab.org/v1/blobs/"
    );

    assert_eq!(local_config.base_publisher_url(), "http://127.0.0.1:31415");
    assert_eq!(
        local_config.reader_url(),
        "http://127.0.0.1:31415/v1/blobs/"
    );
}

#[test]
fn test_epoch_clamping() {
    let config = WalrusConfig::default();
    let _client = WalrusClient::with_config(config.clone());

    // Test epoch clamping in save params
    let _params_low = SaveToWalrusParams {
        data: WalrusData::Blob("test".to_string()),
        address: None,
        num_epochs: Some(1), // Below min
    };

    let _params_high = SaveToWalrusParams {
        data: WalrusData::Blob("test".to_string()),
        address: None,
        num_epochs: Some(100), // Above max
    };

    // Note: We can't easily test the actual clamping without calling save_to_walrus,
    // but we can verify the config values are correct
    assert_eq!(config.min_epochs, 2);
    assert_eq!(config.max_epochs, 53);
}

#[tokio::test]
async fn test_save_quilt_to_walrus() {
    dotenv().ok();
    let client = WalrusClient::new();

    // Create a quilt with multiple pieces of data
    let quilt_data = vec![
        ("config".to_string(), r#"{"version": "1.0", "app": "test"}"#.to_string()),
        ("readme".to_string(), "This is a test quilt for Walrus storage".to_string()),
        ("data".to_string(), "Some important data that belongs together".to_string()),
    ];

    let params = SaveToWalrusParams {
        data: WalrusData::Quilt(quilt_data),
        address: None,
        num_epochs: Some(53),
    };

    let result = client.save_to_walrus(params).await;
    println!("save_quilt_to_walrus result: {:?}", result);

    match result {
        Ok(Some(store_result)) => {
            assert!(!store_result.blob_id.is_empty(), "blob_id should not be empty");
            println!("Successfully created quilt with ID: {}", store_result.blob_id);
            
            if let Some(tx) = &store_result.tx_digest {
                println!("Transaction digest: {}", tx);
            }
            
            if let Some(patches) = &store_result.quilt_patches {
                println!("Quilt patches:");
                for patch in patches {
                    println!("  - {}: {}", patch.identifier, patch.quilt_patch_id);
                }
                assert_eq!(patches.len(), 3, "Should have 3 quilt patches");
            }
        }
        Ok(None) => panic!("Expected store result but got None"),
        Err(e) => panic!("Failed to save quilt to Walrus: {}", e),
    }
}

#[tokio::test]
async fn test_read_quilt_piece() {
    dotenv().ok();
    let client = WalrusClient::new();

    // First save a quilt
    let quilt_data = vec![
        ("test-config".to_string(), r#"{"test": "config"}"#.to_string()),
        ("test-data".to_string(), "Test data content".to_string()),
    ];

    let params = SaveToWalrusParams {
        data: WalrusData::Quilt(quilt_data),
        address: None,
        num_epochs: Some(53),
    };

    let store_result = match client.save_to_walrus(params).await {
        Ok(Some(result)) => result,
        Ok(None) => panic!("Failed to save quilt"),
        Err(e) => panic!("Failed to save quilt: {}", e),
    };

    println!("Saved quilt with ID: {}", store_result.blob_id);

    // Try to read a specific quilt piece
    let read_params = ReadFromWalrusParams {
        blob_id: store_result.blob_id.clone(),
        quilt_identifier: Some("test-config".to_string()),
    };

    let result = client.read_from_walrus(read_params).await;
    println!("Read quilt piece result: {:?}", result);
    
    // Verify the content matches what we saved
    match result {
        Ok(Some(content)) => {
            assert_eq!(content, r#"{"test": "config"}"#, "Retrieved quilt piece should match original");
        }
        Ok(None) => panic!("Expected quilt piece content but got None"),
        Err(e) => panic!("Failed to read quilt piece: {}", e),
    }
    
    // Test reading the second piece
    let read_params2 = ReadFromWalrusParams {
        blob_id: store_result.blob_id.clone(),
        quilt_identifier: Some("test-data".to_string()),
    };
    
    let result2 = client.read_from_walrus(read_params2).await;
    println!("Read second quilt piece result: {:?}", result2);
    
    match result2 {
        Ok(Some(content)) => {
            assert_eq!(content, "Test data content", "Second quilt piece should match original");
        }
        Ok(None) => panic!("Expected second quilt piece content but got None"),
        Err(e) => panic!("Failed to read second quilt piece: {}", e),
    }
}
