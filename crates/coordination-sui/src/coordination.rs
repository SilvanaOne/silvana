//! Sui implementation of the Coordination trait
//!
//! This module provides the `SuiCoordination` struct which implements the
//! `Coordination` trait for the Sui blockchain, wrapping existing Sui functionality.

use anyhow::anyhow;
use async_stream::stream;
use async_trait::async_trait;
use futures_util::StreamExt;
use silvana_coordination_trait::{
    AppInstance, Block, BlockSettlement, Coordination, CoordinationLayer, EventStream, Job,
    JobCreatedEvent, JobStatus, MulticallOperations, MulticallResult, Proof, ProofCalculation,
    ProofStatus, SequenceState, Settlement,
};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use sui::{
    app_instance::{
        add_metadata_tx, complete_job_tx, create_app_job_tx, create_merge_job_with_proving_tx,
        create_settle_job_tx, delete_kv_tx, fail_job_tx, multicall_job_operations_tx, purge_tx,
        reject_proof_tx, remove_failed_jobs_tx, restart_failed_jobs_with_sequences_tx, set_kv_tx,
        start_proving_tx, submit_proof_tx, terminate_app_job_tx, terminate_job_tx,
        try_create_block_tx, update_block_proof_data_availability_tx,
        update_block_settlement_tx_hash_tx, update_block_settlement_tx_included_in_block_tx,
        update_block_state_data_availability_tx, update_state_for_sequence_tx,
    },
    error::SilvanaSuiInterfaceError,
    fetch::{
        self,
        app_instance::{fetch_app_instance, fetch_block_settlement},
        block::{fetch_block_info, fetch_blocks_range},
        jobs::{
            fetch_failed_jobs_from_app_instance, fetch_job_by_id,
            fetch_pending_jobs_from_app_instance, get_failed_jobs_count,
        },
        prover::{fetch_proof_calculation, fetch_proof_calculations_range},
        sequence_state::fetch_sequence_state_by_id,
    },
    interface::SilvanaSuiInterface,
    state::SharedSuiState,
};

/// Sui coordination layer implementation
pub struct SuiCoordination {
    /// Internal interface for some operations
    #[allow(dead_code)]
    interface: SilvanaSuiInterface,
    /// Optional chain ID override (defaults to "sui")
    chain_id: String,
}

impl SuiCoordination {
    /// Create a new SuiCoordination instance
    pub fn new() -> Self {
        // Ensure SharedSuiState is initialized before creating coordination
        if !SharedSuiState::is_initialized() {
            panic!("SharedSuiState must be initialized before creating SuiCoordination");
        }

        Self {
            interface: SilvanaSuiInterface::new(),
            chain_id: "sui".to_string(),
        }
    }

    /// Create with a custom chain ID
    pub fn with_chain_id(chain_id: String) -> Self {
        if !SharedSuiState::is_initialized() {
            panic!("SharedSuiState must be initialized before creating SuiCoordination");
        }

        Self {
            interface: SilvanaSuiInterface::new(),
            chain_id,
        }
    }

