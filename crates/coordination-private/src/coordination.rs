//! Private Coordination layer implementation (gRPC client)

use async_trait::async_trait;
use silvana_coordination_trait::{
    AppInstance, Block, BlockSettlement, Coordination, CoordinationLayer, EventStream,
    Job, MulticallOperations, MulticallResult, ProofCalculation, SequenceState,
};
use std::collections::HashMap;
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::debug;

use proto::silvana::state::v1::state_service_client::StateServiceClient;

use crate::auth::CoordinatorAuth;
use crate::config::PrivateCoordinationConfig;
use crate::error::{PrivateCoordinationError, Result};

/// Private Coordination layer implementation using gRPC client
pub struct PrivateCoordination {
    /// gRPC client to private state server
    client: StateServiceClient<Channel>,

    /// Coordinator authentication
    auth: CoordinatorAuth,

    /// Chain ID
    chain_id: String,

    /// Request timeout
    request_timeout: Duration,
}

impl PrivateCoordination {
    /// Create a new Private Coordination instance
    pub async fn new(config: PrivateCoordinationConfig) -> Result<Self> {
        // Get coordinator private key from environment variable
        let private_key = config.coordinator_private_key
            .or_else(|| std::env::var("SUI_SECRET_KEY").ok())
            .ok_or_else(|| PrivateCoordinationError::InvalidInput(
                "Coordinator private key not provided (set SUI_SECRET_KEY or config)".to_string()
            ))?;

        // Create authentication helper
        let auth = CoordinatorAuth::from_private_key_hex(&private_key)?;

        debug!(
            "Initializing Private Coordination with coordinator public key: {}",
            auth.public_key()
        );

        // Build gRPC channel
        let channel = if config.tls_enabled {
            let tls_config = if let Some(ca_cert_path) = &config.tls_ca_cert {
                let ca_cert = std::fs::read(ca_cert_path)
                    .map_err(|e| PrivateCoordinationError::InvalidInput(
                        format!("Failed to read CA cert: {}", e)
                    ))?;
                ClientTlsConfig::new()
                    .ca_certificate(tonic::transport::Certificate::from_pem(ca_cert))
            } else {
                ClientTlsConfig::new()
            };

            Channel::from_shared(config.grpc_endpoint.clone())
                .map_err(|e| PrivateCoordinationError::Other(e.into()))?
                .tls_config(tls_config)
                .map_err(|e| PrivateCoordinationError::Other(e.into()))?
                .connect()
                .await?
        } else {
            Channel::from_shared(config.grpc_endpoint.clone())
                .map_err(|e| PrivateCoordinationError::Other(e.into()))?
                .connect()
                .await?
        };

        let client = StateServiceClient::new(channel);

        Ok(Self {
            client,
            auth,
            chain_id: config.chain_id,
            request_timeout: Duration::from_secs(config.request_timeout_secs),
        })
    }
}

#[async_trait]
impl Coordination for PrivateCoordination {
    type TransactionHash = String;
    type Error = PrivateCoordinationError;

    fn coordination_layer(&self) -> CoordinationLayer {
        CoordinationLayer::Private
    }

    fn chain_id(&self) -> String {
        self.chain_id.clone()
    }

    // ===== Job Management =====

