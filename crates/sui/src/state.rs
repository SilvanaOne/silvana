use std::sync::Arc;
use std::sync::OnceLock;
use std::str::FromStr;
use sui_rpc::Client;
use sui_sdk_types as sui;
use sui_crypto::ed25519::Ed25519PrivateKey;
use tracing::info;
use crate::chain::load_sender_from_env_or_key;
use anyhow::Result;
use tokio::sync::Mutex;

// Global static values initialized once from environment variables
static COORDINATOR_ID: OnceLock<String> = OnceLock::new();
static CHAIN: OnceLock<String> = OnceLock::new();
static COORDINATION_PACKAGE_ID: OnceLock<sui::Address> = OnceLock::new();

// Global static SharedSuiState instance with initialization lock
static SHARED_SUI_STATE: OnceLock<Arc<SharedSuiState>> = OnceLock::new();
static INIT_LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

pub struct SharedSuiState {
    sui_client: Client,  // Sui client (cloneable)
    sui_address: Option<sui::Address>,  // Sui address from environment (optional for read-only)
    sui_private_key: Option<Ed25519PrivateKey>,  // Sui private key from environment (optional for read-only)
    coordination_package_id: Option<sui::Address>,  // Coordination package ID from environment (optional for read-only)
}

impl SharedSuiState {
    /// Check if SharedSuiState is already initialized
    pub fn is_initialized() -> bool {
        SHARED_SUI_STATE.get().is_some()
    }
    
    /// Initialize the global SharedSuiState instance for read-only operations
    /// This version doesn't require SUI_ADDRESS or SUI_SECRET_KEY
    pub async fn initialize_read_only(rpc_url: &str) -> Result<()> {
        // Return early if already initialized
        if Self::is_initialized() {
            return Ok(());
        }
        
        // Get or create initialization lock
        let init_lock = INIT_LOCK.get_or_init(|| Arc::new(Mutex::new(())));
        
        // Acquire lock to ensure only one thread initializes at a time
        let _guard = init_lock.lock().await;
        
        // Double-check after acquiring lock (another thread might have initialized)
        if Self::is_initialized() {
            return Ok(());
        }
        
        info!("Initializing SharedSuiState (read-only mode) with RPC URL: {}", rpc_url);
        
        // Create Sui client
        let sui_client = Client::new(rpc_url)
            .map_err(|e| anyhow::anyhow!("Failed to create Sui client: {}", e))?;
        
        // Initialize optional static values (don't fail if not present)
        Self::try_init_coordinator_id();
        Self::try_init_chain();
        Self::try_init_coordination_package_id();
        
        info!("Initialized SharedSuiState in read-only mode");
        
        let state = Arc::new(Self {
            sui_client,
            sui_address: None,
            sui_private_key: None,
            coordination_package_id: COORDINATION_PACKAGE_ID.get().cloned(),
        });
        
        // Set the state - this should succeed since we checked twice under lock
        SHARED_SUI_STATE.set(state)
            .map_err(|_| anyhow::anyhow!("Failed to set SharedSuiState - this should not happen"))?;
        
        Ok(())
    }
    
    /// Initialize the global SharedSuiState instance
    /// If private_key_str is provided, it will be used instead of loading from environment
    pub async fn initialize(rpc_url: &str) -> Result<()> {
        Self::initialize_with_optional_key(rpc_url, None).await
    }
    
