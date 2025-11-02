//! Private Coordination layer implementation

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set, NotSet,
    PaginatorTrait, TransactionTrait,
};
use silvana_coordination_trait::{
    AppInstance, Block, BlockSettlement, Coordination, CoordinationLayer,
    Job, JobStatus, MulticallOperations, MulticallResult,
    ProofCalculation, SequenceState,
};
use state::entity;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    auth::JwtAuth,
    config::PrivateCoordinationConfig,
    database::DatabaseManager,
    error::{PrivateCoordinationError, Result},
};

/// Private Coordination layer implementation using TiDB
pub struct PrivateCoordination {
    /// Database connection manager
    db: Arc<DatabaseManager>,

    /// JWT authentication handler
    auth: Arc<JwtAuth>,

    /// Chain ID
    chain_id: String,

    /// Optional JWT token for current session
    current_token: Option<String>,
}

impl PrivateCoordination {
    /// Create a new PrivateCoordination instance
    pub async fn new(config: PrivateCoordinationConfig) -> Result<Self> {
        let db = Arc::new(DatabaseManager::new(&config).await?);
        let auth = Arc::new(JwtAuth::new(&config.jwt_secret));

        Ok(Self {
            db,
            auth,
            chain_id: config.chain_id,
            current_token: None,
        })
    }

    /// Set the JWT token for authentication
    pub fn set_auth_token(&mut self, token: String) {
        self.current_token = Some(token);
    }

    /// Get the current auth token or return error
    fn get_auth_token(&self) -> Result<&str> {
        self.current_token
            .as_deref()
            .ok_or_else(|| PrivateCoordinationError::Authentication("No auth token set".to_string()))
    }

    /// Verify ownership of app instance
    async fn verify_app_ownership(&self, app_instance: &str) -> Result<()> {
        let token = self.get_auth_token()?;
        let claims = self.auth.verify_token(token)?;

        if claims.app_instance_id != app_instance {
            return Err(PrivateCoordinationError::Unauthorized(
                format!("Token not valid for app_instance {}", app_instance)
            ));
        }

        // Verify the owner matches in database
        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        if app.owner != claims.sub {
            return Err(PrivateCoordinationError::Unauthorized(
                "Token owner does not match app instance owner".to_string()
            ));
        }

        Ok(())
    }

    /// Convert internal job status to trait JobStatus
    fn convert_job_status(&self, status: &str, error_message: Option<&String>) -> JobStatus {
        match status {
            "PENDING" => JobStatus::Pending,
            "RUNNING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            "FAILED" => JobStatus::Failed(error_message.map(|e| e.clone()).unwrap_or_else(|| "Unknown error".to_string())),
            _ => JobStatus::Pending,
        }
    }

    /// Convert database job model to trait Job
    fn convert_job(&self, job: entity::jobs::Model) -> Job {
        Job {
            job_sequence: job.job_sequence as u64,
            description: job.description,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            app: String::new(), // Not stored in database
            app_instance: job.app_instance_id,
            app_instance_method: String::new(), // Not stored in database
            block_number: job.block_number,
            sequences: job.sequences.and_then(|j| serde_json::from_value(j).ok()),
            sequences1: job.sequences1.and_then(|j| serde_json::from_value(j).ok()),
            sequences2: job.sequences2.and_then(|j| serde_json::from_value(j).ok()),
            data: job.data.unwrap_or_default(),
            status: self.convert_job_status(&job.status, job.error_message.as_ref()),
            attempts: job.attempts as u8,
            interval_ms: job.interval_ms,
            next_scheduled_at: job.next_scheduled_at.map(|t| t.timestamp() as u64),
            created_at: job.created_at.timestamp() as u64,
            updated_at: job.updated_at.timestamp() as u64,
        }
    }

    /// Generate a new UUID for transaction hash
    fn generate_tx_hash(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

#[async_trait]
impl Coordination for PrivateCoordination {
    type TransactionHash = String;
    type Error = PrivateCoordinationError;

    // ===== Coordination Layer Identification =====

    fn coordination_layer(&self) -> CoordinationLayer {
        CoordinationLayer::Private
    }

    fn chain_id(&self) -> String {
        self.chain_id.clone()
    }

    // ===== Job Management =====

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        self.verify_app_ownership(app_instance).await?;

        let jobs = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("PENDING"))
            .order_by_asc(entity::jobs::Column::JobSequence)
            .all(self.db.connection())
            .await?;

        Ok(jobs.into_iter().map(|j| self.convert_job(j)).collect())
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        self.verify_app_ownership(app_instance).await?;

        let jobs = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("FAILED"))
            .order_by_asc(entity::jobs::Column::JobSequence)
            .all(self.db.connection())
            .await?;

        Ok(jobs.into_iter().map(|j| self.convert_job(j)).collect())
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        self.verify_app_ownership(app_instance).await?;

