//! Coordination Trait - Core abstraction for multiple coordination layers
//!
//! This crate defines the `Coordination` trait which provides a unified interface
//! for different coordination layers (Sui, Private/TiDB, future Ethereum/Solana).

use async_trait::async_trait;
use futures_util::stream::Stream;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::pin::Pin;

pub mod error;
pub mod layer;
pub mod types;

pub use error::{CoordinationError, CoordinationResult};
pub use layer::{CoordinationLayer, CoordinationLayerOperationMode};
pub use types::*;

/// Type alias for event stream
/// Returns a stream of JobCreatedEvent or errors
pub type EventStream = Pin<Box<dyn Stream<Item = Result<JobCreatedEvent, Box<dyn std::error::Error + Send + Sync>>> + Send>>;

/// Main Coordination trait that all coordination layers must implement
///
/// This trait is object-safe, allowing dynamic dispatch with `Box<dyn Coordination>`.
/// All ID types use concrete types (u64 for sequences/blocks, String for transactions)
/// to enable trait object usage.
#[async_trait]
pub trait Coordination: Send + Sync {
    /// Type for Transaction Hashes (varies by blockchain)
    type TransactionHash: Display + Clone + Send + Sync + Debug;

    /// Error type for this coordination layer
    type Error: std::error::Error + Send + Sync + 'static;

    // ===== Coordination Layer Identification =====

    /// Get the type of this coordination layer
    fn coordination_layer(&self) -> CoordinationLayer;

    /// Get the chain/network ID
    fn chain_id(&self) -> String;

    // ===== Job Management =====

    // Read operations

    /// Fetch all pending jobs for an app instance
    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job>, Self::Error>;

    /// Fetch all failed jobs for an app instance
    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>, Self::Error>;

    /// Get the count of failed jobs
    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error>;

    /// Fetch a specific job by its sequence number
    async fn fetch_job_by_id(&self, app_instance: &str, job_sequence: u64) -> Result<Option<Job>, Self::Error>;

    /// Get the count of pending jobs for an app instance
    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error>;

    /// Get the count of all jobs for an app instance
    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error>;

    /// Get settlement job IDs for an app instance (maps chain name to job ID)
    async fn get_settlement_job_ids(&self, app_instance: &str) -> Result<HashMap<String, u64>, Self::Error>;

    /// Get jobs table info (returns app_instance_id and jobs_table_id)
    /// This is layer-specific metadata needed for batch job operations
    async fn get_jobs_info(&self, app_instance: &str) -> Result<Option<(String, String)>, Self::Error>;

    /// Fetch multiple jobs by their IDs in a batch
    async fn fetch_jobs_batch(&self, app_instance: &str, job_ids: &[u64]) -> Result<Vec<Job>, Self::Error>;

    /// Fetch pending job sequences for an app instance
    async fn fetch_pending_job_sequences(&self, app_instance: &str) -> Result<Vec<u64>, Self::Error>;

    /// Fetch pending job sequences filtered by developer, agent, and agent method
    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>, Self::Error>;

    // Write operations

    /// Start a job (returns true if successfully started)
    async fn start_job(&self, app_instance: &str, job_sequence: u64) -> Result<bool, Self::Error>;

    /// Mark a job as completed
    async fn complete_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash, Self::Error>;

    /// Mark a job as failed with an error message
    async fn fail_job(&self, app_instance: &str, job_sequence: u64, error: &str) -> Result<Self::TransactionHash, Self::Error>;

    /// Terminate a job
    async fn terminate_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash, Self::Error>;