    /// Initialize the global SharedSuiState instance with an optional private key
    pub async fn initialize_with_optional_key(rpc_url: &str, private_key_str: Option<&str>) -> Result<()> {
        // Return early if already initialized
        if Self::is_initialized() {
            return Ok(());
        }
        
        // Get or create initialization lock
        let init_lock = INIT_LOCK.get_or_init(|| Arc::new(Mutex::new(())));
        
        // Acquire lock to ensure only one thread initializes at a time
        let _guard = init_lock.lock().await;
        
        // Double-check after acquiring lock (another thread might have initialized)
        if Self::is_initialized() {
            return Ok(());
        }
        
        info!("Initializing SharedSuiState with RPC URL: {}", rpc_url);
        
        // Create Sui client
        let sui_client = Client::new(rpc_url)
            .map_err(|e| anyhow::anyhow!("Failed to create Sui client: {}", e))?;
        
        // Initialize static values from environment variables
        Self::init_coordinator_id();
        Self::init_chain();
        Self::init_coordination_package_id();
        
        // Load sender address and private key - use provided key or fall back to environment
        let (sui_address, sui_private_key) = load_sender_from_env_or_key(
            private_key_str.map(|s| s.to_string())
        )?;
        
        // Get coordination package ID from environment
        let coordination_package_id = COORDINATION_PACKAGE_ID.get()
            .expect("Coordination package ID should be initialized")
            .clone();
        
        info!("Initialized SharedSuiState with address: {}", sui_address);
        
        let state = Arc::new(Self {
            sui_client,
            sui_address: Some(sui_address),
            sui_private_key: Some(sui_private_key),
            coordination_package_id: Some(coordination_package_id),
        });
        
        // Set the state - this should succeed since we checked twice under lock
        SHARED_SUI_STATE.set(state)
            .map_err(|_| anyhow::anyhow!("Failed to set SharedSuiState - this should not happen"))?;
        
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
            match std::env::var("SUI_ADDRESS") {
                Ok(addr) => addr,
                Err(_) => {
                    eprintln!("Error: SUI_ADDRESS environment variable must be set");
                    std::process::exit(1);
                }
            }
        });
    }
    
    /// Try to initialize coordinator ID from SUI_ADDRESS environment variable (don't fail)
    fn try_init_coordinator_id() {
        if let Ok(addr) = std::env::var("SUI_ADDRESS") {
            COORDINATOR_ID.get_or_init(|| addr);
        }
    }
    
    /// Initialize chain from SUI_CHAIN environment variable
    fn init_chain() {
        CHAIN.get_or_init(|| {
            match std::env::var("SUI_CHAIN") {
                Ok(chain) => chain,
                Err(_) => {
                    eprintln!("Error: SUI_CHAIN environment variable must be set");
                    std::process::exit(1);
                }
            }
        });
    }
    
    /// Try to initialize chain from SUI_CHAIN environment variable (default to devnet)
    fn try_init_chain() {
        CHAIN.get_or_init(|| {
            std::env::var("SUI_CHAIN")
                .unwrap_or_else(|_| "devnet".to_string())
        });
    }
    
    /// Initialize coordination package ID from SILVANA_REGISTRY_PACKAGE environment variable
    fn init_coordination_package_id() {
        COORDINATION_PACKAGE_ID.get_or_init(|| {
            match std::env::var("SILVANA_REGISTRY_PACKAGE") {
                Ok(package_id_str) => {
                    match sui::Address::from_str(&package_id_str) {
                        Ok(addr) => addr,
                        Err(e) => {
                            eprintln!("Error: Invalid SILVANA_REGISTRY_PACKAGE address format: {}", e);
                            eprintln!("   Expected format: 0x... (66 characters hex string)");
                            std::process::exit(1);
                        }
                    }
                },
                Err(_) => {
                    eprintln!("Error: SILVANA_REGISTRY_PACKAGE environment variable must be set");
                    std::process::exit(1);
                }
            }
        });
    }
    
    /// Try to initialize coordination package ID from SILVANA_REGISTRY_PACKAGE environment variable (don't fail)
    fn try_init_coordination_package_id() {
        if let Ok(package_id_str) = std::env::var("SILVANA_REGISTRY_PACKAGE") {
            if let Ok(addr) = sui::Address::from_str(&package_id_str) {
                COORDINATION_PACKAGE_ID.get_or_init(|| addr);
            }
        }
    }
    
    /// Get the coordinator ID (returns Option for read-only mode)
    pub fn get_coordinator_id(&self) -> Option<&String> {
        COORDINATOR_ID.get()
    }
    
    /// Get the coordinator ID or panic (for transaction operations)
    pub fn get_coordinator_id_required(&self) -> &String {
        COORDINATOR_ID.get()
            .expect("Coordinator ID should be initialized for transaction operations")
    }
    
    /// Get the chain
    pub fn get_chain(&self) -> &String {
        CHAIN.get()
            .expect("Chain should be initialized")
    }
    
    pub(crate) fn get_sui_client(&self) -> Client {
        self.sui_client.clone()
    }
    
    /// Get the Sui address (returns Option for read-only mode)
    pub fn get_sui_address(&self) -> Option<sui::Address> {
        self.sui_address
    }
    
    /// Get the Sui address or panic (for transaction operations)
    pub fn get_sui_address_required(&self) -> sui::Address {
        self.sui_address
            .expect("Sui address should be initialized for transaction operations")
    }
    
    /// Get the Sui private key (returns Option for read-only mode)
    #[allow(dead_code)]
    pub(crate) fn get_sui_private_key(&self) -> Option<&Ed25519PrivateKey> {
        self.sui_private_key.as_ref()
    }
    
    /// Get the Sui private key or panic (for transaction operations)
    pub(crate) fn get_sui_private_key_required(&self) -> &Ed25519PrivateKey {
        self.sui_private_key.as_ref()
            .expect("Sui private key should be initialized for transaction operations")
    }
    
    /// Get the coordination package ID (returns Option for read-only mode)
    pub fn get_coordination_package_id(&self) -> Option<sui::Address> {
        self.coordination_package_id
    }
    
    /// Get the coordination package ID or panic (for transaction operations)
    pub fn get_coordination_package_id_required(&self) -> sui::Address {
        self.coordination_package_id
            .expect("Coordination package ID should be initialized for transaction operations")
    }
}