    /// Convert from internal Job type to trait Job type
    fn convert_job(&self, job: fetch::jobs::Job) -> Job {
        Job {
            job_sequence: job.job_sequence,
            description: job.description,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            app: job.app,
            app_instance: job.app_instance,
            app_instance_method: job.app_instance_method,
            block_number: job.block_number,
            sequences: job.sequences,
            sequences1: job.sequences1,
            sequences2: job.sequences2,
            data: job.data,
            status: self.convert_job_status(job.status),
            attempts: job.attempts,
            interval_ms: job.interval_ms,
            next_scheduled_at: job.next_scheduled_at,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }

    /// Convert from internal JobStatus to trait JobStatus
    fn convert_job_status(&self, status: fetch::jobs::JobStatus) -> JobStatus {
        match status {
            fetch::jobs::JobStatus::Pending => JobStatus::Pending,
            fetch::jobs::JobStatus::Running => JobStatus::Running,
            fetch::jobs::JobStatus::Failed(err) => JobStatus::Failed(err),
        }
    }

    /// Convert from internal Block type to trait Block type
    fn convert_block(&self, block: fetch::block::Block) -> Block {
        Block {
            name: block.name,
            block_number: block.block_number,
            start_sequence: block.start_sequence,
            end_sequence: block.end_sequence,
            actions_commitment: block.actions_commitment,
            state_commitment: block.state_commitment,
            time_since_last_block: block.time_since_last_block,
            number_of_transactions: block.number_of_transactions,
            start_actions_commitment: block.start_actions_commitment,
            end_actions_commitment: block.end_actions_commitment,
            state_data_availability: block.state_data_availability,
            proof_data_availability: block.proof_data_availability,
            created_at: block.created_at,
            state_calculated_at: block.state_calculated_at,
            proved_at: block.proved_at,
        }
    }

    /// Convert from internal SequenceState to trait SequenceState
    fn convert_sequence_state(&self, state: fetch::sequence_state::SequenceState) -> SequenceState {
        SequenceState {
            sequence: state.sequence,
            state: state.state,
            data_availability: state.data_availability,
            optimistic_state: state.optimistic_state,
            transition_data: state.transition_data,
        }
    }

    /// Convert from internal ProofStatus to trait ProofStatus
    fn convert_proof_status(&self, status: fetch::prover::ProofStatus) -> ProofStatus {
        match status {
            fetch::prover::ProofStatus::Started => ProofStatus::Started,
            fetch::prover::ProofStatus::Calculated => ProofStatus::Calculated,
            fetch::prover::ProofStatus::Rejected => ProofStatus::Rejected,
            fetch::prover::ProofStatus::Reserved => ProofStatus::Reserved,
            fetch::prover::ProofStatus::Used => ProofStatus::Used,
        }
    }

    /// Convert from internal Proof to trait Proof
    fn convert_proof(&self, proof: fetch::prover::Proof) -> Proof {
        Proof {
            status: self.convert_proof_status(proof.status),
            da_hash: proof.da_hash,
            sequence1: proof.sequence1,
            sequence2: proof.sequence2,
            rejected_count: proof.rejected_count,
            timestamp: proof.timestamp,
            prover: proof.prover,
            user: proof.user,
            job_id: proof.job_id,
            sequences: proof.sequences,
        }
    }

    /// Convert from internal ProofCalculation to trait ProofCalculation
    fn convert_proof_calculation(&self, calc: fetch::prover::ProofCalculation) -> ProofCalculation {
        ProofCalculation {
            id: calc.id,
            block_number: calc.block_number,
            start_sequence: calc.start_sequence,
            end_sequence: calc.end_sequence,
            proofs: calc
                .proofs
                .into_iter()
                .map(|p| self.convert_proof(p))
                .collect(),
            block_proof: calc.block_proof,
            block_proof_submitted: calc.is_finished, // Map is_finished to block_proof_submitted
        }
    }

    /// Convert from internal BlockSettlement to trait BlockSettlement
    fn convert_block_settlement(
        &self,
        settlement: fetch::app_instance::BlockSettlement,
    ) -> BlockSettlement {
        BlockSettlement {
            block_number: settlement.block_number,
            settlement_tx_hash: settlement.settlement_tx_hash,
            settlement_tx_included_in_block: settlement.settlement_tx_included_in_block,
            sent_to_settlement_at: settlement.sent_to_settlement_at,
            settled_at: settlement.settled_at,
        }
    }

    /// Convert from internal Settlement to trait Settlement
    fn convert_settlement(&self, settlement: fetch::app_instance::Settlement) -> Settlement {
        Settlement {
            chain: settlement.chain,
            last_settled_block_number: settlement.last_settled_block_number,
            settlement_address: settlement.settlement_address,
            settlement_job: settlement.settlement_job,
        }
    }

    /// Extract job_sequence from JobCreatedEvent in transaction
    async fn extract_job_sequence_from_tx(
        &self,
        tx_hash: &str,
    ) -> Result<u64, SilvanaSuiInterfaceError> {
        // Fetch transaction events
        let events_json = sui::transactions::fetch_transaction_events_as_json(tx_hash)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))?;

