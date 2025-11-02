//! Ethereum Coordination Layer Implementation
//!
//! This module implements the `Coordination` trait for Ethereum and EVM-compatible chains.
//!
//! **Important**: This implementation does NOT support multicall batching. Each operation
//! executes as a separate transaction for simplicity and reliability.

use async_trait::async_trait;
use silvana_coordination_trait::*;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::{
    abi::*,
    abi::job_manager::DataTypes,
    config::EthereumCoordinationConfig,
    contract::ContractClient,
    conversions::*,
    error::{EthereumCoordinationError, Result},
};

use alloy::primitives::{Bytes, TxHash, U256};
use alloy::providers::Provider;

/// Ethereum coordination layer implementation
///
/// This struct provides a complete implementation of the `Coordination` trait
/// for Ethereum and EVM-compatible chains (Polygon, Base, etc.).
///
/// # Architecture
///
/// - **No multicall batching**: Each operation is a separate transaction
/// - **Stateless providers**: New provider created for each operation
/// - **Simple and reliable**: No background tasks or queuing
///
/// # Example
///
/// ```ignore
/// use silvana_coordination_ethereum::{EthereumCoordination, EthereumCoordinationConfig};
///
/// let config = EthereumCoordinationConfig {
///     rpc_url: "https://rpc-amoy.polygon.technology".to_string(),
///     chain_id: 80002,
///     contract_address: "0xYourContractAddress".to_string(),
///     private_key: Some("0xYourPrivateKey".to_string()),
///     ..Default::default()
/// };
///
/// let coord = EthereumCoordination::new(config)?;
/// let jobs = coord.fetch_pending_jobs("my-app").await?;
/// ```
pub struct EthereumCoordination {
    /// Contract client with connection details
    contract: Arc<ContractClient>,

    /// Configuration
    config: EthereumCoordinationConfig,

    /// Chain ID as string (for trait implementation)
    chain_id: String,
}

impl EthereumCoordination {
    /// Create a new Ethereum coordination instance
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the Ethereum coordination layer
    ///
    /// # Returns
    ///
    /// A new `EthereumCoordination` instance
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Configuration validation fails
    /// - Contract address is invalid
    /// - Private key is invalid (if provided)
    pub fn new(config: EthereumCoordinationConfig) -> Result<Self> {
        info!(
            "Initializing Ethereum coordination layer for chain {}",
            config.chain_id
        );

        // Validate configuration
        config
            .validate()
            .map_err(|e| EthereumCoordinationError::Configuration(e))?;

        // Create contract client
        let contract = Arc::new(ContractClient::new(config.clone())?);
        let chain_id = config.chain_id.to_string();

        info!("Ethereum coordination layer initialized successfully");

        Ok(Self {
            contract,
            config,
            chain_id,
        })
    }

    /// Format transaction hash for return as String
    fn format_tx_hash(&self, hash: TxHash) -> String {
        format!("0x{:x}", hash)
    }

    /// Determine coordination layer based on chain ID
    fn get_coordination_layer(&self) -> CoordinationLayer {
        match self.config.chain_id {
            1 | 17000 => CoordinationLayer::Ethereum,      // Mainnet, Holesky
            137 | 80002 => CoordinationLayer::Polygon,     // Polygon, Amoy
            8453 | 84532 => CoordinationLayer::Base,       // Base, Base Sepolia
            _ => CoordinationLayer::Ethereum,              // Default to Ethereum
        }
    }
}

/// Implementation of the Coordination trait for Ethereum
///
/// This provides all 53 coordination methods for interacting with the
/// Ethereum-based coordination smart contracts.
#[async_trait]
impl Coordination for EthereumCoordination {
    type TransactionHash = String;
    type Error = EthereumCoordinationError;

    // ===== Coordination Layer Identification =====

    fn coordination_layer(&self) -> CoordinationLayer {
        self.get_coordination_layer()
    }

    fn chain_id(&self) -> String {
        self.chain_id.clone()
    }

