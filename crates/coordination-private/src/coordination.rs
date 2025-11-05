//! Private Coordination layer implementation (gRPC client)

use async_trait::async_trait;
use futures::StreamExt;
use silvana_coordination_trait::{
    AppInstance, Block, BlockSettlement, Coordination, CoordinationLayer, EventStream,
    Job, JobCreatedEvent, MulticallOperations, MulticallResult, Proof, ProofCalculation, ProofStatus, SequenceState,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::task_local;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::debug;

use proto::silvana::state::v1::state_service_client::StateServiceClient;

use crate::auth::CoordinatorAuth;
use crate::config::PrivateCoordinationConfig;
use crate::error::{PrivateCoordinationError, Result};

// Thread-local storage for current job being executed
// Stores (app_instance_id, job_sequence)
task_local! {
    static CURRENT_JOB_CONTEXT: (String, u64);
}

/// JWT context for caching tokens
struct JWTContext {
    token: String,
    expires_at: u64,
    app_instance_id: String,
}

/// Private Coordination layer implementation using gRPC client
pub struct PrivateCoordination {
    /// gRPC client to private state server
    client: StateServiceClient<Channel>,

    /// Coordinator authentication
    auth: CoordinatorAuth,

    /// Chain ID
    chain_id: String,

    /// Request timeout (reserved for future use)
    #[allow(dead_code)]
    request_timeout: Duration,

    /// JWT context for agent operations (cached tokens)
    jwt_context: tokio::sync::RwLock<Option<JWTContext>>,
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
        let auth = CoordinatorAuth::from_private_key(&private_key)?;

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
            jwt_context: tokio::sync::RwLock::new(None),
        })
    }

    /// Get or create JWT for app instance context
    async fn get_or_create_jwt(&self, app_instance_id: &str) -> Result<String> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Check if we have a valid cached JWT
        {
            let ctx = self.jwt_context.read().await;
            if let Some(jwt_ctx) = ctx.as_ref() {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                // Use cached JWT if it's for the same app and hasn't expired (with 60s buffer)
                if jwt_ctx.app_instance_id == app_instance_id && jwt_ctx.expires_at > now + 60 {
                    return Ok(jwt_ctx.token.clone());
                }
            }
        }

        // Create new JWT (valid for 1 hour)
        let token = crate::jwt::create_coordinator_jwt(
            self.auth.secret_key_bytes(),
            self.auth.public_key(),
            app_instance_id,
            3600,  // 1 hour
        )?;

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() + 3600;

        // Cache the JWT
        {
            let mut ctx = self.jwt_context.write().await;
            *ctx = Some(JWTContext {
                token: token.clone(),
                expires_at,
                app_instance_id: app_instance_id.to_string(),
            });
        }

        Ok(token)
    }

    /// Get JWT for agent operations with fallback logic
    ///
    /// Strategy:
    /// 1. If running within job context -> use job's embedded JWT
    /// 2. Otherwise -> create coordinator JWT on-demand
    ///
    /// This enables both agent job execution (using job JWT) and
    /// coordinator direct access (using coordinator-created JWT)
    async fn get_jwt_for_operation(&self, app_instance: &str) -> Result<String> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Try to get JWT from current job context (if we're running inside a job)
        if let Ok((ctx_app, ctx_sequence)) = CURRENT_JOB_CONTEXT.try_with(|ctx| ctx.clone()) {
            // Only use job context if it matches the requested app instance
            if ctx_app == app_instance {
                // Fetch job to get its JWT (job was created with JWT by coordinator)
                if let Ok(Some(job)) = self.fetch_job_by_sequence(&ctx_app, ctx_sequence).await {
                    if let Some(jwt) = job.agent_jwt {
                        // Validate JWT hasn't expired
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        let is_expired = job.jwt_expires_at
                            .map(|exp| now >= exp)
                            .unwrap_or(false);

                        if !is_expired {
                            debug!("Using job-embedded JWT for job {}", ctx_sequence);
                            return Ok(jwt);
                        } else {
                            debug!("Job JWT expired, falling back to coordinator JWT");
                        }
                    }
                }
            }
        }

        // Not in job context OR job JWT missing/expired
        // Fall back to coordinator-created JWT
        debug!("Using coordinator-created JWT for app {}", app_instance);
        self.get_or_create_jwt(app_instance).await
    }

    /// Parse method name in format "developer/agent/method" into components
    fn parse_method_name(&self, method_name: &str) -> (String, String, String) {
        let parts: Vec<&str> = method_name.split('/').collect();
        match parts.as_slice() {
            [developer, agent, method] => (
                developer.to_string(),
                agent.to_string(),
                method.to_string(),
            ),
            _ => (
                "default".to_string(),
                "default".to_string(),
                method_name.to_string(),
            ),
        }
    }

    /// Extract JWT from job data if present
    /// Returns (jwt, expires_at, remaining_data)
    fn extract_jwt_from_data(&self, data: &[u8]) -> Result<(Option<String>, Option<u64>, Vec<u8>)> {
        // For now, return None for JWT - this will be enhanced in Phase 3
        // when we implement proper JWT extraction logic
        Ok((None, None, data.to_vec()))
    }

    /// Convert proto Job to trait Job type
    fn proto_job_to_trait_job(&self, proto_job: proto::silvana::state::v1::Job) -> Job {
        use silvana_coordination_trait::JobStatus;

        // Convert proto JobStatus enum (i32) to trait JobStatus
        let status = match proto_job.status {
            0 => JobStatus::Pending,
            1 => JobStatus::Running,
            2 => JobStatus::Completed,
            3 => JobStatus::Failed(proto_job.error_message.clone().unwrap_or_default()),
            _ => JobStatus::Pending,
        };

        // Convert next_scheduled_at from Timestamp to Option<u64>
        let next_scheduled_at = proto_job.next_scheduled_at.map(|ts| ts.seconds as u64 * 1000);

        // Convert created_at and updated_at from Timestamp to u64
        let created_at = proto_job.created_at.map(|ts| ts.seconds as u64 * 1000).unwrap_or(0);
        let updated_at = proto_job.updated_at.map(|ts| ts.seconds as u64 * 1000).unwrap_or(0);

        // Extract JWT from proto Job (it's stored there during creation)
        let agent_jwt = proto_job.agent_jwt;
        let jwt_expires_at = proto_job.jwt_expires_at.map(|ts| ts.seconds as u64);

        Job {
            job_sequence: proto_job.job_sequence,
            description: proto_job.description,
            developer: proto_job.developer,
            agent: proto_job.agent,
            agent_method: proto_job.agent_method,
            app: "".to_string(), // Not available in proto - would need separate query
            app_instance: proto_job.app_instance_id,
            app_instance_method: "".to_string(), // Not available in proto
            block_number: proto_job.block_number,
            sequences: if proto_job.sequences.is_empty() { None } else { Some(proto_job.sequences) },
            sequences1: if proto_job.sequences1.is_empty() { None } else { Some(proto_job.sequences1) },
            sequences2: if proto_job.sequences2.is_empty() { None } else { Some(proto_job.sequences2) },
            data: proto_job.data.unwrap_or_default(),
            status,
            attempts: proto_job.attempts as u8,
            interval_ms: proto_job.interval_ms,
            next_scheduled_at,
            created_at,
            updated_at,
            agent_jwt,
            jwt_expires_at,
        }
    }

    /// Execute a closure with job context set for JWT-based agent operations
    /// This enables agent operations to fetch the JWT from the job on-demand
    pub async fn with_job_context<F, T>(&self, app_instance: &str, job_sequence: u64, f: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        CURRENT_JOB_CONTEXT
            .scope((app_instance.to_string(), job_sequence), f)
            .await
    }

    /// Get JWT from the current job context
    /// This fetches the job and extracts the JWT on-demand - JWT is never stored
    ///
    /// # Deprecated
    /// Use `get_jwt_for_operation()` instead for better fallback handling
    #[allow(dead_code)]
    async fn get_current_job_jwt(&self) -> Result<String> {
        // Get current job context
        let (app_instance, job_sequence) = CURRENT_JOB_CONTEXT
            .try_with(|ctx| ctx.clone())
            .map_err(|_| PrivateCoordinationError::InvalidInput(
                "No job context set. Agent operations must be called within with_job_context()".to_string()
            ))?;

        // Fetch the job to get JWT
        let job = self.fetch_job_by_sequence(&app_instance, job_sequence).await?
            .ok_or_else(|| PrivateCoordinationError::NotFound(
                format!("Job {} not found for app instance {}", job_sequence, app_instance)
            ))?;

        // Extract and validate JWT
        let jwt = job.agent_jwt.ok_or_else(|| PrivateCoordinationError::InvalidInput(
            format!("Job {} has no agent JWT", job_sequence)
        ))?;

        // Check JWT expiration if provided
        if let Some(expires_at) = job.jwt_expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if now >= expires_at {
                return Err(PrivateCoordinationError::InvalidInput(
                    format!("JWT for job {} has expired", job_sequence)
                ));
            }
        }

        Ok(jwt)
    }

    /// Create coordinator authentication for an operation
    fn create_coordinator_auth(
        &self,
        operation: &str,
        app_instance: &str,
    ) -> proto::silvana::state::v1::CoordinatorAuth {
        self.auth.sign_request(app_instance, operation)
    }

    /// Convert proto State response to trait SequenceState
    fn proto_state_to_sequence_state(&self, response: proto::silvana::state::v1::GetStateResponse) -> Option<SequenceState> {
        use proto::silvana::state::v1::get_state_response::State as ProtoState;

        match response.state? {
            ProtoState::UserAction(_user_action) => {
                // UserAction doesn't map to SequenceState
                None
            }
            ProtoState::OptimisticState(opt_state) => {
                Some(SequenceState {
                    sequence: opt_state.sequence,
                    state: None,
                    data_availability: opt_state.state_da,
                    optimistic_state: opt_state.state_data,
                    transition_data: opt_state.transition_data.unwrap_or_default(),
                })
            }
            ProtoState::ProvedState(proved_state) => {
                Some(SequenceState {
                    sequence: proved_state.sequence,
                    state: proved_state.state_data,
                    data_availability: proved_state.state_da,
                    optimistic_state: Vec::new(),
                    transition_data: Vec::new(),
                })
            }
        }
    }

    /// Convert proto Block to trait Block
    fn proto_block_to_trait_block(&self, proto_block: proto::silvana::state::v1::Block) -> Block {
        Block {
            name: format!("block-{}", proto_block.block_number),
            block_number: proto_block.block_number,
            start_sequence: proto_block.start_sequence,
            end_sequence: proto_block.end_sequence,
            actions_commitment: proto_block.actions_commitment.unwrap_or_default(),
            state_commitment: proto_block.state_commitment.unwrap_or_default(),
            time_since_last_block: proto_block.time_since_last_block.unwrap_or(0),
            number_of_transactions: proto_block.number_of_transactions,
            start_actions_commitment: proto_block.start_actions_commitment.unwrap_or_default(),
            end_actions_commitment: proto_block.end_actions_commitment.unwrap_or_default(),
            state_data_availability: proto_block.state_data_availability,
            proof_data_availability: proto_block.proof_data_availability,
            created_at: proto_block
                .created_at
                .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000)
                .unwrap_or(0),
            state_calculated_at: proto_block
                .state_calculated_at
                .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000),
            proved_at: proto_block
                .proved_at
                .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000),
        }
    }

    /// Convert proto ProofCalculation to trait ProofCalculation
    fn proto_proof_calculation_to_trait(
        &self,
        proto_calc: proto::silvana::state::v1::ProofCalculation,
    ) -> ProofCalculation {
        ProofCalculation {
            id: proto_calc.id,
            block_number: proto_calc.block_number,
            start_sequence: proto_calc.start_sequence,
            end_sequence: proto_calc.end_sequence,
            proofs: proto_calc
                .proofs
                .into_iter()
                .map(|p| self.proto_proof_to_trait(p))
                .collect(),
            block_proof: proto_calc.block_proof,
            block_proof_submitted: proto_calc.block_proof_submitted,
        }
    }

    /// Convert proto ProofInCalculation to trait Proof
    fn proto_proof_to_trait(
        &self,
        proto_proof: proto::silvana::state::v1::ProofInCalculation,
    ) -> Proof {
        let status = match proto_proof.status {
            1 => ProofStatus::Started,
            2 => ProofStatus::Calculated,
            3 => ProofStatus::Rejected,
            4 => ProofStatus::Reserved,
            5 => ProofStatus::Used,
            _ => ProofStatus::Started, // Default fallback
        };

        Proof {
            status,
            da_hash: proto_proof.da_hash,
            sequence1: if proto_proof.sequence1.is_empty() {
                None
            } else {
                Some(proto_proof.sequence1)
            },
            sequence2: if proto_proof.sequence2.is_empty() {
                None
            } else {
                Some(proto_proof.sequence2)
            },
            rejected_count: proto_proof.rejected_count as u16,
            timestamp: proto_proof.timestamp,
            prover: proto_proof.prover,
            user: proto_proof.user,
            job_id: proto_proof.job_id.unwrap_or_default(),
            sequences: proto_proof.sequences,
        }
    }

    /// Convert proto BlockSettlement to trait BlockSettlement
    fn proto_block_settlement_to_trait(
        &self,
        proto: proto::silvana::state::v1::BlockSettlement,
    ) -> BlockSettlement {
        BlockSettlement {
            block_number: proto.block_number,
            settlement_tx_hash: proto.settlement_tx_hash,
            settlement_tx_included_in_block: proto.settlement_tx_included_in_block.is_some(),
            sent_to_settlement_at: proto.sent_to_settlement_at.map(|ts| {
                (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000
            }),
            settled_at: proto.settled_at.map(|ts| {
                (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000
            }),
        }
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

    async fn fetch_pending_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_pending_jobs")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetPendingJobs(
                proto::silvana::state::v1::GetPendingJobsOperation {},
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.jobs.into_iter().map(|j| self.proto_job_to_trait_job(j)).collect())
    }

    async fn fetch_failed_jobs(&self, app_instance: &str) -> Result<Vec<Job>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_failed_jobs")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetFailedJobs(
                proto::silvana::state::v1::GetFailedJobsOperation {},
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.jobs.into_iter().map(|j| self.proto_job_to_trait_job(j)).collect())
    }

    async fn get_failed_jobs_count(&self, app_instance: &str) -> Result<u64> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_failed_count")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetFailedCount(
                proto::silvana::state::v1::GetFailedJobsCountOperation {},
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.count.unwrap_or(0))
    }

    async fn fetch_job_by_sequence(&self, app_instance: &str, job_sequence: u64) -> Result<Option<Job>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Get(
                proto::silvana::state::v1::GetJobOperation { job_sequence },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.job.map(|j| self.proto_job_to_trait_job(j)))
    }

    async fn get_pending_jobs_count(&self, app_instance: &str) -> Result<u64> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_pending_count")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetPendingCount(
                proto::silvana::state::v1::GetPendingJobsCountOperation {},
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.count.unwrap_or(0))
    }

    async fn get_total_jobs_count(&self, app_instance: &str) -> Result<u64> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_total_count")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetTotalCount(
                proto::silvana::state::v1::GetTotalJobsCountOperation {},
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.count.unwrap_or(0))
    }

    async fn get_settlement_job_sequences(&self, _app_instance: &str) -> Result<HashMap<String, u64>> {
        // Settlement operations not accessible to coordinators
        Err(PrivateCoordinationError::NotImplemented("Settlement operations not accessible by coordinators".to_string()))
    }

    async fn get_jobs_info(&self, _app_instance: &str) -> Result<Option<(String, String)>> {
        // Layer-specific metadata not accessible to coordinators
        Err(PrivateCoordinationError::NotImplemented("Layer-specific metadata not accessible by coordinators".to_string()))
    }

    async fn fetch_jobs_batch(&self, app_instance: &str, job_sequences: &[u64]) -> Result<Vec<Job>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_jobs_batch")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetJobsBatch(
                proto::silvana::state::v1::GetJobsBatchOperation {
                    job_sequences: job_sequences.to_vec(),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.jobs.into_iter().map(|j| self.proto_job_to_trait_job(j)).collect())
    }

    async fn fetch_pending_job_sequences(&self, app_instance: &str) -> Result<Vec<u64>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_pending_sequences")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetPendingSequences(
                proto::silvana::state::v1::GetPendingJobSequencesOperation {
                    developer: None,
                    agent: None,
                    agent_method: None,
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.job_sequences)
    }

    async fn fetch_pending_job_sequences_by_method(
        &self,
        app_instance: &str,
        developer: &str,
        agent: &str,
        agent_method: &str,
    ) -> Result<Vec<u64>> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "get_pending_sequences")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::GetPendingSequences(
                proto::silvana::state::v1::GetPendingJobSequencesOperation {
                    developer: Some(developer.to_string()),
                    agent: Some(agent.to_string()),
                    agent_method: Some(agent_method.to_string()),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.job_sequences)
    }

    async fn start_job(&self, app_instance: &str, job_sequence: u64) -> Result<bool> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "start_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Start(
                proto::silvana::state::v1::StartJobOperation { job_sequence },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        Ok(response.success)
    }

    async fn complete_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "complete_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Complete(
                proto::silvana::state::v1::CompleteJobOperation { job_sequence },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to complete job: {}",
                response.message
            )));
        }

        Ok(format!("private-complete-{}", job_sequence))
    }

    async fn fail_job(&self, app_instance: &str, job_sequence: u64, error: &str) -> Result<Self::TransactionHash> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "fail_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Fail(
                proto::silvana::state::v1::FailJobOperation {
                    job_sequence,
                    error_message: error.to_string(),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to fail job: {}",
                response.message
            )));
        }

        Ok(format!("private-fail-{}", job_sequence))
    }

    async fn terminate_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash> {
        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "terminate_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Terminate(
                proto::silvana::state::v1::TerminateJobOperation { job_sequence },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to terminate job: {}",
                response.message
            )));
        }

        Ok(format!("private-terminate-{}", job_sequence))
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
        _next_scheduled_at: Option<u64>,
        _settlement_chain: Option<String>,
    ) -> Result<(Self::TransactionHash, u64)> {
        let (developer, agent, agent_method) = self.parse_method_name(&method_name);
        let (agent_jwt, jwt_expires_at, job_data) = self.extract_jwt_from_data(&data)?;

        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "create_job")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::Create(
                proto::silvana::state::v1::CreateJobOperation {
                    description: job_description,
                    developer,
                    agent,
                    agent_method,
                    block_number,
                    sequences: sequences.unwrap_or_default(),
                    sequences1: sequences1.unwrap_or_default(),
                    sequences2: sequences2.unwrap_or_default(),
                    data: Some(job_data),
                    data_da: None,
                    interval_ms,
                    agent_jwt,
                    jwt_expires_at: jwt_expires_at.map(|ts| prost_types::Timestamp {
                        seconds: ts as i64,
                        nanos: 0,
                    }),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to create job: {}",
                response.message
            )));
        }

        let job_sequence = response.job_sequence.ok_or_else(|| {
            PrivateCoordinationError::Other(anyhow::anyhow!("No job sequence returned"))
        })?;

        Ok((format!("private-job-{}", job_sequence), job_sequence))
    }

    async fn create_merge_job_with_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        // Use coordinator Ed25519 auth (NOT JWT)
        let coordinator_auth = self.create_coordinator_auth("create_merge_job", app_instance);

        let operation = proto::silvana::state::v1::coordinator_job_request::Operation::CreateMergeJob(
            proto::silvana::state::v1::CreateMergeJobOperation {
                block_number,
                sequences,
                sequences1,
                sequences2,
                job_description,
            },
        );

        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(coordinator_auth),
            app_instance_id: app_instance.to_string(),
            operation: Some(operation),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if response.success {
            Ok(response.job_sequence.unwrap_or(0).to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to create merge job: {}",
                response.message
            )))
        }
    }

    async fn create_settle_job(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<Self::TransactionHash> {
        // Use coordinator Ed25519 auth (NOT JWT)
        let coordinator_auth = self.create_coordinator_auth("create_settle_job", app_instance);

        let operation = proto::silvana::state::v1::coordinator_job_request::Operation::CreateSettleJob(
            proto::silvana::state::v1::CreateSettleJobOperation {
                block_number,
                chain,
                job_description,
            },
        );

        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(coordinator_auth),
            app_instance_id: app_instance.to_string(),
            operation: Some(operation),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if response.success {
            Ok(response.job_sequence.unwrap_or(0).to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to create settle job: {}",
                response.message
            )))
        }
    }

    async fn terminate_app_job(&self, app_instance: &str, job_sequence: u64) -> Result<Self::TransactionHash> {
        // terminate_app_job is semantically the same as terminate_job for coordinators
        self.terminate_job(app_instance, job_sequence).await
    }

    async fn restart_failed_jobs(
        &self,
        app_instance: &str,
        job_sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        let count = job_sequences.as_ref().map(|s| s.len()).unwrap_or(0);

        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "restart_failed_jobs")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::RestartFailedJobs(
                proto::silvana::state::v1::RestartFailedJobsOperation {
                    job_sequences: job_sequences.unwrap_or_default(),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to restart jobs: {}",
                response.message
            )));
        }

        Ok(format!("private-restart-{}", count))
    }

    async fn remove_failed_jobs(
        &self,
        app_instance: &str,
        sequences: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        let count = sequences.as_ref().map(|s| s.len()).unwrap_or(0);

        let request = proto::silvana::state::v1::CoordinatorJobRequest {
            coordinator_auth: Some(self.auth.sign_request(app_instance, "remove_failed_jobs")),
            app_instance_id: app_instance.to_string(),
            operation: Some(proto::silvana::state::v1::coordinator_job_request::Operation::RemoveFailedJobs(
                proto::silvana::state::v1::RemoveFailedJobsOperation {
                    job_sequences: sequences.unwrap_or_default(),
                },
            )),
        };

        let response = self
            .client
            .clone()
            .coordinator_job_operation(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to remove jobs: {}",
                response.message
            )));
        }

        Ok(format!("private-remove-{}", count))
    }

    // ===== Sequence State Management =====

    async fn fetch_sequence_state(&self, app_instance: &str, sequence: u64) -> Result<Option<SequenceState>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetStateRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            sequence,
            state_type: 3, // STATE_TYPE_PROVED
        };

        let response = self
            .client
            .clone()
            .get_state(request)
            .await?
            .into_inner();

        Ok(self.proto_state_to_sequence_state(response))
    }

    async fn fetch_sequence_states_range(
        &self,
        app_instance: &str,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<Vec<SequenceState>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetStateRangeRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            from_sequence,
            to_sequence,
            include_proofs: false,
        };

        let response = self
            .client
            .clone()
            .get_state_range(request)
            .await?
            .into_inner();

        // Convert proved states to SequenceState
        let states = response.proved_states.into_iter()
            .map(|proved_state| SequenceState {
                sequence: proved_state.sequence,
                state: proved_state.state_data,
                data_availability: proved_state.state_da,
                optimistic_state: Vec::new(),
                transition_data: Vec::new(),
            })
            .collect();

        Ok(states)
    }

    async fn get_current_sequence(&self, app_instance: &str) -> Result<u64> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetLatestStateRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_latest_state(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::NotFound(
                format!("No state found for app instance {}", app_instance)
            ));
        }

        let state = response.state.ok_or_else(|| PrivateCoordinationError::NotFound(
            format!("No state in response for app instance {}", app_instance)
        ))?;

        Ok(state.sequence)
    }

    async fn update_state_for_sequence(
        &self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::UpdateStateForSequenceRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            sequence,
            new_state_data,
            new_data_availability_hash,
        };

        let response = self
            .client
            .clone()
            .update_state_for_sequence(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to update state: {}",
                response.message
            )))
        }
    }

    // ===== Block Management =====

    async fn fetch_block(&self, app_instance: &str, block_number: u64) -> Result<Option<Block>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetBlockRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
        };

        let response = self
            .client
            .clone()
            .get_block(request)
            .await?
            .into_inner();

        Ok(response.block.map(|b| self.proto_block_to_trait_block(b)))
    }

    async fn fetch_blocks_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Block>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetBlocksRangeRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            from_block,
            to_block,
        };

        let response = self
            .client
            .clone()
            .get_blocks_range(request)
            .await?
            .into_inner();

        Ok(response
            .blocks
            .into_iter()
            .map(|b| self.proto_block_to_trait_block(b))
            .collect())
    }

    async fn try_create_block(&self, app_instance: &str) -> Result<Option<u64>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::TryCreateBlockRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .try_create_block(request)
            .await?
            .into_inner();

        Ok(response.block_number)
    }

    async fn update_block_state_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        state_da: String,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::UpdateBlockStateDaRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            state_da,
        };

        let response = self
            .client
            .clone()
            .update_block_state_da(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to update block state DA: {}",
                response.message
            )))
        }
    }

    async fn update_block_proof_data_availability(
        &self,
        app_instance: &str,
        block_number: u64,
        proof_da: String,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::UpdateBlockProofDaRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            proof_da,
        };

        let response = self
            .client
            .clone()
            .update_block_proof_da(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to update block proof DA: {}",
                response.message
            )))
        }
    }

    // ===== Proof Management =====

    async fn fetch_proof_calculation(
        &self,
        app_instance: &str,
        block_number: u64,
    ) -> Result<Option<ProofCalculation>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetProofCalculationRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
        };

        let response = self
            .client
            .clone()
            .get_proof_calculation(request)
            .await?
            .into_inner();

        Ok(response
            .proof_calculation
            .map(|pc| self.proto_proof_calculation_to_trait(pc)))
    }

    async fn fetch_proof_calculations_range(
        &self,
        app_instance: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<ProofCalculation>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetProofCalculationsRangeRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            from_block,
            to_block,
        };

        let response = self
            .client
            .clone()
            .get_proof_calculations_range(request)
            .await?
            .into_inner();

        Ok(response
            .proof_calculations
            .into_iter()
            .map(|pc| self.proto_proof_calculation_to_trait(pc))
            .collect())
    }

    async fn start_proving(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::StartProvingRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            sequences,
            merged_sequences_1: merged_sequences_1.unwrap_or_default(),
            merged_sequences_2: merged_sequences_2.unwrap_or_default(),
        };

        let response = self
            .client
            .clone()
            .start_proving(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to start proving: {}",
                response.message
            )))
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
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::SubmitCalculatedProofRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            sequences,
            merged_sequences_1: merged_sequences_1.unwrap_or_default(),
            merged_sequences_2: merged_sequences_2.unwrap_or_default(),
            job_id,
            da_hash,
            cpu_cores: cpu_cores as u32,
            prover_architecture,
            prover_memory,
            cpu_time,
        };

        let response = self
            .client
            .clone()
            .submit_calculated_proof(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to submit proof: {}",
                response.message
            )))
        }
    }

    async fn reject_proof(
        &self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::RejectProofRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            sequences,
        };

        let response = self
            .client
            .clone()
            .reject_proof(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to reject proof: {}",
                response.message
            )))
        }
    }

    // ===== Settlement =====

    async fn fetch_block_settlement(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: &str,
    ) -> Result<Option<BlockSettlement>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetBlockSettlementRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            chain: chain.to_string(),
        };

        match self
            .client
            .clone()
            .get_block_settlement(request)
            .await
        {
            Ok(response) => {
                let inner = response.into_inner();
                Ok(inner.block_settlement.map(|bs| self.proto_block_settlement_to_trait(bs)))
            }
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn get_settlement_chains(&self, app_instance: &str) -> Result<Vec<String>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetSettlementChainsRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_settlement_chains(request)
            .await?
            .into_inner();

        Ok(response.chains)
    }

    async fn get_settlement_address(&self, app_instance: &str, chain: &str) -> Result<Option<String>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetSettlementAddressRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            chain: chain.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_settlement_address(request)
            .await?
            .into_inner();

        Ok(response.settlement_address)
    }

    async fn get_settlement_job_for_chain(&self, app_instance: &str, chain: &str) -> Result<Option<u64>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetSettlementJobForChainRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            chain: chain.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_settlement_job_for_chain(request)
            .await?
            .into_inner();

        Ok(response.settlement_job)
    }

    async fn set_settlement_address(
        &self,
        app_instance: &str,
        chain: String,
        address: Option<String>,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::SetSettlementAddressRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            chain,
            settlement_address: address,
        };

        let response = self
            .client
            .clone()
            .set_settlement_address(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to set settlement address: {}",
                response.message
            )))
        }
    }

    async fn update_block_settlement_tx_hash(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let sent_to_settlement_at = prost_types::Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        };

        let request = proto::silvana::state::v1::UpdateBlockSettlementTxHashRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            chain,
            settlement_tx_hash: Some(settlement_tx_hash),
            sent_to_settlement_at: Some(sent_to_settlement_at),
        };

        let response = self
            .client
            .clone()
            .update_block_settlement_tx_hash(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to update block settlement tx hash: {}",
                response.message
            )))
        }
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let settled_at_ts = prost_types::Timestamp {
            seconds: (settled_at / 1000) as i64,
            nanos: ((settled_at % 1000) * 1_000_000) as i32,
        };

        let request = proto::silvana::state::v1::UpdateBlockSettlementTxIncludedInBlockRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            block_number,
            chain,
            settlement_tx_included_in_block: Some(block_number), // Use block_number as the settlement block
            settled_at: Some(settled_at_ts),
        };

        let response = self
            .client
            .clone()
            .update_block_settlement_tx_included_in_block(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to update block settlement tx included: {}",
                response.message
            )))
        }
    }

    // ===== Key-Value Storage =====

    async fn get_kv_string(&self, app_instance: &str, key: &str) -> Result<Option<String>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetKvStringRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_kv_string(request)
            .await?
            .into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.entry.map(|e| e.value))
    }

    async fn get_all_kv_string(&self, app_instance: &str) -> Result<HashMap<String, String>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        // List all keys (no prefix filter, no limit)
        let keys_request = proto::silvana::state::v1::ListKvStringKeysRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt.clone() }),
            app_instance_id: app_instance.to_string(),
            prefix: None,
            limit: None,
        };

        let keys_response = self
            .client
            .clone()
            .list_kv_string_keys(keys_request)
            .await?
            .into_inner();

        let mut result = HashMap::new();

        // Fetch each key's value
        for key in keys_response.keys {
            let get_request = proto::silvana::state::v1::GetKvStringRequest {
                auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt.clone() }),
                app_instance_id: app_instance.to_string(),
                key: key.clone(),
            };

            let get_response = self
                .client
                .clone()
                .get_kv_string(get_request)
                .await?
                .into_inner();

            if let Some(entry) = get_response.entry {
                result.insert(key, entry.value);
            }
        }

        Ok(result)
    }

    async fn list_kv_string_keys(
        &self,
        app_instance: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<String>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::ListKvStringKeysRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            prefix: prefix.map(|s| s.to_string()),
            limit: limit.map(|l| l as i32),
        };

        let response = self
            .client
            .clone()
            .list_kv_string_keys(request)
            .await?
            .into_inner();

        Ok(response.keys)
    }

    async fn set_kv_string(&self, app_instance: &str, key: String, value: String) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::SetKvStringRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.clone(),
            value,
        };

        let response = self
            .client
            .clone()
            .set_kv_string(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to set KV string: {}",
                response.message
            )));
        }

        Ok(format!("private-kv-set-{}", key))
    }

    async fn delete_kv_string(&self, app_instance: &str, key: &str) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::DeleteKvStringRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.to_string(),
        };

        let response = self
            .client
            .clone()
            .delete_kv_string(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to delete KV string: {}",
                response.message
            )));
        }

        Ok(format!("private-kv-delete-{}", key))
    }

    async fn get_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetKvBinaryRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.to_vec(),
        };

        let response = self
            .client
            .clone()
            .get_kv_binary(request)
            .await?
            .into_inner();

        if !response.success {
            return Ok(None);
        }

        Ok(response.entry.map(|e| e.value))
    }

    async fn list_kv_binary_keys(
        &self,
        app_instance: &str,
        prefix: Option<&[u8]>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<u8>>> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::ListKvBinaryKeysRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            prefix: prefix.map(|p| p.to_vec()),
            limit: limit.map(|l| l as i32),
        };

        let response = self
            .client
            .clone()
            .list_kv_binary_keys(request)
            .await?
            .into_inner();

        Ok(response.keys)
    }

    async fn set_kv_binary(&self, app_instance: &str, key: Vec<u8>, value: Vec<u8>) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::SetKvBinaryRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.clone(),
            value,
            value_da: None,
        };

        let response = self
            .client
            .clone()
            .set_kv_binary(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to set KV binary: {}",
                response.message
            )));
        }

        Ok(format!("private-kv-set-binary-{}", hex::encode(&key)))
    }

    async fn delete_kv_binary(&self, app_instance: &str, key: &[u8]) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::DeleteKvBinaryRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            key: key.to_vec(),
        };

        let response = self
            .client
            .clone()
            .delete_kv_binary(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to delete KV binary: {}",
                response.message
            )));
        }

        Ok(format!("private-kv-delete-binary-{}", hex::encode(key)))
    }

    // ===== Metadata Management =====

    async fn get_metadata(&self, _app_instance: &str, _key: &str) -> Result<Option<String>> {
        // Metadata operations are application-level and not exposed via gRPC
        // Metadata should be accessed through AppInstance or KV storage instead
        Err(PrivateCoordinationError::NotImplemented(
            "get_metadata - use fetch_app_instance or KV storage".to_string()
        ))
    }

    async fn get_all_metadata(&self, _app_instance: &str) -> Result<HashMap<String, String>> {
        // Metadata operations are application-level and not exposed via gRPC
        // Metadata should be accessed through AppInstance or KV storage instead
        Err(PrivateCoordinationError::NotImplemented(
            "get_all_metadata - use fetch_app_instance or KV storage".to_string()
        ))
    }

    async fn add_metadata(&self, _app_instance: &str, _key: String, _value: String) -> Result<Self::TransactionHash> {
        // Metadata operations are application-level and not exposed via gRPC
        // Metadata should be accessed through AppInstance or KV storage instead
        Err(PrivateCoordinationError::NotImplemented(
            "add_metadata - use KV storage".to_string()
        ))
    }

    // ===== App Instance Data =====

    async fn fetch_app_instance(&self, app_instance: &str) -> Result<AppInstance> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetAppInstanceRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_app_instance(request)
            .await?
            .into_inner();

        let proto_app = response.app_instance.ok_or_else(|| {
            PrivateCoordinationError::NotFound(format!("App instance {} not found", app_instance))
        })?;

        // Convert proto AppInstance metadata - it's a Struct which we can't easily convert
        // For now, return empty metadata - full metadata would require proto updates
        let metadata = HashMap::new();

        Ok(AppInstance {
            id: proto_app.app_instance_id,
            silvana_app_name: proto_app.name,
            description: proto_app.description,
            metadata,
            kv: HashMap::new(), // KV is accessed separately via KV operations
            sequence: 0, // Sequence needs separate query (get_current_sequence)
            admin: proto_app.owner,
            block_number: 0, // Block number needs separate query
            previous_block_timestamp: 0, // Needs separate query
            previous_block_last_sequence: 0, // Needs separate query
            last_proved_block_number: 0, // Needs separate query
            last_settled_block_number: 0, // Needs separate query
            last_settled_sequence: 0, // Needs separate query
            last_purged_sequence: 0, // Needs separate query
            settlements: HashMap::new(), // Settlements need separate query
            is_paused: false, // Pause status needs separate query
            min_time_between_blocks: 0, // Needs separate query
            created_at: proto_app.created_at.map(|ts| ts.seconds as u64 * 1000).unwrap_or(0),
            updated_at: proto_app.updated_at.map(|ts| ts.seconds as u64 * 1000).unwrap_or(0),
        })
    }

    async fn get_app_instance_admin(&self, app_instance: &str) -> Result<String> {
        let app = self.fetch_app_instance(app_instance).await?;
        Ok(app.admin)
    }

    async fn is_app_paused(&self, app_instance: &str) -> Result<bool> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::IsAppPausedRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .is_app_paused(request)
            .await?
            .into_inner();

        Ok(response.is_paused)
    }

    async fn get_min_time_between_blocks(&self, app_instance: &str) -> Result<u64> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::GetMinTimeBetweenBlocksRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
        };

        let response = self
            .client
            .clone()
            .get_min_time_between_blocks(request)
            .await?
            .into_inner();

        Ok(response.min_time_seconds)
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

    async fn purge(&self, app_instance: &str, sequences_to_purge: u64) -> Result<Self::TransactionHash> {
        let jwt = self.get_jwt_for_operation(app_instance).await?;

        let request = proto::silvana::state::v1::PurgeRequest {
            auth: Some(proto::silvana::state::v1::JwtAuth { token: jwt }),
            app_instance_id: app_instance.to_string(),
            sequences_to_purge,
        };

        let response = self
            .client
            .clone()
            .purge(request)
            .await?
            .into_inner();

        if response.success {
            Ok("success".to_string())
        } else {
            Err(PrivateCoordinationError::Other(anyhow::anyhow!(
                "Failed to purge: {}",
                response.message
            )))
        }
    }

    // ===== Event Streaming =====

    async fn event_stream(&self) -> Result<EventStream> {
        use proto::silvana::state::v1::{SubscribeJobEventsRequest, JwtAuth};

        // TODO: Temporarily using empty JWT and app_instance_id for development
        // Will add proper JWT auth and app_instance_id filtering later
        let request = SubscribeJobEventsRequest {
            auth: Some(JwtAuth {
                token: String::new(), // Empty JWT - server will skip auth check temporarily
            }),
            app_instance_id: String::new(), // Empty = all app instances
            from_job_sequence: None, // Start from current
        };

        let mut client = self.client.clone();
        let response = client
            .subscribe_job_events(request)
            .await?  // Will convert tonic::Status to PrivateCoordinationError::Status
            .into_inner();

        // Convert proto stream to EventStream
        let stream = response.map(|result| {
            result
                .map(|event| {
                    let job = event.job.unwrap_or_default();
                    JobCreatedEvent {
                        job_sequence: event.job_sequence,
                        developer: job.developer,
                        agent: job.agent,
                        agent_method: job.agent_method,
                        app_instance: job.app_instance_id.clone(),
                        app_instance_method: String::new(), // Not available in private coordination
                        block_number: job.block_number.unwrap_or(0),
                        created_at: job.created_at
                            .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000)
                            .unwrap_or(0),
                    }
                })
                .map_err(|e| Box::new(PrivateCoordinationError::Status(e)) as Box<dyn std::error::Error + Send + Sync>)
        });

        Ok(Box::pin(stream))
    }
}
