use std::sync::Arc;
use std::sync::OnceLock;
use std::str::FromStr;
use sui_rpc::Client;
use sui_sdk_types as sui;
use sui_crypto::ed25519::Ed25519PrivateKey;
use tracing::info;
use crate::chain::load_sender_from_env;
use anyhow::Result;

// Global static values initialized once from environment variables
static COORDINATOR_ID: OnceLock<String> = OnceLock::new();
static CHAIN: OnceLock<String> = OnceLock::new();
static COORDINATION_PACKAGE_ID: OnceLock<sui::Address> = OnceLock::new();

// Global static SharedSuiState instance
static SHARED_SUI_STATE: OnceLock<Arc<SharedSuiState>> = OnceLock::new();

pub struct SharedSuiState {
    sui_client: Client,  // Sui client (cloneable)
    sui_address: sui::Address,  // Sui address from environment
    sui_private_key: Ed25519PrivateKey,  // Sui private key from environment
    coordination_package_id: sui::Address,  // Coordination package ID from environment
}

impl SharedSuiState {
    /// Initialize the global SharedSuiState instance
    pub async fn initialize(rpc_url: &str) -> Result<()> {
        info!("Initializing SharedSuiState with RPC URL: {}", rpc_url);
        
        // Create Sui client
        let sui_client = Client::new(rpc_url)
            .map_err(|e| anyhow::anyhow!("Failed to create Sui client: {}", e))?;
        
        // Initialize static values from environment variables
        Self::init_coordinator_id();
        Self::init_chain();
        Self::init_coordination_package_id();
        
        // Load sender address and private key from environment
        let (sui_address, sui_private_key) = load_sender_from_env()?;
        
        // Get coordination package ID from environment
        let coordination_package_id = COORDINATION_PACKAGE_ID.get()
            .expect("Coordination package ID should be initialized")
            .clone();
        
        info!("Initialized SharedSuiState with address: {}", sui_address);
        
        let state = Arc::new(Self {
            sui_client,
            sui_address,
            sui_private_key,
            coordination_package_id,
        });
        
        SHARED_SUI_STATE.set(state)
            .map_err(|_| anyhow::anyhow!("SharedSuiState already initialized"))?;
        
        Ok(())
    }
    
    /// Get the global SharedSuiState instance
    pub fn get_instance() -> Arc<SharedSuiState> {
        SHARED_SUI_STATE.get()
            .expect("SharedSuiState not initialized. Call SharedSuiState::initialize() first.")
            .clone()
    }
    
    /// Initialize coordinator ID from SUI_ADDRESS environment variable
    fn init_coordinator_id() {
        COORDINATOR_ID.get_or_init(|| {
            std::env::var("SUI_ADDRESS")
                .expect("SUI_ADDRESS environment variable must be set")
        });
    }
    
    /// Initialize chain from SUI_CHAIN environment variable
    fn init_chain() {
        CHAIN.get_or_init(|| {
            std::env::var("SUI_CHAIN")
                .expect("SUI_CHAIN environment variable must be set")
        });
    }
    
    /// Initialize coordination package ID from SILVANA_REGISTRY_PACKAGE environment variable
    fn init_coordination_package_id() {
        COORDINATION_PACKAGE_ID.get_or_init(|| {
            let package_id_str = std::env::var("SILVANA_REGISTRY_PACKAGE")
                .expect("SILVANA_REGISTRY_PACKAGE environment variable must be set");
            sui::Address::from_str(&package_id_str)
                .expect("Invalid SILVANA_REGISTRY_PACKAGE address format")
        });
    }
    
    /// Get the coordinator ID
    pub fn get_coordinator_id(&self) -> &String {
        COORDINATOR_ID.get()
            .expect("Coordinator ID should be initialized")
    }
    
    /// Get the chain
    pub fn get_chain(&self) -> &String {
        CHAIN.get()
            .expect("Chain should be initialized")
    }
    
    pub(crate) fn get_sui_client(&self) -> Client {
        self.sui_client.clone()
    }
    
    /// Get the Sui address
    pub fn get_sui_address(&self) -> sui::Address {
        self.sui_address
    }
    
    /// Get the Sui private key
    pub(crate) fn get_sui_private_key(&self) -> &Ed25519PrivateKey {
        &self.sui_private_key
    }
    
    /// Get the coordination package ID
    pub fn get_coordination_package_id(&self) -> sui::Address {
        self.coordination_package_id
    }
}