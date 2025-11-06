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
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::{
    abi::*,
    abi::job_manager::DataTypes,
    config::EthereumCoordinationConfig,
    contract::ContractClient,
    conversions::*,
    error::{EthereumCoordinationError, Result},
};

use alloy::primitives::{Address, Bytes, TxHash, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::Filter;
use alloy::sol;
use alloy::sol_types::SolEvent;
use async_stream::stream;
use futures::StreamExt;
use std::str::FromStr;

// Define JobCreated event ABI using Alloy's sol! macro
// This must match the Solidity event definition in DataTypes.sol
sol! {
    #[derive(Debug)]
    event JobCreated(
        string appInstance,
        uint64 indexed jobSequence,
        string developer,
        string agent,
        string agentMethod,
        uint256 timestamp
    );
}

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

    /// JobManager contract address (for event listening)
    job_manager_address: Address,
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

        // Parse JobManager address if provided, otherwise use coordination address as fallback
        // The event stream will fetch the actual JobManager address lazily if needed
        let job_manager_address = if let Some(ref jm_addr) = config.job_manager_address {
            Address::from_str(jm_addr).map_err(|e| {
                EthereumCoordinationError::Configuration(format!(
                    "Invalid job_manager_address '{}': {}",
                    jm_addr, e
                ))
            })?
        } else {
            // Use coordination address as temporary fallback
            // Will be replaced by actual JobManager address when event stream starts
            *contract.contract_address()
        };

        info!("Ethereum coordination layer initialized successfully");

        Ok(Self {
            contract,
            config,
            chain_id,
            job_manager_address,
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

    /// Decode JobCreated event from Ethereum log (currently unused, kept for reference)
    #[allow(dead_code)]
    fn decode_job_created_event(&self, log: &alloy::rpc::types::Log) -> Result<JobCreatedEvent> {
        // Decode the log using Alloy's generated event decoder
        let decoded = JobCreated::decode_log_data(&log.inner.data)
            .map_err(|e| EthereumCoordinationError::EventParsing(format!("Failed to decode JobCreated event: {}", e)))?;

        // Get block number from log metadata
        let block_number = log.block_number.unwrap_or(0);

        // With the Solidity fix, appInstance is no longer indexed,
        // so we get the full string value directly from the event
        Ok(JobCreatedEvent {
            job_sequence: decoded.jobSequence,
            developer: decoded.developer,
            agent: decoded.agent,
            agent_method: decoded.agentMethod,
            app_instance: decoded.appInstance,
            app_instance_method: String::new(), // Not in Solidity event
            block_number,
            created_at: decoded.timestamp.try_into().unwrap_or(0),
        })
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
                    agent_jwt: None, // Ethereum layer doesn't support JWT
                    jwt_expires_at: None,
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
                    agent_jwt: None, // Ethereum layer doesn't support JWT
                    jwt_expires_at: None,
                }
            })
            .collect();

        debug!("Fetched {} failed jobs", jobs.len());
        Ok(jobs)
    }

    async fn fetch_running_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        debug!("Fetching running jobs for app_instance: {}", app_instance);

        let provider = self.contract.create_provider()?;
        let contract = JobManager::new(*self.contract.contract_address(), &provider);

        // Use getActiveJobs which should return Running jobs
        let active_jobs = contract
            .getActiveJobs(app_instance.to_string(), U256::from(1000u64), U256::from(0u64))
            .call()
            .await
            .map_err(|e| EthereumCoordinationError::ContractCall(e.to_string()))?;

        // Convert to our Job type - active jobs are Running status
        let running_jobs: Vec<Job> = active_jobs
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
                    status: JobStatus::Running, // We know it's Running from the filter
                    attempts: solidity_job.attempts,
                    interval_ms: optional_u64(solidity_job.intervalMs),
                    next_scheduled_at: optional_u64(solidity_job.nextScheduledAt),
                    created_at: timestamp_to_u64(solidity_job.createdAt),
                    updated_at: timestamp_to_u64(solidity_job.updatedAt),
                    agent_jwt: None, // Ethereum layer doesn't support JWT
                    jwt_expires_at: None,
                }
            })
            .collect();

        debug!("Fetched {} running jobs", running_jobs.len());
        Ok(running_jobs)
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

    async fn fetch_job_by_sequence(
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
            agent_jwt: None, // Ethereum layer doesn't support JWT
            jwt_expires_at: None,
        };

        Ok(Some(job))
    }

    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64> {
        debug!("Getting pending jobs count for app_instance: {}", app_instance);

        // Fetch pending jobs and count them
        let pending_jobs = self.fetch_pending_jobs(app_instance).await?;
        Ok(pending_jobs.len() as u64)
    }

    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64> {
        debug!("Getting total jobs count for app_instance: {}", app_instance);

        // Ethereum layer doesn't track total jobs count easily
        // Return sum of pending + failed counts as approximation
        let pending = self.get_pending_jobs_count(app_instance).await?;
        let failed = self.get_failed_jobs_count(app_instance).await?;
        Ok(pending + failed)
    }

    async fn get_settlement_job_sequences(&self, _app_instance: &str) -> Result<HashMap<String, u64>> {
        // Ethereum layer doesn't implement settlement jobs in the same way as Sui
        // Settlement is handled differently on Ethereum
        Ok(HashMap::new())
    }

    async fn get_jobs_info(&self, _app_instance: &str) -> Result<Option<(String, String)>> {
        // Ethereum doesn't use object tables like Sui
        // Jobs are stored in contract mappings
        Ok(None)
    }

    async fn fetch_jobs_batch(&self, app_instance: &str, job_ids: &[u64]) -> Result<Vec<Job>> {
        debug!("Fetching batch of {} jobs for app_instance: {}", job_ids.len(), app_instance);

        let mut jobs = Vec::new();
        for &job_id in job_ids {
            if let Some(job) = self.fetch_job_by_sequence(app_instance, job_id).await? {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    async fn fetch_pending_job_sequences(&self, app_instance: &str) -> Result<Vec<u64>> {
        debug!("Fetching pending job sequences for app_instance: {}", app_instance);

        // Fetch pending jobs and extract their sequences
        let pending_jobs = self.fetch_pending_jobs(app_instance).await?;
        Ok(pending_jobs.into_iter().map(|j| j.job_sequence).collect())
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>> {
        debug!("Fetching pending job sequences by method for app_instance: {}", app_instance);

        // Fetch all pending jobs and filter by developer, agent, and method
        let pending_jobs = self.fetch_pending_jobs(app_instance).await?;
        Ok(pending_jobs
            .into_iter()
            .filter(|j| {
                j.developer == developer && j.agent == agent && j.agent_method == agent_method
            })
            .map(|j| j.job_sequence)
            .collect())
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
        _block_number: Option<u64>,
        _sequences: Option<Vec<u64>>,
        _sequences1: Option<Vec<u64>>,
        _sequences2: Option<Vec<u64>>,
        data: Vec<u8>,
        interval_ms: Option<u64>,
        _next_scheduled_at: Option<u64>,
        _settlement_chain: Option<String>,
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
                let job_sequence = decoded.data().jobSequence;
                let tx_hash = self.format_tx_hash(receipt.transaction_hash);
                debug!("Created job with sequence: {}, tx hash: {}", job_sequence, tx_hash);
                return Ok((tx_hash, job_sequence));
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
        _sequences: Option<Vec<u64>>,
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
        _settled_at: u64,
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
        _app_instance: &str,
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
        _app_instance: &str,
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

    async fn get_all_metadata(&self, _app_instance: &str) -> Result<HashMap<String, String>> {
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

    // ===== Event Streaming =====

    async fn event_stream(&self) -> Result<EventStream> {
        // Check if WebSocket URL is configured
        let ws_url = self.config.ws_url.as_ref()
            .ok_or_else(|| EthereumCoordinationError::Configuration(
                "WebSocket URL not configured - cannot create event stream".to_string()
            ))?;

        let ws_url_clone = ws_url.clone();
        let job_manager_address = self.job_manager_address;

        info!("🔌 Ethereum: Connecting to WebSocket at {}", ws_url);
        info!("👁️  Ethereum: Listening for JobCreated events from JobManager at {}", job_manager_address);

        let stream = stream! {
            loop {
                // Connect to WebSocket using ProviderBuilder
                match ProviderBuilder::new().connect(&ws_url_clone).await {
                    Ok(provider) => {
                        info!("✅ Ethereum: Connected to WebSocket event stream");

                        // Create filter for JobCreated events from JobManager contract
                        // Only subscribe to new events (from Latest block)
                        let filter = Filter::new()
                            .address(job_manager_address)
                            .event(JobCreated::SIGNATURE)
                            .from_block(alloy::rpc::types::BlockNumberOrTag::Latest);

                        // Subscribe to logs
                        match provider.subscribe_logs(&filter).await {
                            Ok(log_stream) => {
                                info!("✅ Ethereum: Subscribed to JobCreated events");

                                // Convert subscription to stream and process logs as they arrive
                                let mut stream = log_stream.into_stream();
                                while let Some(log) = stream.next().await {
                                    // Decode the event
                                    match JobCreated::decode_log_data(&log.inner.data) {
                                        Ok(decoded) => {
                                            // Get block number from log metadata, not from event data
                                            let block_number = log.block_number.unwrap_or(0);

                                            // With the Solidity fix, appInstance is no longer indexed,
                                            // so we get the full string value directly from the event
                                            let event = JobCreatedEvent {
                                                job_sequence: decoded.jobSequence,
                                                developer: decoded.developer,
                                                agent: decoded.agent,
                                                agent_method: decoded.agentMethod,
                                                app_instance: decoded.appInstance,
                                                app_instance_method: String::new(), // Not in Solidity event
                                                block_number,
                                                created_at: decoded.timestamp.try_into().unwrap_or(0),
                                            };

                                            debug!(
                                                "Ethereum: JobCreated event - seq={}, app={}",
                                                event.job_sequence,
                                                &event.app_instance[..16.min(event.app_instance.len())]
                                            );

                                            yield Ok(event);
                                        }
                                        Err(e) => {
                                            error!("Ethereum: Failed to decode JobCreated event: {}", e);
                                            yield Err(Box::new(
                                                EthereumCoordinationError::EventParsing(
                                                    format!("Failed to decode event: {}", e)
                                                )
                                            ) as Box<dyn std::error::Error + Send + Sync>);
                                        }
                                    }
                                }

                                // If we get here, stream ended
                                warn!("Ethereum: Log subscription stream ended");
                            }
                            Err(e) => {
                                error!("Ethereum: Failed to subscribe to logs: {}", e);
                                yield Err(Box::new(
                                    EthereumCoordinationError::Connection(
                                        format!("Failed to subscribe to logs: {}", e)
                                    )
                                ) as Box<dyn std::error::Error + Send + Sync>);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Ethereum: WebSocket connection failed: {}", e);
                        yield Err(Box::new(
                            EthereumCoordinationError::Connection(
                                format!("WebSocket connection failed: {}", e)
                            )
                        ) as Box<dyn std::error::Error + Send + Sync>);
                    }
                }

                // Wait before reconnecting
                warn!("Ethereum: WebSocket stream disconnected, reconnecting in 5s...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        };

        Ok(Box::pin(stream))
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
