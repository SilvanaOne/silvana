//! Integration tests for Ethereum Coordination Layer
//!
//! These tests require:
//! 1. Anvil running on localhost:8545
//! 2. Contracts deployed via `make deploy-ethereum-local`
//! 3. Deployment info in `build/ethereum-local-*.json`

use silvana_coordination_ethereum::{EthereumCoordination, EthereumCoordinationConfig};
use silvana_coordination_trait::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{sleep, Duration};

// Counter for unique app instance names
static APP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test configuration loaded from deployment JSON
struct TestFixture {
    coord: EthereumCoordination,
    _config: EthereumCoordinationConfig,
    app_instance: String,
    _admin_address: String,
}

impl TestFixture {
    /// Create a new test fixture with a unique app instance
    async fn new() -> Self {
        let config = load_test_config();
        let coord = EthereumCoordination::new(config.clone())
            .expect("Failed to create EthereumCoordination");

        // Create unique app instance for test isolation
        let counter = APP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let app_instance = format!("test-app-{}", counter);

        let admin_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_string();

        Self {
            coord,
            _config: config,
            app_instance,
            _admin_address: admin_address,
        }
    }

    /// Get coordination instance
    fn coordination(&self) -> &EthereumCoordination {
        &self.coord
    }

    /// Get app instance name
    fn app_instance(&self) -> &str {
        &self.app_instance
    }

    /// Sleep briefly to allow blockchain state to settle
    async fn wait_for_block(&self) {
        sleep(Duration::from_millis(200)).await;
    }
}

