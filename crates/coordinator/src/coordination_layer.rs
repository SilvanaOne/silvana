//! Coordination layer enum with unified error handling
//!
//! This module provides a unified `CoordinationLayer` enum that wraps all coordination
//! layer implementations (Sui, Private, Ethereum) and implements the `Coordination` trait
//! directly. This eliminates the need for trait objects while preserving error type
//! information through a unified error enum.

use async_trait::async_trait;
use silvana_coordination_trait::{
    AppInstance, Block, Coordination, CoordinationLayer as LayerType, Job, SequenceState,
};
use std::collections::HashMap;
use thiserror::Error;

/// Unified error type wrapping all coordination layer errors
///
/// This enum preserves the specific error types from each coordination layer,
/// allowing access to layer-specific error information and helper methods.
#[derive(Debug, Error)]
pub enum CoordinationError {
    /// Sui coordination layer error
    #[error("Sui coordination error: {0}")]
    Sui(#[from] silvana_coordination_sui::SuiCoordinationError),

    /// Private coordination layer error
    #[error("Private coordination error: {0}")]
    Private(#[from] silvana_coordination_private::PrivateCoordinationError),

    /// Ethereum coordination layer error
    #[error("Ethereum coordination error: {0}")]
    Ethereum(#[from] silvana_coordination_ethereum::EthereumCoordinationError),

    /// Sui interface error
    #[error("Sui interface error: {0}")]
    SuiInterface(#[from] sui::error::SilvanaSuiInterfaceError),
}

impl CoordinationError {
    /// Check if this error is retriable
    ///
    /// Delegates to the underlying layer-specific error's `is_retriable()` method.
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::Sui(e) => e.is_retriable(),
            Self::Ethereum(e) => e.is_retriable(),
            Self::Private(_) => false, // Private layer doesn't have is_retriable
            Self::SuiInterface(_) => false,
        }
    }

    /// Check if this error indicates a network problem
    pub fn is_network_error(&self) -> bool {
        match self {
            Self::Sui(e) => e.is_network_error(),
            Self::Ethereum(e) => e.is_network_error(),
            Self::Private(_) => false,
            Self::SuiInterface(_) => false,
        }
    }

    /// Check if this error indicates a configuration problem
    pub fn is_configuration_error(&self) -> bool {
        match self {
            Self::Sui(e) => e.is_configuration_error(),
            Self::Ethereum(e) => e.is_configuration_error(),
            Self::Private(_) => false,
            Self::SuiInterface(_) => false,
        }
    }
}

/// Coordination layer enum wrapping concrete implementations
///
/// This enum directly implements the `Coordination` trait by delegating to
/// the underlying implementation. No wrapper methods needed - just call
/// the trait methods directly on the enum.
pub enum CoordinationLayer {
    /// Sui blockchain coordination layer
    Sui(silvana_coordination_sui::SuiCoordination),

    /// Private (TiDB) coordination layer
    Private(silvana_coordination_private::PrivateCoordination),

    /// Ethereum coordination layer
    Ethereum(silvana_coordination_ethereum::EthereumCoordination),
}

impl CoordinationLayer {
    /// Create a new Sui coordination layer
    pub fn new_sui(layer_id: String) -> Self {
        Self::Sui(silvana_coordination_sui::SuiCoordination::with_chain_id(
            layer_id,
        ))
    }

    /// Create a new Private coordination layer
    pub async fn new_private(
        config: silvana_coordination_private::PrivateCoordinationConfig,
    ) -> Result<Self, CoordinationError> {
        Ok(Self::Private(
            silvana_coordination_private::PrivateCoordination::new(config).await?,
        ))
    }

    /// Create a new Ethereum coordination layer
    pub fn new_ethereum(
        config: silvana_coordination_ethereum::EthereumCoordinationConfig,
    ) -> Result<Self, CoordinationError> {
        Ok(Self::Ethereum(
            silvana_coordination_ethereum::EthereumCoordination::new(config)?,
        ))
    }

    /// Get the layer ID
    ///
    /// For now, this returns the chain_id. In the current implementation,
    /// layer_id and chain_id are the same value.
    pub fn layer_id(&self) -> String {
        self.chain_id()
    }
}

/// Implement the Coordination trait directly on the enum
///
/// All methods simply match on the enum variant and delegate to the
/// underlying implementation, converting errors to the unified CoordinationError.
#[async_trait]
impl Coordination for CoordinationLayer {
    type TransactionHash = String;
    type Error = CoordinationError;

    // ===== Coordination Layer Identification =====

    fn coordination_layer(&self) -> LayerType {
        match self {
            Self::Sui(sui) => sui.coordination_layer(),
            Self::Private(private) => private.coordination_layer(),
            Self::Ethereum(eth) => eth.coordination_layer(),
        }
    }

    fn chain_id(&self) -> String {
        match self {
            Self::Sui(sui) => sui.chain_id(),
            Self::Private(private) => private.chain_id(),
            Self::Ethereum(eth) => eth.chain_id(),
        }
    }

    // ===== Job Management =====

    async fn fetch_pending_jobs(
        &self,
        app_instance: &str,
    ) -> Result<Vec<Job>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_pending_jobs(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_pending_jobs(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_pending_jobs(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_failed_jobs(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_failed_jobs(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_failed_jobs(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_failed_jobs_count(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_failed_jobs_count(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_failed_jobs_count(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn fetch_job_by_id(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Option<Job>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_job_by_id(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_job_by_id(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_job_by_id(app_instance, job_sequence)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        match self {
            Self::Sui(sui) => sui.get_pending_jobs_count(app_instance).await.map_err(Into::into),
            Self::Private(private) => private.get_pending_jobs_count(app_instance).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.get_pending_jobs_count(app_instance).await.map_err(Into::into),
        }
    }

    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        match self {
            Self::Sui(sui) => sui.get_total_jobs_count(app_instance).await.map_err(Into::into),
            Self::Private(private) => private.get_total_jobs_count(app_instance).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.get_total_jobs_count(app_instance).await.map_err(Into::into),
        }
    }

    async fn get_settlement_job_ids(&self, app_instance: &str) -> Result<HashMap<String, u64>, Self::Error> {
        match self {
            Self::Sui(sui) => sui.get_settlement_job_ids(app_instance).await.map_err(Into::into),
            Self::Private(private) => private.get_settlement_job_ids(app_instance).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.get_settlement_job_ids(app_instance).await.map_err(Into::into),
        }
    }

    async fn get_jobs_info(&self, app_instance: &str) -> Result<Option<(String, String)>, Self::Error> {
        match self {
            Self::Sui(sui) => sui.get_jobs_info(app_instance).await.map_err(Into::into),
            Self::Private(private) => private.get_jobs_info(app_instance).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.get_jobs_info(app_instance).await.map_err(Into::into),
        }
    }

    async fn fetch_jobs_batch(&self, app_instance: &str, job_ids: &[u64]) -> Result<Vec<Job>, Self::Error> {
        match self {
            Self::Sui(sui) => sui.fetch_jobs_batch(app_instance, job_ids).await.map_err(Into::into),
            Self::Private(private) => private.fetch_jobs_batch(app_instance, job_ids).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.fetch_jobs_batch(app_instance, job_ids).await.map_err(Into::into),
        }
    }

    async fn fetch_pending_job_sequences(&self, app_instance: &str) -> Result<Vec<u64>, Self::Error> {
        match self {
            Self::Sui(sui) => sui.fetch_pending_job_sequences(app_instance).await.map_err(Into::into),
            Self::Private(private) => private.fetch_pending_job_sequences(app_instance).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.fetch_pending_job_sequences(app_instance).await.map_err(Into::into),
        }
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>, Self::Error> {
        match self {
            Self::Sui(sui) => sui.fetch_pending_job_sequences_by_method(app_instance, developer, agent, agent_method).await.map_err(Into::into),
            Self::Private(private) => private.fetch_pending_job_sequences_by_method(app_instance, developer, agent, agent_method).await.map_err(Into::into),
            Self::Ethereum(eth) => eth.fetch_pending_job_sequences_by_method(app_instance, developer, agent, agent_method).await.map_err(Into::into),
        }
    }

    async fn start_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<bool, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .start_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .start_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .start_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
        }
    }

    async fn complete_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .complete_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .complete_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .complete_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
        }
    }

    async fn fail_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
        error: &str,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fail_job(app_instance, job_sequence, error)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fail_job(app_instance, job_sequence, error)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fail_job(app_instance, job_sequence, error)
                .await
                .map_err(Into::into),
        }
    }

    async fn terminate_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .terminate_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .terminate_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .terminate_job(app_instance, job_sequence)
                .await
                .map_err(Into::into),
        }
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
    ) -> Result<(Self::TransactionHash, u64), Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .create_app_job(
                    app_instance,
                    method_name,
                    job_description,
                    block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data,
                    interval_ms,
                    next_scheduled_at,
                    settlement_chain,
                )
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .create_app_job(
                    app_instance,
                    method_name,
                    job_description,
                    block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data,
                    interval_ms,
                    next_scheduled_at,
                    settlement_chain,
                )
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .create_app_job(
                    app_instance,
                    method_name,
                    job_description,
                    block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data,
                    interval_ms,
                    next_scheduled_at,
                    settlement_chain,
                )
                .await
                .map_err(Into::into),
        }
    }

    async fn create_merge_job_with_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .create_merge_job_with_proving(app_instance, block_number, sequences, sequences1, sequences2, job_description)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .create_merge_job_with_proving(app_instance, block_number, sequences, sequences1, sequences2, job_description)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .create_merge_job_with_proving(app_instance, block_number, sequences, sequences1, sequences2, job_description)
                .await
                .map_err(Into::into),
        }
    }

    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .create_settle_job(app_instance, block_number, chain, job_description)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .create_settle_job(app_instance, block_number, chain, job_description)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .create_settle_job(app_instance, block_number, chain, job_description)
                .await
                .map_err(Into::into),
        }
    }

    async fn terminate_app_job(
        &self,
        app_instance: &str,
        job_id: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .terminate_app_job(app_instance, job_id)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .terminate_app_job(app_instance, job_id)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .terminate_app_job(app_instance, job_id)
                .await
                .map_err(Into::into),
        }
    }

    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .restart_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .restart_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .restart_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
        }
    }

    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .remove_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .remove_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .remove_failed_jobs(app_instance, sequences)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(
        &self,
        app_instance: &str,
        sequence: u64,
    ) -> Result<Option<SequenceState>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_sequence_state(app_instance, sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_sequence_state(app_instance, sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_sequence_state(app_instance, sequence)
                .await
                .map_err(Into::into),
        }
    }

    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_sequence_states_range(app_instance, from_sequence, to_sequence)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_sequence_states_range(app_instance, from_sequence, to_sequence)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_sequence_states_range(app_instance, from_sequence, to_sequence)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_current_sequence(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_current_sequence(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_current_sequence(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .update_state_for_sequence(app_instance, sequence, new_state_data, new_data_availability_hash)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .update_state_for_sequence(app_instance, sequence, new_state_data, new_data_availability_hash)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .update_state_for_sequence(app_instance, sequence, new_state_data, new_data_availability_hash)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Block Management =====

    async fn fetch_block(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<Block>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_block(app_instance, block_number)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_block(app_instance, block_number)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_block(app_instance, block_number)
                .await
                .map_err(Into::into),
        }
    }

    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_blocks_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_blocks_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_blocks_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
        }
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .try_create_block(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .try_create_block(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .try_create_block(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_data_availability: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .update_block_state_data_availability(
                    app_instance,
                    block_number,
                    state_data_availability,
                )
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .update_block_state_data_availability(
                    app_instance,
                    block_number,
                    state_data_availability,
                )
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .update_block_state_data_availability(
                    app_instance,
                    block_number,
                    state_data_availability,
                )
                .await
                .map_err(Into::into),
        }
    }

    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_data_availability: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .update_block_proof_data_availability(
                    app_instance,
                    block_number,
                    proof_data_availability,
                )
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .update_block_proof_data_availability(
                    app_instance,
                    block_number,
                    proof_data_availability,
                )
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .update_block_proof_data_availability(
                    app_instance,
                    block_number,
                    proof_data_availability,
                )
                .await
                .map_err(Into::into),
        }
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<silvana_coordination_trait::ProofCalculation>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_proof_calculation(app_instance, block_number)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_proof_calculation(app_instance, block_number)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_proof_calculation(app_instance, block_number)
                .await
                .map_err(Into::into),
        }
    }

    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<silvana_coordination_trait::ProofCalculation>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_proof_calculations_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_proof_calculations_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_proof_calculations_range(app_instance, from_block, to_block)
                .await
                .map_err(Into::into),
        }
    }

    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .start_proving(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .start_proving(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .start_proving(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2)
                .await
                .map_err(Into::into),
        }
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
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .submit_proof(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2, job_id, da_hash, cpu_cores, prover_architecture, prover_memory, cpu_time)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .submit_proof(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2, job_id, da_hash, cpu_cores, prover_architecture, prover_memory, cpu_time)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .submit_proof(app_instance, block_number, sequences, merged_sequences_1, merged_sequences_2, job_id, da_hash, cpu_cores, prover_architecture, prover_memory, cpu_time)
                .await
                .map_err(Into::into),
        }
    }

    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .reject_proof(app_instance, block_number, sequences)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .reject_proof(app_instance, block_number, sequences)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .reject_proof(app_instance, block_number, sequences)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Settlement Management =====

    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<silvana_coordination_trait::BlockSettlement>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_block_settlement(app_instance, block_number, chain)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_block_settlement(app_instance, block_number, chain)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_block_settlement(app_instance, block_number, chain)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_settlement_chains(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_settlement_chains(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_settlement_chains(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_settlement_address(
        &self,
        app_instance: &str,
        chain: &str,
    ) -> Result<Option<String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_settlement_address(app_instance, chain)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_settlement_address(app_instance, chain)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_settlement_address(app_instance, chain)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_settlement_job_for_chain(
        &self,
        app_instance: &str,
        chain: &str,
    ) -> Result<Option<u64>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_settlement_job_for_chain(app_instance, chain)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_settlement_job_for_chain(app_instance, chain)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_settlement_job_for_chain(app_instance, chain)
                .await
                .map_err(Into::into),
        }
    }

    async fn set_settlement_address(
        &self,
        app_instance: &str,
        chain: String,
        address: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .set_settlement_address(app_instance, chain, address)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .set_settlement_address(app_instance, chain, address)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .set_settlement_address(app_instance, chain, address)
                .await
                .map_err(Into::into),
        }
    }

    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        tx_hash: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .update_block_settlement_tx_hash(app_instance, block_number, chain, tx_hash)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .update_block_settlement_tx_hash(app_instance, block_number, chain, tx_hash)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .update_block_settlement_tx_hash(app_instance, block_number, chain, tx_hash)
                .await
                .map_err(Into::into),
        }
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        included_in_block: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .update_block_settlement_tx_included_in_block(
                    app_instance,
                    block_number,
                    chain,
                    included_in_block,
                )
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .update_block_settlement_tx_included_in_block(
                    app_instance,
                    block_number,
                    chain,
                    included_in_block,
                )
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .update_block_settlement_tx_included_in_block(
                    app_instance,
                    block_number,
                    chain,
                    included_in_block,
                )
                .await
                .map_err(Into::into),
        }
    }

    // ===== KV Storage =====

    async fn get_kv_string(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Option<String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_all_kv_string(
        &self,
        app_instance: &str,
    ) -> Result<HashMap<String, String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_all_kv_string(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_all_kv_string(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_all_kv_string(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .list_kv_string_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .list_kv_string_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .list_kv_string_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
        }
    }

    async fn set_kv_string(
        &self,
        app_instance: &str,
        key: String,
        value: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .set_kv_string(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .set_kv_string(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .set_kv_string(app_instance, key, value)
                .await
                .map_err(Into::into),
        }
    }

    async fn delete_kv_string(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .delete_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .delete_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .delete_kv_string(app_instance, key)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_kv_binary(
        &self,
        app_instance: &str,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
        }
    }

    async fn list_kv_binary_keys(
        &self,
        app_instance: &str,
        prefix: Option<&[u8]>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .list_kv_binary_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .list_kv_binary_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .list_kv_binary_keys(app_instance, prefix, limit)
                .await
                .map_err(Into::into),
        }
    }

    async fn set_kv_binary(
        &self,
        app_instance: &str,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .set_kv_binary(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .set_kv_binary(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .set_kv_binary(app_instance, key, value)
                .await
                .map_err(Into::into),
        }
    }

    async fn delete_kv_binary(
        &self,
        app_instance: &str,
        key: &[u8],
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .delete_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .delete_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .delete_kv_binary(app_instance, key)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Metadata =====

    async fn get_metadata(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Option<String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_metadata(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_metadata(app_instance, key)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_metadata(app_instance, key)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_all_metadata(
        &self,
        app_instance: &str,
    ) -> Result<HashMap<String, String>, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_all_metadata(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_all_metadata(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_all_metadata(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn add_metadata(
        &self,
        app_instance: &str,
        key: String,
        value: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .add_metadata(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .add_metadata(app_instance, key, value)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .add_metadata(app_instance, key, value)
                .await
                .map_err(Into::into),
        }
    }

    // ===== App Instance Operations =====

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .fetch_app_instance(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .fetch_app_instance(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .fetch_app_instance(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_app_instance_admin(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_app_instance_admin(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_app_instance_admin(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .is_app_paused(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .is_app_paused(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .is_app_paused(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .get_min_time_between_blocks(app_instance)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .get_min_time_between_blocks(app_instance)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .get_min_time_between_blocks(app_instance)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Multicall Operations =====

    fn supports_multicall(&self) -> bool {
        match self {
            Self::Sui(sui) => sui.supports_multicall(),
            Self::Private(private) => private.supports_multicall(),
            Self::Ethereum(eth) => eth.supports_multicall(),
        }
    }

    async fn multicall_job_operations(
        &self,
        operations: Vec<silvana_coordination_trait::MulticallOperations>,
    ) -> Result<silvana_coordination_trait::MulticallResult, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .multicall_job_operations(operations)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .multicall_job_operations(operations)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .multicall_job_operations(operations)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Cleanup Operations =====

    async fn purge(
        &self,
        app_instance: &str,
        sequences_to_purge: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        match self {
            Self::Sui(sui) => sui
                .purge(app_instance, sequences_to_purge)
                .await
                .map_err(Into::into),
            Self::Private(private) => private
                .purge(app_instance, sequences_to_purge)
                .await
                .map_err(Into::into),
            Self::Ethereum(eth) => eth
                .purge(app_instance, sequences_to_purge)
                .await
                .map_err(Into::into),
        }
    }

    // ===== Event Streaming =====

    async fn event_stream(&self) -> Result<silvana_coordination_trait::EventStream, Self::Error> {
        match self {
            Self::Sui(sui) => sui.event_stream().await.map_err(Into::into),
            Self::Private(private) => private.event_stream().await.map_err(Into::into),
            Self::Ethereum(eth) => eth.event_stream().await.map_err(Into::into),
        }
    }
}