    /// Create a new app job
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
    ) -> Result<(Self::TransactionHash, u64), Self::Error>;

    /// Create a merge job with proving
    async fn create_merge_job_with_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Create a settlement job
    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Terminate an app job by its ID
    async fn terminate_app_job(&self, app_instance: &str, job_id: u64) -> Result<Self::TransactionHash, Self::Error>;

    /// Restart failed jobs (optionally specify which job sequences)
    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        job_sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Remove failed jobs from the failed jobs table
    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Sequence State Management =====

    // Read operations

    /// Fetch the state at a specific sequence
    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>, Self::Error>;

    /// Fetch states for a range of sequences
    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>, Self::Error>;

    /// Get the current sequence number
    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64, Self::Error>;

    // Write operations

    /// Update the state for a specific sequence
    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Block Management =====

    // Read operations

    /// Fetch a block by its number
    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>, Self::Error>;

    /// Fetch blocks in a range
    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>, Self::Error>;

    // Write operations

    /// Try to create a new block (returns block number if created)
    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>, Self::Error>;

    /// Update block's state data availability
    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_da: String,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Update block's proof data availability
    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_da: String,
    ) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Proof Management =====

    // Read operations

    /// Fetch proof calculation for a block
    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<ProofCalculation>, Self::Error>;

    /// Fetch proof calculations for a range of blocks
    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<ProofCalculation>, Self::Error>;

    // Write operations

    /// Start proving (reserve proof)
    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Submit a proof
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
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Reject a proof
    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Settlement =====

    // Read operations

    /// Fetch block settlement information
    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>, Self::Error>;

    /// Get all configured settlement chains
    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>, Self::Error>;

    /// Get settlement address for a specific chain
    async fn get_settlement_address(&self, app_instance: &str, chain: &str) -> Result<Option<String>, Self::Error>;

    /// Get active settlement job for a chain
    async fn get_settlement_job_for_chain(&self, app_instance: &str, chain: &str) -> Result<Option<u64>, Self::Error>;

    // Write operations

    /// Set settlement address for a chain
    async fn set_settlement_address(
        &self,
        app_instance: &str,
        chain: String,
        address: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Update block settlement transaction hash
    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<Self::TransactionHash, Self::Error>;

    /// Update block settlement inclusion
    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Key-Value Storage =====

    // String KV - Read operations

    /// Get a string value by key
    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>, Self::Error>;

    /// Get all string key-value pairs
    async fn get_all_kv_string(&self, app_instance: &str) -> Result<HashMap<String, String>, Self::Error>;

    /// List string keys with optional prefix filter
    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, Self::Error>;

    // String KV - Write operations

    /// Set a string key-value pair
    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<Self::TransactionHash, Self::Error>;

    /// Delete a string key
    async fn delete_kv_string(&self, app_instance: &str, key: &str) -> Result<Self::TransactionHash, Self::Error>;

    // Binary KV - Read operations

    /// Get a binary value by key
    async fn get_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;

    /// List binary keys with optional prefix filter
    async fn list_kv_binary_keys(
        &self,
        app_instance: &str,
        prefix: Option<&[u8]>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>, Self::Error>;

    // Binary KV - Write operations

    /// Set a binary key-value pair
    async fn set_kv_binary(&self, app_instance: &str, key: Vec<u8>, value: Vec<u8>) -> Result<Self::TransactionHash, Self::Error>;

    /// Delete a binary key
    async fn delete_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Metadata Management =====

    // Read operations

    /// Get metadata value by key
    async fn get_metadata(&self, app_instance: &str, key: &str) -> Result<Option<String>, Self::Error>;

    /// Get all metadata
    async fn get_all_metadata(&self, app_instance: &str) -> Result<HashMap<String, String>, Self::Error>;

    // Write operations

    /// Add or update metadata
    async fn add_metadata(&self, app_instance: &str, key: String, value: String) -> Result<Self::TransactionHash, Self::Error>;

    // ===== App Instance Data =====

    /// Fetch app instance information
    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance, Self::Error>;

    /// Get app instance admin address
    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String, Self::Error>;

    /// Check if app is paused
    async fn is_app_paused(&self, app_instance: &str) -> Result<bool, Self::Error>;

    /// Get minimum time between blocks
    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64, Self::Error>;

    // ===== Batch Operations =====

    /// Check if this coordination layer supports/requires multicall batching
    fn supports_multicall(&self) -> bool;

    /// Execute multiple operations in a single transaction (only if supports_multicall returns true)
    async fn multicall_job_operations(
        &self,
        operations: Vec<MulticallOperations>,
    ) -> Result<MulticallResult, Self::Error>;

    // ===== State Purging =====

    /// Purge old states up to a certain sequence
    async fn purge(&self, app_instance: &str, sequences_to_purge: u64) -> Result<Self::TransactionHash, Self::Error>;

    // ===== Event Streaming =====

    /// Subscribe to coordination layer events (new jobs only)
    ///
    /// Returns a stream of JobCreatedEvent that can be consumed by the coordinator.
    /// The stream should:
    /// - Emit events only for NEW jobs (not historical jobs)
    /// - Handle reconnections automatically with exponential backoff
    /// - Skip backfilling of historical events (only new events from now on)
    /// - Filter for JobCreated events only
    /// - Continue running indefinitely until the coordinator shuts down
    ///
    /// # Implementation Notes
    /// - Sui: Uses gRPC checkpoint subscription
    /// - Ethereum: Uses WebSocket eth_subscribe with logs filter
    /// - Private: May return empty stream if events aren't supported
    async fn event_stream(&self) -> Result<EventStream, Self::Error>;
}