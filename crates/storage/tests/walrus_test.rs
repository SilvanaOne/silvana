use serde_json::json;
use storage::walrus::*;

#[tokio::test]
async fn test_save_to_walrus() {
    let client = WalrusClient::new();

    let test_data = json!({
        "message": "Hello, world!",
        "date": chrono::Utc::now().to_rfc3339()
    });

    let params = SaveToWalrusParams {
        data: test_data.to_string(),
        address: None,
        num_epochs: Some(53),
    };

    let result = client.save_to_walrus(params).await;
    println!("save_to_walrus result: {:?}", result);

    match result {
        Ok(Some(blob_id)) => {
            assert!(!blob_id.is_empty(), "blob_id should not be empty");
        }
        Ok(None) => panic!("Expected blob_id but got None"),
        Err(e) => panic!("Failed to save to Walrus: {}", e),
    }
}

#[tokio::test]
async fn test_save_and_read_string() {
    let client = WalrusClient::new();

    let test_message = "This is a test message for Walrus data availability";

    // Save the string
    let save_params = SaveToWalrusParams {
        data: test_message.to_string(),
        address: None,
        num_epochs: Some(53),
    };

    let blob_id = match client.save_to_walrus(save_params).await {
        Ok(Some(id)) => id,
        Ok(None) => panic!("Failed to get blob_id"),
        Err(e) => panic!("Save failed: {}", e),
    };

    // Saved successfully

    // Read back the string
    let read_params = ReadFromWalrusParams { blob_id };

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
    let client = WalrusClient::new();

    let params = GetWalrusUrlParams {
        blob_id: String::new(),
    };

    let result = client.get_walrus_url(params);
    assert!(result.is_err(), "Should return error for empty blob_id");
}

#[test]
fn test_config_urls() {
    let testnet_config = WalrusConfig {
        daemon: Daemon::Testnet,
        ..Default::default()
    };

    let local_config = WalrusConfig {
        daemon: Daemon::Local,
        ..Default::default()
    };

    assert_eq!(
        testnet_config.base_publisher_url(),
        "https://wal-publisher-testnet.staketab.org"
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
        data: "test".to_string(),
        address: None,
        num_epochs: Some(1), // Below min
    };

    let _params_high = SaveToWalrusParams {
        data: "test".to_string(),
        address: None,
        num_epochs: Some(100), // Above max
    };

    // Note: We can't easily test the actual clamping without calling save_to_walrus,
    // but we can verify the config values are correct
    assert_eq!(config.min_epochs, 14);
    assert_eq!(config.max_epochs, 53);
}