        let count = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("FAILED"))
            .count(self.db.connection())
            .await?;

        Ok(count)
    }

    async fn fetch_job_by_id(&self, app_instance: &str, job_sequence: u64) -> Result<Option<Job>> {
        self.verify_app_ownership(app_instance).await?;

        let job = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::JobSequence.eq(job_sequence as i64))
            .one(self.db.connection())
            .await?;

        Ok(job.map(|j| self.convert_job(j)))
    }

    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64> {
        self.verify_app_ownership(app_instance).await?;

        let count = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("PENDING"))
            .count(self.db.connection())
            .await? as u64;

        Ok(count)
    }

    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64> {
        self.verify_app_ownership(app_instance).await?;

        let count = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .count(self.db.connection())
            .await? as u64;

        Ok(count)
    }

    async fn get_settlement_job_ids(&self, app_instance: &str) -> Result<HashMap<String, u64>> {
        self.verify_app_ownership(app_instance).await?;

        // Private layer doesn't have settlement jobs concept
        // Settlement happens on blockchain layers only
        Ok(HashMap::new())
    }

    async fn get_jobs_info(&self, app_instance: &str) -> Result<Option<(String, String)>> {
        self.verify_app_ownership(app_instance).await?;

        // Private layer uses database, not object tables
        // Return None as this is layer-specific metadata
        Ok(None)
    }

    async fn fetch_jobs_batch(&self, app_instance: &str, job_ids: &[u64]) -> Result<Vec<Job>> {
        self.verify_app_ownership(app_instance).await?;

        let jobs = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::JobSequence.is_in(job_ids.iter().map(|id| *id as i64)))
            .all(self.db.connection())
            .await?;

        Ok(jobs.into_iter().map(|j| self.convert_job(j)).collect())
    }

    async fn fetch_pending_job_sequences(&self, app_instance: &str) -> Result<Vec<u64>> {
        self.verify_app_ownership(app_instance).await?;

        let jobs = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("PENDING"))
            .all(self.db.connection())
            .await?;

        Ok(jobs.into_iter().map(|j| j.job_sequence as u64).collect())
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>> {
        self.verify_app_ownership(app_instance).await?;

        let jobs = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("PENDING"))
            .filter(entity::jobs::Column::Developer.eq(developer))
            .filter(entity::jobs::Column::Agent.eq(agent))
            .filter(entity::jobs::Column::AgentMethod.eq(agent_method))
            .all(self.db.connection())
            .await?;

        Ok(jobs.into_iter().map(|j| j.job_sequence as u64).collect())
    }

    async fn start_job(&self, app_instance: &str, job_sequence: u64) -> Result<bool> {
        self.verify_app_ownership(app_instance).await?;

        // Find and update job
        let job = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::JobSequence.eq(job_sequence as i64))
            .filter(entity::jobs::Column::Status.eq("PENDING"))
            .one(self.db.connection())
            .await?;

        if let Some(job) = job {
            let mut job: entity::jobs::ActiveModel = job.into();
            job.status = Set("RUNNING".to_string());
            job.updated_at = Set(Utc::now());
            job.update(self.db.connection()).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn complete_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let job = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::JobSequence.eq(job_sequence as i64))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Job {} not found", job_sequence)
            ))?;

        let mut job: entity::jobs::ActiveModel = job.into();
        job.status = Set("COMPLETED".to_string());
        job.updated_at = Set(Utc::now());
        job.update(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    async fn fail_job(&self, app_instance: &str, job_sequence: u64, error: &str) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let job = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::JobSequence.eq(job_sequence as i64))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Job {} not found", job_sequence)
            ))?;

        let mut job: entity::jobs::ActiveModel = job.into();
        job.status = Set("FAILED".to_string());
        job.error_message = Set(Some(error.to_string()));
        job.attempts = Set(job.attempts.unwrap() + 1);
        job.updated_at = Set(Utc::now());
        job.update(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    async fn terminate_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash> {
        // For Private layer, terminate is same as marking completed
        self.complete_job(app_instance, job_sequence).await
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
        _settlement_chain: Option<String>,
    ) -> Result<(Self::TransactionHash, u64)> {
        self.verify_app_ownership(app_instance).await?;

        // Get next job sequence
        let app_instance_clone = app_instance.to_string();
        let next_seq = self.db.connection().transaction::<_, u64, PrivateCoordinationError>(|txn| {
            let app_instance = app_instance_clone.clone();
            Box::pin(async move {
                // Get or create job_seq entry
                let seq_entry = entity::job_seq::Entity::find_by_id(&app_instance)
                    .one(txn)
                    .await?;

                let next_seq = if let Some(entry) = seq_entry {
                    let current = entry.next_seq;
                    let mut entry: entity::job_seq::ActiveModel = entry.into();
                    entry.next_seq = Set(current + 1);
                    entry.update(txn).await?;
                    current
                } else {
                    // Create new entry
                    let new_entry = entity::job_seq::ActiveModel {
                        app_instance_id: Set(app_instance.clone()),
                        next_seq: Set(2), // Next will be 2
                    };
                    new_entry.insert(txn).await?;
                    1
                };

                Ok(next_seq)
            })
        }).await.map_err(|e| match e {
            sea_orm::TransactionError::Connection(err) => PrivateCoordinationError::Database(err),
            sea_orm::TransactionError::Transaction(err) => err,
        })?;

        // Create the job
        let new_job = entity::jobs::ActiveModel {
            app_instance_id: Set(app_instance.to_string()),
            job_sequence: Set(next_seq),
            description: Set(job_description),
            developer: Set(String::new()), // Would need to extract from method registry
            agent: Set(String::new()),      // Would need to extract from method registry
            agent_method: Set(method_name),
            block_number: Set(block_number),
            sequences: Set(sequences.map(|v| serde_json::json!(v))),
            sequences1: Set(sequences1.map(|v| serde_json::json!(v))),
            sequences2: Set(sequences2.map(|v| serde_json::json!(v))),
            data: Set(Some(data)),
            data_da: Set(None),
            status: Set("PENDING".to_string()),
            error_message: Set(None),
            attempts: Set(0),
            interval_ms: Set(interval_ms),
            next_scheduled_at: Set(next_scheduled_at.map(|ts| {
                chrono::DateTime::from_timestamp(ts as i64, 0).unwrap()
            })),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(None),
        };

        new_job.insert(self.db.connection()).await?;

        Ok((self.generate_tx_hash(), next_seq))
    }

    // Continue in next part...
    async fn create_merge_job_with_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        // Create a merge job using create_app_job
        let (tx_hash, _) = self.create_app_job(
            app_instance,
            "merge".to_string(),
            job_description,
            Some(block_number),
            Some(sequences),
            Some(sequences1),
            Some(sequences2),
            Vec::new(),
            None,
            None,
            None,
        ).await?;

        Ok(tx_hash)
    }

    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        // Create a settlement job
        let (tx_hash, _) = self.create_app_job(
            app_instance,
            "settle".to_string(),
            job_description,
            Some(block_number),
            None,
            None,
            None,
            chain.as_bytes().to_vec(),
            None,
            None,
            Some(chain),
        ).await?;

        Ok(tx_hash)
    }

    async fn terminate_app_job(&self, app_instance: &str, job_id: u64) -> Result<Self::TransactionHash> {
        // Pass u64 directly - no conversion needed
        self.terminate_job(app_instance, job_id).await
    }

    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        job_sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let mut filter = entity::jobs::Entity::find()
            .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
            .filter(entity::jobs::Column::Status.eq("FAILED"));

        if let Some(sequences) = job_sequences {
            filter = filter.filter(entity::jobs::Column::JobSequence.is_in(sequences));
        }

        let failed_jobs = filter.all(self.db.connection()).await?;

        for job in failed_jobs {
            let mut job: entity::jobs::ActiveModel = job.into();
            job.status = Set("PENDING".to_string());
            job.error_message = Set(None);
            job.attempts = Set(0);
            job.updated_at = Set(Utc::now());
            job.update(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        if let Some(seqs) = sequences {
            entity::jobs::Entity::delete_many()
                .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
                .filter(entity::jobs::Column::Status.eq("FAILED"))
                .filter(entity::jobs::Column::JobSequence.is_in(seqs))
                .exec(self.db.connection())
                .await?;
        } else {
            entity::jobs::Entity::delete_many()
                .filter(entity::jobs::Column::AppInstanceId.eq(app_instance))
                .filter(entity::jobs::Column::Status.eq("FAILED"))
                .exec(self.db.connection())
                .await?;
        }

        Ok(self.generate_tx_hash())
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>> {
        self.verify_app_ownership(app_instance).await?;

        // Try to get from proved state first
        if let Some(proved) = entity::state::Entity::find()
            .filter(entity::state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::state::Column::Sequence.eq(sequence))
            .one(self.db.connection())
            .await? {
            return Ok(Some(SequenceState {
                sequence,
                state: proved.state_data,
                data_availability: proved.state_da,
                optimistic_state: vec![],
                transition_data: vec![],
            }));
        }

        // Fall back to optimistic state
        if let Some(optimistic) = entity::optimistic_state::Entity::find()
            .filter(entity::optimistic_state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::optimistic_state::Column::Sequence.eq(sequence))
            .one(self.db.connection())
            .await? {
            return Ok(Some(SequenceState {
                sequence,
                state: Some(optimistic.state_data.clone()),
                data_availability: optimistic.state_da.clone(),
                optimistic_state: optimistic.state_data,
                transition_data: optimistic.transition_data.unwrap_or_default(),
            }));
        }

        Ok(None)
    }

    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>> {
        self.verify_app_ownership(app_instance).await?;

        let mut states = Vec::new();

        // Get proved states
        let proved_states = entity::state::Entity::find()
            .filter(entity::state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::state::Column::Sequence.gte(from_sequence))
            .filter(entity::state::Column::Sequence.lte(to_sequence))
            .all(self.db.connection())
            .await?;

        for s in proved_states {
            states.push(SequenceState {
                sequence: s.sequence,
                state: s.state_data,
                data_availability: s.state_da,
                optimistic_state: vec![],
                transition_data: vec![],
            });
        }

        // Get optimistic states not in proved
        let optimistic_states = entity::optimistic_state::Entity::find()
            .filter(entity::optimistic_state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::optimistic_state::Column::Sequence.gte(from_sequence))
            .filter(entity::optimistic_state::Column::Sequence.lte(to_sequence))
            .all(self.db.connection())
            .await?;

        for o in optimistic_states {
            if !states.iter().any(|s| s.sequence == o.sequence) {
                states.push(SequenceState {
                    sequence: o.sequence,
                    state: Some(o.state_data.clone()),
                    data_availability: o.state_da.clone(),
                    optimistic_state: o.state_data,
                    transition_data: o.transition_data.unwrap_or_default(),
                });
            }
        }

        states.sort_by_key(|s| s.sequence);
        Ok(states)
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        self.verify_app_ownership(app_instance).await?;

        // Get max sequence from optimistic_state
        let max_seq = entity::optimistic_state::Entity::find()
            .filter(entity::optimistic_state::Column::AppInstanceId.eq(app_instance))
            .order_by_desc(entity::optimistic_state::Column::Sequence)
            .one(self.db.connection())
            .await?
            .map(|s| s.sequence)
            .unwrap_or(0);

        Ok(max_seq)
    }

    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state: Option<Vec<u8>>,
        new_data_availability: Option<String>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Update or create optimistic state
        let existing = entity::optimistic_state::Entity::find()
            .filter(entity::optimistic_state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::optimistic_state::Column::Sequence.eq(sequence))
            .one(self.db.connection())
            .await?;

        if let Some(existing) = existing {
            let mut state: entity::optimistic_state::ActiveModel = existing.into();
            if let Some(data) = new_state {
                state.state_data = Set(data);
            }
            if let Some(da) = new_data_availability {
                state.state_da = Set(Some(da));
            }
            state.update(self.db.connection()).await?;
        } else {
            let new_state_model = entity::optimistic_state::ActiveModel {
                id: NotSet,
                app_instance_id: Set(app_instance.to_string()),
                sequence: Set(sequence),
                state_hash: Set(vec![0; 32]), // Placeholder hash
                state_data: Set(new_state.unwrap_or_default()),
                state_da: Set(new_data_availability),
                transition_data: Set(None),
                transition_da: Set(None),
                commitment: Set(None),
                computed_at: Set(Utc::now()),
                metadata: Set(None),
            };
            new_state_model.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    // ===== Block Management =====

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        self.verify_app_ownership(app_instance).await?;

        let block = entity::blocks::Entity::find()
            .filter(entity::blocks::Column::AppInstanceId.eq(app_instance))
            .filter(entity::blocks::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?;

        if let Some(b) = block {
            Ok(Some(Block {
                name: format!("block_{}", b.block_number),
                block_number: b.block_number,
                start_sequence: b.start_sequence,
                end_sequence: b.end_sequence,
                actions_commitment: b.actions_commitment.unwrap_or_default(),
                state_commitment: b.state_commitment.unwrap_or_default(),
                time_since_last_block: b.time_since_last_block.unwrap_or(0),
                number_of_transactions: b.number_of_transactions,
                start_actions_commitment: b.start_actions_commitment.unwrap_or_default(),
                end_actions_commitment: b.end_actions_commitment.unwrap_or_default(),
                state_data_availability: b.state_data_availability,
                proof_data_availability: b.proof_data_availability,
                created_at: b.created_at.timestamp() as u64,
                state_calculated_at: b.state_calculated_at.map(|t| t.timestamp() as u64),
                proved_at: b.proved_at.map(|t| t.timestamp() as u64),
            }))
        } else {
            Ok(None)
        }
    }

    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>> {
        self.verify_app_ownership(app_instance).await?;

        let blocks = entity::blocks::Entity::find()
            .filter(entity::blocks::Column::AppInstanceId.eq(app_instance))
            .filter(entity::blocks::Column::BlockNumber.gte(from_block))
            .filter(entity::blocks::Column::BlockNumber.lte(to_block))
            .order_by_asc(entity::blocks::Column::BlockNumber)
            .all(self.db.connection())
            .await?;

        Ok(blocks.into_iter().map(|b| Block {
            name: format!("block_{}", b.block_number),
            block_number: b.block_number,
            start_sequence: b.start_sequence,
            end_sequence: b.end_sequence,
            actions_commitment: b.actions_commitment.unwrap_or_default(),
            state_commitment: b.state_commitment.unwrap_or_default(),
            time_since_last_block: b.time_since_last_block.unwrap_or(0),
            number_of_transactions: b.number_of_transactions,
            start_actions_commitment: b.start_actions_commitment.unwrap_or_default(),
            end_actions_commitment: b.end_actions_commitment.unwrap_or_default(),
            state_data_availability: b.state_data_availability,
            proof_data_availability: b.proof_data_availability,
            created_at: b.created_at.timestamp() as u64,
            state_calculated_at: b.state_calculated_at.map(|t| t.timestamp() as u64),
            proved_at: b.proved_at.map(|t| t.timestamp() as u64),
        }).collect())
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>> {
        self.verify_app_ownership(app_instance).await?;

        // Get app instance to check block creation conditions
        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        // Check if paused
        if app.is_paused {
            return Ok(None);
        }

        // Check minimum time between blocks
        let last_block = entity::blocks::Entity::find()
            .filter(entity::blocks::Column::AppInstanceId.eq(app_instance))
            .order_by_desc(entity::blocks::Column::BlockNumber)
            .one(self.db.connection())
            .await?;

        if let Some(ref last) = last_block {
            let last_created = last.created_at;
            let elapsed = (Utc::now() - last_created).num_seconds() as u64;
            if elapsed < app.min_time_between_blocks {
                return Ok(None); // Not enough time passed
            }
        }

        // Create new block
        let next_block_number = app.block_number + 1;
        let start_sequence = app.last_settled_sequence + 1;
        let end_sequence = app.sequence;

        // Check if there are sequences to include
        if end_sequence < start_sequence {
            return Ok(None); // No new sequences
        }

        let new_block = entity::blocks::ActiveModel {
            app_instance_id: Set(app_instance.to_string()),
            block_number: Set(next_block_number),
            start_sequence: Set(start_sequence),
            end_sequence: Set(end_sequence),
            actions_commitment: Set(None),
            state_commitment: Set(None),
            time_since_last_block: Set(last_block.as_ref()
                .map(|l| (Utc::now() - l.created_at).num_milliseconds() as u64)),
            number_of_transactions: Set(end_sequence - start_sequence + 1),
            start_actions_commitment: Set(None),
            end_actions_commitment: Set(None),
            state_data_availability: Set(None),
            proof_data_availability: Set(None),
            created_at: Set(Utc::now()),
            state_calculated_at: Set(None),
            proved_at: Set(None),
        };

        new_block.insert(self.db.connection()).await?;

        // Update app instance block number
        let mut app_update: entity::app_instances::ActiveModel = app.into();
        app_update.block_number = Set(next_block_number);
        app_update.update(self.db.connection()).await?;

        Ok(Some(next_block_number))
    }

    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_da: String,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let block = entity::blocks::Entity::find()
            .filter(entity::blocks::Column::AppInstanceId.eq(app_instance))
            .filter(entity::blocks::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Block {} not found", block_number)
            ))?;

        let mut block_update: entity::blocks::ActiveModel = block.into();
        block_update.state_data_availability = Set(Some(state_da));
        block_update.state_calculated_at = Set(Some(Utc::now()));
        block_update.update(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_da: String,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let block = entity::blocks::Entity::find()
            .filter(entity::blocks::Column::AppInstanceId.eq(app_instance))
            .filter(entity::blocks::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Block {} not found", block_number)
            ))?;

        let mut block_update: entity::blocks::ActiveModel = block.into();
        block_update.proof_data_availability = Set(Some(proof_da));
        block_update.proved_at = Set(Some(Utc::now()));
        block_update.update(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<ProofCalculation>> {
        self.verify_app_ownership(app_instance).await?;

        let calc = entity::proof_calculations::Entity::find()
            .filter(entity::proof_calculations::Column::AppInstanceId.eq(app_instance))
            .filter(entity::proof_calculations::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?;

        if let Some(c) = calc {
            Ok(Some(ProofCalculation {
                id: c.id,
                block_number: c.block_number,
                start_sequence: c.start_sequence,
                end_sequence: c.end_sequence,
                proofs: c.proofs.and_then(|j| serde_json::from_value(j).ok()).unwrap_or_default(),
                block_proof: c.block_proof,
                block_proof_submitted: c.block_proof_submitted,
            }))
        } else {
            Ok(None)
        }
    }

    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<ProofCalculation>> {
        self.verify_app_ownership(app_instance).await?;

        let calcs = entity::proof_calculations::Entity::find()
            .filter(entity::proof_calculations::Column::AppInstanceId.eq(app_instance))
            .filter(entity::proof_calculations::Column::BlockNumber.gte(from_block))
            .filter(entity::proof_calculations::Column::BlockNumber.lte(to_block))
            .order_by_asc(entity::proof_calculations::Column::BlockNumber)
            .all(self.db.connection())
            .await?;

        Ok(calcs.into_iter().map(|c| ProofCalculation {
            id: c.id,
            block_number: c.block_number,
            start_sequence: c.start_sequence,
            end_sequence: c.end_sequence,
            proofs: c.proofs.and_then(|j| serde_json::from_value(j).ok()).unwrap_or_default(),
            block_proof: c.block_proof,
            block_proof_submitted: c.block_proof_submitted,
        }).collect())
    }

    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        let proof_id = Uuid::new_v4().to_string();
        let start_sequence = sequences.first().copied().unwrap_or(0);
        let end_sequence = sequences.last().copied();

        // Create proof data structure
        let mut proofs = vec![];
        if !sequences.is_empty() {
            proofs.push(serde_json::json!({
                "sequences": sequences,
                "status": "pending"
            }));
        }
        if let Some(seq1) = merged_sequences_1 {
            proofs.push(serde_json::json!({
                "merged_sequences_1": seq1,
                "status": "pending"
            }));
        }
        if let Some(seq2) = merged_sequences_2 {
            proofs.push(serde_json::json!({
                "merged_sequences_2": seq2,
                "status": "pending"
            }));
        }

        let new_calc = entity::proof_calculations::ActiveModel {
            app_instance_id: Set(app_instance.to_string()),
            id: Set(proof_id),
            block_number: Set(block_number),
            start_sequence: Set(start_sequence),
            end_sequence: Set(end_sequence),
            proofs: Set(Some(serde_json::json!(proofs))),
            block_proof: Set(None),
            block_proof_submitted: Set(false),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        new_calc.insert(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    async fn submit_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        _sequences: Vec<u64>,
        _merged_sequences_1: Option<Vec<u64>>,
        _merged_sequences_2: Option<Vec<u64>>,
        job_id: String,
        da_hash: String,
        cpu_cores: u8,
        prover_architecture: String,
        prover_memory: u64,
        cpu_time: u64,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Find the proof calculation for this block
        let calc = entity::proof_calculations::Entity::find()
            .filter(entity::proof_calculations::Column::AppInstanceId.eq(app_instance))
            .filter(entity::proof_calculations::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Proof calculation for block {} not found", block_number)
            ))?;

        // Create proof metadata
        let proof_metadata = serde_json::json!({
            "job_id": job_id,
            "da_hash": da_hash,
            "cpu_cores": cpu_cores,
            "prover_architecture": prover_architecture,
            "prover_memory": prover_memory,
            "cpu_time": cpu_time,
            "submitted_at": Utc::now().timestamp()
        });

        let mut calc_update: entity::proof_calculations::ActiveModel = calc.into();
        calc_update.block_proof = Set(Some(proof_metadata.to_string()));
        calc_update.block_proof_submitted = Set(true);
        calc_update.updated_at = Set(Utc::now());
        calc_update.update(self.db.connection()).await?;

        // Update block's last proved block number
        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        if block_number > app.last_proved_block_number {
            let mut app_update: entity::app_instances::ActiveModel = app.into();
            app_update.last_proved_block_number = Set(block_number);
            app_update.update(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        _sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Find and update the proof calculation
        let calc = entity::proof_calculations::Entity::find()
            .filter(entity::proof_calculations::Column::AppInstanceId.eq(app_instance))
            .filter(entity::proof_calculations::Column::BlockNumber.eq(block_number))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Proof calculation for block {} not found", block_number)
            ))?;

        // Mark proof as rejected
        let rejected_metadata = serde_json::json!({
            "status": "rejected",
            "rejected_at": Utc::now().timestamp()
        });

        let mut calc_update: entity::proof_calculations::ActiveModel = calc.into();
        calc_update.block_proof = Set(Some(rejected_metadata.to_string()));
        calc_update.block_proof_submitted = Set(false);
        calc_update.updated_at = Set(Utc::now());
        calc_update.update(self.db.connection()).await?;

        Ok(self.generate_tx_hash())
    }

    // ===== Settlement =====

    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>> {
        self.verify_app_ownership(app_instance).await?;

        let settlement = entity::block_settlements::Entity::find()
            .filter(entity::block_settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::block_settlements::Column::BlockNumber.eq(block_number))
            .filter(entity::block_settlements::Column::Chain.eq(chain))
            .one(self.db.connection())
            .await?;

        if let Some(s) = settlement {
            Ok(Some(BlockSettlement {
                block_number: s.block_number,
                settlement_tx_hash: s.settlement_tx_hash,
                settlement_tx_included_in_block: s.settlement_tx_included_in_block.is_some(),
                sent_to_settlement_at: s.sent_to_settlement_at.map(|t| t.timestamp() as u64),
                settled_at: s.settled_at.map(|t| t.timestamp() as u64),
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>> {
        self.verify_app_ownership(app_instance).await?;

        let settlements = entity::settlements::Entity::find()
            .filter(entity::settlements::Column::AppInstanceId.eq(app_instance))
            .all(self.db.connection())
            .await?;

        Ok(settlements.into_iter().map(|s| s.chain).collect())
    }

    async fn get_settlement_address(&self, app_instance: &str, chain: &str) -> Result<Option<String>> {
        self.verify_app_ownership(app_instance).await?;

        let settlement = entity::settlements::Entity::find()
            .filter(entity::settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::settlements::Column::Chain.eq(chain))
            .one(self.db.connection())
            .await?;

        Ok(settlement.and_then(|s| s.settlement_address))
    }

    async fn get_settlement_job_for_chain(&self, app_instance: &str, chain: &str) -> Result<Option<u64>> {
        self.verify_app_ownership(app_instance).await?;

        let settlement = entity::settlements::Entity::find()
            .filter(entity::settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::settlements::Column::Chain.eq(chain))
            .one(self.db.connection())
            .await?;

        Ok(settlement.and_then(|s| s.settlement_job))
    }

    async fn set_settlement_address(
        &self,
        app_instance: &str,
        chain: String,
        address: Option<String>,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Check if settlement exists
        let existing = entity::settlements::Entity::find()
            .filter(entity::settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::settlements::Column::Chain.eq(&chain))
            .one(self.db.connection())
            .await?;

        if let Some(settlement) = existing {
            // Update existing
            let mut settlement_update: entity::settlements::ActiveModel = settlement.into();
            settlement_update.settlement_address = Set(address);
            settlement_update.updated_at = Set(Utc::now());
            settlement_update.update(self.db.connection()).await?;
        } else {
            // Create new
            let new_settlement = entity::settlements::ActiveModel {
                app_instance_id: Set(app_instance.to_string()),
                chain: Set(chain),
                last_settled_block_number: Set(0),
                settlement_address: Set(address),
                settlement_job: Set(None),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
            };
            new_settlement.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Check if block settlement exists
        let existing = entity::block_settlements::Entity::find()
            .filter(entity::block_settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::block_settlements::Column::BlockNumber.eq(block_number))
            .filter(entity::block_settlements::Column::Chain.eq(&chain))
            .one(self.db.connection())
            .await?;

        if let Some(block_settlement) = existing {
            // Update existing
            let mut update: entity::block_settlements::ActiveModel = block_settlement.into();
            update.settlement_tx_hash = Set(Some(settlement_tx_hash));
            update.sent_to_settlement_at = Set(Some(Utc::now()));
            update.updated_at = Set(Utc::now());
            update.update(self.db.connection()).await?;
        } else {
            // Create new
            let new_settlement = entity::block_settlements::ActiveModel {
                app_instance_id: Set(app_instance.to_string()),
                block_number: Set(block_number),
                chain: Set(chain),
                settlement_tx_hash: Set(Some(settlement_tx_hash)),
                settlement_tx_included_in_block: Set(None),
                sent_to_settlement_at: Set(Some(Utc::now())),
                settled_at: Set(None),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
            };
            new_settlement.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Find the block settlement
        let block_settlement = entity::block_settlements::Entity::find()
            .filter(entity::block_settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::block_settlements::Column::BlockNumber.eq(block_number))
            .filter(entity::block_settlements::Column::Chain.eq(&chain))
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Block settlement for block {} on chain {} not found", block_number, chain)
            ))?;

        // Update with settled information
        let mut update: entity::block_settlements::ActiveModel = block_settlement.into();
        update.settlement_tx_included_in_block = Set(Some(settled_at));
        update.settled_at = Set(Some(chrono::DateTime::from_timestamp(settled_at as i64, 0).unwrap()));
        update.updated_at = Set(Utc::now());
        update.update(self.db.connection()).await?;

        // Update settlement's last settled block number
        let settlement = entity::settlements::Entity::find()
            .filter(entity::settlements::Column::AppInstanceId.eq(app_instance))
            .filter(entity::settlements::Column::Chain.eq(&chain))
            .one(self.db.connection())
            .await?;

        if let Some(s) = settlement {
            if block_number > s.last_settled_block_number {
                let mut settlement_update: entity::settlements::ActiveModel = s.into();
                settlement_update.last_settled_block_number = Set(block_number);
                settlement_update.updated_at = Set(Utc::now());
                settlement_update.update(self.db.connection()).await?;
            }
        }

        // Update app instance's last settled block number if appropriate
        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        if block_number > app.last_settled_block_number {
            let mut app_update: entity::app_instances::ActiveModel = app.into();
            app_update.last_settled_block_number = Set(block_number);
            app_update.update(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    // ===== String KV Storage (IMPLEMENTED) =====

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        self.verify_app_ownership(app_instance).await?;

        let kv = entity::app_instance_kv_string::Entity::find()
            .filter(entity::app_instance_kv_string::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_string::Column::Key.eq(key))
            .one(self.db.connection())
            .await?;

        Ok(kv.map(|k| k.value))
    }

    async fn get_all_kv_string(&self, app_instance: &str) -> Result<HashMap<String, String>> {
        self.verify_app_ownership(app_instance).await?;

        let kvs = entity::app_instance_kv_string::Entity::find()
            .filter(entity::app_instance_kv_string::Column::AppInstanceId.eq(app_instance))
            .all(self.db.connection())
            .await?;

        let mut map = HashMap::new();
        for kv in kvs {
            map.insert(kv.key, kv.value);
        }

        Ok(map)
    }

    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>> {
        self.verify_app_ownership(app_instance).await?;

        let mut query = entity::app_instance_kv_string::Entity::find()
            .filter(entity::app_instance_kv_string::Column::AppInstanceId.eq(app_instance));

        if let Some(prefix) = prefix {
            query = query.filter(entity::app_instance_kv_string::Column::Key.starts_with(prefix));
        }

        if let Some(limit) = limit {
            query = query.limit(limit as u64);
        }

        let kvs = query.all(self.db.connection()).await?;
        Ok(kvs.into_iter().map(|kv| kv.key).collect())
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Check if key exists
        let existing = entity::app_instance_kv_string::Entity::find()
            .filter(entity::app_instance_kv_string::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_string::Column::Key.eq(&key))
            .one(self.db.connection())
            .await?;

        if let Some(existing) = existing {
            let mut kv: entity::app_instance_kv_string::ActiveModel = existing.into();
            kv.value = Set(value);
            kv.updated_at = Set(Utc::now());
            kv.update(self.db.connection()).await?;
        } else {
            let new_kv = entity::app_instance_kv_string::ActiveModel {
                app_instance_id: Set(app_instance.to_string()),
                key: Set(key),
                value: Set(value),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
            };
            new_kv.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn delete_kv_string(&self, app_instance: &str, key: &str) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        entity::app_instance_kv_string::Entity::delete_many()
            .filter(entity::app_instance_kv_string::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_string::Column::Key.eq(key))
            .exec(self.db.connection())
            .await?;

        Ok(self.generate_tx_hash())
    }

    // ===== Binary KV Storage (IMPLEMENTED) =====

    async fn get_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.verify_app_ownership(app_instance).await?;

        let kv = entity::app_instance_kv_binary::Entity::find()
            .filter(entity::app_instance_kv_binary::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_binary::Column::Key.eq(key))
            .one(self.db.connection())
            .await?;

        Ok(kv.map(|k| k.value))
    }

    async fn list_kv_binary_keys(
        &self,
        app_instance: &str,
        prefix: Option<&[u8]>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>> {
        self.verify_app_ownership(app_instance).await?;

        let mut query = entity::app_instance_kv_binary::Entity::find()
            .filter(entity::app_instance_kv_binary::Column::AppInstanceId.eq(app_instance));

        // Note: prefix filtering on binary keys would need raw SQL
        if prefix.is_some() {
            warn!("Binary key prefix filtering not yet implemented");
        }

        if let Some(limit) = limit {
            query = query.limit(limit as u64);
        }

        let kvs = query.all(self.db.connection()).await?;
        Ok(kvs.into_iter().map(|kv| kv.key).collect())
    }

    async fn set_kv_binary(&self, app_instance: &str, key: Vec<u8>, value: Vec<u8>) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Check if key exists
        let existing = entity::app_instance_kv_binary::Entity::find()
            .filter(entity::app_instance_kv_binary::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_binary::Column::Key.eq(key.clone()))
            .one(self.db.connection())
            .await?;

        if let Some(existing) = existing {
            let mut kv: entity::app_instance_kv_binary::ActiveModel = existing.into();
            kv.value = Set(value);
            kv.updated_at = Set(Utc::now());
            kv.update(self.db.connection()).await?;
        } else {
            let new_kv = entity::app_instance_kv_binary::ActiveModel {
                app_instance_id: Set(app_instance.to_string()),
                key: Set(key),
                value: Set(value),
                value_da: Set(None),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
            };
            new_kv.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    async fn delete_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        entity::app_instance_kv_binary::Entity::delete_many()
            .filter(entity::app_instance_kv_binary::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_kv_binary::Column::Key.eq(key))
            .exec(self.db.connection())
            .await?;

        Ok(self.generate_tx_hash())
    }

    // ===== Metadata Management =====

    async fn get_metadata(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        self.verify_app_ownership(app_instance).await?;

        let metadata = entity::app_instance_metadata::Entity::find()
            .filter(entity::app_instance_metadata::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_metadata::Column::Key.eq(key))
            .one(self.db.connection())
            .await?;

        Ok(metadata.map(|m| m.value))
    }

    async fn get_all_metadata(&self, app_instance: &str) -> Result<HashMap<String, String>> {
        self.verify_app_ownership(app_instance).await?;

        let metadata_entries = entity::app_instance_metadata::Entity::find()
            .filter(entity::app_instance_metadata::Column::AppInstanceId.eq(app_instance))
            .all(self.db.connection())
            .await?;

        let mut result = HashMap::new();
        for entry in metadata_entries {
            result.insert(entry.key, entry.value);
        }

        Ok(result)
    }

    async fn add_metadata(&self, app_instance: &str, key: String, value: String) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Check if metadata exists
        let existing = entity::app_instance_metadata::Entity::find()
            .filter(entity::app_instance_metadata::Column::AppInstanceId.eq(app_instance))
            .filter(entity::app_instance_metadata::Column::Key.eq(&key))
            .one(self.db.connection())
            .await?;

        if let Some(metadata) = existing {
            // Update existing
            let mut metadata_update: entity::app_instance_metadata::ActiveModel = metadata.into();
            metadata_update.value = Set(value);
            metadata_update.updated_at = Set(Utc::now());
            metadata_update.update(self.db.connection()).await?;
        } else {
            // Create new
            let new_metadata = entity::app_instance_metadata::ActiveModel {
                app_instance_id: Set(app_instance.to_string()),
                key: Set(key),
                value: Set(value),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
            };
            new_metadata.insert(self.db.connection()).await?;
        }

        Ok(self.generate_tx_hash())
    }

    // ===== App Instance Data =====

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        self.verify_app_ownership(app_instance).await?;

        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        // Note: Many fields are not in the database yet
        Ok(AppInstance {
            id: app.app_instance_id.clone(),
            silvana_app_name: app.name,
            description: app.description,
            admin: app.owner.clone(), // Using owner as admin for now
            metadata: HashMap::new(),
            kv: HashMap::new(),
            sequence: app.sequence,
            block_number: app.block_number,
            previous_block_timestamp: 0,
            previous_block_last_sequence: 0,
            last_proved_block_number: app.last_proved_block_number,
            last_settled_block_number: app.last_settled_block_number,
            last_settled_sequence: app.last_settled_sequence,
            last_purged_sequence: app.last_purged_sequence,
            settlements: HashMap::new(),
            is_paused: app.is_paused,
            min_time_between_blocks: app.min_time_between_blocks,
            created_at: app.created_at.timestamp() as u64,
            updated_at: app.updated_at.timestamp() as u64,
        })
    }

    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String> {
        self.verify_app_ownership(app_instance).await?;

        let app = entity::app_instances::Entity::find_by_id(app_instance)
            .one(self.db.connection())
            .await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("App instance {} not found", app_instance)
            ))?;

        Ok(app.owner) // Using owner as admin
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        self.verify_app_ownership(app_instance).await?;
        // Not stored in database, default to false
        Ok(false)
    }

    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64> {
        self.verify_app_ownership(app_instance).await?;
        // Not stored in database, default to 60 seconds
        Ok(60)
    }

    // ===== Batch Operations =====

    fn supports_multicall(&self) -> bool {
        // Private layer uses direct execution, no batching needed
        false
    }

    async fn multicall_job_operations(
        &self,
        _operations: Vec<MulticallOperations>,
    ) -> Result<MulticallResult> {
        // Private layer doesn't support multicall - execute operations directly instead
        Err(PrivateCoordinationError::NotImplemented(
            "Multicall not supported in Private layer - use direct operations".to_string()
        ))
    }

    // ===== State Purging =====

    async fn purge(&self, app_instance: &str, sequences_to_purge: u64) -> Result<Self::TransactionHash> {
        self.verify_app_ownership(app_instance).await?;

        // Delete old optimistic states
        entity::optimistic_state::Entity::delete_many()
            .filter(entity::optimistic_state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::optimistic_state::Column::Sequence.lte(sequences_to_purge))
            .exec(self.db.connection())
            .await?;

        // Delete old proved states
        entity::state::Entity::delete_many()
            .filter(entity::state::Column::AppInstanceId.eq(app_instance))
            .filter(entity::state::Column::Sequence.lte(sequences_to_purge))
            .exec(self.db.connection())
            .await?;

        info!("Purged sequences up to {} for app_instance {}", sequences_to_purge, app_instance);

        Ok(self.generate_tx_hash())
    }
}