        // Parse events array and look for JobCreatedEvent
        if let Some(events_array) = events_json.as_array() {
            for event in events_array {
                if let Some(event_type) = event["event_type"].as_str() {
                    if event_type.contains("JobCreatedEvent") {
                        // Try different JSON structures (parsed_json, contents, direct)
                        let event_data = if event["parsed_json"].is_object()
                            && !event["parsed_json"]["job_sequence"].is_null()
                        {
                            &event["parsed_json"]
                        } else if event["contents"].is_object()
                            && !event["contents"]["job_sequence"].is_null()
                        {
                            &event["contents"]
                        } else {
                            event
                        };

                        // Extract job_sequence
                        if let Some(job_seq) = event_data["job_sequence"].as_u64() {
                            return Ok(job_seq);
                        } else if let Some(job_seq_str) = event_data["job_sequence"].as_str() {
                            return job_seq_str.parse::<u64>().map_err(|e| {
                                SilvanaSuiInterfaceError::Other(anyhow!(
                                    "Failed to parse job_sequence: {}",
                                    e
                                ))
                            });
                        }
                    }
                }
            }
        }

        Err(SilvanaSuiInterfaceError::Other(anyhow!(
            "JobCreatedEvent not found in transaction"
        )))
    }

    /// Extract total operation count from MulticallExecutedEvent in transaction
    async fn extract_operation_count_from_tx(
        &self,
        tx_hash: &str,
    ) -> Result<usize, SilvanaSuiInterfaceError> {
        // Fetch transaction events
        let events_json = sui::transactions::fetch_transaction_events_as_json(tx_hash)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))?;

        let mut total_operations = 0;

        // Parse events array and look for MulticallExecutedEvent
        if let Some(events_array) = events_json.as_array() {
            for event in events_array {
                if let Some(event_type) = event["event_type"].as_str() {
                    if event_type.contains("MulticallExecutedEvent") {
                        // Try different JSON structures
                        let event_data = if event["parsed_json"].is_object()
                            && !event["parsed_json"]["start_jobs"].is_null()
                        {
                            &event["parsed_json"]
                        } else if event["contents"].is_object()
                            && !event["contents"]["start_jobs"].is_null()
                        {
                            &event["contents"]
                        } else {
                            event
                        };

                        // Count operations from each array
                        if let Some(start_jobs) = event_data["start_jobs"].as_array() {
                            total_operations += start_jobs.len();
                        }
                        if let Some(complete_jobs) = event_data["complete_jobs"].as_array() {
                            total_operations += complete_jobs.len();
                        }
                        if let Some(fail_jobs) = event_data["fail_jobs"].as_array() {
                            total_operations += fail_jobs.len();
                        }
                        if let Some(terminate_jobs) = event_data["terminate_jobs"].as_array() {
                            total_operations += terminate_jobs.len();
                        }
                    }
                }
            }
        }

        if total_operations == 0 {
            return Err(SilvanaSuiInterfaceError::Other(anyhow!(
                "No MulticallExecutedEvent found in transaction or no operations in event"
            )));
        }

        Ok(total_operations)
    }

    /// Convert from internal AppInstance to trait AppInstance
    fn convert_app_instance(&self, app: fetch::app_instance::AppInstance) -> AppInstance {
        AppInstance {
            id: app.id,
            silvana_app_name: app.silvana_app_name,
            description: app.description,
            metadata: app.metadata,
            kv: app.kv,
            sequence: app.sequence,
            admin: app.admin,
            block_number: app.block_number,
            previous_block_timestamp: app.previous_block_timestamp,
            previous_block_last_sequence: app.previous_block_last_sequence,
            last_proved_block_number: app.last_proved_block_number,
            last_settled_block_number: app.last_settled_block_number,
            last_settled_sequence: app.last_settled_sequence,
            last_purged_sequence: app.last_purged_sequence,
            settlements: app
                .settlements
                .into_iter()
                .map(|(k, v)| (k, self.convert_settlement(v)))
                .collect(),
            is_paused: app.is_paused,
            min_time_between_blocks: app.min_time_between_blocks,
            created_at: app.created_at,
            updated_at: app.updated_at,
        }
    }

    /// Convert from trait MulticallOperations to internal type
    fn convert_multicall_operations(
        &self,
        ops: MulticallOperations,
    ) -> sui::types::MulticallOperations {
        sui::types::MulticallOperations {
            app_instance: ops.app_instance,
            complete_job_sequences: ops.complete_job_sequences,
            fail_job_sequences: ops.fail_job_sequences,
            fail_errors: ops.fail_errors,
            terminate_job_sequences: ops.terminate_job_sequences,
            start_job_sequences: ops.start_job_sequences,
            start_job_memory_requirements: ops.start_job_memory_requirements,
            available_memory: ops.available_memory,
            update_state_for_sequences: ops.update_state_for_sequences,
            submit_proofs: ops.submit_proofs,
            create_jobs: ops.create_jobs,
            create_merge_jobs: ops.create_merge_jobs,
        }
    }
}

