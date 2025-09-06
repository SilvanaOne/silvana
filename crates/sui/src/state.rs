use std::sync::Arc;
use std::sync::OnceLock;
use std::str::FromStr;
use std::collections::VecDeque;
use parking_lot::Mutex;
use sui_rpc::Client;
use sui_sdk_types as sui;
use sui_crypto::ed25519::Ed25519PrivateKey;
use tracing::{info, debug};
use crate::chain::load_sender_from_env_or_key;
use anyhow::Result;

// Global static values initialized once from environment variables
static COORDINATOR_ID: OnceLock<String> = OnceLock::new();
static CHAIN: OnceLock<String> = OnceLock::new();
static COORDINATION_PACKAGE_ID: OnceLock<sui::Address> = OnceLock::new();

// Global static SharedSuiState instance
static SHARED_SUI_STATE: OnceLock<Arc<SharedSuiState>> = OnceLock::new();

/// Information about a job that has been started on the blockchain
#[derive(Debug, Clone)]
pub struct StartedJob {
    pub app_instance: String,
    pub job_sequence: u64,
    pub memory_requirement: u64,
}

pub struct SharedSuiState {
    sui_client: Client,  // Sui client (cloneable)
    sui_address: sui::Address,  // Sui address from environment
    sui_private_key: Ed25519PrivateKey,  // Sui private key from environment
    coordination_package_id: sui::Address,  // Coordination package ID from environment
    started_jobs_buffer: Arc<Mutex<VecDeque<StartedJob>>>,  // Buffer of jobs started on blockchain
}

impl SharedSuiState {
    /// Initialize the global SharedSuiState instance
    /// If private_key_str is provided, it will be used instead of loading from environment
    pub async fn initialize(rpc_url: &str) -> Result<()> {
        Self::initialize_with_optional_key(rpc_url, None).await
    }
    
    /// Initialize the global SharedSuiState instance with an optional private key
    pub async fn initialize_with_optional_key(rpc_url: &str, private_key_str: Option<&str>) -> Result<()> {
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
            sui_address,
            sui_private_key,
            coordination_package_id,
            started_jobs_buffer: Arc::new(Mutex::new(VecDeque::new())),
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
    
    /// Add started jobs to the buffer
    pub fn add_started_jobs(&self, jobs: Vec<StartedJob>) {
        let mut buffer = self.started_jobs_buffer.lock();
        for job in jobs {
            debug!("Adding started job to buffer: app_instance={}, sequence={}, memory={}",
                  job.app_instance, job.job_sequence, job.memory_requirement);
            buffer.push_back(job);
        }
        debug!("Started jobs buffer now contains {} jobs", buffer.len());
    }
    
    /// Get the next started job from the buffer (thread-safe)
    pub fn get_next_started_job(&self) -> Option<StartedJob> {
        let mut buffer = self.started_jobs_buffer.lock();
        let job = buffer.pop_front();
        if let Some(ref j) = job {
            debug!("Retrieved started job from buffer: app_instance={}, sequence={}, memory={}",
                  j.app_instance, j.job_sequence, j.memory_requirement);
            debug!("Started jobs buffer now contains {} jobs", buffer.len());
        }
        job
    }
    
    /// Get the number of jobs in the buffer
    pub fn get_started_jobs_count(&self) -> usize {
        self.started_jobs_buffer.lock().len()
    }
    
    /// Clear all jobs from the buffer (for emergency use)
    pub fn clear_started_jobs_buffer(&self) {
        let mut buffer = self.started_jobs_buffer.lock();
        let count = buffer.len();
        buffer.clear();
        if count > 0 {
            info!("Cleared {} jobs from started jobs buffer", count);
        }
    }
}