/// Load test configuration from deployment JSON
fn load_test_config() -> EthereumCoordinationConfig {
    // Find latest deployment JSON
    let build_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("build");

    let mut deployment_files: Vec<_> = std::fs::read_dir(&build_dir)
        .expect("build/ directory not found - run `make deploy-ethereum-local` first")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("ethereum-local-") && s.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();

    deployment_files.sort_by_key(|e| e.metadata().unwrap().modified().unwrap());

    let deployment_file = deployment_files
        .last()
        .expect("No deployment JSON found - run `make deploy-ethereum-local` first");

    let deployment_json = std::fs::read_to_string(deployment_file.path())
        .expect("Failed to read deployment JSON");

    let deployment: serde_json::Value =
        serde_json::from_str(&deployment_json).expect("Failed to parse deployment JSON");

    // Extract contract address (SilvanaCoordination is the main entry point)
    let contract_address = deployment["contracts"]["SilvanaCoordination"]
        .as_str()
        .expect("SilvanaCoordination address not found");

    // Use Anvil's first funded account
    let private_key = Some("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string());

    EthereumCoordinationConfig {
        rpc_url: "http://localhost:8545".to_string(),
        chain_id: 31337,
        contract_address: contract_address.to_string(),
        private_key,
        multicall_enabled: false, // Per user directive: no multicall
        multicall_interval_secs: 1,
        gas_limit_multiplier: 1.2,
        max_gas_price_gwei: None,
        confirmation_blocks: 1,
    }
}

// ============================================================================
// Configuration & Initialization Tests
// ============================================================================

#[test]
fn test_config_validation_valid() {
    let config = EthereumCoordinationConfig {
        rpc_url: "http://localhost:8545".to_string(),
        chain_id: 31337,
        contract_address: "0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00".to_string(),
        private_key: Some("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string()),
        multicall_enabled: false,
        multicall_interval_secs: 1,
        gas_limit_multiplier: 1.2,
        max_gas_price_gwei: None,
        confirmation_blocks: 1,
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_address() {
    let config = EthereumCoordinationConfig {
        rpc_url: "http://localhost:8545".to_string(),
        chain_id: 31337,
        contract_address: "0xinvalid".to_string(), // Invalid address
        private_key: None,
        multicall_enabled: false,
        multicall_interval_secs: 1,
        gas_limit_multiplier: 1.2,
        max_gas_price_gwei: None,
        confirmation_blocks: 1,
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_invalid_rpc() {
    let config = EthereumCoordinationConfig {
        rpc_url: "invalid-url".to_string(), // Invalid RPC URL
        chain_id: 31337,
        contract_address: "0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00".to_string(),
        private_key: None,
        multicall_enabled: false,
        multicall_interval_secs: 1,
        gas_limit_multiplier: 1.2,
        max_gas_price_gwei: None,
        confirmation_blocks: 1,
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_can_write() {
    let mut config = load_test_config();
    assert!(config.can_write()); // Has private key

    config.private_key = None;
    assert!(!config.can_write()); // No private key
}

#[tokio::test]
async fn test_coordination_creation() {
    let config = load_test_config();
    let coord = EthereumCoordination::new(config);
    assert!(coord.is_ok());

    let coord = coord.unwrap();
    assert_eq!(coord.chain_id(), "31337");
    assert_eq!(coord.coordination_layer(), CoordinationLayer::Ethereum);
}

// ============================================================================
// Job Management - Read Operations
// ============================================================================

#[tokio::test]
async fn test_fetch_pending_jobs() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Test that fetch_pending_jobs works without error
    // Note: May return jobs from other tests running on same Anvil instance
    let result = coord
        .fetch_pending_jobs(fixture.app_instance())
        .await;

    assert!(result.is_ok(), "fetch_pending_jobs should not error");
}

#[tokio::test]
#[ignore] // Requires app instance to be registered first
async fn test_fetch_job_by_id_not_found() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Use correct job ID format: app-instance-sequence (with dash, not colon)
    let job_id = format!("{}-1", fixture.app_instance());
    let job = coord
        .fetch_job_by_id(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to fetch job");

    assert!(job.is_none(), "Job should not exist");
}

#[tokio::test]
async fn test_fetch_sequence_state_not_found() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let state = coord
        .fetch_sequence_state(fixture.app_instance(), 999)
        .await
        .expect("Failed to fetch sequence state");

    assert!(state.is_none(), "Non-existent sequence should return None");
}

#[tokio::test]
async fn test_fetch_sequence_states_range_empty() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let states = coord
        .fetch_sequence_states_range(fixture.app_instance(), 1, 10)
        .await
        .expect("Failed to fetch sequence states range");

    assert_eq!(states.len(), 0, "Should have no sequences initially");
}

// ============================================================================
// Job Management - Write Operations (Sequential)
// ============================================================================

#[tokio::test]
#[ignore] // Requires app instance to be registered via SilvanaCoordination.registerApp()
async fn test_create_and_fetch_job() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Note: This test requires the app instance to be registered first
    // via SilvanaCoordination.registerApp() which is not exposed in the coordination trait

    // Create a job
    let (job_id, job_sequence) = coord
        .create_app_job(
            fixture.app_instance(),
            "test_method".to_string(),
            Some("Test job description".to_string()),
            None,
            None,
            None,
            None,
            vec![1, 2, 3, 4],
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create job");

    fixture.wait_for_block().await;

    assert!(job_sequence > 0, "Job sequence should be positive");
    assert!(job_id.contains(fixture.app_instance()), "Job ID should contain app instance");

    // Fetch the created job
    let job = coord
        .fetch_job_by_id(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to fetch job")
        .expect("Job should exist");

    assert_eq!(job.id, job_id);
    assert_eq!(job.job_sequence, job_sequence);
    assert_eq!(job.app_instance, fixture.app_instance());
    assert_eq!(job.data, vec![1, 2, 3, 4]);
    assert_eq!(job.status, JobStatus::Pending);
}

#[tokio::test]
async fn test_job_lifecycle_complete() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Create job
    let (job_id, _) = coord
        .create_app_job(
            fixture.app_instance(),
            "lifecycle_test".to_string(),
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create job");

    fixture.wait_for_block().await;

    // Start job
    let started = coord
        .start_job(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to start job");
    assert!(started, "Job should start successfully");

    fixture.wait_for_block().await;

    // Complete job
    let tx_hash = coord
        .complete_job(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to complete job");

    assert!(tx_hash.starts_with("0x"), "Should return transaction hash");
}

#[tokio::test]
async fn test_job_lifecycle_fail() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Create job
    let (job_id, _) = coord
        .create_app_job(
            fixture.app_instance(),
            "fail_test".to_string(),
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create job");

    fixture.wait_for_block().await;

    // Start job
    coord
        .start_job(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to start job");

    fixture.wait_for_block().await;

    // Fail job
    let tx_hash = coord
        .fail_job(fixture.app_instance(), &job_id, "Test error message")
        .await
        .expect("Failed to fail job");

    assert!(tx_hash.starts_with("0x"), "Should return transaction hash");
}

#[tokio::test]
#[ignore] // Requires app instance to be registered via SilvanaCoordination.registerApp()
async fn test_terminate_job() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Create job
    let (job_id, _) = coord
        .create_app_job(
            fixture.app_instance(),
            "terminate_test".to_string(),
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create job");

    fixture.wait_for_block().await;

    // Terminate job
    let tx_hash = coord
        .terminate_job(fixture.app_instance(), &job_id)
        .await
        .expect("Failed to terminate job");

    assert!(tx_hash.starts_with("0x"), "Should return transaction hash");
}

// ============================================================================
// Key-Value Storage Tests
// ============================================================================

#[tokio::test]
async fn test_kv_string_operations() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Get non-existent key
    let value = coord
        .get_kv_string(fixture.app_instance(), "test_key")
        .await
        .expect("Failed to get KV string");
    assert!(value.is_none(), "Key should not exist initially");

    // Set key
    let tx_hash = coord
        .set_kv_string(
            fixture.app_instance(),
            "test_key".to_string(),
            "test_value".to_string(),
        )
        .await
        .expect("Failed to set KV string");
    assert!(tx_hash.starts_with("0x"));

    fixture.wait_for_block().await;

    // Get key
    let value = coord
        .get_kv_string(fixture.app_instance(), "test_key")
        .await
        .expect("Failed to get KV string")
        .expect("Key should exist");
    assert_eq!(value, "test_value");

    // Delete key
    let tx_hash = coord
        .delete_kv_string(fixture.app_instance(), "test_key")
        .await
        .expect("Failed to delete KV string");
    assert!(tx_hash.starts_with("0x"));

    fixture.wait_for_block().await;

    // Verify deleted
    let value = coord
        .get_kv_string(fixture.app_instance(), "test_key")
        .await
        .expect("Failed to get KV string");
    assert!(value.is_none(), "Key should be deleted");
}

#[tokio::test]
async fn test_kv_binary_operations() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let key = b"binary_key";
    let value = vec![1, 2, 3, 4, 5];

    // Get non-existent key
    let result = coord
        .get_kv_binary(fixture.app_instance(), key)
        .await
        .expect("Failed to get KV binary");
    assert!(result.is_none(), "Key should not exist initially");

    // Set key
    let tx_hash = coord
        .set_kv_binary(fixture.app_instance(), key.to_vec(), value.clone())
        .await
        .expect("Failed to set KV binary");
    assert!(tx_hash.starts_with("0x"));

    fixture.wait_for_block().await;

    // Get key
    let result = coord
        .get_kv_binary(fixture.app_instance(), key)
        .await
        .expect("Failed to get KV binary")
        .expect("Key should exist");
    assert_eq!(result, value);

    // Delete key
    let tx_hash = coord
        .delete_kv_binary(fixture.app_instance(), key)
        .await
        .expect("Failed to delete KV binary");
    assert!(tx_hash.starts_with("0x"));

    fixture.wait_for_block().await;

    // Verify deleted
    let result = coord
        .get_kv_binary(fixture.app_instance(), key)
        .await
        .expect("Failed to get KV binary");
    assert!(result.is_none(), "Key should be deleted");
}

#[tokio::test]
async fn test_get_all_kv_string() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Set multiple keys
    coord
        .set_kv_string(fixture.app_instance(), "key1".to_string(), "value1".to_string())
        .await
        .expect("Failed to set key1");

    fixture.wait_for_block().await;

    coord
        .set_kv_string(fixture.app_instance(), "key2".to_string(), "value2".to_string())
        .await
        .expect("Failed to set key2");

    fixture.wait_for_block().await;

    // Get all keys
    let all_kvs = coord
        .get_all_kv_string(fixture.app_instance())
        .await
        .expect("Failed to get all KV strings");

    assert!(all_kvs.len() >= 2, "Should have at least 2 keys");
    assert_eq!(all_kvs.get("key1"), Some(&"value1".to_string()));
    assert_eq!(all_kvs.get("key2"), Some(&"value2".to_string()));
}

// ============================================================================
// Metadata Tests
// ============================================================================

#[tokio::test]
async fn test_metadata_operations() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    // Use unique key with timestamp to avoid collision with other tests
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique_key = format!("meta_key_{}_{}", APP_COUNTER.fetch_add(1, Ordering::SeqCst), timestamp);

    // Add metadata (don't check if exists first since other tests may have created keys)
    let tx_hash = coord
        .add_metadata(
            fixture.app_instance(),
            unique_key.clone(),
            "meta_value".to_string(),
        )
        .await
        .expect("Failed to add metadata");
    assert!(tx_hash.starts_with("0x"));

    fixture.wait_for_block().await;

    // Get metadata - should exist after we just set it
    let value = coord
        .get_metadata(fixture.app_instance(), &unique_key)
        .await
        .expect("Failed to get metadata")
        .expect("Metadata should exist after setting");
    assert_eq!(value, "meta_value");
}

// ============================================================================
// Unsupported Operations Tests
// ============================================================================

#[tokio::test]
async fn test_multicall_not_supported() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    assert!(!coord.supports_multicall(), "Multicall should not be supported");

    let result = coord.multicall_job_operations(vec![]).await;
    assert!(result.is_err(), "Multicall should return error");
}

#[tokio::test]
async fn test_remove_failed_jobs_not_supported() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let result = coord.remove_failed_jobs(fixture.app_instance(), None).await;
    assert!(result.is_err(), "remove_failed_jobs should not be supported");
}

#[tokio::test]
async fn test_list_kv_keys_not_supported() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let result = coord
        .list_kv_string_keys(fixture.app_instance(), None, None)
        .await;
    assert!(result.is_err(), "list_kv_string_keys should not be supported");

    let result = coord
        .list_kv_binary_keys(fixture.app_instance(), None, None)
        .await;
    assert!(result.is_err(), "list_kv_binary_keys should not be supported");
}

#[tokio::test]
async fn test_purge_not_supported() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let result = coord.purge(fixture.app_instance(), 10).await;
    assert!(result.is_err(), "purge should not be supported yet");
}

#[tokio::test]
async fn test_try_create_block_not_supported() {
    let fixture = TestFixture::new().await;
    let coord = fixture.coordination();

    let result = coord.try_create_block(fixture.app_instance()).await;
    assert!(result.is_err(), "try_create_block should not be supported");
}
