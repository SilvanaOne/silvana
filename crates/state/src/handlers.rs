//! Service handler implementations for StateService

use anyhow::Result;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, NotSet, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use sea_orm::prelude::Json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::{
    auth::{verify_jwt, AuthInfo},
    concurrency::ConcurrencyController,
    coordinator_auth::verify_and_authorize,
    database::Database,
    entity::{
        app_instance_kv_binary, app_instance_kv_string, app_instances, block_settlements, blocks, jobs,
        object_lock_queue, object_versions, objects, optimistic_state, proof_calculations, proofs, settlements, state, user_actions,
        lock_request_bundle,
        coordinator_groups, app_instance_coordinator_access,
    },
    proto::state_service_server::StateService,
    proto::*,
    storage::PrivateStateStorage,
};

/// Helper function to log and return internal errors
fn log_internal_error(context: &str, error: impl std::fmt::Display) -> Status {
    error!("Internal error - {}: {}", context, error);
    Status::internal(format!("{}: {}", context, error))
}

/// Helper function to log and return already_exists errors
#[allow(dead_code)]
fn log_already_exists(context: &str) -> Status {
    warn!("Already exists: {}", context);
    Status::already_exists(context)
}

/// Helper function to log and return not_found errors
#[allow(dead_code)]
fn log_not_found(context: &str) -> Status {
    warn!("Not found: {}", context);
    Status::not_found(context)
}

/// Convert chrono DateTime to protobuf Timestamp
fn to_timestamp(dt: chrono::DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// Convert prost_types::Struct to serde_json::Value
fn prost_struct_to_json(s: prost_types::Struct) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in s.fields {
        map.insert(k, prost_value_to_json(v));
    }
    serde_json::Value::Object(map)
}

/// Convert prost_types::Value to serde_json::Value
fn prost_value_to_json(v: prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match v.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::json!(n),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(b),
        Some(Kind::StructValue(s)) => prost_struct_to_json(s),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.into_iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

/// Convert serde_json::Value to prost_types::Struct
fn json_to_prost_struct(val: serde_json::Value) -> Option<prost_types::Struct> {
    if let serde_json::Value::Object(map) = val {
        let mut fields = BTreeMap::new();
        for (k, v) in map {
            fields.insert(k, json_to_prost_value(v));
        }
        Some(prost_types::Struct { fields })
    } else {
        None
    }
}

/// Convert serde_json::Value to prost_types::Value
fn json_to_prost_value(val: serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match val {
        serde_json::Value::Null => Some(Kind::NullValue(0)),
        serde_json::Value::Bool(b) => Some(Kind::BoolValue(b)),
        serde_json::Value::Number(n) => {
            Some(Kind::NumberValue(n.as_f64().unwrap_or(0.0)))
        }
        serde_json::Value::String(s) => Some(Kind::StringValue(s)),
        serde_json::Value::Array(arr) => {
            let values = arr.into_iter().map(json_to_prost_value).collect();
            Some(Kind::ListValue(prost_types::ListValue { values }))
        }
        serde_json::Value::Object(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                fields.insert(k, json_to_prost_value(v));
            }
            Some(Kind::StructValue(prost_types::Struct { fields }))
        }
    };
    prost_types::Value { kind }
}

/// Calculate the next binary prefix for range queries
/// Used to create an upper bound for prefix filtering: [prefix, next_prefix)
///
/// Returns None if the prefix is all 0xFF bytes (cannot increment further)
fn next_binary_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    if prefix.is_empty() {
        return None;
    }

    let mut next = prefix.to_vec();

    // Increment from the rightmost byte, carrying overflow left
    for i in (0..next.len()).rev() {
        if next[i] < 0xFF {
            next[i] += 1;
            return Some(next);
        }
        // This byte is 0xFF, set to 0 and continue carry
        next[i] = 0;
    }

    // All bytes were 0xFF, cannot increment
    None
}

// ============================================================================
// Proof Calculation Helper Types and Functions (matching prover.move logic)
// ============================================================================

/// Proof data structure matching Move Proof struct from prover.move
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct ProofData {
    status: u8,  // 1=Started, 2=Calculated, 3=Rejected, 4=Reserved, 5=Used
    da_hash: Option<String>,
    sequence1: Option<Vec<u64>>,  // For merge proofs
    sequence2: Option<Vec<u64>>,  // For merge proofs
    rejected_count: u16,
    timestamp: u64,  // Milliseconds since epoch
    prover: String,  // Address from JWT
    user: Option<String>,
    job_id: Option<String>,
    sequences: Vec<u64>,  // Key for lookup (always sorted)
}

/// Proof timeout constant from prover.move line 130
const PROOF_TIMEOUT_MS: u64 = 300_000; // 5 minutes

/// Parse JSON proofs array from database
fn parse_proofs_json(json_value: Option<Json>) -> Result<Vec<ProofData>, Status> {
    match json_value {
        Some(json) => serde_json::from_value(json)
            .map_err(|e| Status::internal(format!("Failed to parse proofs JSON: {}", e))),
        None => Ok(Vec::new()),
    }
}

/// Serialize proofs back to JSON for database storage
fn serialize_proofs_json(proofs: &[ProofData]) -> Result<Json, Status> {
    serde_json::to_value(proofs)
        .map_err(|e| Status::internal(format!("Failed to serialize proofs: {}", e)))
}

/// Sort and validate sequences (prover.move lines 663-704)
fn sort_and_validate_sequences(
    sequences: &[u64],
    start_sequence: u64,
    end_sequence: Option<u64>,
) -> Result<Vec<u64>, Status> {
    if sequences.is_empty() {
        return Err(Status::invalid_argument("Sequences cannot be empty"));
    }

    let mut sorted = sequences.to_vec();
    sorted.sort_unstable();

    if sorted[0] < start_sequence {
        return Err(Status::invalid_argument(format!(
            "Sequence {} is below start_sequence {}",
            sorted[0], start_sequence
        )));
    }

    if let Some(end) = end_sequence {
        if sorted[sorted.len() - 1] > end {
            return Err(Status::invalid_argument(format!(
                "Sequence {} is above end_sequence {}",
                sorted[sorted.len() - 1], end
            )));
        }
    }

    Ok(sorted)
}

/// Check if proof can be restarted (prover.move lines 444-462)
fn can_restart_proof(proof: &ProofData, current_time_ms: u64) -> bool {
    let is_timed_out = current_time_ms > proof.timestamp + PROOF_TIMEOUT_MS;
    proof.status == 3 /* REJECTED */ || is_timed_out
}

/// Check if sub-proof can be reserved (prover.move lines 349-366, 520-530)
fn can_reserve_proof(proof: &ProofData, current_time_ms: u64, is_block_proof: bool) -> bool {
    let is_timed_out = current_time_ms > proof.timestamp + PROOF_TIMEOUT_MS;
    proof.status == 2 /* CALCULATED */ ||
    is_timed_out ||
    (is_block_proof && (proof.status == 5 /* USED */ || proof.status == 4 /* RESERVED */))
}

/// Return sub-proof to calculated status (prover.move lines 334-338)
fn return_proof_to_calculated(proof: &mut ProofData, current_time_ms: u64) {
    proof.status = 2; // CALCULATED
    proof.timestamp = current_time_ms;
    proof.user = None;
}

/// Reserve a proof for merge (prover.move lines 340-372)
fn reserve_proof(proof: &mut ProofData, current_time_ms: u64, user: String) {
    proof.status = 4; // RESERVED
    proof.timestamp = current_time_ms;
    proof.user = Some(user);
}

/// Mark proof as used (prover.move lines 324-332)
fn mark_proof_as_used(proof: &mut ProofData, current_time_ms: u64, user: String) {
    proof.status = 5; // USED
    proof.timestamp = current_time_ms;
    proof.user = Some(user);
}

/// Convert ProofData to proto ProofInCalculation
fn proof_data_to_proto(proof: &ProofData) -> ProofInCalculation {
    ProofInCalculation {
        status: proof.status as u32,
        da_hash: proof.da_hash.clone(),
        sequence1: proof.sequence1.clone().unwrap_or_default(),
        sequence2: proof.sequence2.clone().unwrap_or_default(),
        rejected_count: proof.rejected_count as u32,
        timestamp: proof.timestamp,
        prover: proof.prover.clone(),
        user: proof.user.clone(),
        job_id: proof.job_id.clone(),
        sequences: proof.sequences.clone(),
    }
}

/// StateService implementation
pub struct StateServiceImpl {
    db: Arc<Database>,
    storage: Option<PrivateStateStorage>,
    concurrency: Arc<ConcurrencyController>,
}

impl StateServiceImpl {
    pub fn new(db: Arc<Database>, storage: Option<PrivateStateStorage>, concurrency: Arc<ConcurrencyController>) -> Self {
        Self {
            db,
            storage,
            concurrency,
        }
    }

    /// Extract auth from request body
    fn extract_auth_from_body(&self, auth: &Option<JwtAuth>) -> Result<AuthInfo, Status> {
        let jwt_auth = auth.as_ref()
            .ok_or_else(|| {
                warn!("Authentication failed: Missing auth field");
                Status::unauthenticated("Missing auth field")
            })?;

        verify_jwt(&jwt_auth.token)
            .map_err(|e| {
                warn!("JWT verification failed: {}", e);
                Status::unauthenticated(format!("Invalid JWT: {}", e))
            })
    }

    /// Check app instance permission and log detailed error if missing
    fn require_app_instance_permission(&self, auth: &AuthInfo, app_instance_id: &str) -> Result<(), Status> {
        if !auth.has_app_instance_permission(app_instance_id) {
            let app_instances: Vec<String> = auth.scope_permissions.iter()
                .filter_map(|p| match p {
                    crate::auth::ScopePermission::AppInstance(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            warn!(
                "Permission denied: JWT scope missing permission for app_instance '{}'. Available app_instances in scope: {:?}",
                app_instance_id, app_instances
            );
            return Err(Status::permission_denied(
                format!("JWT scope missing permission for app_instance {}", app_instance_id),
            ));
        }
        Ok(())
    }

    /// Check object permission and log detailed error if missing
    #[allow(dead_code)]
    fn require_object_permission(&self, auth: &AuthInfo, object_id: &str) -> Result<(), Status> {
        if !auth.has_object_permission(object_id) {
            let objects: Vec<String> = auth.scope_permissions.iter()
                .filter_map(|p| match p {
                    crate::auth::ScopePermission::Object(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            warn!(
                "Permission denied: JWT scope missing permission for object '{}'. Available objects in scope: {:?}",
                object_id, objects
            );
            return Err(Status::permission_denied(
                format!("JWT scope missing permission for object {}", object_id),
            ));
        }
        Ok(())
    }

    /// Verify ownership of app instance
    async fn verify_ownership(&self, app_instance_id: &str, auth: &AuthInfo) -> Result<(), Status> {
        let owns = self
            .db
            .verify_app_instance_ownership(app_instance_id, &auth.public_key)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if !owns {
            warn!(
                "Authorization failed: user {} is not owner of app_instance {}",
                auth.public_key, app_instance_id
            );
            return Err(Status::permission_denied(
                "Not authorized for this app instance",
            ));
        }

        Ok(())
    }

    // ===== Coordinator Helper Functions =====

    /// Log coordinator action to audit log
    async fn log_coordinator_action(
        &self,
        coordinator_public_key: &str,
        app_instance_id: &str,
        action: &str,
        job_sequence: Option<u64>,
        success: bool,
        error_message: Option<String>,
    ) -> Result<()> {
        use crate::entity::coordinator_audit_log;

        let audit_entry = coordinator_audit_log::ActiveModel {
            id: NotSet,
            coordinator_public_key: Set(coordinator_public_key.to_string()),
            app_instance_id: Set(app_instance_id.to_string()),
            action: Set(action.to_string()),
            job_sequence: Set(job_sequence),
            timestamp: Set(Utc::now()),
            success: Set(success),
            error_message: Set(error_message),
        };

        coordinator_audit_log::Entity::insert(audit_entry)
            .exec(self.db.connection())
            .await?;

        Ok(())
    }

    /// Convert proto Job to database jobs::Model for response
    fn db_job_to_proto(&self, job: jobs::Model) -> Job {
        Job {
            job_sequence: job.job_sequence,
            description: job.description,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            app_instance_id: job.app_instance_id,
            block_number: job.block_number,
            sequences: job.sequences.and_then(|j| serde_json::from_value(j).ok()).unwrap_or_default(),
            sequences1: job.sequences1.and_then(|j| serde_json::from_value(j).ok()).unwrap_or_default(),
            sequences2: job.sequences2.and_then(|j| serde_json::from_value(j).ok()).unwrap_or_default(),
            data: job.data,
            data_da: job.data_da,
            interval_ms: job.interval_ms,
            next_scheduled_at: job.next_scheduled_at.map(|dt| to_timestamp(dt)),
            status: match job.status.as_str() {
                "pending" => 0,
                "running" => 1,
                "completed" => 2,
                "failed" => 3,
                _ => 0,
            },
            error_message: job.error_message,
            attempts: job.attempts as u32,
            created_at: Some(to_timestamp(job.created_at)),
            updated_at: Some(to_timestamp(job.updated_at)),
            metadata: job.metadata.and_then(json_to_prost_struct),
            agent_jwt: job.agent_jwt,
            jwt_expires_at: job.jwt_expires_at.map(|dt| to_timestamp(dt)),
        }
    }

    /// Start background cleanup task for old completed jobs
    pub fn start_jobs_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400)); // Daily
            loop {
                interval.tick().await;
                if let Err(e) = self.cleanup_completed_jobs().await {
                    warn!("Failed to cleanup completed jobs: {}", e);
                }
            }
        });
    }

    /// Clean up completed jobs older than 30 days
    async fn cleanup_completed_jobs(&self) -> Result<u64, Status> {
        use sea_orm::EntityTrait;

        let cutoff = Utc::now() - chrono::Duration::days(30);

        let result = jobs::Entity::delete_many()
            .filter(jobs::Column::Status.eq("COMPLETED"))
            .filter(jobs::Column::UpdatedAt.lt(cutoff))
            .exec(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Failed to delete old completed jobs: {}", e);
                Status::internal("Cleanup failed")
            })?;

        info!("Cleaned up {} old completed jobs", result.rows_affected);
        Ok(result.rows_affected)
    }

    // ===== Individual Job Operation Handlers (Private helpers for coordinator ops) =====

    async fn handle_create_job(
        &self,
        app_instance_id: &str,
        op: CreateJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        // Create job in database
        let job = jobs::ActiveModel {
            job_sequence: NotSet,
            app_instance_id: Set(app_instance_id.to_string()),
            description: Set(op.description),
            developer: Set(op.developer),
            agent: Set(op.agent),
            agent_method: Set(op.agent_method),
            block_number: Set(op.block_number),
            sequences: Set(if op.sequences.is_empty() { None } else { Some(serde_json::to_value(&op.sequences).unwrap()) }),
            sequences1: Set(if op.sequences1.is_empty() { None } else { Some(serde_json::to_value(&op.sequences1).unwrap()) }),
            sequences2: Set(if op.sequences2.is_empty() { None } else { Some(serde_json::to_value(&op.sequences2).unwrap()) }),
            data: Set(op.data),
            data_da: Set(op.data_da),
            status: Set("pending".to_string()),
            error_message: Set(None),
            attempts: Set(0),
            interval_ms: Set(op.interval_ms),
            next_scheduled_at: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(None),  // Coordinator operations don't set metadata
            agent_jwt: Set(op.agent_jwt),
            jwt_expires_at: Set(op.jwt_expires_at.map(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_else(Utc::now)
            })),
        };