// Helper functions for event parsing (standalone to avoid lifetime issues)

/// Parse JobCreatedEvent from Sui checkpoint
async fn parse_job_created_events(
    checkpoint: &sui_rpc::proto::sui::rpc::v2::Checkpoint,
    checkpoint_seq: u64,
) -> Vec<JobCreatedEvent> {
    let mut events = Vec::new();

    for (tx_index, transaction) in checkpoint.transactions.iter().enumerate() {
        if let Some(tx_events) = &transaction.events {
            for (event_index, event) in tx_events.events.iter().enumerate() {
                if let Some(event_type) = &event.event_type {
                    if event_type.contains("::JobCreatedEvent") {
                        // Fetch the full event with contents from the transaction
                        match sui::fetch::checkpoint::fetch_event_with_contents(
                            checkpoint_seq,
                            tx_index,
                            event_index,
                        )
                        .await
                        {
                            Ok(Some(full_event)) => {
                                if let Some(contents) = &full_event.contents {
                                    if let Some(value) = &contents.value {
                                        debug!(
                                            "🔍 RAW BCS data length: {} bytes, first 100 bytes: {:02x?}",
                                            value.len(),
                                            &value[..100.min(value.len())]
                                        );
                                        match decode_job_created_event(value) {
                                            Ok(job_event) => {
                                                info!(
                                                    "✅ Parsed JobCreatedEvent from checkpoint {}: seq={}, app={}",
                                                    checkpoint_seq,
                                                    job_event.job_sequence,
                                                    &job_event.app_instance
                                                        [..16.min(job_event.app_instance.len())]
                                                );
                                                events.push(job_event);
                                            }
                                            Err(e) => {
                                                error!(
                                                    "❌ Failed to decode JobCreatedEvent from checkpoint {}: {}",
                                                    checkpoint_seq, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                warn!(
                                    "Event not found when fetching contents for checkpoint {} tx {} event {}",
                                    checkpoint_seq, tx_index, event_index
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to fetch event contents for checkpoint {} tx {} event {}: {}",
                                    checkpoint_seq, tx_index, event_index, e
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    events
}

/// Decode JobCreatedEvent from BCS bytes
fn decode_job_created_event(bcs_data: &[u8]) -> Result<JobCreatedEvent, SilvanaSuiInterfaceError> {
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    enum JobStatusBcs {
        Pending,
        Running,
        Failed(String),
    }

    #[derive(serde::Deserialize)]
    struct JobCreatedEventBcs {
        job_sequence: u64,
        description: Option<String>,
        developer: String,
        agent: String,
        agent_method: String,
        app: String,
        app_instance: String,
        app_instance_method: String,
        block_number: Option<u64>,
        sequences: Option<Vec<u64>>,
        sequences1: Option<Vec<u64>>,
        sequences2: Option<Vec<u64>>,
        data: Vec<u8>,
        status: JobStatusBcs,
        created_at: u64,
    }

    let bcs_event: JobCreatedEventBcs =
        bcs::from_bytes(bcs_data).map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))?;

    debug!(
        "📦 BCS decoded ALL fields:\n  seq={}\n  description={:?}\n  developer='{}'\n  agent='{}'\n  agent_method='{}'\n  app='{}' (len={})\n  app_instance='{}' (len={})\n  app_instance_method='{}' (len={})\n  block_number={:?}",
        bcs_event.job_sequence,
        bcs_event.description,
        bcs_event.developer,
        bcs_event.agent,
        bcs_event.agent_method,
        bcs_event.app,
        bcs_event.app.len(),
        bcs_event.app_instance,
        bcs_event.app_instance.len(),
        bcs_event.app_instance_method,
        bcs_event.app_instance_method.len(),
        bcs_event.block_number
    );

    // Add "0x" prefix to app_instance if not present (Sui addresses need the prefix)
    let app_instance = if bcs_event.app_instance.starts_with("0x") {
        bcs_event.app_instance
    } else {
        format!("0x{}", bcs_event.app_instance)
    };

    Ok(JobCreatedEvent {
        job_sequence: bcs_event.job_sequence,
        developer: bcs_event.developer,
        agent: bcs_event.agent,
        agent_method: bcs_event.agent_method,
        app_instance,
        app_instance_method: bcs_event.app_instance_method,
        block_number: bcs_event.block_number.unwrap_or(0),
        created_at: bcs_event.created_at,
    })
}

impl fmt::Debug for SuiCoordination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SuiCoordination")
            .field("chain_id", &self.chain_id)
            .finish()
    }
}

#[async_trait]
impl Coordination for SuiCoordination {
    type TransactionHash = String;
    type Error = SilvanaSuiInterfaceError;

    // ===== Coordination Layer Identification =====

    fn coordination_layer(&self) -> CoordinationLayer {
        CoordinationLayer::Sui
    }

    fn chain_id(&self) -> String {
        self.chain_id.clone()
    }

    // ===== Job Management =====

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;

        // Get the Jobs object and extract pending_jobs VecSet
        let jobs_obj = match app.jobs.as_ref() {
            Some(jobs) => jobs,
            None => return Ok(vec![]),
        };

        // Get all pending job sequences from the VecSet
        let pending_sequences: Vec<u64> = jobs_obj.pending_jobs.clone();

        if pending_sequences.is_empty() {
            return Ok(vec![]);
        }

        // Fetch all pending jobs in a batch
        let jobs_table_id = &jobs_obj.jobs_table_id;
        let jobs_map = sui::fetch::fetch_jobs_batch(jobs_table_id, &pending_sequences)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))?;

        // Convert and filter for Pending status (defensive check)
        let pending_jobs: Vec<Job> = jobs_map
            .into_values()
            .filter(|job| matches!(job.status, sui::fetch::JobStatus::Pending))
            .map(|job| self.convert_job(job))
            .collect();

        Ok(pending_jobs)
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let jobs = fetch_failed_jobs_from_app_instance(&app).await?;
        Ok(jobs.into_iter().map(|j| self.convert_job(j)).collect())
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(get_failed_jobs_count(&app).await)
    }

    async fn fetch_job_by_id(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Option<Job>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let jobs_table_id = app
            .jobs
            .as_ref()
            .map(|j| j.jobs_table_id.clone())
            .ok_or_else(|| SilvanaSuiInterfaceError::Other(anyhow!("Jobs table not found")))?;
        match fetch_job_by_id(&jobs_table_id, job_sequence).await? {
            Some(job) => Ok(Some(self.convert_job(job))),
            None => Ok(None),
        }
    }

    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.jobs.as_ref().map(|j| j.pending_jobs_count).unwrap_or(0))
    }

    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.jobs.as_ref().map(|j| j.total_jobs_count).unwrap_or(0))
    }

    async fn get_settlement_job_ids(
        &self,
        app_instance: &str,
    ) -> Result<HashMap<String, u64>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        sui::fetch::app_instance::get_settlement_job_ids_for_instance(&app)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))
    }

    async fn get_jobs_info(
        &self,
        app_instance: &str,
    ) -> Result<Option<(String, String)>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        sui::fetch::get_jobs_info_from_app_instance(&app)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))
    }

    async fn fetch_jobs_batch(
        &self,
        app_instance: &str,
        job_ids: &[u64],
    ) -> Result<Vec<Job>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let jobs_table_id = app
            .jobs
            .as_ref()
            .map(|j| j.jobs_table_id.clone())
            .ok_or_else(|| SilvanaSuiInterfaceError::Other(anyhow!("Jobs table not found")))?;
        let jobs_map = sui::fetch::fetch_jobs_batch(&jobs_table_id, job_ids)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))?;
        // Convert HashMap<u64, Job> to Vec<Job>
        Ok(jobs_map
            .into_values()
            .map(|j| self.convert_job(j))
            .collect())
    }

    async fn fetch_pending_job_sequences(
        &self,
        app_instance: &str,
    ) -> Result<Vec<u64>, Self::Error> {
        // Fetch all pending jobs and extract their sequences
        let pending_jobs = self.fetch_pending_jobs(app_instance).await?;
        Ok(pending_jobs.into_iter().map(|j| j.job_sequence).collect())
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        sui::fetch::fetch_pending_job_sequences_from_app_instance(
            &app,
            developer,
            agent,
            agent_method,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e.into()))
    }

    async fn start_job(&self, app_instance: &str, job_sequence: u64) -> Result<bool, Self::Error> {
        // Use the existing interface method which returns bool
        let mut interface = SilvanaSuiInterface::new();
        Ok(interface.start_job(app_instance, job_sequence).await)
    }

    async fn complete_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        complete_job_tx(app_instance, job_sequence)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn fail_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
        error: &str,
    ) -> Result<Self::TransactionHash, Self::Error> {
        fail_job_tx(app_instance, job_sequence, error)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn terminate_job(
        &self,
        app_instance: &str,
        job_sequence: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        terminate_job_tx(app_instance, job_sequence, None)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
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
        let tx_hash = create_app_job_tx(
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
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))?;

        // Extract job_sequence from transaction events
        let job_sequence = self.extract_job_sequence_from_tx(&tx_hash).await?;
        Ok((tx_hash, job_sequence))
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
        create_merge_job_with_proving_tx(
            app_instance,
            block_number,
            sequences,
            sequences1,
            sequences2,
            job_description,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        create_settle_job_tx(app_instance, block_number, chain, job_description)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn terminate_app_job(
        &self,
        app_instance: &str,
        job_id: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        terminate_app_job_tx(app_instance, job_id)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        job_sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        restart_failed_jobs_with_sequences_tx(app_instance, job_sequences, None)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        remove_failed_jobs_tx(app_instance, sequences, None)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(
        &self,
        app_instance: &str,
        sequence: u64,
    ) -> Result<Option<SequenceState>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;

        // Get the sequence state manager table ID
        let sequence_state_manager = serde_json::from_value::<serde_json::Value>(
            app.sequence_state_manager,
        )
        .map_err(|e| {
            SilvanaSuiInterfaceError::ParseError(format!(
                "Failed to parse sequence_state_manager: {}",
                e
            ))
        })?;

        let table_id = sequence_state_manager
            .get("sequence_states_table_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SilvanaSuiInterfaceError::ParseError("Missing sequence_states_table_id".to_string())
            })?;

        match fetch_sequence_state_by_id(table_id, sequence).await? {
            Some(state) => Ok(Some(self.convert_sequence_state(state))),
            None => Ok(None),
        }
    }

    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>, Self::Error> {
        let mut states = Vec::new();

        // Fetch states one by one for now
        // TODO: Optimize with batch fetching
        for seq in from_sequence..=to_sequence {
            if let Some(state) = self.fetch_sequence_state(app_instance, seq).await? {
                states.push(state);
            }
        }

        Ok(states)
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.sequence)
    }

    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        update_state_for_sequence_tx(
            app_instance,
            sequence,
            new_state_data,
            new_data_availability_hash,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Block Management =====

    async fn fetch_block(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<Block>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        match fetch_block_info(&app, block_number).await? {
            Some(block) => Ok(Some(self.convert_block(block))),
            None => Ok(None),
        }
    }

    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let blocks = fetch_blocks_range(&app, from_block, to_block).await?;
        Ok(blocks
            .into_iter()
            .map(|(_, b)| self.convert_block(b))
            .collect())
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>, Self::Error> {
        match try_create_block_tx(app_instance).await {
            Ok(_tx_hash) => {
                // Fetch the app instance to get the new block number
                let app = fetch_app_instance(app_instance).await?;
                Ok(Some(app.block_number))
            }
            Err(e) => {
                // Check if it's a "no new sequences" error
                if e.to_string().contains("no new sequences") {
                    Ok(None)
                } else {
                    Err(SilvanaSuiInterfaceError::Other(e))
                }
            }
        }
    }

    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_da: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        update_block_state_data_availability_tx(app_instance, block_number, state_da)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_da: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        update_block_proof_data_availability_tx(app_instance, block_number, proof_da)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<ProofCalculation>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        match fetch_proof_calculation(&app, block_number).await? {
            Some(calc) => Ok(Some(self.convert_proof_calculation(calc))),
            None => Ok(None),
        }
    }

    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<ProofCalculation>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let calcs = fetch_proof_calculations_range(&app, from_block, to_block).await?;
        Ok(calcs
            .into_iter()
            .map(|(_, c)| self.convert_proof_calculation(c))
            .collect())
    }

    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        start_proving_tx(
            app_instance,
            block_number,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))
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
        submit_proof_tx(
            app_instance,
            block_number,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            job_id,
            da_hash,
            cpu_cores,
            prover_architecture,
            prover_memory,
            cpu_time,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        reject_proof_tx(app_instance, block_number, sequences)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Settlement =====

    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let settlement = app.settlements.get(chain).ok_or_else(|| {
            SilvanaSuiInterfaceError::Other(anyhow!("Settlement not found for chain"))
        })?;
        match fetch_block_settlement(settlement, block_number).await? {
            Some(block_settlement) => Ok(Some(self.convert_block_settlement(block_settlement))),
            None => Ok(None),
        }
    }

    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.settlements.keys().cloned().collect())
    }

    async fn get_settlement_address(
        &self,
        app_instance: &str,
        chain: &str,
    ) -> Result<Option<String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app
            .settlements
            .get(chain)
            .and_then(|s| s.settlement_address.clone()))
    }

    async fn get_settlement_job_for_chain(
        &self,
        app_instance: &str,
        chain: &str,
    ) -> Result<Option<u64>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.settlements.get(chain).and_then(|s| s.settlement_job))
    }

    async fn set_settlement_address(
        &self,
        _app_instance: &str,
        _chain: String,
        _address: Option<String>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        // This requires a new Move function to be added
        Err(SilvanaSuiInterfaceError::Other(anyhow::anyhow!(
            "set_settlement_address not yet implemented for Sui"
        )))
    }

    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        update_block_settlement_tx_hash_tx(app_instance, block_number, chain, settlement_tx_hash)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        update_block_settlement_tx_included_in_block_tx(
            app_instance,
            block_number,
            chain,
            settled_at,
        )
        .await
        .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Key-Value Storage =====

    async fn get_kv_string(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Option<String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.kv.get(key).cloned())
    }

    async fn get_all_kv_string(
        &self,
        app_instance: &str,
    ) -> Result<HashMap<String, String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.kv)
    }

    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        let mut keys: Vec<String> = if let Some(prefix) = prefix {
            app.kv
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            app.kv.keys().cloned().collect()
        };

        keys.sort();

        if let Some(limit) = limit {
            keys.truncate(limit as usize);
        }

        Ok(keys)
    }

    async fn set_kv_string(
        &self,
        app_instance: &str,
        key: String,
        value: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        set_kv_tx(app_instance, key, value)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    async fn delete_kv_string(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Self::TransactionHash, Self::Error> {
        delete_kv_tx(app_instance, key.to_string())
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // Binary KV operations - not supported on Sui yet
    async fn get_kv_binary(
        &self,
        _app_instance: &str,
        _key: &[u8],
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        Err(SilvanaSuiInterfaceError::Other(anyhow::anyhow!(
            "Binary KV operations not yet supported on Sui"
        )))
    }

    async fn list_kv_binary_keys(
        &self,
        _app_instance: &str,
        _prefix: Option<&[u8]>,
        _limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>, Self::Error> {
        Err(SilvanaSuiInterfaceError::Other(anyhow::anyhow!(
            "Binary KV operations not yet supported on Sui"
        )))
    }

    async fn set_kv_binary(
        &self,
        _app_instance: &str,
        _key: Vec<u8>,
        _value: Vec<u8>,
    ) -> Result<Self::TransactionHash, Self::Error> {
        Err(SilvanaSuiInterfaceError::Other(anyhow::anyhow!(
            "Binary KV operations not yet supported on Sui"
        )))
    }

    async fn delete_kv_binary(
        &self,
        _app_instance: &str,
        _key: &[u8],
    ) -> Result<Self::TransactionHash, Self::Error> {
        Err(SilvanaSuiInterfaceError::Other(anyhow::anyhow!(
            "Binary KV operations not yet supported on Sui"
        )))
    }

    // ===== Metadata Management =====

    async fn get_metadata(
        &self,
        app_instance: &str,
        key: &str,
    ) -> Result<Option<String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.metadata.get(key).cloned())
    }

    async fn get_all_metadata(
        &self,
        app_instance: &str,
    ) -> Result<HashMap<String, String>, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.metadata)
    }

    async fn add_metadata(
        &self,
        app_instance: &str,
        key: String,
        value: String,
    ) -> Result<Self::TransactionHash, Self::Error> {
        add_metadata_tx(app_instance, key, value)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== App Instance Data =====

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(self.convert_app_instance(app))
    }

    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.admin)
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.is_paused)
    }

    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64, Self::Error> {
        let app = fetch_app_instance(app_instance).await?;
        Ok(app.min_time_between_blocks)
    }

    // ===== Batch Operations =====

    fn supports_multicall(&self) -> bool {
        true // Sui supports multicall for gas optimization
    }

    async fn multicall_job_operations(
        &self,
        operations: Vec<MulticallOperations>,
    ) -> Result<MulticallResult, Self::Error> {
        // Convert to internal type
        let internal_ops: Vec<sui::types::MulticallOperations> = operations
            .into_iter()
            .map(|op| self.convert_multicall_operations(op))
            .collect();

        let tx_hash = multicall_job_operations_tx(internal_ops, None, None)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))?;

        // Extract operation count from transaction events
        let operation_count = self.extract_operation_count_from_tx(&tx_hash).await?;
        Ok(MulticallResult::success(tx_hash, operation_count))
    }

    // ===== State Purging =====

    async fn purge(
        &self,
        app_instance: &str,
        sequences_to_purge: u64,
    ) -> Result<Self::TransactionHash, Self::Error> {
        purge_tx(app_instance, sequences_to_purge, None, None)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::Other(e))
    }

    // ===== Event Streaming =====

    async fn event_stream(&self) -> Result<EventStream, Self::Error> {
        let stream = stream! {
            loop {
                // Create checkpoint stream
                match sui::events::create_checkpoint_stream().await {
                    Ok(mut checkpoint_stream) => {
                        info!("✅ Sui: Connected to checkpoint stream");

                        // Process checkpoints
                        while let Ok(Some(response)) = timeout(
                            Duration::from_secs(120),
                            checkpoint_stream.next()
                        ).await {
                            match response {
                                Ok(checkpoint_response) => {
                                    if let (Some(cursor), Some(checkpoint)) =
                                        (checkpoint_response.cursor, checkpoint_response.checkpoint)
                                    {
                                        // Parse events from this checkpoint
                                        let events = parse_job_created_events(&checkpoint, cursor).await;

                                        // Yield each event
                                        for event in events {
                                            yield Ok(event);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Sui checkpoint stream error: {}", e);
                                    yield Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
                                    break; // Reconnect
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to create Sui checkpoint stream: {}", e);
                        yield Err(Box::new(SilvanaSuiInterfaceError::Other(e)) as Box<dyn std::error::Error + Send + Sync>);
                    }
                }

                // Exponential backoff before reconnect
                warn!("Sui checkpoint stream disconnected, reconnecting in 5s...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sui_coordination_creation() {
        // This will panic if SharedSuiState is not initialized, which is expected in tests
        let result = std::panic::catch_unwind(|| {
            SuiCoordination::new();
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_coordination_layer() {
        // Create a mock coordination without SharedSuiState check
        let coord = SuiCoordination {
            interface: SilvanaSuiInterface::new(),
            chain_id: "sui".to_string(),
        };

        assert_eq!(coord.coordination_layer(), CoordinationLayer::Sui);
        assert_eq!(coord.chain_id(), "sui");
        assert!(coord.supports_multicall());
    }
}