    async fn fetch_pending_jobs(&self, _app_instance: &str) -> Result<Vec<Job>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_pending_jobs - Phase 2".to_string()))
    }

    async fn fetch_failed_jobs(&self, _app_instance: &str) -> Result<Vec<Job>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_failed_jobs - Phase 2".to_string()))
    }

    async fn get_failed_jobs_count(&self, _app_instance: &str) -> Result<u64> {
        Err(PrivateCoordinationError::NotImplemented("get_failed_jobs_count - Phase 2".to_string()))
    }

    async fn fetch_job_by_sequence(&self, _app_instance: &str, _job_sequence: u64) -> Result<Option<Job>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_job_by_sequence - Phase 2".to_string()))
    }

    async fn get_pending_jobs_count(&self, _app_instance: &str) -> Result<u64> {
        Err(PrivateCoordinationError::NotImplemented("get_pending_jobs_count - Phase 2".to_string()))
    }

    async fn get_total_jobs_count(&self, _app_instance: &str) -> Result<u64> {
        Err(PrivateCoordinationError::NotImplemented("get_total_jobs_count - Phase 2".to_string()))
    }

    async fn get_settlement_job_sequences(&self, _app_instance: &str) -> Result<HashMap<String, u64>> {
        Err(PrivateCoordinationError::NotImplemented("get_settlement_job_sequences - Phase 2".to_string()))
    }

    async fn get_jobs_info(&self, _app_instance: &str) -> Result<Option<(String, String)>> {
        Err(PrivateCoordinationError::NotImplemented("get_jobs_info - Phase 2".to_string()))
    }

    async fn fetch_jobs_batch(&self, _app_instance: &str, _job_sequences: &[u64]) -> Result<Vec<Job>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_jobs_batch - Phase 2".to_string()))
    }

    async fn fetch_pending_job_sequences(&self, _app_instance: &str) -> Result<Vec<u64>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_pending_job_sequences - Phase 2".to_string()))
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        _app_instance: &str,
        _developer: &str,
        _agent: &str,
        _agent_method: &str,
    ) -> Result<Vec<u64>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_pending_job_sequences_by_method - Phase 2".to_string()))
    }

    async fn start_job(&self, _app_instance: &str, _job_sequence: u64) -> Result<bool> {
        Err(PrivateCoordinationError::NotImplemented("start_job - Phase 2".to_string()))
    }

    async fn complete_job(&self, _app_instance: &str, _job_sequence: u64) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("complete_job - Phase 2".to_string()))
    }

    async fn fail_job(&self, _app_instance: &str, _job_sequence: u64, _error: &str) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("fail_job - Phase 2".to_string()))
    }

    async fn terminate_job(&self, _app_instance: &str, _job_sequence: u64) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("terminate_job - Phase 2".to_string()))
    }

    async fn create_app_job(
        &self,
        _app_instance: &str,
        _method_name: String,
        _job_description: Option<String>,
        _block_number: Option<u64>,
        _sequences: Option<Vec<u64>>,
        _sequences1: Option<Vec<u64>>,
        _sequences2: Option<Vec<u64>>,
        _data: Vec<u8>,
        _interval_ms: Option<u64>,
        _next_scheduled_at: Option<u64>,
        _settlement_chain: Option<String>,
    ) -> Result<(Self::TransactionHash, u64)> {
        Err(PrivateCoordinationError::NotImplemented("create_app_job - Phase 2".to_string()))
    }

    async fn create_merge_job_with_proving(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _sequences: Vec<u64>,
        _sequences1: Vec<u64>,
        _sequences2: Vec<u64>,
        _job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("create_merge_job_with_proving - Phase 2".to_string()))
    }

    async fn create_settle_job(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _chain: String,
        _job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("create_settle_job - Phase 2".to_string()))
    }

    async fn terminate_app_job(&self, _app_instance: &str, _job_sequence: u64) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("terminate_app_job - Phase 2".to_string()))
    }

    async fn restart_failed_jobs(
        &self,
        _app_instance: &str,
        _job_sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("restart_failed_jobs - Phase 2".to_string()))
    }

    async fn remove_failed_jobs(
        &self,
        _app_instance: &str,
        _sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("remove_failed_jobs - Phase 2".to_string()))
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(&self, _app_instance: &str, _sequence: u64) -> Result<Option<SequenceState>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_sequence_state - Phase 2".to_string()))
    }

    async fn fetch_sequence_states_range(
        &self,
        _app_instance: &str,
        _from_sequence: u64,
        _to_sequence: u64,
    ) -> Result<Vec<SequenceState>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_sequence_states_range - Phase 2".to_string()))
    }

    async fn get_current_sequence(&self, _app_instance: &str) -> Result<u64> {
        Err(PrivateCoordinationError::NotImplemented("get_current_sequence - Phase 2".to_string()))
    }

    async fn update_state_for_sequence(
        &self,
        _app_instance: &str,
        _sequence: u64,
        _new_state_data: Option<Vec<u8>>,
        _new_data_availability_hash: Option<String>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("update_state_for_sequence - Phase 2".to_string()))
    }

    // ===== Block Management =====

    async fn fetch_block(&self, _app_instance: &str, _block_number: u64) -> Result<Option<Block>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_block - Phase 2".to_string()))
    }

    async fn fetch_blocks_range(
        &self,
        _app_instance: &str,
        _from_block: u64,
        _to_block: u64,
    ) -> Result<Vec<Block>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_blocks_range - Phase 2".to_string()))
    }

    async fn try_create_block(&self, _app_instance: &str) -> Result<Option<u64>> {
        Err(PrivateCoordinationError::NotImplemented("try_create_block - Phase 2".to_string()))
    }

    async fn update_block_state_data_availability(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _state_da: String,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("update_block_state_data_availability - Phase 2".to_string()))
    }

    async fn update_block_proof_data_availability(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _proof_da: String,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("update_block_proof_data_availability - Phase 2".to_string()))
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        _app_instance: &str,
        _block_number: u64,
    ) -> Result<Option<ProofCalculation>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_proof_calculation - Phase 2".to_string()))
    }

    async fn fetch_proof_calculations_range(
        &self,
        _app_instance: &str,
        _from_block: u64,
        _to_block: u64,
    ) -> Result<Vec<ProofCalculation>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_proof_calculations_range - Phase 2".to_string()))
    }

    async fn start_proving(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _sequences: Vec<u64>,
        _merged_sequences_1: Option<Vec<u64>>,
        _merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("start_proving - Phase 2".to_string()))
    }

    async fn submit_proof(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _sequences: Vec<u64>,
        _merged_sequences_1: Option<Vec<u64>>,
        _merged_sequences_2: Option<Vec<u64>>,
        _job_id: String,
        _da_hash: String,
        _cpu_cores: u8,
        _prover_architecture: String,
        _prover_memory: u64,
        _cpu_time: u64,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("submit_proof - Phase 2".to_string()))
    }

    async fn reject_proof(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("reject_proof - Phase 2".to_string()))
    }

    // ===== Settlement =====

    async fn fetch_block_settlement(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _chain: &str,
    ) -> Result<Option<BlockSettlement>> {
        Err(PrivateCoordinationError::NotImplemented("fetch_block_settlement - Phase 2".to_string()))
    }

    async fn get_settlement_chains(&self, _app_instance: &str) -> Result<Vec<String>> {
        Err(PrivateCoordinationError::NotImplemented("get_settlement_chains - Phase 2".to_string()))
    }

    async fn get_settlement_address(&self, _app_instance: &str, _chain: &str) -> Result<Option<String>> {
        Err(PrivateCoordinationError::NotImplemented("get_settlement_address - Phase 2".to_string()))
    }

    async fn get_settlement_job_for_chain(&self, _app_instance: &str, _chain: &str) -> Result<Option<u64>> {
        Err(PrivateCoordinationError::NotImplemented("get_settlement_job_for_chain - Phase 2".to_string()))
    }

    async fn set_settlement_address(
        &self,
        _app_instance: &str,
        _chain: String,
        _address: Option<String>,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("set_settlement_address - Phase 2".to_string()))
    }

    async fn update_block_settlement_tx_hash(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _chain: String,
        _settlement_tx_hash: String,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("update_block_settlement_tx_hash - Phase 2".to_string()))
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        _app_instance: &str,
        _block_number: u64,
        _chain: String,
        _settled_at: u64,
    ) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("update_block_settlement_tx_included_in_block - Phase 2".to_string()))
    }

    // ===== Key-Value Storage =====

    async fn get_kv_string(&self, _app_instance: &str, _key: &str) -> Result<Option<String>> {
        Err(PrivateCoordinationError::NotImplemented("get_kv_string - Phase 2".to_string()))
    }

    async fn get_all_kv_string(&self, _app_instance: &str) -> Result<HashMap<String, String>> {
        Err(PrivateCoordinationError::NotImplemented("get_all_kv_string - Phase 2".to_string()))
    }

    async fn list_kv_string_keys(
        &self,
        _app_instance: &str,
        _prefix: Option<&str>,
        _limit: Option<u32>,
    ) -> Result<Vec<String>> {
        Err(PrivateCoordinationError::NotImplemented("list_kv_string_keys - Phase 2".to_string()))
    }

    async fn set_kv_string(&self, _app_instance: &str, _key: String, _value: String) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("set_kv_string - Phase 2".to_string()))
    }

    async fn delete_kv_string(&self, _app_instance: &str, _key: &str) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("delete_kv_string - Phase 2".to_string()))
    }

    async fn get_kv_binary(&self, _app_instance: &str, _key: &[u8]) -> Result<Option<Vec<u8>>> {
        Err(PrivateCoordinationError::NotImplemented("get_kv_binary - Phase 2".to_string()))
    }

    async fn list_kv_binary_keys(
        &self,
        _app_instance: &str,
        _prefix: Option<&[u8]>,
        _limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>> {
        Err(PrivateCoordinationError::NotImplemented("list_kv_binary_keys - Phase 2".to_string()))
    }

    async fn set_kv_binary(&self, _app_instance: &str, _key: Vec<u8>, _value: Vec<u8>) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("set_kv_binary - Phase 2".to_string()))
    }

    async fn delete_kv_binary(&self, _app_instance: &str, _key: &[u8]) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("delete_kv_binary - Phase 2".to_string()))
    }

    // ===== Metadata Management =====

    async fn get_metadata(&self, _app_instance: &str, _key: &str) -> Result<Option<String>> {
        Err(PrivateCoordinationError::NotImplemented("get_metadata - Phase 2".to_string()))
    }

    async fn get_all_metadata(&self, _app_instance: &str) -> Result<HashMap<String, String>> {
        Err(PrivateCoordinationError::NotImplemented("get_all_metadata - Phase 2".to_string()))
    }

    async fn add_metadata(&self, _app_instance: &str, _key: String, _value: String) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("add_metadata - Phase 2".to_string()))
    }

    // ===== App Instance Data =====

    async fn fetch_app_instance(&self, _app_instance: &str) -> Result<AppInstance> {
        Err(PrivateCoordinationError::NotImplemented("fetch_app_instance - Phase 2".to_string()))
    }

    async fn get_app_instance_admin(&self, _app_instance: &str) -> Result<String> {
        Err(PrivateCoordinationError::NotImplemented("get_app_instance_admin - Phase 2".to_string()))
    }

    async fn is_app_paused(&self, _app_instance: &str) -> Result<bool> {
        Err(PrivateCoordinationError::NotImplemented("is_app_paused - Phase 2".to_string()))
    }

    async fn get_min_time_between_blocks(&self, _app_instance: &str) -> Result<u64> {
        Err(PrivateCoordinationError::NotImplemented("get_min_time_between_blocks - Phase 2".to_string()))
    }

    // ===== Batch Operations =====

    fn supports_multicall(&self) -> bool {
        false
    }

    async fn multicall_job_operations(
        &self,
        _operations: Vec<MulticallOperations>,
    ) -> Result<MulticallResult> {
        Err(PrivateCoordinationError::NotImplemented("multicall_job_operations - not supported".to_string()))
    }

    // ===== State Purging =====

    async fn purge(&self, _app_instance: &str, _sequences_to_purge: u64) -> Result<Self::TransactionHash> {
        Err(PrivateCoordinationError::NotImplemented("purge - Phase 2".to_string()))
    }

    // ===== Event Streaming =====

    async fn event_stream(&self) -> Result<EventStream> {
        Err(PrivateCoordinationError::NotImplemented("event_stream - not supported for Private coordination".to_string()))
    }
}