        let inserted = job.insert(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to create job", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Job {} created successfully", inserted.job_sequence),
            job: None,
            job_sequence: Some(inserted.job_sequence),
            job_sequences: vec![],
            count: None,
            jobs: vec![],
        })
    }

    async fn handle_start_job(
        &self,
        app_instance_id: &str,
        op: StartJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.eq(op.job_sequence))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found(format!("Job {} not found", op.job_sequence)))?;

        let mut job: jobs::ActiveModel = job.into();
        job.status = Set("running".to_string());
        job.updated_at = Set(Utc::now());
        job.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to start job", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Job {} started successfully", op.job_sequence),
            job: None,
            job_sequence: Some(op.job_sequence),
            job_sequences: vec![],
            count: None,
            jobs: vec![],
        })
    }

    async fn handle_complete_job(
        &self,
        app_instance_id: &str,
        op: CompleteJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.eq(op.job_sequence))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found(format!("Job {} not found", op.job_sequence)))?;

        let mut job: jobs::ActiveModel = job.into();
        job.status = Set("completed".to_string());
        job.updated_at = Set(Utc::now());
        job.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to complete job", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Job {} completed successfully", op.job_sequence),
            job: None,
            job_sequence: Some(op.job_sequence),
            job_sequences: vec![],
            count: None,
            jobs: vec![],
        })
    }

    async fn handle_fail_job(
        &self,
        app_instance_id: &str,
        op: FailJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.eq(op.job_sequence))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found(format!("Job {} not found", op.job_sequence)))?;

        let mut job: jobs::ActiveModel = job.into();
        job.status = Set("failed".to_string());
        job.error_message = Set(Some(op.error_message));
        job.attempts = Set(job.attempts.unwrap() + 1);
        job.updated_at = Set(Utc::now());
        job.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to fail job", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Job {} marked as failed", op.job_sequence),
            job: None,
            job_sequence: Some(op.job_sequence),
            job_sequences: vec![],
            count: None,
            jobs: vec![],
        })
    }

    async fn handle_get_job(
        &self,
        app_instance_id: &str,
        op: GetJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.eq(op.job_sequence))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        match job {
            Some(job) => Ok(CoordinatorJobResponse {
                success: true,
                message: "Job found".to_string(),
                job: Some(self.db_job_to_proto(job)),
                job_sequence: Some(op.job_sequence),
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            }),
            None => Ok(CoordinatorJobResponse {
                success: false,
                message: format!("Job {} not found", op.job_sequence),
                job: None,
                job_sequence: None,
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            }),
        }
    }

    async fn handle_terminate_job(
        &self,
        app_instance_id: &str,
        op: TerminateJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let result = jobs::Entity::delete_many()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.eq(op.job_sequence))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to terminate job", e))?;

        if result.rows_affected > 0 {
            Ok(CoordinatorJobResponse {
                success: true,
                message: format!("Job {} terminated successfully", op.job_sequence),
                job: None,
                job_sequence: Some(op.job_sequence),
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            })
        } else {
            Ok(CoordinatorJobResponse {
                success: false,
                message: format!("Job {} not found", op.job_sequence),
                job: None,
                job_sequence: None,
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            })
        }
    }

    async fn handle_get_pending_sequences(
        &self,
        app_instance_id: &str,
        op: GetPendingJobSequencesOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let mut query = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("pending"));

        if let Some(dev) = op.developer {
            query = query.filter(jobs::Column::Developer.eq(dev));
        }
        if let Some(agent) = op.agent {
            query = query.filter(jobs::Column::Agent.eq(agent));
        }
        if let Some(method) = op.agent_method {
            query = query.filter(jobs::Column::AgentMethod.eq(method));
        }

        let jobs = query
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let sequences: Vec<u64> = jobs.iter().map(|j| j.job_sequence).collect();

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Found {} pending jobs", sequences.len()),
            job: None,
            job_sequence: None,
            job_sequences: sequences,
            count: None,
            jobs: vec![],
        })
    }

    async fn handle_get_pending_count(
        &self,
        app_instance_id: &str,
    ) -> Result<CoordinatorJobResponse, Status> {
        let count = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("pending"))
            .count(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Pending jobs count: {}", count),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: Some(count),
            jobs: vec![],
        })
    }

    async fn handle_get_failed_count(
        &self,
        app_instance_id: &str,
    ) -> Result<CoordinatorJobResponse, Status> {
        let count = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("failed"))
            .count(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Failed jobs count: {}", count),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: Some(count),
            jobs: vec![],
        })
    }

    async fn handle_get_total_count(
        &self,
        app_instance_id: &str,
    ) -> Result<CoordinatorJobResponse, Status> {
        let count = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .count(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Total jobs count: {}", count),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: Some(count),
            jobs: vec![],
        })
    }

    async fn handle_get_pending_jobs(
        &self,
        app_instance_id: &str,
    ) -> Result<CoordinatorJobResponse, Status> {
        let jobs_list = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("pending"))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let proto_jobs: Vec<Job> = jobs_list.into_iter().map(|j| self.db_job_to_proto(j)).collect();

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Found {} pending jobs", proto_jobs.len()),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: None,
            jobs: proto_jobs,
        })
    }

    async fn handle_get_failed_jobs(
        &self,
        app_instance_id: &str,
    ) -> Result<CoordinatorJobResponse, Status> {
        let jobs_list = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("failed"))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let proto_jobs: Vec<Job> = jobs_list.into_iter().map(|j| self.db_job_to_proto(j)).collect();

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Found {} failed jobs", proto_jobs.len()),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: None,
            jobs: proto_jobs,
        })
    }

    async fn handle_get_jobs_batch(
        &self,
        app_instance_id: &str,
        op: GetJobsBatchOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let jobs_list = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::JobSequence.is_in(op.job_sequences))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let proto_jobs: Vec<Job> = jobs_list.into_iter().map(|j| self.db_job_to_proto(j)).collect();

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Found {} jobs", proto_jobs.len()),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: None,
            jobs: proto_jobs,
        })
    }

    async fn handle_restart_failed_jobs(
        &self,
        app_instance_id: &str,
        op: RestartFailedJobsOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let mut query = jobs::Entity::update_many()
            .col_expr(jobs::Column::Status, sea_orm::sea_query::Expr::value("pending"))
            .col_expr(jobs::Column::ErrorMessage, sea_orm::sea_query::Expr::value(None::<String>))
            .col_expr(jobs::Column::UpdatedAt, sea_orm::sea_query::Expr::value(Utc::now()))
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("failed"));

        if !op.job_sequences.is_empty() {
            query = query.filter(jobs::Column::JobSequence.is_in(op.job_sequences));
        }

        let result = query
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to restart failed jobs", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Restarted {} failed jobs", result.rows_affected),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: Some(result.rows_affected),
            jobs: vec![],
        })
    }

    async fn handle_remove_failed_jobs(
        &self,
        app_instance_id: &str,
        op: RemoveFailedJobsOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        let mut query = jobs::Entity::delete_many()
            .filter(jobs::Column::AppInstanceId.eq(app_instance_id))
            .filter(jobs::Column::Status.eq("failed"));

        if !op.job_sequences.is_empty() {
            query = query.filter(jobs::Column::JobSequence.is_in(op.job_sequences));
        }

        let result = query
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to remove failed jobs", e))?;

        Ok(CoordinatorJobResponse {
            success: true,
            message: format!("Removed {} failed jobs", result.rows_affected),
            job: None,
            job_sequence: None,
            job_sequences: vec![],
            count: Some(result.rows_affected),
            jobs: vec![],
        })
    }

    async fn handle_create_merge_job(
        &self,
        app_instance_id: &str,
        op: CreateMergeJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        // Step 1: Verify proof calculation exists (matching app_instance.move:1211-1219)
        // For simplicity, we'll check if proofs can be reserved by querying the proof_calculations table
        let proof_calc = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.eq(op.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculation", e))?;

        if proof_calc.is_none() {
            return Ok(CoordinatorJobResponse {
                success: false,
                message: "Failed to reserve proofs for merge: proof calculation not found".to_string(),
                job: None,
                job_sequence: None,
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            });
        }

        // Step 2: Create the merge job (matching app_instance.move:1238-1250)
        let create_job_op = CreateJobOperation {
            description: op.job_description,
            developer: "system".to_string(),
            agent: "merge".to_string(),
            agent_method: "merge_proofs".to_string(),
            block_number: Some(op.block_number),
            sequences: op.sequences,
            sequences1: op.sequences1,
            sequences2: op.sequences2,
            data: None,
            data_da: None,
            interval_ms: None,
            agent_jwt: None,
            jwt_expires_at: None,
        };

        // Reuse the handle_create_job method
        self.handle_create_job(app_instance_id, create_job_op).await
    }

    async fn handle_create_settle_job(
        &self,
        app_instance_id: &str,
        op: CreateSettleJobOperation,
    ) -> Result<CoordinatorJobResponse, Status> {
        // Step 1: Verify chain exists in settlements table
        let settlement = settlements::Entity::find()
            .filter(settlements::Column::AppInstanceId.eq(app_instance_id))
            .filter(settlements::Column::Chain.eq(&op.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch settlement", e))?;

        if settlement.is_none() {
            return Ok(CoordinatorJobResponse {
                success: false,
                message: format!("Settlement chain '{}' not found for app instance", op.chain),
                job: None,
                job_sequence: None,
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            });
        }

        let settlement = settlement.unwrap();

        // Step 2: Check if there's already an active settlement job for this chain
        if settlement.settlement_job.is_some() {
            return Ok(CoordinatorJobResponse {
                success: false,
                message: format!("Settlement job already exists for chain '{}'", op.chain),
                job: None,
                job_sequence: None,
                job_sequences: vec![],
                count: None,
                jobs: vec![],
            });
        }

        // Step 3: Create the settlement job
        // Encode chain in the data field
        let chain_data = op.chain.as_bytes().to_vec();

        let create_job_op = CreateJobOperation {
            description: op.job_description,
            developer: "system".to_string(),
            agent: "settlement".to_string(),
            agent_method: "settle_block".to_string(),
            block_number: Some(op.block_number),
            sequences: vec![],
            sequences1: vec![],
            sequences2: vec![],
            data: Some(chain_data),
            data_da: None,
            interval_ms: None,
            agent_jwt: None,
            jwt_expires_at: None,
        };

        // Create the job
        let job_result = self.handle_create_job(app_instance_id, create_job_op).await?;

        // Step 4: Update settlements.settlement_job with the new job_sequence
        if let Some(job_sequence) = job_result.job_sequence {
            let mut settlement_active: settlements::ActiveModel = settlement.into();
            settlement_active.settlement_job = Set(Some(job_sequence));
            settlement_active.updated_at = Set(chrono::Utc::now().into());

            settlement_active
                .update(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to update settlement_job", e))?;

            info!(
                "Created settlement job {} for chain '{}' in app_instance {}",
                job_sequence, op.chain, app_instance_id
            );
        }

        Ok(job_result)
    }
}

#[tonic::async_trait]
impl StateService for StateServiceImpl {
    type SubscribeJobEventsStream = std::pin::Pin<
        Box<dyn Stream<Item = Result<JobEventMessage, Status>> + Send>
    >;

    async fn create_app_instance(
        &self,
        request: Request<CreateAppInstanceRequest>,
    ) -> Result<Response<CreateAppInstanceResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Create entity
        let app_instance = app_instances::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            owner: Set(auth.public_key.clone()),
            name: Set(req.name),
            description: Set(req.description),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            // Coordination layer support fields - default values
            admin: Set(None),
            is_paused: Set(false),
            min_time_between_blocks: Set(60),
            block_number: Set(0),
            sequence: Set(0),
            last_proved_block_number: Set(0),
            last_settled_block_number: Set(0),
            last_settled_sequence: Set(0),
            last_purged_sequence: Set(0),
        };

        // Insert to database
        let result = app_instance
            .insert(self.db.connection())
            .await
            .map_err(|e| {
                if e.to_string().contains("Duplicate") {
                    Status::already_exists("App instance already exists")
                } else {
                    warn!("Database error creating app instance: {}", e);
                    Status::internal("Failed to create app instance")
                }
            })?;

        info!("Created app instance {} for owner {}", result.app_instance_id, result.owner);

        // Return response
        Ok(Response::new(CreateAppInstanceResponse {
            success: true,
            message: "App instance created successfully".to_string(),
            app_instance: Some(AppInstance {
                app_instance_id: result.app_instance_id,
                owner: result.owner,
                name: result.name,
                description: result.description,
                created_at: Some(to_timestamp(result.created_at)),
                updated_at: Some(to_timestamp(result.updated_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
                admin: result.admin,
                is_paused: result.is_paused,
                min_time_between_blocks: result.min_time_between_blocks,
                block_number: result.block_number,
                sequence: result.sequence,
                last_proved_block_number: result.last_proved_block_number,
                last_settled_block_number: result.last_settled_block_number,
                last_settled_sequence: result.last_settled_sequence,
                last_purged_sequence: result.last_purged_sequence,
            }),
        }))
    }

    async fn get_app_instance(
        &self,
        request: Request<GetAppInstanceRequest>,
    ) -> Result<Response<GetAppInstanceResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch from database
        let app_instance = app_instances::Entity::find_by_id(req.app_instance_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Database error fetching app instance: {}", e);
                Status::internal("Failed to fetch app instance")
            })?
            .ok_or_else(|| {
                debug!("App instance not found: app_instance_id={}", req.app_instance_id);
                Status::not_found("App instance not found")
            })?;

        debug!("Retrieved app instance {}", app_instance.app_instance_id);

        // Return response
        Ok(Response::new(GetAppInstanceResponse {
            app_instance: Some(AppInstance {
                app_instance_id: app_instance.app_instance_id,
                owner: app_instance.owner,
                name: app_instance.name,
                description: app_instance.description,
                created_at: Some(to_timestamp(app_instance.created_at)),
                updated_at: Some(to_timestamp(app_instance.updated_at)),
                metadata: app_instance.metadata.and_then(json_to_prost_struct),
                admin: app_instance.admin,
                is_paused: app_instance.is_paused,
                min_time_between_blocks: app_instance.min_time_between_blocks,
                block_number: app_instance.block_number,
                sequence: app_instance.sequence,
                last_proved_block_number: app_instance.last_proved_block_number,
                last_settled_block_number: app_instance.last_settled_block_number,
                last_settled_sequence: app_instance.last_settled_sequence,
                last_purged_sequence: app_instance.last_purged_sequence,
            }),
        }))
    }

    async fn list_app_instances(
        &self,
        request: Request<ListAppInstancesRequest>,
    ) -> Result<Response<ListAppInstancesResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Prepare query
        let limit = req.limit.unwrap_or(100).min(1000) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        // Query instances owned by this user
        let paginator = app_instances::Entity::find()
            .filter(app_instances::Column::Owner.eq(&auth.public_key))
            .order_by_desc(app_instances::Column::CreatedAt)
            .paginate(self.db.connection(), limit);

        // Get total count
        let total = paginator.num_items()
            .await
            .map_err(|e| {
                warn!("Database error counting app instances: {}", e);
                Status::internal("Failed to count app instances")
            })? as i32;

        // Fetch the page
        let instances = paginator.fetch_page(offset / limit)
            .await
            .map_err(|e| {
                warn!("Database error listing app instances: {}", e);
                Status::internal("Failed to list app instances")
            })?;

        debug!("Listed {} app instances for owner {}", instances.len(), auth.public_key);

        // Convert to proto messages
        let app_instances = instances
            .into_iter()
            .map(|instance| AppInstance {
                app_instance_id: instance.app_instance_id,
                owner: instance.owner,
                name: instance.name,
                description: instance.description,
                created_at: Some(to_timestamp(instance.created_at)),
                updated_at: Some(to_timestamp(instance.updated_at)),
                metadata: instance.metadata.and_then(json_to_prost_struct),
                admin: instance.admin,
                is_paused: instance.is_paused,
                min_time_between_blocks: instance.min_time_between_blocks,
                block_number: instance.block_number,
                sequence: instance.sequence,
                last_proved_block_number: instance.last_proved_block_number,
                last_settled_block_number: instance.last_settled_block_number,
                last_settled_sequence: instance.last_settled_sequence,
                last_purged_sequence: instance.last_purged_sequence,
            })
            .collect();

        Ok(Response::new(ListAppInstancesResponse {
            app_instances,
            total,
        }))
    }

    async fn submit_user_action(
        &self,
        request: Request<SubmitUserActionRequest>,
    ) -> Result<Response<SubmitUserActionResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Calculate action hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&req.action_data);
        let action_hash = hasher.finalize().to_vec();

        // Store action data inline (S3 offloading removed - use explicit blob storage API if needed)
        let action_data = req.action_data.clone();
        let action_da = req.action_da;

        // Start transaction with pessimistic locking for gapless sequence generation
        use sea_orm::{TransactionTrait, DatabaseBackend, Statement};
        let txn = self.db.connection().begin().await
            .map_err(|e| log_internal_error("Failed to start transaction", e))?;

        // Get or create sequence counter with FOR UPDATE lock
        // First ensure the counter exists, then select it with lock
        let _insert_result = txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            INSERT INTO action_seq (app_instance_id, next_seq)
            VALUES (?, 1)
            ON DUPLICATE KEY UPDATE next_seq = next_seq
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to initialize sequence", e))?;

        // Now get the current sequence with FOR UPDATE lock
        let seq_result = txn.query_one(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            SELECT next_seq FROM action_seq
            WHERE app_instance_id = ?
            FOR UPDATE
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to get sequence", e))?
            .ok_or_else(|| {
                error!("Failed to get sequence counter - query returned no rows");
                Status::internal("Failed to get sequence counter")
            })?;

        let current_seq: u64 = seq_result.try_get("", "next_seq")
            .map_err(|e| log_internal_error("Failed to parse sequence", e))?;

        // Create user action entity with the assigned sequence
        let user_action = user_actions::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(current_seq),
            action_type: Set(req.action_type.clone()),
            action_data: Set(action_data),
            action_hash: Set(action_hash.clone()),
            action_da: Set(action_da.clone()),
            submitter: Set(auth.public_key.clone()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            created_at: Set(Utc::now()),
        };

        // Insert user action with the assigned sequence
        let _result = user_action
            .insert(&txn)
            .await
            .map_err(|e| log_internal_error("Failed to insert action", e))?;

        // Increment the counter for next use
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            UPDATE action_seq
            SET next_seq = next_seq + 1
            WHERE app_instance_id = ?
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to update sequence", e))?;

        // Commit transaction
        txn.commit().await
            .map_err(|e| log_internal_error("Failed to commit transaction", e))?;

        info!("Submitted user action {} for app {} with sequence {}",
            req.action_type, req.app_instance_id, current_seq);

        // Return simplified response with just the sequence number
        Ok(Response::new(SubmitUserActionResponse {
            success: true,
            message: "User action submitted successfully".to_string(),
            action_sequence: current_seq as u64,
        }))
    }

    async fn get_user_actions(
        &self,
        request: Request<GetUserActionsRequest>,
    ) -> Result<Response<GetUserActionsResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query user actions in range
        let actions = user_actions::Entity::find()
            .filter(user_actions::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(user_actions::Column::Sequence.gte(req.from_sequence as i64))
            .filter(user_actions::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(user_actions::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Database error fetching user actions: {}", e);
                Status::internal("Failed to fetch user actions")
            })?;

        debug!("Retrieved {} user actions for app {} in range {}-{}",
            actions.len(), req.app_instance_id, req.from_sequence, req.to_sequence);

        // Convert to proto messages, retrieving from S3 if needed
        let mut user_actions = Vec::new();
        for action in actions {
            // Retrieve data from inline or S3
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let action_data = action.action_data.clone();

            user_actions.push(UserAction {
                app_instance_id: action.app_instance_id,
                sequence: action.sequence as u64,
                action_type: action.action_type,
                action_data,
                action_hash: action.action_hash,
                action_da: action.action_da,
                submitter: action.submitter,
                created_at: Some(to_timestamp(action.created_at)),
                metadata: action.metadata.and_then(json_to_prost_struct),
            });
        }

        Ok(Response::new(GetUserActionsResponse {
            user_actions,
        }))
    }

    async fn update_optimistic_state(
        &self,
        request: Request<UpdateOptimisticStateRequest>,
    ) -> Result<Response<UpdateOptimisticStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&req.state_data);
        let computed_hash = hasher.finalize().to_vec();

        if req.state_hash != computed_hash {
            warn!(
                "State hash mismatch for app_instance_id={}, sequence={}: client_hash={}, computed_hash={}",
                req.app_instance_id,
                req.sequence,
                hex::encode(&req.state_hash),
                hex::encode(&computed_hash)
            );
            return Err(Status::invalid_argument("State hash mismatch"));
        }

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = req.state_data.clone();
        let state_da = req.state_da;

        let (transition_data, transition_da) = if let Some(td) = req.transition_data {
            (Some(td), req.transition_da)
        } else {
            (None, req.transition_da)
        };

        let opt_state = optimistic_state::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(req.sequence),
            state_hash: Set(req.state_hash.clone()),
            state_data: Set(state_data.clone()), // Vec<u8>
            state_da: Set(state_da),
            transition_data: Set(transition_data.clone()), // Option<Vec<u8>>
            transition_da: Set(transition_da),
            commitment: Set(req.commitment),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            computed_at: Set(Utc::now()),
        };

        let result = opt_state.insert(self.db.connection()).await.map_err(|e| {
            if e.to_string().contains("Duplicate") {
                Status::already_exists(format!("Optimistic state for sequence {} exists", req.sequence))
            } else {
                warn!("Database error: {}", e);
                Status::internal("Failed to update optimistic state")
            }
        })?;

        info!("Updated optimistic state for {} sequence {}", req.app_instance_id, req.sequence);

        Ok(Response::new(UpdateOptimisticStateResponse {
            success: true,
            message: "Optimistic state updated".to_string(),
            optimistic_state: Some(OptimisticState {
                app_instance_id: result.app_instance_id,
                sequence: result.sequence as u64,
                state_hash: result.state_hash,
                state_data: state_data.clone(),
                state_da: result.state_da,
                transition_data,
                transition_da: result.transition_da,
                commitment: result.commitment,
                computed_at: Some(to_timestamp(result.computed_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_optimistic_state(
        &self,
        request: Request<GetOptimisticStateRequest>,
    ) -> Result<Response<GetOptimisticStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Build query based on whether sequence is provided
        let opt_state = match req.sequence {
            Some(sequence) => {
                // Get specific sequence
                optimistic_state::Entity::find()
                    .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
                    .filter(optimistic_state::Column::Sequence.eq(sequence as i64))
                    .one(self.db.connection())
                    .await
                    .map_err(|e| {
                        warn!("Database error: {}", e);
                        log_internal_error("Failed to fetch optimistic state", e)
                    })?
            }
            None => {
                // Get latest optimistic state (highest sequence)
                optimistic_state::Entity::find()
                    .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
                    .order_by_desc(optimistic_state::Column::Sequence)
                    .one(self.db.connection())
                    .await
                    .map_err(|e| {
                        warn!("Database error: {}", e);
                        log_internal_error("Failed to fetch latest optimistic state", e)
                    })?
            }
        };

        // Handle not found case gracefully
        let opt_state = match opt_state {
            Some(state) => state,
            None => {
                let msg = if req.sequence.is_some() {
                    format!("Optimistic state not found for sequence {}", req.sequence.unwrap())
                } else {
                    "No optimistic state found for this app instance".to_string()
                };
                return Ok(Response::new(GetOptimisticStateResponse {
                    success: false,
                    message: msg,
                    optimistic_state: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = opt_state.state_data.clone();
        let transition_data = opt_state.transition_data.clone();

        Ok(Response::new(GetOptimisticStateResponse {
            success: true,
            message: "Optimistic state found".to_string(),
            optimistic_state: Some(OptimisticState {
                app_instance_id: opt_state.app_instance_id,
                sequence: opt_state.sequence as u64,
                state_hash: opt_state.state_hash,
                state_data,
                state_da: opt_state.state_da,
                transition_data,
                transition_da: opt_state.transition_da,
                commitment: opt_state.commitment,
                computed_at: Some(to_timestamp(opt_state.computed_at)),
                metadata: opt_state.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn submit_proved_state(
        &self,
        request: Request<SubmitProvedStateRequest>,
    ) -> Result<Response<SubmitProvedStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let (state_data, state_da) = if let Some(sd) = req.state_data {
            (Some(sd), req.state_da)
        } else {
            (None, req.state_da)
        };

        let (proof_data, proof_da) = if let Some(pd) = req.proof_data {
            (Some(pd), req.proof_da)
        } else {
            (None, req.proof_da)
        };

        let proved_state = state::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(req.sequence),
            state_hash: Set(req.state_hash.clone()),
            state_data: Set(state_data.clone()), // Option<Vec<u8>>
            state_da: Set(state_da),
            proof_data: Set(proof_data.clone()), // Option<Vec<u8>>
            proof_da: Set(proof_da),
            proof_hash: Set(req.proof_hash),
            commitment: Set(req.commitment),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            proved_at: Set(Utc::now()),
        };

        let result = proved_state.insert(self.db.connection()).await.map_err(|e| {
            if e.to_string().contains("Duplicate") {
                Status::already_exists(format!("Proved state for sequence {} exists", req.sequence))
            } else {
                warn!("Database error: {}", e);
                Status::internal("Failed to submit proved state")
            }
        })?;

        info!("Submitted proved state for {} sequence {}", req.app_instance_id, req.sequence);

        Ok(Response::new(SubmitProvedStateResponse {
            success: true,
            message: "Proved state submitted".to_string(),
            state: Some(State {
                app_instance_id: result.app_instance_id,
                sequence: result.sequence as u64,
                state_hash: result.state_hash,
                state_data: state_data.clone(),
                state_da: result.state_da,
                proof_data: proof_data.clone(),
                proof_da: result.proof_da,
                proof_hash: result.proof_hash,
                commitment: result.commitment,
                proved_at: Some(to_timestamp(result.proved_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let proved_state = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.eq(req.sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Proved state not found"))?;

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = proved_state.state_data.clone();

        use crate::proto::get_state_response;
        Ok(Response::new(GetStateResponse {
            state: Some(get_state_response::State::ProvedState(State {
                app_instance_id: proved_state.app_instance_id,
                sequence: proved_state.sequence as u64,
                state_hash: proved_state.state_hash,
                state_data,
                state_da: proved_state.state_da,
                proof_data: None, // Don't return proof by default
                proof_da: proved_state.proof_da,
                proof_hash: proved_state.proof_hash,
                commitment: proved_state.commitment,
                proved_at: Some(to_timestamp(proved_state.proved_at)),
                metadata: proved_state.metadata.and_then(json_to_prost_struct),
            })),
        }))
    }

    async fn get_latest_state(
        &self,
        request: Request<GetLatestStateRequest>,
    ) -> Result<Response<GetLatestStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let latest = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .order_by_desc(state::Column::Sequence)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Handle not found case gracefully
        let latest = match latest {
            Some(state) => state,
            None => {
                return Ok(Response::new(GetLatestStateResponse {
                    success: false,
                    message: "No proved state found for this app instance".to_string(),
                    state: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = latest.state_data.clone();

        Ok(Response::new(GetLatestStateResponse {
            success: true,
            message: "Latest proved state found".to_string(),
            state: Some(State {
                app_instance_id: latest.app_instance_id,
                sequence: latest.sequence as u64,
                state_hash: latest.state_hash,
                state_data,
                state_da: latest.state_da,
                proof_data: None,
                proof_da: latest.proof_da,
                proof_hash: latest.proof_hash,
                commitment: latest.commitment,
                proved_at: Some(to_timestamp(latest.proved_at)),
                metadata: latest.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_state_range(
        &self,
        request: Request<GetStateRangeRequest>,
    ) -> Result<Response<GetStateRangeResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query user actions in range
        let actions = user_actions::Entity::find()
            .filter(user_actions::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(user_actions::Column::Sequence.gte(req.from_sequence as i64))
            .filter(user_actions::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(user_actions::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut user_actions_result = Vec::new();
        for action in actions {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let action_data = action.action_data.clone();

            user_actions_result.push(UserAction {
                app_instance_id: action.app_instance_id,
                sequence: action.sequence as u64,
                action_type: action.action_type,
                action_data,
                action_hash: action.action_hash,
                action_da: action.action_da,
                submitter: action.submitter,
                created_at: Some(to_timestamp(action.created_at)),
                metadata: action.metadata.and_then(json_to_prost_struct),
            });
        }

        // Query optimistic states in range
        let opt_states = optimistic_state::Entity::find()
            .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(optimistic_state::Column::Sequence.gte(req.from_sequence as i64))
            .filter(optimistic_state::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(optimistic_state::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut optimistic_states_result = Vec::new();
        for opt in opt_states {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let state_data = opt.state_data.clone();
            let transition_data = opt.transition_data.clone();

            optimistic_states_result.push(OptimisticState {
                app_instance_id: opt.app_instance_id,
                sequence: opt.sequence as u64,
                state_hash: opt.state_hash,
                state_data,
                state_da: opt.state_da,
                transition_data,
                transition_da: opt.transition_da,
                commitment: opt.commitment,
                computed_at: Some(to_timestamp(opt.computed_at)),
                metadata: opt.metadata.and_then(json_to_prost_struct),
            });
        }

        // Query proved states in range
        let states = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.gte(req.from_sequence as i64))
            .filter(state::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(state::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut proved_states_result = Vec::new();
        for s in states {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let state_data = s.state_data.clone();
            let proof_data = if req.include_proofs {
                s.proof_data.clone()
            } else {
                None
            };

            proved_states_result.push(State {
                app_instance_id: s.app_instance_id,
                sequence: s.sequence as u64,
                state_hash: s.state_hash,
                state_data,
                state_da: s.state_da,
                proof_data,
                proof_da: s.proof_da,
                proof_hash: s.proof_hash,
                commitment: s.commitment,
                proved_at: Some(to_timestamp(s.proved_at)),
                metadata: s.metadata.and_then(json_to_prost_struct),
            });
        }

        Ok(Response::new(GetStateRangeResponse {
            user_actions: user_actions_result,
            optimistic_states: optimistic_states_result,
            proved_states: proved_states_result,
        }))
    }

    async fn get_kv_string(
        &self,
        request: Request<GetKvStringRequest>,
    ) -> Result<Response<GetKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let entry = app_instance_kv_string::Entity::find()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_string::Column::Key.eq(&req.key))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        match entry {
            Some(entry) => Ok(Response::new(GetKvStringResponse {
                success: true,
                message: "Key found".to_string(),
                entry: Some(KvStringEntry {
                    app_instance_id: entry.app_instance_id,
                    key: entry.key,
                    value: entry.value,
                    created_at: Some(to_timestamp(entry.created_at)),
                    updated_at: Some(to_timestamp(entry.updated_at)),
                }),
            })),
            None => Ok(Response::new(GetKvStringResponse {
                success: false,
                message: format!("Key '{}' not found", req.key),
                entry: None,
            })),
        }
    }

    async fn set_kv_string(
        &self,
        request: Request<SetKvStringRequest>,
    ) -> Result<Response<SetKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sea_orm::sea_query::OnConflict;
        let entry = app_instance_kv_string::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            key: Set(req.key.clone()),
            value: Set(req.value.clone()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        app_instance_kv_string::Entity::insert(entry)
            .on_conflict(
                OnConflict::columns(vec![
                    app_instance_kv_string::Column::AppInstanceId,
                    app_instance_kv_string::Column::Key,
                ])
                .update_columns(vec![
                    app_instance_kv_string::Column::Value,
                    app_instance_kv_string::Column::UpdatedAt,
                ])
                .to_owned()
            )
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(Response::new(SetKvStringResponse {
            success: true,
            message: "KV string set successfully".to_string(),
        }))
    }

    async fn delete_kv_string(
        &self,
        request: Request<DeleteKvStringRequest>,
    ) -> Result<Response<DeleteKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let result = app_instance_kv_string::Entity::delete_many()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_string::Column::Key.eq(&req.key))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if result.rows_affected == 0 {
            debug!(
                "KV string key not found for deletion: app_instance_id={}, key={}",
                req.app_instance_id, req.key
            );
            return Err(Status::not_found("Key not found"));
        }

        Ok(Response::new(DeleteKvStringResponse {
            success: true,
            message: "KV string deleted successfully".to_string(),
        }))
    }

    async fn list_kv_string_keys(
        &self,
        request: Request<ListKvStringKeysRequest>,
    ) -> Result<Response<ListKvStringKeysResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000);

        let mut query = app_instance_kv_string::Entity::find()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id));

        // Apply prefix filter if provided
        if let Some(prefix) = req.prefix {
            query = query.filter(app_instance_kv_string::Column::Key.starts_with(&prefix));
        }

        let paginator = query
            .order_by_asc(app_instance_kv_string::Column::Key)
            .paginate(self.db.connection(), limit as u64);

        let entries = paginator.fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let keys: Vec<String> = entries.into_iter().map(|e| e.key).collect();

        Ok(Response::new(ListKvStringKeysResponse {
            keys,
        }))
    }

    async fn batch_kv_string(
        &self,
        request: Request<BatchKvStringRequest>,
    ) -> Result<Response<BatchKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        use sea_orm::sea_query::OnConflict;
        let now = Utc::now();
        let mut sets_processed = 0;
        let mut deletes_processed = 0;

        // Process SET operations
        for set_req in req.sets {
            // Verify ownership for each request
            self.verify_ownership(&set_req.app_instance_id, &auth).await?;

            let entry = app_instance_kv_string::ActiveModel {
                app_instance_id: Set(set_req.app_instance_id),
                key: Set(set_req.key),
                value: Set(set_req.value),
                created_at: Set(now),
                updated_at: Set(now),
            };

            app_instance_kv_string::Entity::insert(entry)
                .on_conflict(
                    OnConflict::columns(vec![
                        app_instance_kv_string::Column::AppInstanceId,
                        app_instance_kv_string::Column::Key,
                    ])
                    .update_columns(vec![
                        app_instance_kv_string::Column::Value,
                        app_instance_kv_string::Column::UpdatedAt,
                    ])
                    .to_owned()
                )
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            sets_processed += 1;
        }

        // Process DELETE operations
        for del_req in req.deletes {
            // Verify ownership for each request
            self.verify_ownership(&del_req.app_instance_id, &auth).await?;

            app_instance_kv_string::Entity::delete_many()
                .filter(app_instance_kv_string::Column::AppInstanceId.eq(&del_req.app_instance_id))
                .filter(app_instance_kv_string::Column::Key.eq(&del_req.key))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            deletes_processed += 1;
        }

        Ok(Response::new(BatchKvStringResponse {
            success: true,
            message: format!("Processed {} sets and {} deletes", sets_processed, deletes_processed),
            sets_processed,
            deletes_processed,
        }))
    }

    async fn get_kv_binary(
        &self,
        request: Request<GetKvBinaryRequest>,
    ) -> Result<Response<GetKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let entry = app_instance_kv_binary::Entity::find()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_binary::Column::Key.eq(req.key.clone()))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        match entry {
            Some(entry) => Ok(Response::new(GetKvBinaryResponse {
                success: true,
                message: "Key found".to_string(),
                entry: Some(KvBinaryEntry {
                    app_instance_id: entry.app_instance_id,
                    key: entry.key.clone(),
                    value: entry.value,
                    value_da: entry.value_da,
                    created_at: Some(to_timestamp(entry.created_at)),
                    updated_at: Some(to_timestamp(entry.updated_at)),
                }),
            })),
            None => Ok(Response::new(GetKvBinaryResponse {
                success: false,
                message: "Key not found".to_string(),
                entry: None,
            })),
        }
    }

    async fn set_kv_binary(
        &self,
        request: Request<SetKvBinaryRequest>,
    ) -> Result<Response<SetKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sea_orm::sea_query::OnConflict;
        let entry = app_instance_kv_binary::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            key: Set(req.key.clone()),
            value: Set(req.value.clone()),
            value_da: Set(req.value_da.clone()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        app_instance_kv_binary::Entity::insert(entry)
            .on_conflict(
                OnConflict::columns(vec![
                    app_instance_kv_binary::Column::AppInstanceId,
                    app_instance_kv_binary::Column::Key,
                ])
                .update_columns(vec![
                    app_instance_kv_binary::Column::Value,
                    app_instance_kv_binary::Column::UpdatedAt,
                ])
                .to_owned()
            )
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(Response::new(SetKvBinaryResponse {
            success: true,
            message: "KV binary set successfully".to_string(),
        }))
    }

    async fn delete_kv_binary(
        &self,
        request: Request<DeleteKvBinaryRequest>,
    ) -> Result<Response<DeleteKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let result = app_instance_kv_binary::Entity::delete_many()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_binary::Column::Key.eq(req.key.clone()))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if result.rows_affected == 0 {
            debug!(
                "KV binary key not found for deletion: app_instance_id={}, key={}",
                req.app_instance_id, hex::encode(&req.key)
            );
            return Err(Status::not_found("Key not found"));
        }

        Ok(Response::new(DeleteKvBinaryResponse {
            success: true,
            message: "KV binary deleted successfully".to_string(),
        }))
    }

    async fn list_kv_binary_keys(
        &self,
        request: Request<ListKvBinaryKeysRequest>,
    ) -> Result<Response<ListKvBinaryKeysResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000);

        let mut query = app_instance_kv_binary::Entity::find()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id));

        // Apply prefix filter using database-level range query
        if let Some(prefix) = &req.prefix {
            // Lower bound: key >= prefix
            query = query.filter(app_instance_kv_binary::Column::Key.gte(prefix.clone()));

            // Upper bound: key < next_prefix (if computable)
            if let Some(next_prefix) = next_binary_prefix(prefix) {
                query = query.filter(app_instance_kv_binary::Column::Key.lt(next_prefix));
            }
            // If next_prefix is None (all 0xFF), no upper bound needed
            // The query will return all keys >= prefix
        }

        let paginator = query
            .order_by_asc(app_instance_kv_binary::Column::Key)
            .paginate(self.db.connection(), limit as u64);

        let entries = paginator.fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let keys: Vec<Vec<u8>> = entries.into_iter().map(|e| e.key).collect();

        Ok(Response::new(ListKvBinaryKeysResponse {
            keys,
        }))
    }

    async fn batch_kv_binary(
        &self,
        request: Request<BatchKvBinaryRequest>,
    ) -> Result<Response<BatchKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        use sea_orm::sea_query::OnConflict;
        let now = Utc::now();
        let mut sets_processed = 0;
        let mut deletes_processed = 0;

        for set_req in req.sets {
            self.verify_ownership(&set_req.app_instance_id, &auth).await?;

            let entry = app_instance_kv_binary::ActiveModel {
                app_instance_id: Set(set_req.app_instance_id),
                key: Set(set_req.key),
                value: Set(set_req.value),
                value_da: Set(set_req.value_da),
                created_at: Set(now),
                updated_at: Set(now),
            };

            app_instance_kv_binary::Entity::insert(entry)
                .on_conflict(
                    OnConflict::columns(vec![
                        app_instance_kv_binary::Column::AppInstanceId,
                        app_instance_kv_binary::Column::Key,
                    ])
                    .update_columns(vec![
                        app_instance_kv_binary::Column::Value,
                        app_instance_kv_binary::Column::UpdatedAt,
                    ])
                    .to_owned()
                )
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            sets_processed += 1;
        }

        for del_req in req.deletes {
            self.verify_ownership(&del_req.app_instance_id, &auth).await?;

            app_instance_kv_binary::Entity::delete_many()
                .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&del_req.app_instance_id))
                .filter(app_instance_kv_binary::Column::Key.eq(del_req.key.clone()))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            deletes_processed += 1;
        }

        Ok(Response::new(BatchKvBinaryResponse {
            success: true,
            message: format!("Processed {} sets and {} deletes", sets_processed, deletes_processed),
            sets_processed,
            deletes_processed,
        }))
    }

    async fn create_object(
        &self,
        request: Request<CreateObjectRequest>,
    ) -> Result<Response<CreateObjectResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object data (use empty if not provided)
        let object_data_bytes = req.object_data.unwrap_or_default();

        // Calculate object hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&object_data_bytes);
        let object_hash = hasher.finalize().to_vec();

        // Store object data inline (S3 offloading removed - use explicit blob storage API if needed)
        let object_data = Some(object_data_bytes.clone());
        let object_da = req.object_da;

        // Create object entity with initial version 1
        let object = objects::ActiveModel {
            object_id: Set(req.object_id.clone()),
            version: Set(1),
            owner: Set(auth.public_key.clone()),  // Owner is from JWT auth
            object_type: Set(req.object_type),
            shared: Set(req.shared),
            object_data: Set(object_data.clone()),
            object_da: Set(object_da.clone()),
            object_hash: Set(object_hash.clone()),
            previous_tx: Set(Option::<String>::None),  // No previous transaction for new object
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
        };

        // Insert to database
        let result = object
            .insert(self.db.connection())
            .await
            .map_err(|e| {
                if e.to_string().contains("Duplicate") {
                    Status::already_exists(format!("Object {} already exists", req.object_id))
                } else {
                    warn!("Database error creating object: {}", e);
                    Status::internal("Failed to create object")
                }
            })?;

        // Also create initial version record
        let version = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(result.object_id.clone()),
            version: Set(1),
            object_data: Set(object_data),
            object_da: Set(object_da),
            object_hash: Set(object_hash),
            owner: Set(result.owner.clone()),
            object_type: Set(result.object_type.clone()),
            shared: Set(result.shared),
            previous_tx: Set(Option::<String>::None),
            created_at: Set(Utc::now()),
        };

        version.insert(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Failed to create object version record: {}", e);
                Status::internal("Failed to create object version")
            })?;

        info!("Created object {} with version 1 for owner {}",
            result.object_id, result.owner);

        Ok(Response::new(CreateObjectResponse {
            success: true,
            message: "Object created successfully".to_string(),
            object: Some(Object {
                object_id: result.object_id,
                version: result.version as u64,
                owner: result.owner,
                object_type: result.object_type,
                shared: result.shared,
                object_data: Some(object_data_bytes), // Return original data
                object_da: result.object_da,
                object_hash: result.object_hash,
                previous_tx: result.previous_tx,
                created_at: Some(to_timestamp(result.created_at)),
                updated_at: Some(to_timestamp(result.updated_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn update_object(
        &self,
        request: Request<UpdateObjectRequest>,
    ) -> Result<Response<UpdateObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get primary app instance from scope for concurrency control
        let app_instance_id = auth.get_primary_app_instance()
            .ok_or_else(|| {
                warn!(
                    "No app_instance permission in JWT scope for update_object (requester: {})",
                    auth.public_key
                );
                Status::permission_denied("No app_instance permission in scope")
            })?;

        let object_data_bytes = req.object_data.unwrap_or_default();
        let object_id_clone = req.object_id.clone();

        // Use optimistic concurrency control
        let result = self.concurrency.optimistic_update(
            &app_instance_id,
            &req.object_id,
            req.expected_version,
            |_txn| {
                Box::pin(async move {
                    Ok(())
                })
            }
        ).await;

        if result.is_err() {
            return Err(Status::failed_precondition("Version mismatch - object was modified"));
        }

        // Get the updated object
        let object = objects::Entity::find_by_id(object_id_clone)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        // Verify ownership
        if object.owner != auth.public_key {
            warn!(
                "Object ownership check failed: object_id={}, owner={}, requester={}",
                object.object_id, object.owner, auth.public_key
            );
            return Err(Status::permission_denied("Not the object owner"));
        }

        // Calculate new hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&object_data_bytes);
        let object_hash = hasher.finalize().to_vec();

        // Store object data inline (S3 offloading removed - use explicit blob storage API if needed)
        let inline_data = Some(object_data_bytes.clone());
        let final_object_da = req.object_da;

        // Update object
        let mut active_model: objects::ActiveModel = object.clone().into();
        active_model.version = Set(object.version + 1);
        active_model.object_data = Set(inline_data.clone());
        active_model.object_da = Set(final_object_da.clone());
        active_model.object_hash = Set(object_hash.clone());
        active_model.updated_at = Set(Utc::now());

        let updated = active_model.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to update object", e))?;

        // Create version record
        let version_record = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(updated.object_id.clone()),
            version: Set(updated.version),
            object_data: Set(inline_data),
            object_da: Set(final_object_da),
            object_hash: Set(object_hash),
            owner: Set(updated.owner.clone()),
            object_type: Set(updated.object_type.clone()),
            shared: Set(updated.shared),
            previous_tx: Set(updated.previous_tx.clone()),
            created_at: Set(Utc::now()),
        };

        version_record.insert(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to create version", e))?;

        info!("Updated object {} to version {}", updated.object_id, updated.version);

        Ok(Response::new(UpdateObjectResponse {
            success: true,
            message: "Object updated successfully".to_string(),
            object: Some(Object {
                object_id: updated.object_id,
                version: updated.version as u64,
                owner: updated.owner,
                object_type: updated.object_type,
                shared: updated.shared,
                object_data: Some(object_data_bytes),
                object_da: updated.object_da,
                object_hash: updated.object_hash,
                previous_tx: updated.previous_tx,
                created_at: Some(to_timestamp(updated.created_at)),
                updated_at: Some(to_timestamp(updated.updated_at)),
                metadata: updated.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_object(
        &self,
        request: Request<GetObjectRequest>,
    ) -> Result<Response<GetObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object (version-specific or latest)
        let object_result = if let Some(version) = req.version {
            object_versions::Entity::find()
                .filter(object_versions::Column::ObjectId.eq(&req.object_id))
                .filter(object_versions::Column::Version.eq(version as i64))
                .one(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?
        } else {
            // Get latest from objects table then convert to version format
            let obj = objects::Entity::find_by_id(&req.object_id)
                .one(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            obj.map(|obj| {
                // Check access: owner or shared
                if obj.owner != auth.public_key && !obj.shared {
                    return None;
                }

                // Convert to version model structure for uniform handling
                Some(object_versions::Model {
                    id: 0,
                    object_id: obj.object_id,
                    version: obj.version,
                    object_data: obj.object_data,
                    object_da: obj.object_da,
                    object_hash: obj.object_hash,
                    owner: obj.owner,
                    object_type: obj.object_type,
                    shared: obj.shared,
                    previous_tx: obj.previous_tx,
                    created_at: obj.created_at,
                })
            }).flatten()
        };

        // Handle not found case
        let object = match object_result {
            Some(obj) => obj,
            None => {
                let msg = if let Some(v) = req.version {
                    format!("Object version {} not found", v)
                } else {
                    "Object not found".to_string()
                };
                return Ok(Response::new(GetObjectResponse {
                    success: false,
                    message: msg,
                    object: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let object_data = object.object_data.clone();

        Ok(Response::new(GetObjectResponse {
            success: true,
            message: "Object found".to_string(),
            object: Some(Object {
                object_id: object.object_id,
                version: object.version as u64,
                owner: object.owner,
                object_type: object.object_type,
                shared: object.shared,
                object_data,
                object_da: object.object_da,
                object_hash: object.object_hash,
                previous_tx: object.previous_tx,
                created_at: Some(to_timestamp(object.created_at)),
                updated_at: None, // Versions don't track updated_at
                metadata: None, // Object versions don't have metadata, only current objects do
            }),
        }))
    }

    async fn get_object_versions(
        &self,
        request: Request<GetObjectVersionsRequest>,
    ) -> Result<Response<GetObjectVersionsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if user has access to this object
        let obj = objects::Entity::find_by_id(&req.object_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        if obj.owner != auth.public_key && !obj.shared {
            warn!(
                "Access denied to object versions: object_id={}, owner={}, requester={}, shared={}",
                obj.object_id, obj.owner, auth.public_key, obj.shared
            );
            return Err(Status::permission_denied("Access denied"));
        }

        let limit = req.limit.unwrap_or(100).min(1000) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        let paginator = object_versions::Entity::find()
            .filter(object_versions::Column::ObjectId.eq(&req.object_id))
            .order_by_desc(object_versions::Column::Version)
            .paginate(self.db.connection(), limit);

        let total = paginator.num_items().await
            .map_err(|e| log_internal_error("Database error", e))? as i32;

        let versions_data = paginator.fetch_page(offset / limit).await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut versions = Vec::new();
        for ver in versions_data {
            versions.push(ObjectVersion {
                object_id: ver.object_id,
                version: ver.version as u64,
                object_data: ver.object_data,
                object_da: ver.object_da,
                object_hash: ver.object_hash,
                owner: ver.owner,
                object_type: ver.object_type,
                shared: ver.shared,
                previous_tx: ver.previous_tx,
                created_at: Some(to_timestamp(ver.created_at)),
            });
        }

        Ok(Response::new(GetObjectVersionsResponse {
            versions,
            total,
        }))
    }

    async fn transfer_object(
        &self,
        request: Request<TransferObjectRequest>,
    ) -> Result<Response<TransferObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object
        let object = objects::Entity::find_by_id(&req.object_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        // Verify current owner
        if object.owner != auth.public_key {
            warn!(
                "Transfer object failed: object_id={}, owner={}, requester={}",
                object.object_id, object.owner, auth.public_key
            );
            return Err(Status::permission_denied("Only the owner can transfer this object"));
        }

        // Update owner
        let mut active_model: objects::ActiveModel = object.clone().into();
        active_model.owner = Set(req.new_owner.clone());
        active_model.version = Set(object.version + 1);
        active_model.updated_at = Set(Utc::now());

        let updated = active_model.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to transfer object", e))?;

        // Create version record for transfer
        let version_record = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(updated.object_id.clone()),
            version: Set(updated.version),
            object_data: Set(object.object_data.clone()),
            object_da: Set(object.object_da.clone()),
            object_hash: Set(object.object_hash.clone()),
            owner: Set(updated.owner.clone()),
            object_type: Set(updated.object_type.clone()),
            shared: Set(updated.shared),
            previous_tx: Set(object.previous_tx.clone()),
            created_at: Set(Utc::now()),
        };

        version_record.insert(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to create version", e))?;

        info!("Transferred object {} to {}", updated.object_id, updated.owner);

        Ok(Response::new(TransferObjectResponse {
            success: true,
            message: format!("Object transferred to {}", req.new_owner),
            object: Some(Object {
                object_id: updated.object_id,
                version: updated.version as u64,
                owner: updated.owner,
                object_type: updated.object_type,
                shared: updated.shared,
                object_data: object.object_data,
                object_da: updated.object_da,
                object_hash: updated.object_hash,
                previous_tx: updated.previous_tx,
                created_at: Some(to_timestamp(updated.created_at)),
                updated_at: Some(to_timestamp(updated.updated_at)),
                metadata: updated.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn request_locks(
        &self,
        request: Request<RequestLocksRequest>,
    ) -> Result<Response<RequestLocksResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get primary app instance from scope
        let app_instance_id = auth.get_primary_app_instance()
            .ok_or_else(|| {
                warn!(
                    "No app_instance permission in JWT scope for request_locks (requester: {})",
                    auth.public_key
                );
                Status::permission_denied("No app_instance permission in scope")
            })?;

        // Sort object IDs for canonical ordering (deadlock prevention)
        let mut object_ids = req.object_ids.clone();
        object_ids.sort();

        let now = Utc::now();

        // Create bundle record
        let bundle = lock_request_bundle::ActiveModel {
            req_id: Set(req.req_id.clone()),
            app_instance_id: Set(app_instance_id.clone()),
            object_ids: Set(serde_json::to_value(&object_ids).unwrap()),
            object_count: Set(object_ids.len() as i32),
            transaction_type: Set(req.transaction_type.clone()),
            created_at: Set(now),
            started_at: Set(Some(now)),
            granted_at: NotSet,
            released_at: NotSet,
            status: Set("ACQUIRING".to_string()),
            wait_time_ms: NotSet,
            hold_time_ms: NotSet,
        };
        bundle.insert(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to create lock bundle", e)
        })?;

        // Queue lock requests for each object in sorted order
        for object_id in &object_ids {
            let lock_entry = object_lock_queue::ActiveModel {
                object_id: Set(object_id.clone()),
                req_id: Set(req.req_id.clone()),
                app_instance_id: Set(app_instance_id.clone()),
                retry_count: Set(0),
                queued_at: Set(now),
                lease_until: NotSet,  // Will be set when granted
                lease_granted_at: NotSet,
                status: Set("WAITING".to_string()),
            };
            lock_entry.insert(self.db.connection()).await.map_err(|e| {
                Status::internal(format!("Failed to queue lock for {}: {}", object_id, e))
            })?;
        }

        // Attempt to acquire the bundle (with timeout)
        let acquired = self.concurrency.attempt_acquire_bundle(
            &req.req_id,
            req.timeout_seconds,
        ).await.map_err(|e| {
            log_internal_error("Failed to acquire locks", e)
        })?;

        if !acquired {
            // Timeout - clean up queue entries
            object_lock_queue::Entity::delete_many()
                .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to clean up queue", e))?;

            return Ok(Response::new(RequestLocksResponse {
                success: false,
                message: format!("Lock acquisition timed out after {} seconds", req.timeout_seconds),
                bundle: None,
            }));
        }

        // Reload bundle to get updated status
        let updated_bundle = lock_request_bundle::Entity::find_by_id(&req.req_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| {
                error!("Bundle disappeared - req_id: {}", req.req_id);
                Status::internal("Bundle disappeared")
            })?;

        // Convert bundle to proto response
        let proto_bundle = LockRequestBundle {
            req_id: updated_bundle.req_id.clone(),
            app_instance_id: updated_bundle.app_instance_id.clone(),
            object_ids: object_ids.clone(),
            object_count: updated_bundle.object_count,
            transaction_type: updated_bundle.transaction_type.clone(),
            created_at: Some(to_timestamp(updated_bundle.created_at)),
            started_at: updated_bundle.started_at.map(to_timestamp),
            granted_at: updated_bundle.granted_at.map(to_timestamp),
            released_at: None,
            status: BundleStatus::Granted.into(),
            wait_time_ms: updated_bundle.wait_time_ms,
            hold_time_ms: None,
        };

        Ok(Response::new(RequestLocksResponse {
            success: true,
            message: format!("Successfully acquired locks for {} objects", object_ids.len()),
            bundle: Some(proto_bundle),
        }))
    }

    async fn release_locks(
        &self,
        request: Request<ReleaseLocksRequest>,
    ) -> Result<Response<ReleaseLocksResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Find the bundle
        let bundle = lock_request_bundle::Entity::find_by_id(req.req_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Lock bundle not found"))?;

        // Update all locks in the bundle to RELEASED
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        for lock in lock_entries {
            let mut active: object_lock_queue::ActiveModel = lock.into();
            active.status = Set("RELEASED".to_string());
            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to release lock", e)
            })?;
        }

        // Update bundle status
        let now = Utc::now();
        let mut active_bundle: lock_request_bundle::ActiveModel = bundle.into();
        active_bundle.status = Set("RELEASED".to_string());
        active_bundle.released_at = Set(Some(now));
        active_bundle.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update bundle", e)
        })?;

        Ok(Response::new(ReleaseLocksResponse {
            success: true,
            message: format!("Released locks for bundle {}", req.req_id),
        }))
    }

    async fn get_lock_status(
        &self,
        request: Request<GetLockStatusRequest>,
    ) -> Result<Response<GetLockStatusResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Find the bundle
        let bundle = lock_request_bundle::Entity::find_by_id(req.req_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Lock bundle not found"))?;

        // Get all queue entries for this bundle
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Convert to proto
        let queue_entries: Vec<LockQueueEntry> = lock_entries
            .into_iter()
            .map(|entry| {
                let status = match entry.status.as_str() {
                    "WAITING" => LockStatus::Waiting,
                    "GRANTED" => LockStatus::Granted,
                    "EXPIRED" => LockStatus::Expired,
                    "RELEASED" => LockStatus::Released,
                    _ => LockStatus::Unspecified,
                };

                LockQueueEntry {
                    object_id: entry.object_id,
                    req_id: entry.req_id,
                    app_instance_id: entry.app_instance_id,
                    retry_count: entry.retry_count,
                    queued_at: Some(to_timestamp(entry.queued_at)),
                    lease_until: entry.lease_until.map(to_timestamp),
                    lease_granted_at: entry.lease_granted_at.map(to_timestamp),
                    status: status.into(),
                }
            })
            .collect();

        // Parse object_ids from JSON
        let object_ids: Vec<String> = serde_json::from_value(bundle.object_ids.clone())
            .unwrap_or_default();

        let bundle_status = match bundle.status.as_str() {
            "QUEUED" => BundleStatus::Queued,
            "ACQUIRING" => BundleStatus::Acquiring,
            "GRANTED" => BundleStatus::Granted,
            "RELEASED" => BundleStatus::Released,
            "TIMEOUT" => BundleStatus::Timeout,
            "FAILED" => BundleStatus::Failed,
            _ => BundleStatus::Unspecified,
        };

        let proto_bundle = LockRequestBundle {
            req_id: bundle.req_id,
            app_instance_id: bundle.app_instance_id,
            object_ids,
            object_count: bundle.object_count,
            transaction_type: bundle.transaction_type,
            created_at: Some(to_timestamp(bundle.created_at)),
            started_at: bundle.started_at.map(to_timestamp),
            granted_at: bundle.granted_at.map(to_timestamp),
            released_at: bundle.released_at.map(to_timestamp),
            status: bundle_status.into(),
            wait_time_ms: bundle.wait_time_ms,
            hold_time_ms: bundle.hold_time_ms,
        };

        Ok(Response::new(GetLockStatusResponse {
            bundle: Some(proto_bundle),
            queue_entries,
        }))
    }

    async fn get_lock_queue(
        &self,
        request: Request<GetLockQueueRequest>,
    ) -> Result<Response<GetLockQueueResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Get all queue entries for this object, ordered by queued_at (FIFO)
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ObjectId.eq(&req.object_id))
            .order_by_asc(object_lock_queue::Column::QueuedAt)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Convert to proto
        let queue_entries: Vec<LockQueueEntry> = lock_entries
            .into_iter()
            .map(|entry| {
                let status = match entry.status.as_str() {
                    "WAITING" => LockStatus::Waiting,
                    "GRANTED" => LockStatus::Granted,
                    "EXPIRED" => LockStatus::Expired,
                    "RELEASED" => LockStatus::Released,
                    _ => LockStatus::Unspecified,
                };

                LockQueueEntry {
                    object_id: entry.object_id,
                    req_id: entry.req_id,
                    app_instance_id: entry.app_instance_id,
                    retry_count: entry.retry_count,
                    queued_at: Some(to_timestamp(entry.queued_at)),
                    lease_until: entry.lease_until.map(to_timestamp),
                    lease_granted_at: entry.lease_granted_at.map(to_timestamp),
                    status: status.into(),
                }
            })
            .collect();

        Ok(Response::new(GetLockQueueResponse {
            queue_entries,
        }))
    }

    async fn create_job(
        &self,
        request: Request<CreateJobRequest>,
    ) -> Result<Response<CreateJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let now = Utc::now();

        // Calculate next_scheduled_at for periodic jobs
        let next_scheduled_at = if let Some(interval_ms) = req.interval_ms {
            Some(now + chrono::Duration::milliseconds(interval_ms as i64))
        } else {
            None
        };

        // Convert sequences to JSON
        let sequences_json = if !req.sequences.is_empty() {
            Some(serde_json::to_value(&req.sequences).unwrap())
        } else {
            None
        };
        let sequences1_json = if !req.sequences1.is_empty() {
            Some(serde_json::to_value(&req.sequences1).unwrap())
        } else {
            None
        };
        let sequences2_json = if !req.sequences2.is_empty() {
            Some(serde_json::to_value(&req.sequences2).unwrap())
        } else {
            None
        };

        // Start transaction with pessimistic locking for gapless sequence generation
        use sea_orm::{TransactionTrait, DatabaseBackend, Statement};
        let txn = self.db.connection().begin().await
            .map_err(|e| log_internal_error("Failed to start transaction", e))?;

        // Get or create sequence counter with FOR UPDATE lock
        let _insert_result = txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            INSERT INTO job_seq (app_instance_id, next_seq)
            VALUES (?, 1)
            ON DUPLICATE KEY UPDATE next_seq = next_seq
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to initialize job sequence", e))?;

        // Get current sequence with FOR UPDATE lock
        let seq_result = txn.query_one(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            SELECT next_seq FROM job_seq
            WHERE app_instance_id = ?
            FOR UPDATE
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to get job sequence", e))?
            .ok_or_else(|| {
                error!("Failed to get job sequence counter - query returned no rows");
                Status::internal("Failed to get job sequence counter")
            })?;

        let current_seq: u64 = seq_result.try_get("", "next_seq")
            .map_err(|e| log_internal_error("Failed to parse job sequence", e))?;

        // Create job with assigned sequence
        let job = jobs::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            job_sequence: Set(current_seq),
            description: Set(req.description.clone()),
            developer: Set(req.developer.clone()),
            agent: Set(req.agent.clone()),
            agent_method: Set(req.agent_method.clone()),
            block_number: Set(req.block_number),
            sequences: Set(sequences_json.clone()),
            sequences1: Set(sequences1_json.clone()),
            sequences2: Set(sequences2_json.clone()),
            data: Set(req.data.clone()),
            data_da: Set(req.data_da.clone()),
            agent_jwt: Set(req.agent_jwt.clone()),
            jwt_expires_at: Set(req.jwt_expires_at.as_ref().map(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                    .expect("Invalid timestamp")
                    .with_timezone(&chrono::Utc)
            })),
            status: Set("PENDING".to_string()),
            error_message: NotSet,
            attempts: Set(0),
            interval_ms: Set(req.interval_ms),
            next_scheduled_at: Set(next_scheduled_at),
            created_at: Set(now),
            updated_at: Set(now),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
        };

        let result = job.insert(&txn).await
            .map_err(|e| log_internal_error("Failed to create job", e))?;

        // Increment counter for next use
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            UPDATE job_seq
            SET next_seq = next_seq + 1
            WHERE app_instance_id = ?
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to update job sequence", e))?;

        // Commit transaction
        txn.commit().await
            .map_err(|e| log_internal_error("Failed to commit transaction", e))?;

        info!("Created job {}/{} for {}/{}",
            req.app_instance_id, current_seq, req.developer, req.agent);

        // Convert to proto
        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences: req.sequences,
            sequences1: req.sequences1,
            sequences2: req.sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Pending.into(),
            error_message: None,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(CreateJobResponse {
            success: true,
            message: "Job created successfully".to_string(),
            job_sequence: current_seq as u64,
            job: Some(proto_job),
        }))
    }

    async fn start_job(
        &self,
        request: Request<StartJobRequest>,
    ) -> Result<Response<StartJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        // Update status to RUNNING
        let mut active: jobs::ActiveModel = job.into();
        active.status = Set("RUNNING".to_string());
        active.attempts = Set(active.attempts.as_ref() + 1);
        active.updated_at = Set(Utc::now());

        let result = active.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update job", e)
        })?;

        // Convert to proto
        let sequences: Vec<u64> = result.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = result.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = result.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences,
            sequences1,
            sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Running.into(),
            error_message: result.error_message,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(StartJobResponse {
            success: true,
            message: format!("Job {}/{} started", req.app_instance_id, req.job_sequence),
            job: Some(proto_job),
        }))
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        let now = Utc::now();

        // For periodic jobs, reschedule. For one-time jobs, mark as COMPLETED
        if let Some(interval_ms) = job.interval_ms {
            // Periodic job - reset to PENDING and update next_scheduled_at
            let next_scheduled_at = now + chrono::Duration::milliseconds(interval_ms as i64);

            let mut active: jobs::ActiveModel = job.into();
            active.status = Set("PENDING".to_string());
            active.next_scheduled_at = Set(Some(next_scheduled_at));
            active.updated_at = Set(now);

            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to reschedule job", e)
            })?;

            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!(
                    "Job {}/{} completed and rescheduled for {:?}",
                    req.app_instance_id, req.job_sequence, next_scheduled_at
                ),
            }))
        } else {
            // One-time job - mark as COMPLETED for audit trail
            let mut active: jobs::ActiveModel = job.into();
            active.status = Set("COMPLETED".to_string());
            active.updated_at = Set(now);

            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to mark job as completed", e)
            })?;

            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {}/{} marked as completed", req.app_instance_id, req.job_sequence),
            }))
        }
    }

    async fn fail_job(
        &self,
        request: Request<FailJobRequest>,
    ) -> Result<Response<FailJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        // Update status to FAILED
        let mut active: jobs::ActiveModel = job.into();
        active.status = Set("FAILED".to_string());
        active.error_message = Set(Some(req.error_message.clone()));
        active.updated_at = Set(Utc::now());

        let result = active.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update job", e)
        })?;

        // Convert to proto
        let sequences: Vec<u64> = result.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = result.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = result.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences,
            sequences1,
            sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Failed.into(),
            error_message: result.error_message,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(FailJobResponse {
            success: true,
            message: format!("Job {}/{} marked as failed", req.app_instance_id, req.job_sequence),
            job: Some(proto_job),
        }))
    }

    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Handle not found case
        let job = match job {
            Some(j) => j,
            None => {
                return Ok(Response::new(GetJobResponse {
                    success: false,
                    message: format!("Job {}/{} not found", req.app_instance_id, req.job_sequence),
                    job: None,
                }));
            }
        };

        // Convert to proto
        let sequences: Vec<u64> = job.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = job.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = job.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let job_status = match job.status.as_str() {
            "PENDING" => JobStatus::Pending,
            "RUNNING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            "FAILED" => JobStatus::Failed,
            _ => JobStatus::Unspecified,
        };

        let proto_job = Job {
            app_instance_id: job.app_instance_id,
            job_sequence: job.job_sequence as u64,
            description: job.description,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            block_number: job.block_number,
            sequences,
            sequences1,
            sequences2,
            data: job.data,
            data_da: job.data_da,
            agent_jwt: job.agent_jwt,
            jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
            status: job_status.into(),
            error_message: job.error_message,
            attempts: job.attempts as u32,
            interval_ms: job.interval_ms,
            next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(job.created_at)),
            updated_at: Some(to_timestamp(job.updated_at)),
            metadata: job.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(GetJobResponse {
            success: true,
            message: "Job found".to_string(),
            job: Some(proto_job),
        }))
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let limit = req.limit.unwrap_or(100) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        // Build query with optional status filter
        let mut query = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id));

        if let Some(status) = req.status {
            let status_str = match JobStatus::try_from(status) {
                Ok(JobStatus::Pending) => "PENDING",
                Ok(JobStatus::Running) => "RUNNING",
                Ok(JobStatus::Completed) => "COMPLETED",
                Ok(JobStatus::Failed) => "FAILED",
                _ => {
                    warn!("Invalid job status value provided: {}", status);
                    return Err(Status::invalid_argument("Invalid job status"));
                }
            };
            query = query.filter(jobs::Column::Status.eq(status_str));
        }

        // Get total count
        let total = query.clone().count(self.db.connection()).await.map_err(|e| {
            log_internal_error("Database error", e)
        })? as i32;

        // Get paginated results
        let paginator = query
            .order_by_asc(jobs::Column::JobSequence)
            .paginate(self.db.connection(), limit);

        let page = offset / limit;
        let job_list = paginator.fetch_page(page).await.map_err(|e| {
            log_internal_error("Database error", e)
        })?;

        // Convert to proto
        let jobs: Vec<Job> = job_list
            .into_iter()
            .map(|job| {
                let sequences: Vec<u64> = job.sequences.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences1: Vec<u64> = job.sequences1.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences2: Vec<u64> = job.sequences2.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let job_status = match job.status.as_str() {
                    "PENDING" => JobStatus::Pending,
                    "RUNNING" => JobStatus::Running,
                    "COMPLETED" => JobStatus::Completed,
                    "FAILED" => JobStatus::Failed,
                    _ => JobStatus::Unspecified,
                };

                Job {
                    app_instance_id: job.app_instance_id,
                    job_sequence: job.job_sequence as u64,
                    description: job.description,
                    developer: job.developer,
                    agent: job.agent,
                    agent_method: job.agent_method,
                    block_number: job.block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data: job.data,
                    data_da: job.data_da,
                    agent_jwt: job.agent_jwt,
                    jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
                    status: job_status.into(),
                    error_message: job.error_message,
                    attempts: job.attempts as u32,
                    interval_ms: job.interval_ms,
                    next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
                    created_at: Some(to_timestamp(job.created_at)),
                    updated_at: Some(to_timestamp(job.updated_at)),
                    metadata: job.metadata.and_then(json_to_prost_struct),
                }
            })
            .collect();

        Ok(Response::new(ListJobsResponse {
            jobs,
            total,
        }))
    }

    async fn get_pending_jobs(
        &self,
        request: Request<GetPendingJobsRequest>,
    ) -> Result<Response<GetPendingJobsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let now = Utc::now();
        let limit = req.limit as u64;

        // Build query for pending jobs
        let mut query = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::Status.eq("PENDING"));

        // Filter by next_scheduled_at (ready to run)
        query = query.filter(
            jobs::Column::NextScheduledAt.is_null()
                .or(jobs::Column::NextScheduledAt.lte(now))
        );

        // Optional filters
        if let Some(developer) = &req.developer {
            query = query.filter(jobs::Column::Developer.eq(developer));
        }
        if let Some(agent) = &req.agent {
            query = query.filter(jobs::Column::Agent.eq(agent));
        }
        if let Some(agent_method) = &req.agent_method {
            query = query.filter(jobs::Column::AgentMethod.eq(agent_method));
        }

        // Get jobs ordered by job_sequence
        let paginator = query
            .order_by_asc(jobs::Column::JobSequence)
            .paginate(self.db.connection(), limit);

        let job_list = paginator.fetch_page(0).await.map_err(|e| {
            log_internal_error("Database error", e)
        })?;

        // Convert to proto
        let jobs: Vec<Job> = job_list
            .into_iter()
            .map(|job| {
                let sequences: Vec<u64> = job.sequences.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences1: Vec<u64> = job.sequences1.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences2: Vec<u64> = job.sequences2.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                Job {
                    app_instance_id: job.app_instance_id,
                    job_sequence: job.job_sequence as u64,
                    description: job.description,
                    developer: job.developer,
                    agent: job.agent,
                    agent_method: job.agent_method,
                    block_number: job.block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data: job.data,
                    data_da: job.data_da,
                    agent_jwt: job.agent_jwt,
                    jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
                    status: JobStatus::Pending.into(),
                    error_message: job.error_message,
                    attempts: job.attempts as u32,
                    interval_ms: job.interval_ms,
                    next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
                    created_at: Some(to_timestamp(job.created_at)),
                    updated_at: Some(to_timestamp(job.updated_at)),
                    metadata: job.metadata.and_then(json_to_prost_struct),
                }
            })
            .collect();

        Ok(Response::new(GetPendingJobsResponse {
            jobs,
        }))
    }

    async fn store_private_blob(
        &self,
        request: Request<StorePrivateBlobRequest>,
    ) -> Result<Response<StorePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Verify scope permission for the resource
        match req.resource_type.as_str() {
            "app_instance" => {
                self.require_app_instance_permission(&auth, &req.resource_id)?;
            }
            "object" => {
                if !auth.has_object_permission(&req.resource_id) {
                    return Err(Status::permission_denied(
                        format!("Missing object permission for {}", req.resource_id)
                    ));
                }
            }
            _ => {
                warn!(
                    "Invalid resource_type for blob storage: provided='{}', expected='app_instance' or 'object'",
                    req.resource_type
                );
                return Err(Status::invalid_argument(
                    "resource_type must be 'app_instance' or 'object'"
                ));
            }
        }

        // Store blob with owner metadata - key format: resource_type/resource_id/hash
        let key = storage
            .store(
                &req.data,
                &auth.public_key,
                &req.resource_type,
                &req.resource_id,
            )
            .await
            .map_err(|e| log_internal_error("Failed to store blob", e))?;

        // Calculate hash for response
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&req.data);
        let content_hash = hex::encode(hash);

        info!(
            "Stored private blob: {} ({} bytes, owner: {})",
            key,
            req.data.len(),
            auth.public_key
        );

        Ok(Response::new(StorePrivateBlobResponse {
            success: true,
            message: "Blob stored successfully".to_string(),
            key,
            size_bytes: req.data.len() as u64,
            content_hash,
        }))
    }

    async fn retrieve_private_blob(
        &self,
        request: Request<RetrievePrivateBlobRequest>,
    ) -> Result<Response<RetrievePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Retrieve blob with authentication
        let data = storage
            .retrieve(
                &req.key,
                &auth.public_key,
                &auth.scope_permissions,
            )
            .await
            .map_err(|e| {
                if e.to_string().contains("Access denied") {
                    Status::permission_denied(e.to_string())
                } else {
                    log_internal_error("Failed to retrieve blob", e)
                }
            })?;

        info!(
            "Retrieved private blob: {} ({} bytes, requester: {})",
            req.key,
            data.len(),
            auth.public_key
        );

        Ok(Response::new(RetrievePrivateBlobResponse {
            success: true,
            message: "Blob retrieved successfully".to_string(),
            data,
        }))
    }

    async fn delete_private_blob(
        &self,
        request: Request<DeletePrivateBlobRequest>,
    ) -> Result<Response<DeletePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Delete blob (owner only)
        storage
            .delete(&req.key, &auth.public_key)
            .await
            .map_err(|e| {
                if e.to_string().contains("Access denied") {
                    Status::permission_denied(e.to_string())
                } else {
                    log_internal_error("Failed to delete blob", e)
                }
            })?;

        info!(
            "Deleted private blob: {} (requester: {})",
            req.key,
            auth.public_key
        );

        Ok(Response::new(DeletePrivateBlobResponse {
            success: true,
            message: "Blob deleted successfully".to_string(),
        }))
    }

    // =========================================================================
    // Proof Management
    // =========================================================================

    async fn submit_proof(
        &self,
        request: Request<SubmitProofRequest>,
    ) -> Result<Response<SubmitProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let (claim_data, claim_da) = if let Some(cd) = req.claim_data {
            (Some(cd), req.claim_da)
        } else {
            (None, req.claim_da)
        };

        let (proof_data, proof_da) = if let Some(pd) = req.proof_data {
            (Some(pd), req.proof_da)
        } else {
            (None, req.proof_da)
        };

        let proof = proofs::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            proof_type: Set(req.proof_type.clone()),
            claim_json: Set(prost_struct_to_json(req.claim_json.unwrap_or_default())),
            claim_hash: Set(req.claim_hash),
            claim_data: Set(claim_data.clone()),
            claim_da: Set(claim_da.clone()),
            proof_data: Set(proof_data.clone()),
            proof_da: Set(proof_da.clone()),
            proof_hash: Set(req.proof_hash),
            proof_time: Set(req.proof_time.map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap())),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            proved_at: Set(Utc::now()),
        };

        let result = proof.insert(self.db.connection()).await.map_err(|e| {
            warn!("Database error inserting proof: {}", e);
            log_internal_error("Failed to insert proof", e)
        })?;

        info!(
            "Submitted proof: id={}, app_instance={}, type={}",
            result.id, result.app_instance_id, result.proof_type
        );

        Ok(Response::new(SubmitProofResponse {
            success: true,
            message: "Proof submitted successfully".to_string(),
            proof: Some(Proof {
                id: result.id,
                app_instance_id: result.app_instance_id,
                proof_type: result.proof_type,
                claim_json: Some(json_to_prost_struct(result.claim_json).unwrap_or_default()),
                claim_hash: result.claim_hash,
                claim_data: result.claim_data,
                claim_da: result.claim_da,
                proof_data: result.proof_data,
                proof_da: result.proof_da,
                proof_hash: result.proof_hash,
                proof_time: result.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(result.proved_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Find the proof
        let proof = proofs::Entity::find_by_id(req.proof_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to query proof", e))?
            .ok_or_else(|| Status::not_found(format!("Proof {} not found", req.proof_id)))?;

        // Verify ownership via app_instance
        self.verify_ownership(&proof.app_instance_id, &auth).await?;

        Ok(Response::new(GetProofResponse {
            success: true,
            message: "Proof retrieved successfully".to_string(),
            proof: Some(Proof {
                id: proof.id,
                app_instance_id: proof.app_instance_id,
                proof_type: proof.proof_type,
                claim_json: Some(json_to_prost_struct(proof.claim_json).unwrap_or_default()),
                claim_hash: proof.claim_hash,
                claim_data: proof.claim_data,
                claim_da: proof.claim_da,
                proof_data: proof.proof_data,
                proof_da: proof.proof_da,
                proof_hash: proof.proof_hash,
                proof_time: proof.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(proof.proved_at)),
                metadata: proof.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn list_proofs(
        &self,
        request: Request<ListProofsRequest>,
    ) -> Result<Response<ListProofsResponse>, Status> {
        use sea_orm::Condition;

        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership of app_instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000) as u64;

        // Build keyset pagination condition
        let mut condition = Condition::all()
            .add(proofs::Column::AppInstanceId.eq(&req.app_instance_id));

        // Add proof_type filter if specified
        if let Some(proof_type) = &req.proof_type {
            condition = condition.add(proofs::Column::ProofType.eq(proof_type));
        }

        // Add keyset pagination (proved_at, app_instance_id, id) > (last_proved_at, last_app_instance_id, last_id)
        if let (Some(last_proved_at), Some(last_app_instance_id), Some(last_id)) =
            (req.last_proved_at, req.last_app_instance_id, req.last_id) {

            let last_ts = chrono::DateTime::from_timestamp(last_proved_at.seconds, last_proved_at.nanos as u32)
                .ok_or_else(|| Status::invalid_argument("Invalid last_proved_at timestamp"))?;

            // Keyset pagination: (proved_at, app_instance_id, id) > (last_proved_at, last_app_instance_id, last_id)
            condition = condition.add(
                Condition::any()
                    .add(proofs::Column::ProvedAt.gt(last_ts))
                    .add(
                        Condition::all()
                            .add(proofs::Column::ProvedAt.eq(last_ts))
                            .add(proofs::Column::AppInstanceId.gt(last_app_instance_id.clone()))
                    )
                    .add(
                        Condition::all()
                            .add(proofs::Column::ProvedAt.eq(last_ts))
                            .add(proofs::Column::AppInstanceId.eq(last_app_instance_id))
                            .add(proofs::Column::Id.gt(last_id))
                    )
            );
        }

        // Query with limit + 1 to check if there are more pages
        let paginator = proofs::Entity::find()
            .filter(condition)
            .order_by_asc(proofs::Column::ProvedAt)
            .order_by_asc(proofs::Column::AppInstanceId)
            .order_by_asc(proofs::Column::Id)
            .paginate(self.db.connection(), limit + 1);

        let proofs = paginator
            .fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Failed to list proofs", e))?;

        // Check if there are more pages
        let has_more = proofs.len() > limit as usize;
        let proofs = if has_more {
            &proofs[..limit as usize]
        } else {
            &proofs[..]
        };

        // Convert to proto messages
        let proto_proofs = proofs
            .iter()
            .map(|proof| Proof {
                id: proof.id,
                app_instance_id: proof.app_instance_id.clone(),
                proof_type: proof.proof_type.clone(),
                claim_json: Some(json_to_prost_struct(proof.claim_json.clone()).unwrap_or_default()),
                claim_hash: proof.claim_hash.clone(),
                claim_data: proof.claim_data.clone(),
                claim_da: proof.claim_da.clone(),
                proof_data: proof.proof_data.clone(),
                proof_da: proof.proof_da.clone(),
                proof_hash: proof.proof_hash.clone(),
                proof_time: proof.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(proof.proved_at)),
                metadata: proof.metadata.as_ref().and_then(|m| json_to_prost_struct(m.clone())),
            })
            .collect();

        Ok(Response::new(ListProofsResponse {
            proofs: proto_proofs,
            has_more,
        }))
    }

    // ============================================================================
    // Block Management Methods
    // ============================================================================

    async fn get_block(
        &self,
        request: Request<GetBlockRequest>,
    ) -> Result<Response<GetBlockResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query block from database
        let block = blocks::Entity::find()
            .filter(blocks::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(blocks::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block", e))?;

        // Convert to proto message if found
        let proto_block = block.map(|b| Block {
            app_instance_id: b.app_instance_id,
            block_number: b.block_number,
            start_sequence: b.start_sequence,
            end_sequence: b.end_sequence,
            actions_commitment: b.actions_commitment,
            state_commitment: b.state_commitment,
            time_since_last_block: b.time_since_last_block,
            number_of_transactions: b.number_of_transactions,
            start_actions_commitment: b.start_actions_commitment,
            end_actions_commitment: b.end_actions_commitment,
            state_data_availability: b.state_data_availability,
            proof_data_availability: b.proof_data_availability,
            created_at: Some(to_timestamp(b.created_at)),
            state_calculated_at: b.state_calculated_at.map(to_timestamp),
            proved_at: b.proved_at.map(to_timestamp),
        });

        Ok(Response::new(GetBlockResponse {
            block: proto_block,
        }))
    }

    async fn get_blocks_range(
        &self,
        request: Request<GetBlocksRangeRequest>,
    ) -> Result<Response<GetBlocksRangeResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Validate range
        if req.from_block > req.to_block {
            return Err(Status::invalid_argument("from_block must be <= to_block"));
        }

        // Query blocks from database
        let blocks = blocks::Entity::find()
            .filter(blocks::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(blocks::Column::BlockNumber.gte(req.from_block))
            .filter(blocks::Column::BlockNumber.lte(req.to_block))
            .order_by_asc(blocks::Column::BlockNumber)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch blocks range", e))?;

        // Convert to proto messages
        let proto_blocks = blocks
            .into_iter()
            .map(|b| Block {
                app_instance_id: b.app_instance_id,
                block_number: b.block_number,
                start_sequence: b.start_sequence,
                end_sequence: b.end_sequence,
                actions_commitment: b.actions_commitment,
                state_commitment: b.state_commitment,
                time_since_last_block: b.time_since_last_block,
                number_of_transactions: b.number_of_transactions,
                start_actions_commitment: b.start_actions_commitment,
                end_actions_commitment: b.end_actions_commitment,
                state_data_availability: b.state_data_availability,
                proof_data_availability: b.proof_data_availability,
                created_at: Some(to_timestamp(b.created_at)),
                state_calculated_at: b.state_calculated_at.map(to_timestamp),
                proved_at: b.proved_at.map(to_timestamp),
            })
            .collect();

        Ok(Response::new(GetBlocksRangeResponse {
            blocks: proto_blocks,
        }))
    }

    async fn try_create_block(
        &self,
        request: Request<TryCreateBlockRequest>,
    ) -> Result<Response<TryCreateBlockResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Verify app instance exists
        let _app = app_instances::Entity::find()
            .filter(app_instances::Column::AppInstanceId.eq(&req.app_instance_id))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch app instance", e))?
            .ok_or_else(|| Status::not_found("App instance not found"))?;

        // Get the last block to find the end sequence
        let last_block = blocks::Entity::find()
            .filter(blocks::Column::AppInstanceId.eq(&req.app_instance_id))
            .order_by_desc(blocks::Column::BlockNumber)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch last block", e))?;

        let (last_block_number, last_end_sequence, last_block_created_at) = match &last_block {
            Some(block) => (block.block_number, block.end_sequence, Some(block.created_at)),
            None => (0, 0, None), // No blocks yet
        };

        // Check if there are new sequences to include in a block
        // We need to query the state table to find the highest sequence
        let latest_state = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .order_by_desc(state::Column::Sequence)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch latest state", e))?;

        let current_sequence = latest_state.map(|s| s.sequence).unwrap_or(0);

        // If no new sequences, return None
        if current_sequence <= last_end_sequence {
            return Ok(Response::new(TryCreateBlockResponse {
                block_number: None,
                block: None,
            }));
        }

        // Create new block
        let new_block_number = last_block_number + 1;
        let start_sequence = last_end_sequence + 1;
        let end_sequence = current_sequence;

        // Calculate time since last block
        let time_since_last_block = last_block_created_at
            .map(|created_at| {
                let now = Utc::now();
                let diff = now.signed_duration_since(created_at);
                diff.num_milliseconds().max(0) as u64
            });

        // Count transactions in this block
        let number_of_transactions = end_sequence - start_sequence + 1;

        let new_block = blocks::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            block_number: Set(new_block_number),
            start_sequence: Set(start_sequence),
            end_sequence: Set(end_sequence),
            actions_commitment: NotSet,
            state_commitment: NotSet,
            time_since_last_block: Set(time_since_last_block),
            number_of_transactions: Set(number_of_transactions),
            start_actions_commitment: NotSet,
            end_actions_commitment: NotSet,
            state_data_availability: NotSet,
            proof_data_availability: NotSet,
            created_at: Set(Utc::now()),
            state_calculated_at: NotSet,
            proved_at: NotSet,
        };

        let inserted = new_block
            .insert(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to create block", e))?;

        // Convert to proto message
        let proto_block = Block {
            app_instance_id: inserted.app_instance_id.clone(),
            block_number: inserted.block_number,
            start_sequence: inserted.start_sequence,
            end_sequence: inserted.end_sequence,
            actions_commitment: inserted.actions_commitment,
            state_commitment: inserted.state_commitment,
            time_since_last_block: inserted.time_since_last_block,
            number_of_transactions: inserted.number_of_transactions,
            start_actions_commitment: inserted.start_actions_commitment,
            end_actions_commitment: inserted.end_actions_commitment,
            state_data_availability: inserted.state_data_availability,
            proof_data_availability: inserted.proof_data_availability,
            created_at: Some(to_timestamp(inserted.created_at)),
            state_calculated_at: inserted.state_calculated_at.map(to_timestamp),
            proved_at: inserted.proved_at.map(to_timestamp),
        };

        info!(
            "Created block #{} for app_instance {} (sequences {}-{})",
            new_block_number, req.app_instance_id, start_sequence, end_sequence
        );

        Ok(Response::new(TryCreateBlockResponse {
            block_number: Some(new_block_number),
            block: Some(proto_block),
        }))
    }

    async fn update_block_state_da(
        &self,
        request: Request<UpdateBlockStateDaRequest>,
    ) -> Result<Response<UpdateBlockStateDaResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Find the block
        let block = blocks::Entity::find()
            .filter(blocks::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(blocks::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block", e))?
            .ok_or_else(|| Status::not_found("Block not found"))?;

        // Update state_data_availability field
        let mut block: blocks::ActiveModel = block.into();
        block.state_data_availability = Set(Some(req.state_da.clone()));

        block
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update block state DA", e))?;

        info!(
            "Updated state DA for block #{} in app_instance {}: {}",
            req.block_number, req.app_instance_id, req.state_da
        );

        Ok(Response::new(UpdateBlockStateDaResponse {
            success: true,
            message: "State data availability updated successfully".to_string(),
        }))
    }

    async fn update_block_proof_da(
        &self,
        request: Request<UpdateBlockProofDaRequest>,
    ) -> Result<Response<UpdateBlockProofDaResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Find the block
        let block = blocks::Entity::find()
            .filter(blocks::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(blocks::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block", e))?
            .ok_or_else(|| Status::not_found("Block not found"))?;

        // Update proof_data_availability field
        let mut block: blocks::ActiveModel = block.into();
        block.proof_data_availability = Set(Some(req.proof_da.clone()));

        block
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update block proof DA", e))?;

        info!(
            "Updated proof DA for block #{} in app_instance {}: {}",
            req.block_number, req.app_instance_id, req.proof_da
        );

        Ok(Response::new(UpdateBlockProofDaResponse {
            success: true,
            message: "Proof data availability updated successfully".to_string(),
        }))
    }

    // ============================================================================
    // Proof Calculation Management Methods
    // ============================================================================

    async fn get_proof_calculation(
        &self,
        request: Request<GetProofCalculationRequest>,
    ) -> Result<Response<GetProofCalculationResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query proof_calculations by app_instance_id and block_number
        let proof_calc = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculation", e))?;

        // Convert to proto if found
        let proto_proof_calc = proof_calc.map(|pc| {
            let proofs_data = parse_proofs_json(pc.proofs).unwrap_or_default();
            ProofCalculation {
                app_instance_id: pc.app_instance_id,
                id: pc.id,
                block_number: pc.block_number,
                start_sequence: pc.start_sequence,
                end_sequence: pc.end_sequence,
                proofs: proofs_data.iter().map(proof_data_to_proto).collect(),
                block_proof: pc.block_proof,
                block_proof_submitted: pc.block_proof_submitted,
                created_at: Some(to_timestamp(pc.created_at)),
                updated_at: Some(to_timestamp(pc.updated_at)),
            }
        });

        Ok(Response::new(GetProofCalculationResponse {
            proof_calculation: proto_proof_calc,
        }))
    }

    async fn get_proof_calculations_range(
        &self,
        request: Request<GetProofCalculationsRangeRequest>,
    ) -> Result<Response<GetProofCalculationsRangeResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Validate range
        if req.from_block > req.to_block {
            return Err(Status::invalid_argument("from_block must be <= to_block"));
        }

        // Query proof_calculations range
        let proof_calcs = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.gte(req.from_block))
            .filter(proof_calculations::Column::BlockNumber.lte(req.to_block))
            .order_by_asc(proof_calculations::Column::BlockNumber)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculations range", e))?;

        // Convert to proto messages
        let proto_proof_calcs = proof_calcs
            .into_iter()
            .map(|pc| {
                let proofs_data = parse_proofs_json(pc.proofs).unwrap_or_default();
                ProofCalculation {
                    app_instance_id: pc.app_instance_id,
                    id: pc.id,
                    block_number: pc.block_number,
                    start_sequence: pc.start_sequence,
                    end_sequence: pc.end_sequence,
                    proofs: proofs_data.iter().map(proof_data_to_proto).collect(),
                    block_proof: pc.block_proof,
                    block_proof_submitted: pc.block_proof_submitted,
                    created_at: Some(to_timestamp(pc.created_at)),
                    updated_at: Some(to_timestamp(pc.updated_at)),
                }
            })
            .collect();

        Ok(Response::new(GetProofCalculationsRangeResponse {
            proof_calculations: proto_proof_calcs,
        }))
    }

    async fn start_proving(
        &self,
        request: Request<StartProvingRequest>,
    ) -> Result<Response<StartProvingResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Get current timestamp
        let current_time_ms = Utc::now().timestamp_millis() as u64;

        // Sort and validate all sequences (prover.move lines 383-413)
        let sorted_sequences = sort_and_validate_sequences(
            &req.sequences,
            0, // We'll get actual start_sequence from proof_calculation
            None,
        )?;

        let sorted_sequence1 = if !req.merged_sequences_1.is_empty() {
            Some(sort_and_validate_sequences(&req.merged_sequences_1, 0, None)?)
        } else {
            None
        };

        let sorted_sequence2 = if !req.merged_sequences_2.is_empty() {
            Some(sort_and_validate_sequences(&req.merged_sequences_2, 0, None)?)
        } else {
            None
        };

        // Find proof_calculations record
        let proof_calc = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculation", e))?
            .ok_or_else(|| Status::not_found("Proof calculation not found"))?;

        // Validate sequences against start/end
        let _ = sort_and_validate_sequences(
            &sorted_sequences,
            proof_calc.start_sequence,
            proof_calc.end_sequence,
        )?;

        // Parse proofs JSON array
        let mut proofs_data = parse_proofs_json(proof_calc.proofs.clone())?;

        // Check if this is a block proof (covers entire range)
        let is_block_proof = if let Some(end_seq) = proof_calc.end_sequence {
            sorted_sequences.first() == Some(&proof_calc.start_sequence)
                && sorted_sequences.last() == Some(&end_seq)
        } else {
            false
        };

        // Check if proof already exists
        let existing_proof_idx = proofs_data.iter().position(|p| p.sequences == sorted_sequences);

        if let Some(idx) = existing_proof_idx {
            // Proof exists - check if can restart (prover.move lines 444-462)
            let existing_proof = &proofs_data[idx];

            if !can_restart_proof(existing_proof, current_time_ms) {
                return Ok(Response::new(StartProvingResponse {
                    success: false,
                    message: format!(
                        "Proof already exists with status {} and not timed out",
                        existing_proof.status
                    ),
                }));
            }

            // Preserve rejected_count (prover.move line 471)
            let rejected_count = existing_proof.rejected_count;
            let old_sequence1 = existing_proof.sequence1.clone();
            let old_sequence2 = existing_proof.sequence2.clone();

            // Update existing proof with new values
            proofs_data[idx] = ProofData {
                status: 1, // STARTED
                da_hash: None,
                sequence1: sorted_sequence1.clone(),
                sequence2: sorted_sequence2.clone(),
                rejected_count,
                timestamp: current_time_ms,
                prover: auth.public_key.clone(),
                user: None,
                job_id: None,
                sequences: sorted_sequences.clone(),
            };

            // Return old sub-proofs to CALCULATED (prover.move lines 473-502)
            if let Some(seq1) = old_sequence1 {
                if let Some(proof1_idx) = proofs_data.iter().position(|p| p.sequences == seq1) {
                    return_proof_to_calculated(&mut proofs_data[proof1_idx], current_time_ms);
                }
            }
            if let Some(seq2) = old_sequence2 {
                if let Some(proof2_idx) = proofs_data.iter().position(|p| p.sequences == seq2) {
                    return_proof_to_calculated(&mut proofs_data[proof2_idx], current_time_ms);
                }
            }
        } else {
            // New proof - check if sub-proofs can be reserved (prover.move lines 505-559)
            if let Some(ref seq1) = sorted_sequence1 {
                let proof1_idx = proofs_data.iter().position(|p| &p.sequences == seq1)
                    .ok_or_else(|| Status::failed_precondition("Sub-proof 1 does not exist"))?;

                if !can_reserve_proof(&proofs_data[proof1_idx], current_time_ms, is_block_proof) {
                    return Ok(Response::new(StartProvingResponse {
                        success: false,
                        message: "Cannot reserve sub-proof 1".to_string(),
                    }));
                }
            }

            if let Some(ref seq2) = sorted_sequence2 {
                let proof2_idx = proofs_data.iter().position(|p| &p.sequences == seq2)
                    .ok_or_else(|| Status::failed_precondition("Sub-proof 2 does not exist"))?;

                if !can_reserve_proof(&proofs_data[proof2_idx], current_time_ms, is_block_proof) {
                    return Ok(Response::new(StartProvingResponse {
                        success: false,
                        message: "Cannot reserve sub-proof 2".to_string(),
                    }));
                }
            }

            // All checks passed - insert new proof (prover.move line 562)
            proofs_data.push(ProofData {
                status: 1, // STARTED
                da_hash: None,
                sequence1: sorted_sequence1.clone(),
                sequence2: sorted_sequence2.clone(),
                rejected_count: 0,
                timestamp: current_time_ms,
                prover: auth.public_key.clone(),
                user: None,
                job_id: None,
                sequences: sorted_sequences.clone(),
            });
        }

        // Reserve sub-proofs (prover.move lines 566-593)
        if let Some(seq1) = sorted_sequence1 {
            if let Some(proof1_idx) = proofs_data.iter().position(|p| p.sequences == seq1) {
                reserve_proof(&mut proofs_data[proof1_idx], current_time_ms, auth.public_key.clone());
            }
        }
        if let Some(seq2) = sorted_sequence2 {
            if let Some(proof2_idx) = proofs_data.iter().position(|p| p.sequences == seq2) {
                reserve_proof(&mut proofs_data[proof2_idx], current_time_ms, auth.public_key.clone());
            }
        }

        // Serialize and save back to database
        let proofs_json = serialize_proofs_json(&proofs_data)?;
        let mut proof_calc_active: proof_calculations::ActiveModel = proof_calc.into();
        proof_calc_active.proofs = Set(Some(proofs_json));
        proof_calc_active.updated_at = Set(Utc::now());

        proof_calc_active
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update proof calculation", e))?;

        info!(
            "Started proving for block #{} in app_instance {}, sequences: {:?}",
            req.block_number, req.app_instance_id, sorted_sequences
        );

        Ok(Response::new(StartProvingResponse {
            success: true,
            message: "Proof started successfully".to_string(),
        }))
    }

    async fn submit_calculated_proof(
        &self,
        request: Request<SubmitCalculatedProofRequest>,
    ) -> Result<Response<SubmitCalculatedProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Get current timestamp
        let current_time_ms = Utc::now().timestamp_millis() as u64;

        // Sort and validate sequences (prover.move lines 722-752)
        let sorted_sequences = sort_and_validate_sequences(&req.sequences, 0, None)?;
        let sorted_sequence1 = if !req.merged_sequences_1.is_empty() {
            Some(sort_and_validate_sequences(&req.merged_sequences_1, 0, None)?)
        } else {
            None
        };
        let sorted_sequence2 = if !req.merged_sequences_2.is_empty() {
            Some(sort_and_validate_sequences(&req.merged_sequences_2, 0, None)?)
        } else {
            None
        };

        // Find proof_calculations record
        let proof_calc = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculation", e))?
            .ok_or_else(|| Status::not_found("Proof calculation not found"))?;

        // Parse proofs JSON array
        let mut proofs_data = parse_proofs_json(proof_calc.proofs.clone())?;

        // Find or create proof
        let existing_proof_idx = proofs_data.iter().position(|p| p.sequences == sorted_sequences);

        if let Some(idx) = existing_proof_idx {
            // Update existing proof, preserve rejected_count (prover.move line 782)
            let rejected_count = proofs_data[idx].rejected_count;
            proofs_data[idx] = ProofData {
                status: 2, // CALCULATED
                da_hash: Some(req.da_hash.clone()),
                sequence1: sorted_sequence1.clone(),
                sequence2: sorted_sequence2.clone(),
                rejected_count,
                timestamp: current_time_ms,
                prover: auth.public_key.clone(),
                user: None,
                job_id: Some(req.job_id.clone()),
                sequences: sorted_sequences.clone(),
            };
        } else {
            // Insert new proof (prover.move line 783)
            proofs_data.push(ProofData {
                status: 2, // CALCULATED
                da_hash: Some(req.da_hash.clone()),
                sequence1: sorted_sequence1.clone(),
                sequence2: sorted_sequence2.clone(),
                rejected_count: 0,
                timestamp: current_time_ms,
                prover: auth.public_key.clone(),
                user: None,
                job_id: Some(req.job_id.clone()),
                sequences: sorted_sequences.clone(),
            });
        }

        // Mark sub-proofs as USED (prover.move lines 784-811)
        if let Some(seq1) = sorted_sequence1 {
            if let Some(proof1_idx) = proofs_data.iter().position(|p| p.sequences == seq1) {
                mark_proof_as_used(&mut proofs_data[proof1_idx], current_time_ms, auth.public_key.clone());
            }
        }
        if let Some(seq2) = sorted_sequence2 {
            if let Some(proof2_idx) = proofs_data.iter().position(|p| p.sequences == seq2) {
                mark_proof_as_used(&mut proofs_data[proof2_idx], current_time_ms, auth.public_key.clone());
            }
        }

        // Check if block proof is complete (prover.move lines 812-825)
        let mut block_proof_ready = false;
        if let Some(end_seq) = proof_calc.end_sequence {
            if sorted_sequences.first() == Some(&proof_calc.start_sequence)
                && sorted_sequences.last() == Some(&end_seq)
            {
                // This is the block proof - mark as finished
                block_proof_ready = true;
            }
        }

        // Serialize and save
        let proofs_json = serialize_proofs_json(&proofs_data)?;
        let mut proof_calc_active: proof_calculations::ActiveModel = proof_calc.into();
        proof_calc_active.proofs = Set(Some(proofs_json));

        if block_proof_ready {
            proof_calc_active.block_proof = Set(Some(req.da_hash.clone()));
        }

        proof_calc_active.updated_at = Set(Utc::now());

        proof_calc_active
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update proof calculation", e))?;

        info!(
            "Submitted proof for block #{} in app_instance {}, sequences: {:?}, block_proof_ready: {}",
            req.block_number, req.app_instance_id, sorted_sequences, block_proof_ready
        );

        Ok(Response::new(SubmitCalculatedProofResponse {
            success: true,
            message: "Proof submitted successfully".to_string(),
            block_proof_ready,
        }))
    }

    async fn reject_proof(
        &self,
        request: Request<RejectProofRequest>,
    ) -> Result<Response<RejectProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify JWT has access to this app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Get current timestamp
        let current_time_ms = Utc::now().timestamp_millis() as u64;

        // Sort and validate sequences (prover.move lines 610-614)
        let sorted_sequences = sort_and_validate_sequences(&req.sequences, 0, None)?;

        // Find proof_calculations record
        let proof_calc = proof_calculations::Entity::find()
            .filter(proof_calculations::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(proof_calculations::Column::BlockNumber.eq(req.block_number))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch proof calculation", e))?
            .ok_or_else(|| Status::not_found("Proof calculation not found"))?;

        // Parse proofs JSON array
        let mut proofs_data = parse_proofs_json(proof_calc.proofs.clone())?;

        // Find the proof
        let proof_idx = proofs_data
            .iter()
            .position(|p| p.sequences == sorted_sequences)
            .ok_or_else(|| Status::not_found("Proof not found"))?;

        // Get sub-proof sequences before updating (prover.move lines 616-627)
        let sequence1 = proofs_data[proof_idx].sequence1.clone();
        let sequence2 = proofs_data[proof_idx].sequence2.clone();

        // Update proof status to REJECTED and increment rejected_count (lines 620-621)
        proofs_data[proof_idx].status = 3; // REJECTED
        proofs_data[proof_idx].rejected_count += 1;
        proofs_data[proof_idx].timestamp = current_time_ms;

        // Return sub-proofs to CALCULATED (prover.move lines 629-652)
        if let Some(seq1) = sequence1 {
            if let Some(proof1_idx) = proofs_data.iter().position(|p| p.sequences == seq1) {
                return_proof_to_calculated(&mut proofs_data[proof1_idx], current_time_ms);
            }
        }
        if let Some(seq2) = sequence2 {
            if let Some(proof2_idx) = proofs_data.iter().position(|p| p.sequences == seq2) {
                return_proof_to_calculated(&mut proofs_data[proof2_idx], current_time_ms);
            }
        }

        // Serialize and save
        let proofs_json = serialize_proofs_json(&proofs_data)?;
        let mut proof_calc_active: proof_calculations::ActiveModel = proof_calc.into();
        proof_calc_active.proofs = Set(Some(proofs_json));
        proof_calc_active.updated_at = Set(Utc::now());

        proof_calc_active
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update proof calculation", e))?;

        info!(
            "Rejected proof for block #{} in app_instance {}, sequences: {:?}",
            req.block_number, req.app_instance_id, sorted_sequences
        );

        Ok(Response::new(RejectProofResponse {
            success: true,
            message: "Proof rejected successfully".to_string(),
        }))
    }

    // ============================================================================
    // Settlement Management Methods
    // ============================================================================

    async fn get_block_settlement(
        &self,
        request: Request<GetBlockSettlementRequest>,
    ) -> Result<Response<GetBlockSettlementResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch block settlement from database
        let block_settlement = block_settlements::Entity::find()
            .filter(block_settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(block_settlements::Column::BlockNumber.eq(req.block_number))
            .filter(block_settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block settlement", e))?
            .ok_or_else(|| Status::not_found("Block settlement not found"))?;

        // Convert to proto
        let proto_block_settlement = crate::proto::BlockSettlement {
            app_instance_id: block_settlement.app_instance_id,
            block_number: block_settlement.block_number,
            chain: block_settlement.chain,
            settlement_tx_hash: block_settlement.settlement_tx_hash,
            settlement_tx_included_in_block: block_settlement.settlement_tx_included_in_block,
            sent_to_settlement_at: block_settlement.sent_to_settlement_at.map(|dt| {
                let timestamp_millis = dt.timestamp_millis();
                prost_types::Timestamp {
                    seconds: timestamp_millis / 1000,
                    nanos: ((timestamp_millis % 1000) * 1_000_000) as i32,
                }
            }),
            settled_at: block_settlement.settled_at.map(|dt| {
                let timestamp_millis = dt.timestamp_millis();
                prost_types::Timestamp {
                    seconds: timestamp_millis / 1000,
                    nanos: ((timestamp_millis % 1000) * 1_000_000) as i32,
                }
            }),
            created_at: Some({
                let timestamp_millis = block_settlement.created_at.timestamp_millis();
                prost_types::Timestamp {
                    seconds: timestamp_millis / 1000,
                    nanos: ((timestamp_millis % 1000) * 1_000_000) as i32,
                }
            }),
            updated_at: Some({
                let timestamp_millis = block_settlement.updated_at.timestamp_millis();
                prost_types::Timestamp {
                    seconds: timestamp_millis / 1000,
                    nanos: ((timestamp_millis % 1000) * 1_000_000) as i32,
                }
            }),
        };

        Ok(Response::new(GetBlockSettlementResponse {
            block_settlement: Some(proto_block_settlement),
        }))
    }

    async fn get_settlement_chains(
        &self,
        request: Request<GetSettlementChainsRequest>,
    ) -> Result<Response<GetSettlementChainsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query distinct chains from settlements table
        let chains: Vec<String> = settlements::Entity::find()
            .filter(settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch settlement chains", e))?
            .into_iter()
            .map(|s| s.chain)
            .collect();

        Ok(Response::new(GetSettlementChainsResponse { chains }))
    }

    async fn get_settlement_address(
        &self,
        request: Request<GetSettlementAddressRequest>,
    ) -> Result<Response<GetSettlementAddressResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch settlement record
        let settlement = settlements::Entity::find()
            .filter(settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch settlement", e))?;

        let settlement_address = settlement.and_then(|s| s.settlement_address);

        Ok(Response::new(GetSettlementAddressResponse {
            settlement_address,
        }))
    }

    async fn get_settlement_job_for_chain(
        &self,
        request: Request<GetSettlementJobForChainRequest>,
    ) -> Result<Response<GetSettlementJobForChainResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch settlement record
        let settlement = settlements::Entity::find()
            .filter(settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch settlement", e))?;

        let settlement_job = settlement.and_then(|s| s.settlement_job);

        Ok(Response::new(GetSettlementJobForChainResponse {
            settlement_job,
        }))
    }

    async fn set_settlement_address(
        &self,
        request: Request<SetSettlementAddressRequest>,
    ) -> Result<Response<SetSettlementAddressResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Find or create settlement record
        let existing_settlement = settlements::Entity::find()
            .filter(settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch settlement", e))?;

        if let Some(settlement) = existing_settlement {
            // Update existing settlement
            let mut active: settlements::ActiveModel = settlement.into();
            active.settlement_address = Set(req.settlement_address.clone());
            active.updated_at = Set(chrono::Utc::now().into());

            active
                .update(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to update settlement address", e))?;
        } else {
            // Create new settlement record
            let new_settlement = settlements::ActiveModel {
                app_instance_id: Set(req.app_instance_id.clone()),
                chain: Set(req.chain.clone()),
                last_settled_block_number: Set(0),
                settlement_address: Set(req.settlement_address.clone()),
                settlement_job: Set(None),
                created_at: Set(chrono::Utc::now().into()),
                updated_at: Set(chrono::Utc::now().into()),
            };

            new_settlement
                .insert(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to create settlement", e))?;
        }

        info!(
            "Set settlement address for app_instance {} chain {} to {:?}",
            req.app_instance_id, req.chain, req.settlement_address
        );

        Ok(Response::new(SetSettlementAddressResponse {
            success: true,
            message: "Settlement address set successfully".to_string(),
        }))
    }

    async fn update_block_settlement_tx_hash(
        &self,
        request: Request<UpdateBlockSettlementTxHashRequest>,
    ) -> Result<Response<UpdateBlockSettlementTxHashResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Find or create block settlement
        let existing_block_settlement = block_settlements::Entity::find()
            .filter(block_settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(block_settlements::Column::BlockNumber.eq(req.block_number))
            .filter(block_settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block settlement", e))?;

        if let Some(block_settlement) = existing_block_settlement {
            // Update existing
            let mut active: block_settlements::ActiveModel = block_settlement.into();
            active.settlement_tx_hash = Set(req.settlement_tx_hash.clone());
            active.sent_to_settlement_at = Set(req.sent_to_settlement_at.map(|ts| {
                let timestamp_millis = (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000;
                chrono::DateTime::from_timestamp_millis(timestamp_millis)
                    .expect("Invalid timestamp")
                    .into()
            }));
            active.updated_at = Set(chrono::Utc::now().into());

            active
                .update(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to update block settlement tx hash", e))?;
        } else {
            // Create new block settlement
            let new_block_settlement = block_settlements::ActiveModel {
                app_instance_id: Set(req.app_instance_id.clone()),
                block_number: Set(req.block_number),
                chain: Set(req.chain.clone()),
                settlement_tx_hash: Set(req.settlement_tx_hash.clone()),
                settlement_tx_included_in_block: Set(None),
                sent_to_settlement_at: Set(req.sent_to_settlement_at.map(|ts| {
                    let timestamp_millis = (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000;
                    chrono::DateTime::from_timestamp_millis(timestamp_millis)
                        .expect("Invalid timestamp")
                        .into()
                })),
                settled_at: Set(None),
                created_at: Set(chrono::Utc::now().into()),
                updated_at: Set(chrono::Utc::now().into()),
            };

            new_block_settlement
                .insert(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to create block settlement", e))?;
        }

        info!(
            "Updated block settlement tx hash for app_instance {} block #{} chain {} to {:?}",
            req.app_instance_id, req.block_number, req.chain, req.settlement_tx_hash
        );

        Ok(Response::new(UpdateBlockSettlementTxHashResponse {
            success: true,
            message: "Block settlement tx hash updated successfully".to_string(),
        }))
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        request: Request<UpdateBlockSettlementTxIncludedInBlockRequest>,
    ) -> Result<Response<UpdateBlockSettlementTxIncludedInBlockResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch block settlement
        let block_settlement = block_settlements::Entity::find()
            .filter(block_settlements::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(block_settlements::Column::BlockNumber.eq(req.block_number))
            .filter(block_settlements::Column::Chain.eq(&req.chain))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch block settlement", e))?
            .ok_or_else(|| Status::not_found("Block settlement not found"))?;

        // Update block settlement
        let mut active: block_settlements::ActiveModel = block_settlement.into();
        active.settlement_tx_included_in_block = Set(req.settlement_tx_included_in_block);
        active.settled_at = Set(req.settled_at.map(|ts| {
            let timestamp_millis = (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000;
            chrono::DateTime::from_timestamp_millis(timestamp_millis)
                .expect("Invalid timestamp")
                .into()
        }));
        active.updated_at = Set(chrono::Utc::now().into());

        active
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update block settlement tx included", e))?;

        // If tx is included, update last_settled_block_number in settlements table
        // following the logic in settlement.move:106-167
        if req.settlement_tx_included_in_block.is_some() {
            let settlement = settlements::Entity::find()
                .filter(settlements::Column::AppInstanceId.eq(&req.app_instance_id))
                .filter(settlements::Column::Chain.eq(&req.chain))
                .one(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to fetch settlement", e))?
                .ok_or_else(|| Status::not_found("Settlement record not found"))?;

            let mut last_settled = settlement.last_settled_block_number;

            // Advance last_settled_block_number to highest consecutive settled block
            while last_settled < req.block_number {
                let next_block = last_settled + 1;

                // Check if next block is settled
                let next_settlement = block_settlements::Entity::find()
                    .filter(block_settlements::Column::AppInstanceId.eq(&req.app_instance_id))
                    .filter(block_settlements::Column::BlockNumber.eq(next_block))
                    .filter(block_settlements::Column::Chain.eq(&req.chain))
                    .one(self.db.connection())
                    .await
                    .map_err(|e| log_internal_error("Failed to check next block settlement", e))?;

                if let Some(next) = next_settlement {
                    if next.settlement_tx_included_in_block.is_some() {
                        last_settled = next_block;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Update settlement record
            let mut settlement_active: settlements::ActiveModel = settlement.into();
            settlement_active.last_settled_block_number = Set(last_settled);
            settlement_active.updated_at = Set(chrono::Utc::now().into());

            settlement_active
                .update(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to update last_settled_block_number", e))?;

            info!(
                "Block #{} settled for app_instance {} chain {}, last_settled_block_number now {}",
                req.block_number, req.app_instance_id, req.chain, last_settled
            );
        }

        Ok(Response::new(UpdateBlockSettlementTxIncludedInBlockResponse {
            success: true,
            message: "Block settlement tx included status updated successfully".to_string(),
        }))
    }

    // ============================================================================
    // State Update & Configuration Methods
    // ============================================================================

    async fn update_state_for_sequence(
        &self,
        request: Request<UpdateStateForSequenceRequest>,
    ) -> Result<Response<UpdateStateForSequenceResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Calculate state hash if state data is provided
        let state_hash = if let Some(ref state_data) = req.new_state_data {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(state_data);
            hasher.finalize().to_vec()
        } else {
            vec![] // Empty hash if only DA hash provided
        };

        // Find existing state record
        let existing_state = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.eq(req.sequence))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch state", e))?;

        if let Some(state_record) = existing_state {
            // Update existing state
            let mut active: state::ActiveModel = state_record.into();

            if let Some(state_data) = req.new_state_data {
                active.state_data = Set(Some(state_data));
                active.state_hash = Set(state_hash);
            }

            if let Some(da_hash) = req.new_data_availability_hash {
                active.state_da = Set(Some(da_hash));
            }

            active
                .update(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to update state", e))?;
        } else {
            // Create new state record
            let new_state = state::ActiveModel {
                app_instance_id: Set(req.app_instance_id.clone()),
                sequence: Set(req.sequence),
                state_hash: Set(state_hash),
                state_data: Set(req.new_state_data),
                state_da: Set(req.new_data_availability_hash),
                proof_data: Set(None),
                proof_da: Set(None),
                proof_hash: Set(None),
                commitment: Set(None),
                metadata: Set(None),
                proved_at: Set(chrono::Utc::now().into()),
                id: NotSet,
            };

            new_state
                .insert(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to create state", e))?;
        }

        info!(
            "Updated state for sequence {} in app_instance {}",
            req.sequence, req.app_instance_id
        );

        Ok(Response::new(UpdateStateForSequenceResponse {
            success: true,
            message: "State updated successfully".to_string(),
        }))
    }

    async fn is_app_paused(
        &self,
        request: Request<IsAppPausedRequest>,
    ) -> Result<Response<IsAppPausedResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch app instance
        let app = app_instances::Entity::find()
            .filter(app_instances::Column::AppInstanceId.eq(&req.app_instance_id))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch app instance", e))?
            .ok_or_else(|| Status::not_found("App instance not found"))?;

        Ok(Response::new(IsAppPausedResponse {
            is_paused: app.is_paused,
        }))
    }

    async fn get_min_time_between_blocks(
        &self,
        request: Request<GetMinTimeBetweenBlocksRequest>,
    ) -> Result<Response<GetMinTimeBetweenBlocksResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch app instance
        let app = app_instances::Entity::find()
            .filter(app_instances::Column::AppInstanceId.eq(&req.app_instance_id))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch app instance", e))?
            .ok_or_else(|| Status::not_found("App instance not found"))?;

        // Return the value in seconds (as stored in database)
        let min_time_seconds = app.min_time_between_blocks;

        Ok(Response::new(GetMinTimeBetweenBlocksResponse {
            min_time_seconds,
        }))
    }

    async fn purge(
        &self,
        request: Request<PurgeRequest>,
    ) -> Result<Response<PurgeResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Validate sequences_to_purge > 0
        if req.sequences_to_purge == 0 {
            return Err(Status::invalid_argument("sequences_to_purge must be greater than 0"));
        }

        // Fetch app instance
        let app = app_instances::Entity::find()
            .filter(app_instances::Column::AppInstanceId.eq(&req.app_instance_id))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch app instance", e))?
            .ok_or_else(|| Status::not_found("App instance not found"))?;

        // Calculate target purge sequence (capped at last_settled_sequence)
        let mut target_sequence = app.last_purged_sequence + req.sequences_to_purge;
        if target_sequence > app.last_settled_sequence {
            target_sequence = app.last_settled_sequence;
        }

        // If no sequences to purge, return early
        if target_sequence <= app.last_purged_sequence {
            return Ok(Response::new(PurgeResponse {
                success: true,
                message: "No sequences to purge".to_string(),
                last_purged_sequence: app.last_purged_sequence,
            }));
        }

        // Delete old records from state table
        state::Entity::delete_many()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.lte(target_sequence))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to purge state records", e))?;

        // Delete old records from optimistic_state table
        optimistic_state::Entity::delete_many()
            .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(optimistic_state::Column::Sequence.lte(target_sequence))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to purge optimistic_state records", e))?;

        // Delete old records from user_actions table
        user_actions::Entity::delete_many()
            .filter(user_actions::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(user_actions::Column::Sequence.lte(target_sequence))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to purge user_actions records", e))?;

        // Update app instance last_purged_sequence
        let mut app_active: app_instances::ActiveModel = app.into();
        app_active.last_purged_sequence = Set(target_sequence);
        app_active.updated_at = Set(chrono::Utc::now().into());

        app_active
            .update(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to update last_purged_sequence", e))?;

        info!(
            "Purged sequences up to {} for app_instance {}",
            target_sequence, req.app_instance_id
        );

        Ok(Response::new(PurgeResponse {
            success: true,
            message: format!("Purged sequences up to {}", target_sequence),
            last_purged_sequence: target_sequence,
        }))
    }

    async fn get_metadata(
        &self,
        request: Request<GetMetadataRequest>,
    ) -> Result<Response<GetMetadataResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use crate::entity::app_instance_metadata;

        let metadata = app_instance_metadata::Entity::find()
            .filter(app_instance_metadata::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_metadata::Column::Key.eq(&req.key))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to fetch metadata", e))?;

        Ok(Response::new(GetMetadataResponse {
            metadata: metadata.map(|m| AppInstanceMetadata {
                app_instance_id: m.app_instance_id,
                key: m.key,
                value: m.value,
                created_at: Some(to_timestamp(m.created_at)),
                updated_at: Some(to_timestamp(m.updated_at)),
            }),
        }))
    }

    async fn add_metadata(
        &self,
        request: Request<AddMetadataRequest>,
    ) -> Result<Response<AddMetadataResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use crate::entity::app_instance_metadata;

        let metadata = app_instance_metadata::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            key: Set(req.key.clone()),
            value: Set(req.value),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        metadata
            .insert(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to insert metadata", e))?;

        Ok(Response::new(AddMetadataResponse {
            success: true,
            message: format!("Metadata added for key '{}'", req.key),
        }))
    }

    // ============================================================================
    // Coordinator Authentication Methods
    // ============================================================================

    async fn coordinator_job_operation(
        &self,
        request: Request<CoordinatorJobRequest>,
    ) -> Result<Response<CoordinatorJobResponse>, Status> {
        let req = request.into_inner();

        // Extract coordinator auth
        let coordinator_auth = req
            .coordinator_auth
            .ok_or_else(|| Status::unauthenticated("Missing coordinator authentication"))?;

        let app_instance_id = &req.app_instance_id;
        let coordinator_public_key = &coordinator_auth.coordinator_public_key;

        // Extract operation
        let operation = req
            .operation
            .ok_or_else(|| Status::invalid_argument("Missing operation"))?;

        // Determine operation name for auth
        let operation_name = match &operation {
            coordinator_job_request::Operation::Create(_) => "create_job",
            coordinator_job_request::Operation::Start(_) => "start_job",
            coordinator_job_request::Operation::Complete(_) => "complete_job",
            coordinator_job_request::Operation::Fail(_) => "fail_job",
            coordinator_job_request::Operation::Get(_) => "get_job",
            coordinator_job_request::Operation::Terminate(_) => "terminate_job",
            coordinator_job_request::Operation::GetPendingSequences(_) => "get_pending_sequences",
            coordinator_job_request::Operation::GetPendingCount(_) => "get_pending_count",
            coordinator_job_request::Operation::GetFailedCount(_) => "get_failed_count",
            coordinator_job_request::Operation::GetTotalCount(_) => "get_total_count",
            coordinator_job_request::Operation::GetPendingJobs(_) => "get_pending_jobs",
            coordinator_job_request::Operation::GetFailedJobs(_) => "get_failed_jobs",
            coordinator_job_request::Operation::GetJobsBatch(_) => "get_jobs_batch",
            coordinator_job_request::Operation::RestartFailedJobs(_) => "restart_failed_jobs",
            coordinator_job_request::Operation::RemoveFailedJobs(_) => "remove_failed_jobs",
            coordinator_job_request::Operation::CreateMergeJob(_) => "create_merge_job",
            coordinator_job_request::Operation::CreateSettleJob(_) => "create_settle_job",
        };

        // Verify signature and access
        verify_and_authorize(
            self.db.connection(),
            &coordinator_auth,
            app_instance_id,
            operation_name,
        )
        .await?;

        // Execute operation
        let result = match operation {
            coordinator_job_request::Operation::Create(op) => {
                self.handle_create_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::Start(op) => {
                self.handle_start_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::Complete(op) => {
                self.handle_complete_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::Fail(op) => {
                self.handle_fail_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::Get(op) => {
                self.handle_get_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::Terminate(op) => {
                self.handle_terminate_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::GetPendingSequences(op) => {
                self.handle_get_pending_sequences(app_instance_id, op).await
            }
            coordinator_job_request::Operation::GetPendingCount(_) => {
                self.handle_get_pending_count(app_instance_id).await
            }
            coordinator_job_request::Operation::GetFailedCount(_) => {
                self.handle_get_failed_count(app_instance_id).await
            }
            coordinator_job_request::Operation::GetTotalCount(_) => {
                self.handle_get_total_count(app_instance_id).await
            }
            coordinator_job_request::Operation::GetPendingJobs(_) => {
                self.handle_get_pending_jobs(app_instance_id).await
            }
            coordinator_job_request::Operation::GetFailedJobs(_) => {
                self.handle_get_failed_jobs(app_instance_id).await
            }
            coordinator_job_request::Operation::GetJobsBatch(op) => {
                self.handle_get_jobs_batch(app_instance_id, op).await
            }
            coordinator_job_request::Operation::RestartFailedJobs(op) => {
                self.handle_restart_failed_jobs(app_instance_id, op).await
            }
            coordinator_job_request::Operation::RemoveFailedJobs(op) => {
                self.handle_remove_failed_jobs(app_instance_id, op).await
            }
            coordinator_job_request::Operation::CreateMergeJob(op) => {
                self.handle_create_merge_job(app_instance_id, op).await
            }
            coordinator_job_request::Operation::CreateSettleJob(op) => {
                self.handle_create_settle_job(app_instance_id, op).await
            }
        };

        // Log result to audit log
        let (success, error_msg, job_seq) = match &result {
            Ok(resp) => (resp.success, None, resp.job_sequence),
            Err(e) => (false, Some(e.to_string()), None),
        };

        if let Err(e) = self
            .log_coordinator_action(
                coordinator_public_key,
                app_instance_id,
                operation_name,
                job_seq,
                success,
                error_msg,
            )
            .await
        {
            warn!("Failed to log coordinator action: {}", e);
        }

        result.map(Response::new)
    }


    async fn grant_coordinator_access(
        &self,
        request: Request<GrantCoordinatorAccessRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();

        // Verify JWT authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership of app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Verify group exists
        let group_exists = coordinator_groups::Entity::find_by_id(&req.group_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .is_some();

        if !group_exists {
            return Err(Status::not_found(format!(
                "Coordinator group {} not found",
                req.group_id
            )));
        }

        // Insert or update access grant
        let access_grant = app_instance_coordinator_access::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            group_id: Set(req.group_id.clone()),
            granted_at: Set(Utc::now()),
            granted_by: Set(auth.public_key.clone()),
            expires_at: Set(req.expires_at.map(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_else(Utc::now)
            })),
            access_level: Set(req.access_level.clone()),
        };

        // Use ON DUPLICATE KEY UPDATE logic via insert with on_conflict
        app_instance_coordinator_access::Entity::insert(access_grant)
            .on_conflict(
                sea_orm::sea_query::OnConflict::columns(vec![
                    app_instance_coordinator_access::Column::AppInstanceId,
                    app_instance_coordinator_access::Column::GroupId,
                ])
                .update_columns(vec![
                    app_instance_coordinator_access::Column::GrantedAt,
                    app_instance_coordinator_access::Column::GrantedBy,
                    app_instance_coordinator_access::Column::ExpiresAt,
                    app_instance_coordinator_access::Column::AccessLevel,
                ])
                .to_owned(),
            )
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to grant coordinator access", e))?;

        info!(
            "Granted {} access to group {} for app instance {} by {}",
            req.access_level, req.group_id, req.app_instance_id, auth.public_key
        );

        Ok(Response::new(()))
    }

    async fn revoke_coordinator_access(
        &self,
        request: Request<RevokeCoordinatorAccessRequest>,
    ) -> Result<Response<()>, Status> {
        let req = request.into_inner();

        // Verify JWT authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership of app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Delete access grant
        let result = app_instance_coordinator_access::Entity::delete_many()
            .filter(app_instance_coordinator_access::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_coordinator_access::Column::GroupId.eq(&req.group_id))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to revoke coordinator access", e))?;

        if result.rows_affected > 0 {
            info!(
                "Revoked access for group {} from app instance {} by {}",
                req.group_id, req.app_instance_id, auth.public_key
            );
            Ok(Response::new(()))
        } else {
            Err(Status::not_found(format!(
                "No access grant found for group {} on app instance {}",
                req.group_id, req.app_instance_id
            )))
        }
    }

    async fn list_coordinator_access(
        &self,
        request: Request<ListCoordinatorAccessRequest>,
    ) -> Result<Response<ListCoordinatorAccessResponse>, Status> {
        let req = request.into_inner();

        // Verify JWT authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership of app instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch all access grants for this app instance
        let access_grants = app_instance_coordinator_access::Entity::find()
            .filter(app_instance_coordinator_access::Column::AppInstanceId.eq(&req.app_instance_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Convert to proto format
        let proto_grants: Vec<AppInstanceAccess> = access_grants
            .into_iter()
            .map(|grant| AppInstanceAccess {
                app_instance_id: grant.app_instance_id,
                group_id: grant.group_id,
                access_level: grant.access_level,
                granted_at: Some(to_timestamp(grant.granted_at)),
                expires_at: grant.expires_at.map(|dt| to_timestamp(dt)),
            })
            .collect();

        Ok(Response::new(ListCoordinatorAccessResponse {
            access_grants: proto_grants,
        }))
    }

    /// Subscribe to job creation events for an app instance
    /// Uses database polling with SeaORM to detect new jobs
    async fn subscribe_job_events(
        &self,
        request: Request<SubscribeJobEventsRequest>,
    ) -> Result<Response<Self::SubscribeJobEventsStream>, Status> {
        let req = request.into_inner();

        // TODO: Temporarily skip auth check - will add proper JWT auth later
        // let auth = self.extract_auth_from_body(&req.auth)?;
        // self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Use provided app_instance_id, or empty string to stream from all app instances
        let app_instance_id = if req.app_instance_id.is_empty() {
            warn!("Event stream requested without app_instance_id - streaming all jobs (temp development mode)");
            None
        } else {
            Some(req.app_instance_id.clone())
        };

        info!(
            "Starting event stream for app_instance={:?}, from_sequence={:?}",
            app_instance_id, req.from_job_sequence
        );

        // Create broadcast channel for this subscription
        let (tx, rx) = broadcast::channel::<JobEventMessage>(100);

        // Clone what we need for the spawned task
        let db = self.db.clone();
        let from_sequence = req.from_job_sequence.unwrap_or(0);

        // Spawn background task to monitor database for new jobs
        tokio::spawn(async move {
            let mut last_sequence = from_sequence;

            loop {
                // Poll database every 1 second for new jobs
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Use SeaORM to query new jobs
                let mut query = jobs::Entity::find()
                    .filter(jobs::Column::JobSequence.gt(last_sequence))
                    .order_by_asc(jobs::Column::JobSequence);

                // Optionally filter by app_instance_id
                if let Some(ref app_id) = app_instance_id {
                    query = query.filter(jobs::Column::AppInstanceId.eq(app_id));
                }

                let result = query
                    .limit(100)
                    .all(db.connection())
                    .await;

                match result {
                    Ok(job_models) => {
                        for job_model in job_models {
                            last_sequence = job_model.job_sequence;

                            // Convert SeaORM model to proto Job using existing helper
                            let proto_job = Job {
                                job_sequence: job_model.job_sequence,
                                description: job_model.description,
                                developer: job_model.developer,
                                agent: job_model.agent,
                                agent_method: job_model.agent_method,
                                app_instance_id: job_model.app_instance_id,
                                block_number: job_model.block_number,
                                sequences: job_model.sequences
                                    .and_then(|j| serde_json::from_value(j).ok())
                                    .unwrap_or_default(),
                                sequences1: job_model.sequences1
                                    .and_then(|j| serde_json::from_value(j).ok())
                                    .unwrap_or_default(),
                                sequences2: job_model.sequences2
                                    .and_then(|j| serde_json::from_value(j).ok())
                                    .unwrap_or_default(),
                                data: job_model.data,
                                data_da: job_model.data_da,
                                interval_ms: job_model.interval_ms,
                                next_scheduled_at: job_model.next_scheduled_at.map(|dt| to_timestamp(dt)),
                                status: match job_model.status.as_str() {
                                    "pending" => 1,
                                    "running" => 2,
                                    "completed" => 3,
                                    "failed" => 4,
                                    _ => 0,
                                },
                                error_message: job_model.error_message,
                                attempts: job_model.attempts as u32,
                                created_at: Some(to_timestamp(job_model.created_at)),
                                updated_at: Some(to_timestamp(job_model.updated_at)),
                                metadata: job_model.metadata.and_then(json_to_prost_struct),
                                agent_jwt: job_model.agent_jwt,
                                jwt_expires_at: job_model.jwt_expires_at.map(|dt| to_timestamp(dt)),
                            };

                            let event = JobEventMessage {
                                job_sequence: last_sequence,
                                job: Some(proto_job),
                                event_time: Some(to_timestamp(Utc::now())),
                            };

                            if tx.send(event).is_err() {
                                // Receiver dropped, exit
                                info!("Event stream closed for app_instance={:?}", app_instance_id);
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Error polling for job events in app_instance={:?}: {}", app_instance_id, e);
                    }
                }
            }
        });

        // Convert broadcast receiver to stream
        let stream = BroadcastStream::new(rx).map(|result| {
            result.map_err(|e| Status::internal(format!("Stream error: {}", e)))
        });

        Ok(Response::new(Box::pin(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_binary_prefix_basic() {
        // Basic increment
        assert_eq!(next_binary_prefix(&[0x01, 0x02]), Some(vec![0x01, 0x03]));
        assert_eq!(next_binary_prefix(&[0x00, 0x00]), Some(vec![0x00, 0x01]));
        assert_eq!(next_binary_prefix(&[0xAB, 0xCD]), Some(vec![0xAB, 0xCE]));
    }

    #[test]
    fn test_next_binary_prefix_carry() {
        // Carry from rightmost byte
        assert_eq!(next_binary_prefix(&[0x01, 0xFF]), Some(vec![0x02, 0x00]));
        assert_eq!(next_binary_prefix(&[0x00, 0xFF]), Some(vec![0x01, 0x00]));

        // Multiple carries
        assert_eq!(next_binary_prefix(&[0x01, 0xFF, 0xFF]), Some(vec![0x02, 0x00, 0x00]));
    }

    #[test]
    fn test_next_binary_prefix_overflow() {
        // All 0xFF bytes - cannot increment
        assert_eq!(next_binary_prefix(&[0xFF]), None);
        assert_eq!(next_binary_prefix(&[0xFF, 0xFF]), None);
        assert_eq!(next_binary_prefix(&[0xFF, 0xFF, 0xFF]), None);
    }

    #[test]
    fn test_next_binary_prefix_edge_cases() {
        // Empty prefix
        assert_eq!(next_binary_prefix(&[]), None);

        // Single byte
        assert_eq!(next_binary_prefix(&[0x00]), Some(vec![0x01]));
        assert_eq!(next_binary_prefix(&[0x7F]), Some(vec![0x80]));
        assert_eq!(next_binary_prefix(&[0xFE]), Some(vec![0xFF]));
    }

    #[test]
    fn test_next_binary_prefix_realistic() {
        // Realistic key prefixes
        let prefix = b"user:";
        let next = next_binary_prefix(prefix).unwrap();
        assert_eq!(next, b"user;");  // ':' + 1 = ';'

        // Namespace separator
        let prefix = b"namespace\x00";
        let next = next_binary_prefix(prefix).unwrap();
        assert_eq!(next, b"namespace\x01");
    }
}