    // ===== Job Management - Read Operations =====

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        debug!("Fetching pending jobs for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getPendingJobs(app_instance.to_string(), U256::from(1000u64), U256::from(0u64))
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        // Convert Solidity jobs to Rust jobs
        let jobs: Vec<Job> = result
            .into_iter()
            .map(|solidity_job| {
                Job {
                    job_sequence: solidity_job.jobSequence,
                    description: optional_string(solidity_job.description),
                    developer: solidity_job.developer,
                    agent: solidity_job.agent,
                    agent_method: solidity_job.agentMethod,
                    app: solidity_job.app,
                    app_instance: app_instance.to_string(),
                    app_instance_method: solidity_job.appInstanceMethod,
                    block_number: if solidity_job.blockNumber == 0 {
                        None
                    } else {
                        Some(solidity_job.blockNumber)
                    },
                    sequences: optional_u64_vec(solidity_job.sequences.clone()),
                    sequences1: optional_u64_vec(solidity_job.sequences1.clone()),
                    sequences2: optional_u64_vec(solidity_job.sequences2.clone()),
                    data: solidity_job.data.to_vec(),
                    status: convert_job_status_with_error(
                        solidity_job.status,
                        "", // Error message not stored in Solidity contract
                    ),
                    attempts: solidity_job.attempts,
                    interval_ms: optional_u64(solidity_job.intervalMs),
                    next_scheduled_at: optional_u64(solidity_job.nextScheduledAt),
                    created_at: timestamp_to_u64(solidity_job.createdAt),
                    updated_at: timestamp_to_u64(solidity_job.updatedAt),
                }
            })
            .collect();

        debug!("Fetched {} pending jobs", jobs.len());
        Ok(jobs)
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        debug!("Fetching failed jobs for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getFailedJobs(app_instance.to_string(), U256::from(1000u64), U256::from(0u64))
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        // Convert Solidity jobs to Rust jobs (same logic as fetch_pending_jobs)
        let jobs: Vec<Job> = result
            .into_iter()
            .map(|solidity_job| {
                Job {
                    job_sequence: solidity_job.jobSequence,
                    description: optional_string(solidity_job.description),
                    developer: solidity_job.developer,
                    agent: solidity_job.agent,
                    agent_method: solidity_job.agentMethod,
                    app: solidity_job.app,
                    app_instance: app_instance.to_string(),
                    app_instance_method: solidity_job.appInstanceMethod,
                    block_number: if solidity_job.blockNumber == 0 {
                        None
                    } else {
                        Some(solidity_job.blockNumber)
                    },
                    sequences: optional_u64_vec(solidity_job.sequences.clone()),
                    sequences1: optional_u64_vec(solidity_job.sequences1.clone()),
                    sequences2: optional_u64_vec(solidity_job.sequences2.clone()),
                    data: solidity_job.data.to_vec(),
                    status: convert_job_status_with_error(
                        solidity_job.status,
                        "", // Error message not stored in Solidity contract
                    ),
                    attempts: solidity_job.attempts,
                    interval_ms: optional_u64(solidity_job.intervalMs),
                    next_scheduled_at: optional_u64(solidity_job.nextScheduledAt),
                    created_at: timestamp_to_u64(solidity_job.createdAt),
                    updated_at: timestamp_to_u64(solidity_job.updatedAt),
                }
            })
            .collect();

        debug!("Fetched {} failed jobs", jobs.len());
        Ok(jobs)
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        debug!("Getting failed jobs count for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getFailedJobCount(app_instance.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        debug!("Failed jobs count: {}", result);
        Ok(result.to::<u64>())
    }

    async fn fetch_job_by_id(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Option<Job>> {
        debug!("Fetching job by ID: {} for app_instance: {}", job_sequence, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getJob(app_instance.to_string(), U256::from(job_sequence))
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        // Check if job exists (jobSequence > 0 indicates it exists)
        if result.jobSequence == 0 {
            return Ok(None);
        }

        // Convert to Rust Job
        let job = Job {
            job_sequence: result.jobSequence,
            description: optional_string(result.description.clone()),
            developer: result.developer.clone(),
            agent: result.agent.clone(),
            agent_method: result.agentMethod.clone(),
            app: result.app.clone(),
            app_instance: app_instance.to_string(),
            app_instance_method: result.appInstanceMethod.clone(),
            block_number: if result.blockNumber == 0 {
                None
            } else {
                Some(result.blockNumber)
            },
            sequences: optional_u64_vec(result.sequences.clone()),
            sequences1: optional_u64_vec(result.sequences1.clone()),
            sequences2: optional_u64_vec(result.sequences2.clone()),
            data: result.data.to_vec(),
            status: convert_job_status_with_error(
                result.status,
                "", // Error message not stored in Solidity contract
            ),
            attempts: result.attempts,
            interval_ms: optional_u64(result.intervalMs),
            next_scheduled_at: optional_u64(result.nextScheduledAt),
            created_at: timestamp_to_u64(result.createdAt),
            updated_at: timestamp_to_u64(result.updatedAt),
        };

        Ok(Some(job))
    }

    // ===== Job Management - Write Operations =====

    async fn start_job(&self, app_instance: &str, job_sequence: u64) -> Result<bool> {
        debug!("Starting job: {} for app_instance: {}", job_sequence, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .takeJob(app_instance.to_string(), U256::from(job_sequence))
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let success = receipt.status();
        debug!("Job start transaction status: {}", success);
        Ok(success)
    }

    async fn complete_job(&self, app_instance: &str, job_sequence: u64) -> Result<String> {
        debug!("Completing job: {} for app_instance: {}", job_sequence, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .completeJob(app_instance.to_string(), U256::from(job_sequence), Bytes::new())
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Job completed with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn fail_job(&self, app_instance: &str, job_sequence: u64, error: &str) -> Result<String> {
        debug!(
            "Failing job: {} for app_instance: {} with error: {}",
            job_sequence, app_instance, error
        );

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .failJob(app_instance.to_string(), U256::from(job_sequence), error.to_string())
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Job failed with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn terminate_job(&self, app_instance: &str, job_sequence: u64) -> Result<String> {
        debug!("Terminating job: {} for app_instance: {}", job_sequence, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .terminateJob(app_instance.to_string(), U256::from(job_sequence))
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Job terminated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn create_app_job(
        &self,
        app_instance: &str,
        method_name: String,
        job_description: Option<String>,
        block_number: Option<u64>,
        sequences: Option<Vec<u64>>,
        sequences1: Option<Vec<u64>>,
        sequences2: Option<Vec<u64>>,
        data: Vec<u8>,
        interval_ms: Option<u64>,
        next_scheduled_at: Option<u64>,
        settlement_chain: Option<String>,
    ) -> Result<(String, u64)> {
        debug!("Creating app job for method: {} in app_instance: {}", method_name, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        // Create JobInput
        let job_input = DataTypes::JobInput {
            description: job_description.unwrap_or_default(),
            developer: "system".to_string(), // TODO: Get from config or context
            agent: "silvana-agent".to_string(), // TODO: Get from config
            agentMethod: method_name.clone(),
            app: app_instance.to_string(), // Using app_instance as app name
            appInstance: app_instance.to_string(),
            appInstanceMethod: method_name,
            data: Bytes::from(data),
            intervalMs: interval_ms.unwrap_or(0),
        };

        let pending_tx = contract
            .createJob(job_input)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        // Parse JobCreated event from logs
        let logs = receipt.inner.logs();
        for log in logs {
            if let Ok(decoded) = log.log_decode::<JobManager::JobCreated>() {
                let jobSequence = decoded.data().jobSequence;
                let tx_hash = self.format_tx_hash(receipt.transaction_hash);
                debug!("Created job with sequence: {}, tx hash: {}", jobSequence, tx_hash);
                return Ok((tx_hash, jobSequence));
            }
        }

        Err(EthereumCoordinationError::EventParsing(
            "JobCreated event not found in transaction logs".to_string(),
        ))
    }

    async fn create_merge_job_with_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<String> {
        debug!("Creating merge job with proving for block {} in app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        // Encode block_number and sequences into data
        let mut data = Vec::new();
        data.extend_from_slice(&block_number.to_be_bytes());
        data.extend(encode_u64_array(&sequences).as_ref());
        data.extend(encode_u64_array(&sequences1).as_ref());
        data.extend(encode_u64_array(&sequences2).as_ref());

        let job_input = DataTypes::JobInput {
            description: job_description.unwrap_or_else(|| format!("Merge and prove block {}", block_number)),
            developer: "system".to_string(),
            agent: "silvana-agent".to_string(),
            agentMethod: "merge_with_proving".to_string(),
            app: app_instance.to_string(),
            appInstance: app_instance.to_string(),
            appInstanceMethod: "process_merge".to_string(),
            data: Bytes::from(data),
            intervalMs: 0,
        };

        let pending_tx = contract
            .createJob(job_input)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Merge job created with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<String> {
        debug!("Creating settle job for block {} on chain {} in app_instance: {}", block_number, chain, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        // Encode block_number and chain into data
        let mut data = Vec::new();
        data.extend_from_slice(&block_number.to_be_bytes());
        data.extend_from_slice(chain.as_bytes());

        let job_input = DataTypes::JobInput {
            description: job_description.unwrap_or_else(|| format!("Settle block {} on {}", block_number, chain)),
            developer: "system".to_string(),
            agent: "silvana-agent".to_string(),
            agentMethod: "settle".to_string(),
            app: app_instance.to_string(),
            appInstance: app_instance.to_string(),
            appInstanceMethod: "process_settlement".to_string(),
            data: Bytes::from(data),
            intervalMs: 0,
        };

        let pending_tx = contract
            .createJob(job_input)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Settle job created with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn terminate_app_job(&self, app_instance: &str, job_id: u64) -> Result<String> {
        debug!("Terminating app job {} in app_instance: {}", job_id, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .terminateJob(app_instance.to_string(), alloy::primitives::U256::from(job_id))
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("App job terminated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        job_sequences: Option<Vec<u64>>,
    ) -> Result<String> {
        debug!("Restarting failed jobs in app_instance: {}", app_instance);

        // Note: The Solidity contract's restartFailedJobs doesn't support filtering by sequences
        // It restarts ALL failed jobs. The job_sequences parameter is ignored for now.
        // TODO: Update contract to support selective restart by job sequences
        if job_sequences.is_some() {
            warn!("job_sequences parameter is not yet supported by Ethereum coordination - restarting all failed jobs");
        }

        let provider = self.contract.create_provider_with_signer()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .restartFailedJobs(app_instance.to_string())
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Failed jobs restarted with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<String> {
        debug!("Removing failed jobs in app_instance: {}", app_instance);

        // Note: The Solidity contract doesn't have a dedicated method to remove failed jobs
        // without restarting them. This functionality would need to be added to the contract.
        // For now, we return an error indicating this is not supported.
        // TODO: Add removeFailedJobs method to Solidity contract

        Err(EthereumCoordinationError::UnsupportedOperation(
            "remove_failed_jobs not implemented in contract".to_string(),
        ))
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(
        &self,
        app_instance: &str,
        sequence: u64,
    ) -> Result<Option<SequenceState>> {
        debug!("Fetching sequence state {} for app_instance: {}", sequence, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        // Try to get sequence details - if not found, return None
        let result = contract
            .getSequenceDetails(app_instance.to_string(), alloy::primitives::U256::from(sequence))
            .call()
            .await;

        match result {
            Ok(state) => {
                let sequence_state = SequenceState {
                    sequence: state.sequenceNumber.to::<u64>(),
                    state: None, // Not available from contract
                    data_availability: None, // Not available from contract
                    optimistic_state: state.stateCommitment.to_vec(),
                    transition_data: state.actionsCommitment.to_vec(),
                };
                Ok(Some(sequence_state))
            }
            Err(_) => Ok(None), // Sequence not found
        }
    }

    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>> {
        debug!("Fetching sequence states range {}-{} for app_instance: {}", from_sequence, to_sequence, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        let mut states = Vec::new();

        // Fetch each sequence individually
        // TODO: Consider batching these calls or adding a contract method to fetch ranges
        for seq in from_sequence..=to_sequence {
            let result = contract
                .getSequenceDetails(app_instance.to_string(), alloy::primitives::U256::from(seq))
                .call()
                .await;

            if let Ok(state) = result {
                let sequence_state = SequenceState {
                    sequence: state.sequenceNumber.to::<u64>(),
                    state: None, // Not available from contract
                    data_availability: None, // Not available from contract
                    optimistic_state: state.stateCommitment.to_vec(),
                    transition_data: state.actionsCommitment.to_vec(),
                };
                states.push(sequence_state);
            }
            // Skip sequences that don't exist
        }

        Ok(states)
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        debug!("Getting current sequence for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        let sequence = contract
            .getCurrentSequence(app_instance.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        Ok(sequence.to::<u64>())
    }

    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<String> {
        debug!("Updating state for sequence {} in app_instance: {}", sequence, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        // Compute commitments from the provided data
        // For state commitment: hash of state data (or use provided DA hash)
        let state_commitment = if let Some(ref hash) = new_data_availability_hash {
            // Parse hex hash
            let hash_bytes = hex::decode(hash.trim_start_matches("0x"))
                .map_err(|e| EthereumCoordinationError::Conversion(format!("Invalid hash: {}", e)))?;
            if hash_bytes.len() != 32 {
                return Err(EthereumCoordinationError::Conversion(
                    "Hash must be 32 bytes".to_string(),
                ));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash_bytes);
            alloy::primitives::FixedBytes::from(arr)
        } else if let Some(ref data) = new_state_data {
            // Hash the state data
            alloy::primitives::keccak256(data)
        } else {
            // No data provided - use empty hash
            alloy::primitives::FixedBytes::ZERO
        };

        // For actions commitment: hash of state data (same for now)
        // TODO: Separate actions data from state data in trait signature
        let actions_commitment = if let Some(ref data) = new_state_data {
            alloy::primitives::keccak256(data)
        } else {
            alloy::primitives::FixedBytes::ZERO
        };

        let pending_tx = contract
            .updateSequence(
                app_instance.to_string(),
                alloy::primitives::U256::from(sequence),
                actions_commitment,
                state_commitment,
            )
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Sequence state updated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    // ===== Block Management =====

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        debug!("Fetching block {} for app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getBlock(app_instance.to_string(), block_number)
            .call()
            .await;

        match result {
            Ok(block_data) => {
                let block = Block {
                    block_number: block_data.blockNumber,
                    name: block_data.name,
                    start_sequence: block_data.startSequence,
                    end_sequence: block_data.endSequence,
                    actions_commitment: block_data.actionsCommitment.to_vec(),
                    state_commitment: block_data.stateCommitment.to_vec(),
                    time_since_last_block: block_data.timeSinceLastBlock,
                    number_of_transactions: block_data.numberOfTransactions,
                    start_actions_commitment: block_data.startActionsCommitment.to_vec(),
                    end_actions_commitment: block_data.endActionsCommitment.to_vec(),
                    state_data_availability: optional_string(block_data.stateDataAvailability),
                    proof_data_availability: optional_string(block_data.proofDataAvailability),
                    created_at: timestamp_to_u64(block_data.createdAt),
                    state_calculated_at: if block_data.stateCalculatedAt == 0 {
                        None
                    } else {
                        Some(timestamp_to_u64(block_data.stateCalculatedAt))
                    },
                    proved_at: if block_data.provedAt == 0 {
                        None
                    } else {
                        Some(timestamp_to_u64(block_data.provedAt))
                    },
                };
                Ok(Some(block))
            }
            Err(_) => Ok(None), // Block not found
        }
    }

    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>> {
        debug!("Fetching blocks range {}-{} for app_instance: {}", from_block, to_block, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getBlocksRange(app_instance.to_string(), from_block, to_block)
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        let blocks: Vec<Block> = result
            .into_iter()
            .filter(|b| b.blockNumber != 0) // Filter out empty blocks
            .map(|block_data| Block {
                block_number: block_data.blockNumber,
                name: block_data.name,
                start_sequence: block_data.startSequence,
                end_sequence: block_data.endSequence,
                actions_commitment: block_data.actionsCommitment.to_vec(),
                state_commitment: block_data.stateCommitment.to_vec(),
                time_since_last_block: block_data.timeSinceLastBlock,
                number_of_transactions: block_data.numberOfTransactions,
                start_actions_commitment: block_data.startActionsCommitment.to_vec(),
                end_actions_commitment: block_data.endActionsCommitment.to_vec(),
                state_data_availability: optional_string(block_data.stateDataAvailability),
                proof_data_availability: optional_string(block_data.proofDataAvailability),
                created_at: timestamp_to_u64(block_data.createdAt),
                state_calculated_at: if block_data.stateCalculatedAt == 0 {
                    None
                } else {
                    Some(timestamp_to_u64(block_data.stateCalculatedAt))
                },
                proved_at: if block_data.provedAt == 0 {
                    None
                } else {
                    Some(timestamp_to_u64(block_data.provedAt))
                },
            })
            .collect();

        Ok(blocks)
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>> {
        debug!("Trying to create block for app_instance: {}", app_instance);

        // Note: The Solidity createBlock method requires:
        // - endSequence: which sequence to end the block at
        // - actionsCommitment: hash of actions in this block
        // - stateCommitment: hash of state after this block
        //
        // The trait doesn't provide these parameters, so we cannot implement
        // automatic block creation without additional logic to determine these values.
        // This would typically require:
        // 1. Getting current sequence number
        // 2. Calculating commitments from state/actions data
        // 3. Determining block boundaries
        //
        // For now, return UnsupportedOperation to indicate this needs a richer API
        // TODO: Either enhance the trait signature or add contract logic for automatic block creation

        Err(EthereumCoordinationError::UnsupportedOperation(
            "try_create_block not implemented - needs richer API".to_string(),
        ))
    }

    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_da: String,
    ) -> Result<String> {
        debug!("Updating block {} state DA for app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        // Note: The contract's setDataAvailability sets both state and proof DA together
        // We set state DA and leave proof DA empty for now
        let pending_tx = contract
            .setDataAvailability(
                app_instance.to_string(),
                block_number,
                state_da,
                String::new(), // Empty proof DA
            )
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Block state DA updated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_da: String,
    ) -> Result<String> {
        debug!("Updating block {} proof DA for app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        // Note: The contract's setDataAvailability sets both state and proof DA together
        // We set proof DA and leave state DA empty for now
        let pending_tx = contract
            .setDataAvailability(
                app_instance.to_string(),
                block_number,
                String::new(), // Empty state DA
                proof_da,
            )
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Block proof DA updated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<ProofCalculation>> {
        debug!("Fetching proof calculation for block {} in app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = ProofManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getProofCalculation(app_instance.to_string(), block_number)
            .call()
            .await;

        match result {
            Ok(data) => {
                let proof_calc = ProofCalculation {
                    id: format!("{}-{}", app_instance, block_number),
                    block_number: data.blockNum,
                    start_sequence: data.startSeq,
                    end_sequence: if data.endSeq == 0 {
                        None
                    } else {
                        Some(data.endSeq)
                    },
                    proofs: Vec::new(), // Note: Cannot retrieve individual proofs from mapping
                    block_proof: optional_string(data.blockProof.clone()),
                    block_proof_submitted: data.blockProof.is_empty() == false,
                };
                Ok(Some(proof_calc))
            }
            Err(_) => Ok(None), // Proof calculation not found
        }
    }

    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<ProofCalculation>> {
        debug!("Fetching proof calculations range {}-{} for app_instance: {}", from_block, to_block, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = ProofManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getProofCalculationsRange(app_instance.to_string(), from_block, to_block)
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        let mut proof_calcs = Vec::new();

        for i in 0..result.blockNumbers.len() {
            // Skip entries that don't exist (blockNumber == 0)
            if result.blockNumbers[i] != 0 {
                let proof_calc = ProofCalculation {
                    id: format!("{}-{}", app_instance, result.blockNumbers[i]),
                    block_number: result.blockNumbers[i],
                    start_sequence: result.startSequences[i],
                    end_sequence: if result.endSequences[i] == 0 {
                        None
                    } else {
                        Some(result.endSequences[i])
                    },
                    proofs: Vec::new(), // Note: Cannot retrieve individual proofs from mapping
                    block_proof: optional_string(result.blockProofs[i].clone()),
                    block_proof_submitted: result.blockProofs[i].is_empty() == false,
                };
                proof_calcs.push(proof_calc);
            }
        }

        Ok(proof_calcs)
    }

    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<String> {
        debug!("Starting proof for block {} in app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = ProofManager::new(*self.contract.contract_address(), &provider);

        // Convert sequences to Solidity types
        let seq_vec: Vec<u64> = sequences;
        let seq1_vec: Vec<u64> = merged_sequences_1.unwrap_or_default();
        let seq2_vec: Vec<u64> = merged_sequences_2.unwrap_or_default();

        let pending_tx = contract
            .startProving(
                app_instance.to_string(),
                block_number,
                seq_vec,
                seq1_vec,
                seq2_vec,
            )
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Proof started with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn submit_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
        job_id: String,
        da_hash: String,
        cpu_cores: u8,
        prover_architecture: String,
        prover_memory: u64,
        cpu_time: u64,
    ) -> Result<String> {
        debug!("Submitting proof for block {} in app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = ProofManager::new(*self.contract.contract_address(), &provider);

        let seq_vec: Vec<u64> = sequences;
        let seq1_vec: Vec<u64> = merged_sequences_1.unwrap_or_default();
        let seq2_vec: Vec<u64> = merged_sequences_2.unwrap_or_default();

        let pending_tx = contract
            .submitProof(
                app_instance.to_string(),
                block_number,
                seq_vec,
                seq1_vec,
                seq2_vec,
                job_id,
                da_hash,
                cpu_cores,
                prover_architecture,
                prover_memory,
                cpu_time,
            )
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Proof submitted with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<String> {
        debug!("Rejecting proof for block {} in app_instance: {}", block_number, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = ProofManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .rejectProof(app_instance.to_string(), block_number, sequences)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Proof rejected with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    // ===== Settlement =====

    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>> {
        debug!("Fetching block settlement for block {} on chain {} in app_instance: {}", block_number, chain, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getBlockSettlement(app_instance.to_string(), chain.to_string(), block_number)
            .call()
            .await;

        match result {
            Ok(settlement_data) if settlement_data.exists => {
                let settlement = BlockSettlement {
                    block_number,
                    settlement_tx_hash: optional_string(settlement_data.txHash),
                    settlement_tx_included_in_block: settlement_data.txIncluded,
                    sent_to_settlement_at: if settlement_data.sentAt == 0 {
                        None
                    } else {
                        Some(settlement_data.sentAt)
                    },
                    settled_at: if settlement_data.settledAt == 0 {
                        None
                    } else {
                        Some(settlement_data.settledAt)
                    },
                };
                Ok(Some(settlement))
            }
            _ => Ok(None),
        }
    }

    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>> {
        debug!("Getting settlement chains for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getSettlementChains(app_instance.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        Ok(result)
    }

    async fn get_settlement_address(&self, app_instance: &str, chain: &str) -> Result<Option<String>> {
        debug!("Getting settlement address for chain {} in app_instance: {}", chain, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getSettlementAddress(app_instance.to_string(), chain.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        Ok(optional_string(result))
    }

    async fn get_settlement_job_for_chain(&self, app_instance: &str, chain: &str) -> Result<Option<u64>> {
        debug!("Getting settlement job for chain {} in app_instance: {}", chain, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getSettlementJobForChain(app_instance.to_string(), chain.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        if result == 0 {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }

    async fn set_settlement_address(
        &self,
        app_instance: &str,
        chain: String,
        address: Option<String>,
    ) -> Result<String> {
        debug!("Setting settlement address for chain {} in app_instance: {}", chain, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .setSettlementAddress(app_instance.to_string(), chain, address.unwrap_or_default())
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Settlement address set with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<String> {
        debug!("Updating block settlement tx hash for block {} on chain {} in app_instance: {}", block_number, chain, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .updateBlockSettlementTxHash(app_instance.to_string(), chain, block_number, settlement_tx_hash)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Block settlement tx hash updated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<String> {
        debug!("Updating block settlement inclusion for block {} on chain {} in app_instance: {}", block_number, chain, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = SettlementManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .updateBlockSettlementTxIncluded(app_instance.to_string(), chain, block_number)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("Block settlement inclusion updated with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    // ===== Key-Value Storage - String Operations =====

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        debug!("Getting KV string key '{}' for app_instance: {}", key, app_instance);

        let provider = self.contract.create_provider()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getKvString(app_instance.to_string(), key.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        if result.exists {
            Ok(Some(result.value))
        } else {
            Ok(None)
        }
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<String> {
        debug!("Setting KV string key '{}' for app_instance: {}", key, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .setKvString(app_instance.to_string(), key, value)
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("KV string set with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn delete_kv_string(&self, app_instance: &str, key: &str) -> Result<String> {
        debug!("Deleting KV string key '{}' for app_instance: {}", key, app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .deleteKvString(app_instance.to_string(), key.to_string())
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("KV string deleted with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn get_all_kv_string(&self, app_instance: &str) -> Result<std::collections::HashMap<String, String>> {
        debug!("Getting all KV strings for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getAllKvString(app_instance.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        let mut map = std::collections::HashMap::new();
        for (key, value) in result.keys.iter().zip(result.values.iter()) {
            map.insert(key.clone(), value.clone());
        }

        Ok(map)
    }

    // ===== Key-Value Storage - Binary Operations =====

    async fn get_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        debug!("Getting KV binary key for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getKvBinary(app_instance.to_string(), Bytes::from(key.to_vec()))
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        if result.exists {
            Ok(Some(result.value.to_vec()))
        } else {
            Ok(None)
        }
    }

    async fn set_kv_binary(&self, app_instance: &str, key: Vec<u8>, value: Vec<u8>) -> Result<String> {
        debug!("Setting KV binary key for app_instance: {}", app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .setKvBinary(app_instance.to_string(), Bytes::from(key), Bytes::from(value))
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("KV binary set with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn delete_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<String> {
        debug!("Deleting KV binary key for app_instance: {}", app_instance);

        let provider = self.contract.create_provider_with_signer()?;
        let contract = StorageManager::new(*self.contract.contract_address(), &provider);

        let pending_tx = contract
            .deleteKvBinary(app_instance.to_string(), Bytes::from(key.to_vec()))
            .send()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| EthereumCoordinationError::Transaction(e.to_string()))?;

        let tx_hash = self.format_tx_hash(receipt.transaction_hash);
        debug!("KV binary deleted with tx hash: {}", tx_hash);
        Ok(tx_hash)
    }

    // ===== KV List Operations =====

    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        _prefix: Option<&str>,
        _limit: Option<u32>,
    ) -> Result<Vec<String>> {
        // Not supported in Ethereum contracts - would need to maintain key index
        warn!("list_kv_string_keys not supported in Ethereum coordination layer");
        Err(EthereumCoordinationError::UnsupportedOperation(
            "list_kv_string_keys requires off-chain indexing".to_string(),
        ))
    }

    async fn list_kv_binary_keys(
        &self,
        app_instance: &str,
        _prefix: Option<&[u8]>,
        _limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>> {
        // Not supported in Ethereum contracts - would need to maintain key index
        warn!("list_kv_binary_keys not supported in Ethereum coordination layer");
        Err(EthereumCoordinationError::UnsupportedOperation(
            "list_kv_binary_keys requires off-chain indexing".to_string(),
        ))
    }

    // ===== Metadata Management =====

    async fn get_metadata(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        // Metadata stored as string KV
        self.get_kv_string(app_instance, key).await
    }

    async fn get_all_metadata(&self, app_instance: &str) -> Result<HashMap<String, String>> {
        // Cannot retrieve all keys without off-chain indexing
        warn!("get_all_metadata not fully supported - requires off-chain indexing");
        Ok(HashMap::new())
    }

    async fn add_metadata(&self, app_instance: &str, key: String, value: String) -> Result<String> {
        // Metadata stored as string KV
        self.set_kv_string(app_instance, key, value).await
    }

    // ===== App Instance Data =====

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        debug!("Fetching app instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = AppInstanceManager::new(*self.contract.contract_address(), &provider);

        let result = contract
            .getAppInstance(app_instance.to_string())
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        Ok(AppInstance {
            id: result.id,
            silvana_app_name: result.silvanaAppName,
            description: None, // Not available from contract
            metadata: HashMap::new(), // Not available from contract
            kv: HashMap::new(), // Not available from contract
            sequence: result.sequenceNumber.to::<u64>(),
            admin: format!("0x{:x}", result.owner),
            block_number: result.blockNumber,
            previous_block_timestamp: result.previousBlockTimestamp,
            previous_block_last_sequence: result.previousBlockLastSequence,
            last_proved_block_number: 0, // Not available from contract
            last_settled_block_number: 0, // Not available from contract
            last_settled_sequence: 0, // Not available from contract
            last_purged_sequence: 0, // Not available from contract
            settlements: HashMap::new(), // Not available from contract
            is_paused: result.isPaused,
            min_time_between_blocks: result.minTimeBetweenBlocks,
            created_at: timestamp_to_u64(result.createdAt),
            updated_at: timestamp_to_u64(result.updatedAt),
        })
    }

    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String> {
        let app_inst = self.fetch_app_instance(app_instance).await?;
        Ok(app_inst.admin)
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        let app_inst = self.fetch_app_instance(app_instance).await?;
        Ok(app_inst.is_paused)
    }

    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64> {
        let app_inst = self.fetch_app_instance(app_instance).await?;
        Ok(app_inst.min_time_between_blocks)
    }

    // ===== Batch Operations =====

    fn supports_multicall(&self) -> bool {
        // As per user directive: Ethereum does NOT support multicall
        false
    }

    async fn multicall_job_operations(
        &self,
        _operations: Vec<MulticallOperations>,
    ) -> Result<MulticallResult> {
        // Not supported per user directive
        Err(EthereumCoordinationError::UnsupportedOperation(
            "Multicall not supported in Ethereum coordination layer".to_string(),
        ))
    }

    // ===== State Purging =====

    async fn purge(&self, app_instance: &str, sequences_to_purge: u64) -> Result<String> {
        debug!("Purging sequences up to {} for app_instance: {}", sequences_to_purge, app_instance);

        // Check if purge method exists in contracts, if not return unsupported
        warn!("purge operation may not be supported by current contracts");
        Err(EthereumCoordinationError::UnsupportedOperation(
            "Purge operation not yet implemented in contracts".to_string(),
        ))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EthereumCoordinationConfig {
        EthereumCoordinationConfig {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 31337,
            contract_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string(),
            private_key: None,
            multicall_enabled: false,
            multicall_interval_secs: 1,
            gas_limit_multiplier: 1.2,
            max_gas_price_gwei: Some(100.0),
            confirmation_blocks: 1,
        }
    }

    #[test]
    fn test_coordination_creation() {
        let config = test_config();
        let coord = EthereumCoordination::new(config);
        assert!(coord.is_ok());
    }

    #[test]
    fn test_coordination_layer_ethereum() {
        let mut config = test_config();
        config.chain_id = 1;
        let coord = EthereumCoordination::new(config).unwrap();
        assert_eq!(coord.coordination_layer(), CoordinationLayer::Ethereum);
    }

    #[test]
    fn test_coordination_layer_polygon() {
        let mut config = test_config();
        config.chain_id = 137;
        let coord = EthereumCoordination::new(config).unwrap();
        assert_eq!(coord.coordination_layer(), CoordinationLayer::Polygon);
    }

    #[test]
    fn test_coordination_layer_base() {
        let mut config = test_config();
        config.chain_id = 8453;
        let coord = EthereumCoordination::new(config).unwrap();
        assert_eq!(coord.coordination_layer(), CoordinationLayer::Base);
    }

    #[test]
    fn test_chain_id() {
        let config = test_config();
        let coord = EthereumCoordination::new(config).unwrap();
        assert_eq!(coord.chain_id(), "31337");
    }

    #[test]
    fn test_format_tx_hash() {
        let config = test_config();
        let coord = EthereumCoordination::new(config).unwrap();
        let hash = TxHash::from([0u8; 32]);
        assert_eq!(
            coord.format_tx_hash(hash),
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
    }
}
