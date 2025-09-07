use crate::constants::{WALRUS_QUILT_BLOCK_INTERVAL, WALRUS_TEST};
use crate::proofs_storage::{ProofMetadata, ProofStorage};
use crate::state::SharedState;
use monitoring::coordinator_metrics;
use std::path::Path;
use sui::fetch::block::fetch_block_info;
use tokio::net::UnixListener;
use tokio::time::Instant;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{Request, Response, Status, transport::Server};
use tracing::{debug, error, info, warn};

pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::{
    AddMetadataRequest, AddMetadataResponse, Block, BlockSettlement, CompleteJobRequest,
    CompleteJobResponse, CreateAppJobRequest, CreateAppJobResponse, DeleteKvRequest,
    DeleteKvResponse, FailJobRequest, FailJobResponse, GetBlockProofRequest, GetBlockProofResponse,
    GetBlockRequest, GetBlockResponse, GetBlockSettlementRequest, GetBlockSettlementResponse,
    GetJobRequest, GetJobResponse, GetKvRequest, GetKvResponse, GetMetadataRequest,
    GetMetadataResponse, GetProofRequest, GetProofResponse, GetSequenceStatesRequest,
    GetSequenceStatesResponse, Job, Metadata, ReadDataAvailabilityRequest,
    ReadDataAvailabilityResponse, RejectProofRequest, RejectProofResponse, RetrieveSecretRequest,
    RetrieveSecretResponse, SequenceState, SetKvRequest, SetKvResponse, SubmitProofRequest,
    SubmitProofResponse, SubmitStateRequest, SubmitStateResponse, TerminateJobRequest,
    TerminateJobResponse, TryCreateBlockRequest, TryCreateBlockResponse,
    UpdateBlockProofDataAvailabilityRequest, UpdateBlockProofDataAvailabilityResponse,
    UpdateBlockSettlementRequest, UpdateBlockSettlementResponse,
    UpdateBlockSettlementTxHashRequest, UpdateBlockSettlementTxHashResponse,
    UpdateBlockSettlementTxIncludedInBlockRequest, UpdateBlockSettlementTxIncludedInBlockResponse,
    UpdateBlockStateDataAvailabilityRequest, UpdateBlockStateDataAvailabilityResponse,
    coordinator_service_server::{CoordinatorService, CoordinatorServiceServer},
};

#[derive(Clone)]
pub struct CoordinatorServiceImpl {
    state: SharedState,
    proof_storage: std::sync::Arc<ProofStorage>,
}

impl CoordinatorServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            proof_storage: std::sync::Arc::new(ProofStorage::new()),
        }
    }
}

#[tonic::async_trait]
impl CoordinatorService for CoordinatorServiceImpl {
    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        // Get buffer stats
        let buffer_count = self.state.get_started_jobs_count().await;

        info!(
            developer = %req.developer,
            agent = %req.agent,
            agent_method = %req.agent_method,
            session_id = %req.session_id,
            buffer_count = buffer_count,
            "📊 GetJob request (buffer: {} jobs)",
            buffer_count
        );

        // First check for session-specific jobs (jobs reserved for this Docker session)
        // Session jobs should ALWAYS be returned, even during shutdown
        'job_search: loop {
            if let Some(agent_job) = self
                .state
                .get_agent_job_db()
                .get_ready_job(
                    &req.developer,
                    &req.agent,
                    &req.agent_method,
                    Some(&req.session_id),
                )
                .await
            {
                debug!(
                    "Found session job for session {}: job_id={}",
                    req.session_id, agent_job.job_id
                );

                // Determine settlement chain if this is a settlement job
                let settlement_chain = if agent_job.pending_job.app_instance_method == "settle" {
                    // Use helper function to get chain by job sequence
                    match sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                        &agent_job.app_instance,
                        agent_job.job_sequence,
                    )
                    .await
                    {
                        Ok(chain) => chain,
                        Err(e) => {
                            warn!(
                                "Failed to lookup settlement chain for job {} in app instance {}: {}",
                                agent_job.job_sequence, agent_job.app_instance, e
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                // If this is a settlement job with no associated chain, terminate it and look for next job
                if agent_job.pending_job.app_instance_method == "settle"
                    && settlement_chain.is_none()
                {
                    error!(
                        "🚨 Settlement job {} has no associated chain - terminating and looking for next job",
                        agent_job.job_sequence
                    );

                    // Add terminate request to multicall queue
                    self.state
                        .add_terminate_job_request(
                            agent_job.app_instance.clone(),
                            agent_job.job_sequence,
                        )
                        .await;

                    // Remove this job from the agent database so we don't pick it again
                    self.state
                        .get_agent_job_db()
                        .terminate_job(&agent_job.job_id)
                        .await;

                    // Continue to look for next job
                    continue 'job_search;
                }

                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetJob: dev={}, agent={}/{}, session_job={}, time={:?}",
                    req.developer, req.agent, req.agent_method, agent_job.job_id, elapsed
                );

                return Ok(Response::new(GetJobResponse {
                    success: true,
                    message: format!("Session job retrieved: {}", agent_job.job_id),
                    job: Some(Job {
                        job_sequence: agent_job.job_sequence,
                        description: agent_job.pending_job.description.clone(),
                        developer: agent_job.developer,
                        agent: agent_job.agent,
                        agent_method: agent_job.agent_method,
                        app: agent_job.pending_job.app.clone(),
                        app_instance: agent_job.app_instance,
                        app_instance_method: agent_job.pending_job.app_instance_method.clone(),
                        block_number: agent_job.pending_job.block_number,
                        sequences: agent_job.pending_job.sequences.unwrap_or_default(),
                        sequences1: agent_job.pending_job.sequences1.unwrap_or_default(),
                        sequences2: agent_job.pending_job.sequences2.unwrap_or_default(),
                        data: agent_job.pending_job.data.clone(),
                        job_id: agent_job.job_id,
                        attempts: agent_job.pending_job.attempts as u32,
                        created_at: agent_job.pending_job.created_at,
                        updated_at: agent_job.pending_job.updated_at,
                        chain: settlement_chain, // Set the settlement chain if this is a settlement job
                    }),
                }));
            }

            // No session-specific job found, break to check buffer
            break 'job_search;
        }

        // Fall back to checking the buffer for started jobs that match this agent
        // The buffer contains jobs that have been started on blockchain via multicall
        if let Some(started_job) = self
            .state
            .get_started_job_for_agent(&req.developer, &req.agent, &req.agent_method)
            .await
        {
            debug!(
                "Found matching started job in buffer: app_instance={}, sequence={}",
                started_job.app_instance, started_job.job_sequence
            );

            // Fetch the app instance to get the jobs table (we know this should work since get_started_job_for_agent already checked)
            let app_instance = match sui::fetch::fetch_app_instance(&started_job.app_instance).await
            {
                Ok(instance) => instance,
                Err(e) => {
                    error!(
                        "Failed to fetch app instance {} (this should not happen): {}",
                        started_job.app_instance, e
                    );
                    return Ok(Response::new(GetJobResponse {
                        success: true,
                        message: "Failed to fetch app instance".to_string(),
                        job: None,
                    }));
                }
            };

            // Get the jobs table ID
            let jobs_table = match &app_instance.jobs {
                Some(jobs) => &jobs.id,
                None => {
                    error!(
                        "App instance {} has no jobs table (this should not happen)",
                        started_job.app_instance
                    );
                    return Ok(Response::new(GetJobResponse {
                        success: true,
                        message: "App instance has no jobs table".to_string(),
                        job: None,
                    }));
                }
            };

            // Fetch the full job details from blockchain using the jobs table
            match sui::fetch::jobs::fetch_job_by_id(jobs_table, started_job.job_sequence).await {
                Ok(Some(pending_job)) => {
                    // We already know this job matches the requesting agent (get_started_job_for_agent checked it)
                    // Store in agent database for tracking
                    let job_id = format!("job_{}", started_job.job_sequence);
                    let agent_job = crate::agent::AgentJob {
                        job_id: job_id.clone(),
                        job_sequence: started_job.job_sequence,
                        app_instance: started_job.app_instance.clone(),
                        developer: pending_job.developer.clone(),
                        agent: pending_job.agent.clone(),
                        agent_method: pending_job.agent_method.clone(),
                        pending_job: pending_job.clone(),
                        memory_requirement: started_job.memory_requirement,
                        sent_at: chrono::Utc::now().timestamp() as u64,
                        start_tx_sent: true, // Already sent via multicall
                        start_tx_hash: None, // Transaction was part of multicall
                    };

                    // Store in agent database
                    self.state
                        .get_agent_job_db()
                        .add_to_pending(agent_job.clone())
                        .await;

                    // Set current agent for this session
                    self.state
                        .set_current_agent(
                            req.session_id.clone(),
                            pending_job.developer.clone(),
                            pending_job.agent.clone(),
                            pending_job.agent_method.clone(),
                        )
                        .await;

                    // Track app_instance for this session

                    // Check if this is a settlement job
                    let settlement_chain = if pending_job.app_instance_method == "settle" {
                        // Use helper function to get chain by job sequence
                        match sui::fetch::app_instance::get_settlement_chain_by_job_sequence(
                            &started_job.app_instance,
                            started_job.job_sequence,
                        )
                        .await
                        {
                            Ok(chain) => chain,
                            Err(e) => {
                                warn!(
                                    "Failed to lookup settlement chain for job {} in app instance {}: {}",
                                    started_job.job_sequence, started_job.app_instance, e
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };

                    // If this is a settlement job with no associated chain, terminate it and return no job
                    if pending_job.app_instance_method == "settle" && settlement_chain.is_none() {
                        error!(
                            "🚨 Settlement job {} (from buffer) has no associated chain - terminating",
                            started_job.job_sequence
                        );

                        // Add terminate request to multicall queue
                        self.state
                            .add_terminate_job_request(
                                started_job.app_instance.clone(),
                                started_job.job_sequence,
                            )
                            .await;

                        // Don't put it back in buffer, just return no job available
                        return Ok(Response::new(GetJobResponse {
                            success: true,
                            message: "Settlement job with no chain terminated".to_string(),
                            job: None,
                        }));
                    }

                    // Convert to protobuf Job
                    let job = Job {
                        job_sequence: started_job.job_sequence,
                        description: pending_job.description.clone(),
                        developer: pending_job.developer.clone(),
                        agent: pending_job.agent.clone(),
                        agent_method: pending_job.agent_method.clone(),
                        app: pending_job.app.clone(),
                        app_instance: started_job.app_instance.clone(),
                        app_instance_method: pending_job.app_instance_method.clone(),
                        block_number: pending_job.block_number,
                        sequences: pending_job.sequences.clone().unwrap_or_default(),
                        sequences1: pending_job.sequences1.clone().unwrap_or_default(),
                        sequences2: pending_job.sequences2.clone().unwrap_or_default(),
                        data: pending_job.data.clone(),
                        job_id,
                        attempts: pending_job.attempts as u32,
                        created_at: pending_job.created_at,
                        updated_at: pending_job.updated_at,
                        chain: settlement_chain,
                    };

                    let elapsed = start_time.elapsed();
                    info!(
                        "✅ GetJob: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, source=buffer, time={:?}",
                        started_job.app_instance,
                        req.developer,
                        req.agent,
                        req.agent_method,
                        started_job.job_sequence,
                        pending_job.app_instance_method,
                        elapsed
                    );

                    return Ok(Response::new(GetJobResponse {
                        success: true,
                        message: format!("Job {} retrieved from buffer", started_job.job_sequence),
                        job: Some(job),
                    }));
                }
                Ok(None) => {
                    warn!(
                        "Started job {} from buffer not found on blockchain, skipping",
                        started_job.job_sequence
                    );
                    // Continue to check if there are more jobs in buffer
                }
                Err(e) => {
                    error!(
                        "Failed to fetch job {} from blockchain: {}",
                        started_job.job_sequence, e
                    );
                    // Continue to check if there are more jobs in buffer
                }
            }
        }

        // If no session jobs and no buffer jobs, check if system is shutting down
        let elapsed = start_time.elapsed();
        let elapsed_ms = elapsed.as_millis() as f64;

        // Record gRPC span for APM
        coordinator_metrics::record_grpc_span("GetJob", elapsed_ms, 200);

        if self.state.is_shutting_down() {
            debug!("System is shutting down and no reserved/buffered jobs available");
            info!(
                "⭕ GetJob: dev={}, agent={}/{}, shutdown_no_jobs, time={:?}",
                req.developer, req.agent, req.agent_method, elapsed
            );
            Ok(Response::new(GetJobResponse {
                success: true,
                message: "System is shutting down".to_string(),
                job: None,
            }))
        } else {
            info!(
                "⭕ GetJob: dev={}, agent={}/{}, no_job_found, time={:?}",
                req.developer, req.agent, req.agent_method, elapsed
            );
            Ok(Response::new(GetJobResponse {
                success: true,
                message: "No pending jobs found".to_string(),
                job: None,
            }))
        }
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            "Received CompleteJob request"
        );

        // Get job from agent database
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("CompleteJob request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(CompleteJobResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "CompleteJob request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(CompleteJobResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                }));
            }
        } else {
            warn!(
                "CompleteJob request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(CompleteJobResponse {
                success: false,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Add complete job request to batch instead of executing immediately
        self.state
            .add_complete_job_request(agent_job.app_instance.clone(), agent_job.job_sequence)
            .await;

        // Remove job from agent database
        let removed_job = self
            .state
            .get_agent_job_db()
            .complete_job(&req.job_id)
            .await;

        if removed_job.is_some() {
            let elapsed = start_time.elapsed();
            info!(
                "✅ CompleteJob (batched): app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, job_id={}, time={}ms",
                agent_job.app_instance,
                agent_job.developer,
                agent_job.agent,
                agent_job.agent_method,
                agent_job.job_sequence,
                agent_job.pending_job.app_instance_method,
                req.job_id,
                elapsed.as_millis()
            );
            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {} marked for completion", agent_job.job_sequence),
            }))
        } else {
            warn!("Job {} not found in agent database", req.job_id);
            Ok(Response::new(CompleteJobResponse {
                success: false,
                message: format!("Job {} not found", req.job_id),
            }))
        }
    }

    async fn fail_job(
        &self,
        request: Request<FailJobRequest>,
    ) -> Result<Response<FailJobResponse>, Status> {
        let req = request.into_inner();

        info!(
            job_id = %req.job_id,
            error = %req.error_message,
            session_id = %req.session_id,
            "Received FailJob request"
        );

        // Get job from agent database
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("FailJob request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(FailJobResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "FailJob request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(FailJobResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                }));
            }
        } else {
            warn!("FailJob request from unknown session: {}", req.session_id);
            return Ok(Response::new(FailJobResponse {
                success: false,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Add fail job request to batch instead of executing immediately
        self.state
            .add_fail_job_request(
                agent_job.app_instance.clone(),
                agent_job.job_sequence,
                req.error_message.clone(),
            )
            .await;

        // Remove job from agent database
        self.state.get_agent_job_db().fail_job(&req.job_id).await;

        info!(
            "Job {} marked for failure (batched): {}",
            req.job_id, req.error_message
        );
        Ok(Response::new(FailJobResponse {
            success: true,
            message: format!("Job {} marked for failure", req.job_id),
        }))
    }

    async fn terminate_job(
        &self,
        request: Request<TerminateJobRequest>,
    ) -> Result<Response<TerminateJobResponse>, Status> {
        let req = request.into_inner();

        // Validate session
        if req.session_id.is_empty() {
            return Ok(Response::new(TerminateJobResponse {
                success: false,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Get job from database
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                return Ok(Response::new(TerminateJobResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                }));
            }
        };

        info!(
            "TerminateJob request for job_id: {} (sequence: {}, app_instance: {})",
            req.job_id, agent_job.job_sequence, agent_job.app_instance
        );

        // Execute terminate_job transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        // Use default gas budget of 0.1 SUI for gRPC calls
        let result = sui_interface
            .terminate_job(&agent_job.app_instance, agent_job.job_sequence, 0.1)
            .await;

        match result {
            Ok(tx_hash) => {
                // Remove job from agent database
                self.state
                    .get_agent_job_db()
                    .terminate_job(&req.job_id)
                    .await;

                info!(
                    "Successfully terminated job {} (tx: {})",
                    req.job_id, tx_hash
                );
                Ok(Response::new(TerminateJobResponse {
                    success: true,
                    message: format!(
                        "Job {} terminated successfully (tx: {})",
                        req.job_id, tx_hash
                    ),
                }))
            }
            Err((error_msg, tx_digest)) => {
                error!(
                    "Failed to terminate job {} on blockchain: {}",
                    req.job_id, error_msg
                );
                let message = if let Some(digest) = tx_digest {
                    format!(
                        "Failed to terminate job {} (tx: {}): {}",
                        req.job_id, digest, error_msg
                    )
                } else {
                    format!("Failed to terminate job {}: {}", req.job_id, error_msg)
                };
                Ok(Response::new(TerminateJobResponse {
                    success: false,
                    message,
                }))
            }
        }
    }

    async fn submit_proof(
        &self,
        request: Request<SubmitProofRequest>,
    ) -> Result<Response<SubmitProofResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        // Get job early to log app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("SubmitProof request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        debug!(
            app_instance = %agent_job.app_instance,
            session_id = %req.session_id,
            block_number = %req.block_number,
            sequences = ?req.sequences,
            job_id = %req.job_id,
            cpu_time = %req.cpu_time,
            "Received SubmitProof request"
        );

        // Validate sequences are sorted
        let sequences = req.sequences;
        if !sequences.windows(2).all(|w| w[0] <= w[1]) {
            return Err(Status::invalid_argument("Sequences must be sorted"));
        }

        // Validate merged sequences are sorted if provided
        if !req.merged_sequences_1.is_empty() {
            if !req.merged_sequences_1.windows(2).all(|w| w[0] <= w[1]) {
                return Err(Status::invalid_argument(
                    "Merged sequences 1 must be sorted",
                ));
            }
        }
        if !req.merged_sequences_2.is_empty() {
            if !req.merged_sequences_2.windows(2).all(|w| w[0] <= w[1]) {
                return Err(Status::invalid_argument(
                    "Merged sequences 2 must be sorted",
                ));
            }
        }

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "SubmitProof request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Err(Status::permission_denied(
                    "Job does not belong to requesting session",
                ));
            }
        } else {
            warn!(
                "SubmitProof request from unknown session: {}",
                req.session_id
            );
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Save proof to cache storage
        debug!("Saving proof to cache storage for job {}", req.job_id);

        // Prepare metadata for the proof
        let mut metadata = ProofMetadata::default();
        metadata
            .data
            .insert("job_id".to_string(), req.job_id.clone());
        metadata
            .data
            .insert("session_id".to_string(), req.session_id.clone());
        metadata.data.insert(
            "app_instance_id".to_string(),
            agent_job.app_instance.clone(),
        );
        metadata
            .data
            .insert("block_number".to_string(), req.block_number.to_string());
        // Limit sequences metadata to avoid S3 tag value limit (256 chars)
        let sequences_str = format!("{:?}", sequences);
        let sequences_metadata = if sequences_str.len() > 200 {
            format!(
                "{}... ({} sequences)",
                &sequences_str[..200],
                sequences.len()
            )
        } else {
            sequences_str
        };
        metadata
            .data
            .insert("sequences".to_string(), sequences_metadata);
        metadata
            .data
            .insert("project".to_string(), "silvana".to_string());
        metadata.data.insert(
            "sui_chain".to_string(),
            std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string()),
        );

        let storage_start = std::time::Instant::now();
        let descriptor = match self
            .proof_storage
            .store_proof("cache", "public", &req.proof, Some(metadata))
            .await
        {
            Ok(desc) => {
                let storage_duration = storage_start.elapsed();
                debug!(
                    "Successfully saved proof to cache with descriptor: {} (took {}ms)",
                    desc.to_string(),
                    storage_duration.as_millis()
                );
                desc
            }
            Err(e) => {
                error!("Error saving proof to cache storage: {}", e);
                return Err(Status::internal("Failed to save proof to storage"));
            }
        };

        // Get hardware information
        let hardware_info = crate::hardware::get_hardware_info();

        // Convert optional repeated fields
        let merged_sequences_1 = if req.merged_sequences_1.is_empty() {
            None
        } else {
            Some(req.merged_sequences_1)
        };
        let merged_sequences_2 = if req.merged_sequences_2.is_empty() {
            None
        } else {
            Some(req.merged_sequences_2)
        };

        // Fetch the real ProofCalculation from blockchain to get start_sequence and end_sequence

        // Add submit_proof request to shared state for multicall
        self.state
            .add_submit_proof_request(
                agent_job.app_instance.clone(),
                crate::state::SubmitProofRequest {
                    block_number: req.block_number,
                    sequences: sequences.clone(),
                    merged_sequences_1,
                    merged_sequences_2,
                    job_id: req.job_id.clone(),
                    da_hash: descriptor.to_string(), // Use full descriptor (chain:network:hash) instead of just hash
                    cpu_cores: hardware_info.cpu_cores,
                    prover_architecture: hardware_info.prover_architecture.clone(),
                    prover_memory: hardware_info.prover_memory,
                    cpu_time: req.cpu_time,
                    _timestamp: Instant::now(),
                },
            )
            .await;

        // Return success immediately - the actual blockchain transaction will be sent via multicall
        let tx_result: Result<String, String> = Ok("".to_string());

        match tx_result {
            Ok(tx_hash) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ SubmitProof: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, block={}, seqs={:?}, da={}, tx={}, time={:?}",
                    agent_job.app_instance,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    agent_job.job_sequence,
                    agent_job.pending_job.app_instance_method,
                    req.block_number,
                    sequences,
                    descriptor.to_string(),
                    tx_hash,
                    elapsed
                );
                debug!(
                    "Successfully submitted proof for job {} with tx: {}",
                    req.job_id, tx_hash
                );

                Ok(Response::new(SubmitProofResponse {
                    success: true,
                    message: "Proof submitted successfully".to_string(),
                    tx_hash,
                    da_hash: descriptor.to_string(),
                }))
            }
            Err(e) => {
                error!(
                    "Failed to submit proof for job {} on blockchain: {}",
                    req.job_id, e
                );

                let elapsed = start_time.elapsed();
                warn!(
                    "❌ SubmitProof: job_id={}, error={}, time={:?}",
                    req.job_id, e, elapsed
                );
                Err(Status::internal(format!(
                    "Failed to submit proof transaction: {}",
                    e
                )))
            }
        }
    }

    async fn reject_proof(
        &self,
        request: Request<RejectProofRequest>,
    ) -> Result<Response<RejectProofResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        // Get job early to log app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("RejectProof request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        debug!(
            app_instance = %agent_job.app_instance,
            session_id = %req.session_id,
            block_number = %req.block_number,
            sequences = ?req.sequences,
            job_id = %req.job_id,
            "Received RejectProof request"
        );

        // Validate sequences are sorted
        let sequences = req.sequences;
        if !sequences.windows(2).all(|w| w[0] <= w[1]) {
            return Err(Status::invalid_argument("Sequences must be sorted"));
        }

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "RejectProof request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Err(Status::permission_denied(
                    "Job does not belong to requesting session",
                ));
            }
        } else {
            warn!(
                "RejectProof request from unknown session: {}",
                req.session_id
            );
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Reject proof transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        let tx_result = sui_interface
            .reject_proof(&agent_job.app_instance, req.block_number, sequences.clone())
            .await;

        match tx_result {
            Ok(tx_hash) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ RejectProof: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, block={}, seqs={:?}, tx={}, time={:?}",
                    agent_job.app_instance,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    agent_job.job_sequence,
                    agent_job.pending_job.app_instance_method,
                    req.block_number,
                    sequences,
                    tx_hash,
                    elapsed
                );
                debug!(
                    "Successfully rejected proof for job {} with tx: {}",
                    req.job_id, tx_hash
                );

                Ok(Response::new(RejectProofResponse {
                    success: true,
                    message: "Proof rejected successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                error!(
                    "Failed to reject proof for job {} on blockchain: {}",
                    req.job_id, e
                );
                warn!(
                    "❌ RejectProof: job_id={}, error={}, time={:?}",
                    req.job_id, e, elapsed
                );
                Err(Status::internal(format!(
                    "Failed to reject proof transaction: {}",
                    e
                )))
            }
        }
    }

    async fn submit_state(
        &self,
        request: Request<SubmitStateRequest>,
    ) -> Result<Response<SubmitStateResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            session_id = %req.session_id,
            sequence = %req.sequence,
            job_id = %req.job_id,
            has_new_state_data = %req.new_state_data.is_some(),
            has_serialized_state = %req.serialized_state.is_some(),
            "Received SubmitState request"
        );

        // Get job from agent database to validate it exists and get app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("SubmitState request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "SubmitState request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Err(Status::permission_denied(
                    "Job does not belong to requesting session",
                ));
            }
        } else {
            warn!(
                "SubmitState request from unknown session: {}",
                req.session_id
            );
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Save serialized state to cache storage if provided
        let da_descriptor = if let Some(serialized_state) = req.serialized_state {
            debug!(
                "Saving state to cache storage for sequence {}",
                req.sequence
            );

            // Prepare metadata for the state
            let mut metadata = ProofMetadata::default();
            metadata
                .data
                .insert("job_id".to_string(), req.job_id.clone());
            metadata
                .data
                .insert("session_id".to_string(), req.session_id.clone());
            metadata.data.insert(
                "app_instance_id".to_string(),
                agent_job.app_instance.clone(),
            );
            metadata
                .data
                .insert("sequence".to_string(), req.sequence.to_string());
            metadata
                .data
                .insert("type".to_string(), "sequence_state".to_string());
            metadata
                .data
                .insert("project".to_string(), "silvana".to_string());
            metadata.data.insert(
                "sui_chain".to_string(),
                std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string()),
            );

            let storage_start = std::time::Instant::now();
            match self
                .proof_storage
                .store_proof("cache", "public", &serialized_state, Some(metadata))
                .await
            {
                Ok(desc) => {
                    let storage_duration = storage_start.elapsed();
                    debug!(
                        "Successfully saved state to cache with descriptor: {} (took {}ms)",
                        desc.to_string(),
                        storage_duration.as_millis()
                    );
                    Some(desc.to_string())
                }
                Err(e) => {
                    error!("Error saving state to cache storage: {}", e);
                    return Err(Status::internal(
                        "Failed to save state to data availability layer",
                    ));
                }
            }
        } else {
            None
        };

        // Add update_state_for_sequence request to shared state for multicall
        self.state
            .add_update_state_for_sequence_request(
                agent_job.app_instance.clone(),
                crate::state::UpdateStateForSequenceRequest {
                    sequence: req.sequence,
                    new_state_data: req.new_state_data,
                    new_data_availability_hash: da_descriptor.clone(),
                    _timestamp: Instant::now(),
                },
            )
            .await;

        // Return success immediately - the actual blockchain transaction will be sent via multicall
        let tx_result: Result<String, String> = Ok("".to_string());

        match tx_result {
            Ok(tx_hash) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ SubmitState: app={}, dev={}, agent={}/{}, seq={}, job_id={}, da={:?}, time={:?}",
                    agent_job.app_instance,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    req.sequence,
                    req.job_id,
                    da_descriptor,
                    elapsed
                );

                Ok(Response::new(SubmitStateResponse {
                    success: true,
                    message: "State submitted successfully".to_string(),
                    tx_hash,
                    da_hash: da_descriptor,
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                warn!(
                    "❌ SubmitState: seq={}, job_id={}, error={}, time={:?}",
                    req.sequence, req.job_id, e, elapsed
                );
                Err(Status::internal(format!(
                    "Failed to update state transaction: {}",
                    e
                )))
            }
        }
    }

    async fn get_sequence_states(
        &self,
        request: Request<GetSequenceStatesRequest>,
    ) -> Result<Response<GetSequenceStatesResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            session_id = %req.session_id,
            job_id = %req.job_id,
            sequence = %req.sequence,
            "Received GetSequenceStates request"
        );

        // Get job from agent database to validate it exists and get app_instance
        debug!("Looking up job_id: {} in agent database", req.job_id);
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => {
                debug!(
                    "Found job: job_id={}, job_sequence={}, app_instance={}, developer={}/{}/{}",
                    req.job_id,
                    job.job_sequence,
                    job.app_instance,
                    job.developer,
                    job.agent,
                    job.agent_method
                );
                job
            }
            None => {
                warn!(
                    "GetSequenceStates request for unknown job_id: {}",
                    req.job_id
                );
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "GetSequenceStates request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Err(Status::permission_denied(
                    "Job does not belong to requesting session",
                ));
            }
        } else {
            warn!(
                "GetSequenceStates request from unknown session: {}",
                req.session_id
            );
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // First fetch the AppInstance object
        let app_instance_id = agent_job.app_instance.clone();
        let app_instance = match sui::fetch::fetch_app_instance(&app_instance_id).await {
            Ok(app_inst) => app_inst,
            Err(e) => {
                error!("Failed to fetch AppInstance {}: {}", app_instance_id, e);
                return Err(Status::internal(format!(
                    "Failed to fetch AppInstance: {}",
                    e
                )));
            }
        };

        // Query sequence states from the coordinator fetch module
        debug!(
            "🔍 Querying sequence states for app_instance={}, sequence={}",
            app_instance_id, req.sequence
        );
        match sui::fetch::query_sequence_states(&app_instance, req.sequence).await {
            Ok(fetch_states) => {
                debug!(
                    "📦 Retrieved {} sequence states from query",
                    fetch_states.len()
                );
                // Convert fetch SequenceState to protobuf SequenceState
                let proto_states: Vec<SequenceState> = fetch_states
                    .into_iter()
                    .enumerate()
                    .map(|(i, state)| {
                        debug!(
                            "State {}: sequence={}, has_state={}, has_data_availability={}",
                            i,
                            state.sequence,
                            state.state.is_some(),
                            state.data_availability.is_some()
                        );
                        SequenceState {
                            sequence: state.sequence,
                            state: state.state,
                            data_availability: state.data_availability,
                            optimistic_state: state.optimistic_state,
                            transition_data: state.transition_data,
                        }
                    })
                    .collect();

                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetSequenceStates: session={}, job_id={}, seq={}, count={}, time={}ms",
                    req.session_id,
                    req.job_id,
                    req.sequence,
                    proto_states.len(),
                    elapsed.as_millis()
                );

                Ok(Response::new(GetSequenceStatesResponse {
                    success: true,
                    message: format!(
                        "Retrieved {} sequence states for sequence {}",
                        proto_states.len(),
                        req.sequence
                    ),
                    states: proto_states,
                }))
            }
            Err(e) => {
                error!(
                    "Failed to query sequence states for sequence {}: {}",
                    req.sequence, e
                );
                Err(Status::internal(format!(
                    "Failed to query sequence states: {}",
                    e
                )))
            }
        }
    }

    async fn read_data_availability(
        &self,
        request: Request<ReadDataAvailabilityRequest>,
    ) -> Result<Response<ReadDataAvailabilityResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            session_id = %req.session_id,
            da_hash = %req.da_hash,
            "Received ReadDataAvailability request"
        );

        // Validate session_id exists
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if current_agent.is_none() {
            warn!(
                "ReadDataAvailability request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(ReadDataAvailabilityResponse {
                data: None,
                success: false,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Validate da_hash is provided
        if req.da_hash.is_empty() {
            warn!("ReadDataAvailability request with empty da_hash");
            return Ok(Response::new(ReadDataAvailabilityResponse {
                data: None,
                success: false,
                message: "Data availability hash is required".to_string(),
            }));
        }

        // Use proofs_storage to read the data using descriptor
        match self.proof_storage.get_proof(&req.da_hash).await {
            Ok((data, _metadata)) => {
                info!(
                    "✅ ReadDA: session={}, descriptor={}, size={} bytes, time={:?}",
                    req.session_id,
                    req.da_hash,
                    data.len(),
                    start_time.elapsed()
                );
                Ok(Response::new(ReadDataAvailabilityResponse {
                    data: Some(data),
                    success: true,
                    message: format!("Successfully read data for descriptor: {}", req.da_hash),
                }))
            }
            Err(e) => {
                error!("Failed to read data for descriptor {}: {}", req.da_hash, e);
                Ok(Response::new(ReadDataAvailabilityResponse {
                    data: None,
                    success: false,
                    message: format!("Failed to read data: {}", e),
                }))
            }
        }
    }

    async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            session_id = %req.session_id,
            block_number = %req.block_number,
            sequences = ?req.sequences,
            job_id = %req.job_id,
            "Received GetProof request"
        );

        // Get job from agent database to validate it exists and get app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("GetProof request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!("Job not found: {}", req.job_id),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "Session mismatch for GetProof: session agent={}/{}/{}, job agent={}/{}/{}",
                    current.developer,
                    current.agent,
                    current.agent_method,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method
                );
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: "Session does not match job assignment".to_string(),
                }));
            }
        } else {
            warn!("GetProof request from unknown session: {}", req.session_id);
            return Ok(Response::new(GetProofResponse {
                success: false,
                proof: None,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Validate sequences are provided
        if req.sequences.is_empty() {
            warn!("GetProof request with empty sequences");
            return Ok(Response::new(GetProofResponse {
                success: false,
                proof: None,
                message: "Sequences are required".to_string(),
            }));
        }

        // Use the app_instance from the job
        let app_instance_id = agent_job.app_instance.clone();

        // First fetch the AppInstance object
        let app_instance = match sui::fetch::fetch_app_instance(&app_instance_id).await {
            Ok(app_inst) => app_inst,
            Err(e) => {
                error!("Failed to fetch AppInstance {}: {}", app_instance_id, e);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!("Failed to fetch AppInstance: {}", e),
                }));
            }
        };

        // Fetch the ProofCalculation using the existing function from sui::fetch::prover
        let proof_calculation = match sui::fetch::fetch_proof_calculation(
            &app_instance,
            req.block_number,
        )
        .await
        {
            Ok(Some(proof_calc)) => proof_calc,
            Ok(None) => {
                // ProofCalculation not found - expected for settled blocks as they are deleted after settlement
                debug!(
                    "No ProofCalculation found for block {} (may have been deleted after settlement)",
                    req.block_number
                );
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!(
                        "No ProofCalculation found for block {} (may have been deleted after settlement)",
                        req.block_number
                    ),
                }));
            }
            Err(e) => {
                error!("Failed to fetch ProofCalculation: {}", e);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!("Failed to fetch ProofCalculation: {}", e),
                }));
            }
        };

        debug!(
            "Found ProofCalculation for block {} with {} proofs",
            req.block_number,
            proof_calculation.proofs.len()
        );

        // Find the proof with matching sequences
        let mut da_hash: Option<String> = None;
        for proof in &proof_calculation.proofs {
            debug!(
                "Checking proof: sequences={:?}, looking for={:?}",
                proof.sequences, req.sequences
            );
            if proof.sequences == req.sequences {
                debug!("Found matching proof for sequences {:?}", req.sequences);
                da_hash = proof.da_hash.clone();
                if da_hash.is_some() {
                    debug!("Found da_hash: {:?}", da_hash);
                    break;
                }
                // Continue searching if this proof has no da_hash
                debug!("da_hash is null for this proof, continuing search");
            }
        }

        let da_hash = match da_hash {
            Some(hash) => hash,
            None => {
                warn!(
                    "No proof found for sequences {:?} in block {}",
                    req.sequences, req.block_number
                );
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!(
                        "No proof found for sequences {:?} in block {}",
                        req.sequences, req.block_number
                    ),
                }));
            }
        };

        debug!("Found proof with descriptor: {}", da_hash);

        // Use proofs_storage to read the proof data using descriptor
        match self.proof_storage.get_proof(&da_hash).await {
            Ok((data, _metadata)) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetProof: app={}, block={}, sequences={:?}, descriptor={}, proof_size={} bytes, time={:?}",
                    app_instance_id,
                    req.block_number,
                    req.sequences,
                    da_hash,
                    data.len(),
                    elapsed
                );
                Ok(Response::new(GetProofResponse {
                    success: true,
                    proof: Some(data),
                    message: "Proof retrieved successfully".to_string(),
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                error!(
                    "❌ GetProof: app={}, block={}, sequences={:?}, descriptor={}, error={}, time={:?}",
                    app_instance_id, req.block_number, req.sequences, da_hash, e, elapsed
                );
                Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: format!("Failed to read proof: {}", e),
                }))
            }
        }
    }

    async fn get_block_proof(
        &self,
        request: Request<GetBlockProofRequest>,
    ) -> Result<Response<GetBlockProofResponse>, Status> {
        let start_time = std::time::Instant::now();
        let req = request.into_inner();

        debug!(
            session_id = %req.session_id,
            block_number = %req.block_number,
            job_id = %req.job_id,
            "Received GetBlockProof request"
        );

        // Get job from agent database to validate it exists and get app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("GetBlockProof request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!("Job not found: {}", req.job_id),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "Session mismatch for GetBlockProof: session agent={}/{}/{}, job agent={}/{}/{}",
                    current.developer,
                    current.agent,
                    current.agent_method,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method
                );
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: "Session does not match job assignment".to_string(),
                }));
            }
        } else {
            warn!(
                "GetBlockProof request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(GetBlockProofResponse {
                success: false,
                block_proof: None,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Use the app_instance from the job
        let app_instance_id = agent_job.app_instance.clone();

        // First fetch the AppInstance object
        let app_instance = match sui::fetch::fetch_app_instance(&app_instance_id).await {
            Ok(app_inst) => app_inst,
            Err(e) => {
                error!("Failed to fetch AppInstance {}: {}", app_instance_id, e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!("Failed to fetch AppInstance: {}", e),
                }));
            }
        };

        // Use the fetch_proof_calculation function from prover module
        let proof_calculation = match sui::fetch::fetch_proof_calculation(
            &app_instance,
            req.block_number,
        )
        .await
        {
            Ok(Some(calc)) => calc,
            Ok(None) => {
                // ProofCalculation not found - expected for settled blocks as they are deleted after settlement
                debug!(
                    "No ProofCalculation found for block {} (may have been deleted after settlement)",
                    req.block_number
                );
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!(
                        "No ProofCalculation found for block {} (may have been deleted after settlement)",
                        req.block_number
                    ),
                }));
            }
            Err(e) => {
                error!("Failed to fetch ProofCalculation: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!("Failed to fetch ProofCalculation: {}", e),
                }));
            }
        };

        debug!(
            "Successfully fetched ProofCalculation for block {}",
            req.block_number
        );
        let block_proof = proof_calculation.block_proof;

        // If we found a block proof DA hash, fetch the actual proof from Walrus
        let da_hash = match block_proof {
            Some(hash) => hash,
            None => {
                let elapsed = start_time.elapsed();
                info!(
                    "⏳ GetBlockProof: block={}, job_id={}, proof_not_ready, time={}ms",
                    req.block_number,
                    req.job_id,
                    elapsed.as_millis()
                );
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!(
                        "Block proof not available yet for block {}",
                        req.block_number
                    ),
                }));
            }
        };

        debug!("Found block proof with descriptor: {}", da_hash);

        // Use proofs_storage to read the proof data using descriptor
        match self.proof_storage.get_proof(&da_hash).await {
            Ok((data, _metadata)) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetBlockProof: block={}, job_id={}, descriptor={}, time={}ms",
                    req.block_number,
                    req.job_id,
                    da_hash,
                    elapsed.as_millis()
                );
                Ok(Response::new(GetBlockProofResponse {
                    success: true,
                    block_proof: Some(data),
                    message: "Block proof retrieved successfully".to_string(),
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                error!(
                    "❌ GetBlockProof: block={}, job_id={}, descriptor={}, error={}, time={}ms",
                    req.block_number,
                    req.job_id,
                    da_hash,
                    e,
                    elapsed.as_millis()
                );
                Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: format!("Failed to read proof: {}", e),
                }))
            }
        }
    }

    async fn retrieve_secret(
        &self,
        request: Request<RetrieveSecretRequest>,
    ) -> Result<Response<RetrieveSecretResponse>, Status> {
        let req = request.into_inner();

        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            secret_name = %req.name,
            "Received RetrieveSecret request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("RetrieveSecret request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    secret_value: None,
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "RetrieveSecret request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    secret_value: None,
                }));
            }
        } else {
            warn!(
                "RetrieveSecret request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(RetrieveSecretResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                secret_value: None,
            }));
        }

        // Get the Silvana RPC client from shared state
        let mut rpc_client = match self.state.get_rpc_client().await {
            Some(client) => client,
            None => {
                error!("Silvana RPC client not initialized");
                return Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: "Silvana RPC service not available".to_string(),
                    secret_value: None,
                }));
            }
        };

        // Build the secret reference from the job information
        use rpc_client::SecretReference;

        // Special handling for private key secrets (prefixed with sk_)
        // These are typically global secrets at the developer/agent level
        // rather than app-instance specific
        let secret_reference = if req.name.starts_with("sk_") {
            debug!(
                "Retrieving private key secret '{}' at developer/agent level",
                req.name
            );
            SecretReference {
                developer: agent_job.developer.clone(),
                agent: agent_job.agent.clone(),
                app: None,          // No app context for private keys
                app_instance: None, // No app instance context for private keys
                name: Some(req.name.clone()),
            }
        } else {
            // Regular secret with full context
            SecretReference {
                developer: agent_job.developer.clone(),
                agent: agent_job.agent.clone(),
                app: Some(agent_job.pending_job.app.clone()),
                app_instance: Some(agent_job.app_instance.clone()),
                name: Some(req.name.clone()),
            }
        };

        // Create the retrieve secret request for the Silvana RPC service
        use rpc_client::RetrieveSecretRequest;
        let retrieve_request = RetrieveSecretRequest {
            reference: Some(secret_reference),
            signature: vec![], // Empty signature for now (not validated yet per proto)
        };

        // Call the retrieve secret method on Silvana RPC
        match rpc_client.retrieve_secret(retrieve_request).await {
            Ok(response) => {
                let inner = response.into_inner();
                if inner.success {
                    info!(
                        "Successfully retrieved secret '{}' for job {} via Silvana RPC",
                        req.name, req.job_id
                    );
                    Ok(Response::new(RetrieveSecretResponse {
                        success: true,
                        message: inner.message,
                        secret_value: Some(inner.secret_value),
                    }))
                } else {
                    warn!(
                        "Failed to retrieve secret '{}' for job {}: {}",
                        req.name, req.job_id, inner.message
                    );
                    Ok(Response::new(RetrieveSecretResponse {
                        success: false,
                        message: inner.message,
                        secret_value: None,
                    }))
                }
            }
            Err(e) => {
                error!("Error calling retrieve secret on Silvana RPC: {}", e);
                Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: format!("Error retrieving secret: {}", e),
                    secret_value: None,
                }))
            }
        }
    }

    async fn set_kv(
        &self,
        request: Request<SetKvRequest>,
    ) -> Result<Response<SetKvResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            key = %req.key,
            "Received SetKV request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("SetKV request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(SetKvResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "SetKV request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(SetKvResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    tx_hash: String::new(),
                }));
            }
        } else {
            warn!("SetKV request from unknown session: {}", req.session_id);
            return Ok(Response::new(SetKvResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                tx_hash: String::new(),
            }));
        }

        // Execute the set_kv transaction
        match sui::set_kv_tx(&agent_job.app_instance, req.key, req.value).await {
            Ok(tx_hash) => {
                info!(
                    "✅ SetKV successful for job {}: tx_hash={}",
                    req.job_id, tx_hash
                );
                Ok(Response::new(SetKvResponse {
                    success: true,
                    message: "Key-value pair set successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to set key-value pair for job {}: {}", req.job_id, e);
                Ok(Response::new(SetKvResponse {
                    success: false,
                    message: format!("Failed to set key-value pair: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn get_kv(
        &self,
        request: Request<GetKvRequest>,
    ) -> Result<Response<GetKvResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            key = %req.key,
            "Received GetKV request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("GetKV request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(GetKvResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    value: None,
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "GetKV request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(GetKvResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    value: None,
                }));
            }
        } else {
            warn!("GetKV request from unknown session: {}", req.session_id);
            return Ok(Response::new(GetKvResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                value: None,
            }));
        }

        // Fetch the app instance and get the key-value
        match sui::fetch::app_instance::fetch_app_instance(&agent_job.app_instance).await {
            Ok(app_instance) => {
                if let Some(value) = app_instance.kv.get(&req.key) {
                    info!(
                        "✅ GetKV successful for job {}: key={}, value={}",
                        req.job_id, req.key, value
                    );
                    Ok(Response::new(GetKvResponse {
                        success: true,
                        message: "Key-value pair retrieved successfully".to_string(),
                        value: Some(value.clone()),
                    }))
                } else {
                    info!("GetKV for job {}: key '{}' not found", req.job_id, req.key);
                    Ok(Response::new(GetKvResponse {
                        success: true,
                        message: format!("Key '{}' not found", req.key),
                        value: None,
                    }))
                }
            }
            Err(e) => {
                error!("Failed to fetch app instance for job {}: {}", req.job_id, e);
                Ok(Response::new(GetKvResponse {
                    success: false,
                    message: format!("Failed to fetch app instance: {}", e),
                    value: None,
                }))
            }
        }
    }

    async fn delete_kv(
        &self,
        request: Request<DeleteKvRequest>,
    ) -> Result<Response<DeleteKvResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            key = %req.key,
            "Received DeleteKV request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("DeleteKV request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(DeleteKvResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "DeleteKV request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(DeleteKvResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    tx_hash: String::new(),
                }));
            }
        } else {
            warn!("DeleteKV request from unknown session: {}", req.session_id);
            return Ok(Response::new(DeleteKvResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                tx_hash: String::new(),
            }));
        }

        // Execute the delete_kv transaction
        match sui::delete_kv_tx(&agent_job.app_instance, req.key).await {
            Ok(tx_hash) => {
                info!(
                    "✅ DeleteKV successful for job {}: tx_hash={}",
                    req.job_id, tx_hash
                );
                Ok(Response::new(DeleteKvResponse {
                    success: true,
                    message: "Key-value pair deleted successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!(
                    "Failed to delete key-value pair for job {}: {}",
                    req.job_id, e
                );
                Ok(Response::new(DeleteKvResponse {
                    success: false,
                    message: format!("Failed to delete key-value pair: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn add_metadata(
        &self,
        request: Request<AddMetadataRequest>,
    ) -> Result<Response<AddMetadataResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            key = %req.key,
            "Received AddMetadata request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("AddMetadata request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(AddMetadataResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "AddMetadata request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(AddMetadataResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    tx_hash: String::new(),
                }));
            }
        } else {
            warn!(
                "AddMetadata request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(AddMetadataResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                tx_hash: String::new(),
            }));
        }

        // Execute the add_metadata transaction
        match sui::add_metadata_tx(&agent_job.app_instance, req.key, req.value).await {
            Ok(tx_hash) => {
                info!(
                    "✅ AddMetadata successful for job {}: tx_hash={}",
                    req.job_id, tx_hash
                );
                Ok(Response::new(AddMetadataResponse {
                    success: true,
                    message: "Metadata added successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to add metadata for job {}: {}", req.job_id, e);
                Ok(Response::new(AddMetadataResponse {
                    success: false,
                    message: format!("Failed to add metadata: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn get_metadata(
        &self,
        request: Request<GetMetadataRequest>,
    ) -> Result<Response<GetMetadataResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            key = ?req.key,
            "Received GetMetadata request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("GetMetadata request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(GetMetadataResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    metadata: None,
                }));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer
                || current.agent != agent_job.agent
                || current.agent_method != agent_job.agent_method
            {
                warn!(
                    "GetMetadata request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    current.developer,
                    current.agent,
                    current.agent_method
                );
                return Ok(Response::new(GetMetadataResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    metadata: None,
                }));
            }
        } else {
            warn!(
                "GetMetadata request from unknown session: {}",
                req.session_id
            );
            return Ok(Response::new(GetMetadataResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                metadata: None,
            }));
        }

        // Fetch the app instance and get the metadata
        match sui::fetch::app_instance::fetch_app_instance(&agent_job.app_instance).await {
            Ok(app_instance) => {
                // Handle the optional key - if provided, look up the metadata value
                let metadata_value = if let Some(ref key) = req.key {
                    if let Some(value) = app_instance.metadata.get(key) {
                        info!(
                            "✅ GetMetadata successful for job {}: key={}, value={}",
                            req.job_id, key, value
                        );
                        Some(value.clone())
                    } else {
                        info!(
                            "GetMetadata for job {}: key '{}' not found",
                            req.job_id, key
                        );
                        None
                    }
                } else {
                    // No key provided, just return app instance info without metadata value
                    info!(
                        "✅ GetMetadata successful for job {}: returning app instance info",
                        req.job_id
                    );
                    None
                };

                Ok(Response::new(GetMetadataResponse {
                    success: true,
                    message: if req.key.is_some() {
                        if metadata_value.is_some() {
                            "Metadata retrieved successfully".to_string()
                        } else {
                            format!("Key '{}' not found", req.key.as_ref().unwrap())
                        }
                    } else {
                        "App instance info retrieved successfully".to_string()
                    },
                    metadata: Some(Metadata {
                        value: metadata_value,
                        app_instance_id: app_instance.id.clone(),
                        silvana_app_name: app_instance.silvana_app_name.clone(),
                        description: app_instance.description.clone(),
                        sequence: app_instance.sequence,
                        admin: app_instance.admin.clone(),
                        block_number: app_instance.block_number,
                        previous_block_timestamp: app_instance.previous_block_timestamp,
                        previous_block_last_sequence: app_instance.previous_block_last_sequence,
                        last_proved_block_number: app_instance.last_proved_block_number,
                        is_paused: app_instance.is_paused,
                        created_at: app_instance.created_at,
                        updated_at: app_instance.updated_at,
                        // Populate the new settlements map
                        settlements: app_instance
                            .settlements
                            .iter()
                            .map(|(chain, settlement)| {
                                (
                                    chain.clone(),
                                    coordinator::SettlementInfo {
                                        chain: chain.clone(),
                                        last_settled_block_number: settlement
                                            .last_settled_block_number,
                                        settlement_address: settlement.settlement_address.clone(),
                                        settlement_job: settlement.settlement_job,
                                    },
                                )
                            })
                            .collect(),
                    }),
                }))
            }
            Err(e) => {
                error!("Failed to fetch app instance for job {}: {}", req.job_id, e);
                Ok(Response::new(GetMetadataResponse {
                    success: false,
                    message: format!("Failed to fetch app instance: {}", e),
                    metadata: None,
                }))
            }
        }
    }

    async fn try_create_block(
        &self,
        request: Request<TryCreateBlockRequest>,
    ) -> Result<Response<TryCreateBlockResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            "Received TryCreateBlock request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("TryCreateBlock request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(TryCreateBlockResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                    block_number: None,
                }));
            }
        };

        // Try to create block
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .try_create_block(&agent_job.app_instance)
            .await
        {
            Ok(tx_hash) => {
                info!("✅ TryCreateBlock successful, tx: {}", tx_hash);
                // TODO: Parse event to get block number if created
                Ok(Response::new(TryCreateBlockResponse {
                    success: true,
                    message: "Block creation attempted successfully".to_string(),
                    tx_hash,
                    block_number: None,
                }))
            }
            Err(e) => {
                error!("Failed to try create block: {}", e);
                Ok(Response::new(TryCreateBlockResponse {
                    success: false,
                    message: format!("Failed to create block: {}", e),
                    tx_hash: String::new(),
                    block_number: None,
                }))
            }
        }
    }

    async fn update_block_state_data_availability(
        &self,
        request: Request<UpdateBlockStateDataAvailabilityRequest>,
    ) -> Result<Response<UpdateBlockStateDataAvailabilityResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            "Received UpdateBlockStateDataAvailability request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!(
                    "UpdateBlockStateDataAvailability request for unknown job_id: {}",
                    req.job_id
                );
                return Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Prepare metadata for the block state
        let mut metadata = ProofMetadata::default();
        metadata
            .data
            .insert("job_id".to_string(), req.job_id.clone());
        metadata
            .data
            .insert("session_id".to_string(), req.session_id.clone());
        metadata.data.insert(
            "app_instance_id".to_string(),
            agent_job.app_instance.clone(),
        );
        metadata
            .data
            .insert("block_number".to_string(), req.block_number.to_string());
        metadata
            .data
            .insert("type".to_string(), "block_state".to_string());
        metadata
            .data
            .insert("project".to_string(), "silvana".to_string());
        metadata.data.insert(
            "sui_chain".to_string(),
            std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string()),
        );

        // Save to cache:public instead of walrus:testnet
        debug!(
            "Saving block state to cache DA for block {}",
            req.block_number
        );
        let storage_start = std::time::Instant::now();

        let descriptor = match self
            .proof_storage
            .store_proof(
                "cache",
                "public",
                &req.state_data_availability,
                Some(metadata),
            )
            .await
        {
            Ok(desc) => {
                let storage_duration = storage_start.elapsed();
                info!(
                    "Successfully saved block state to cache with descriptor: {} (took {}ms)",
                    desc.to_string(),
                    storage_duration.as_millis()
                );
                desc
            }
            Err(cache_err) => {
                error!("Failed to save block state to cache: {}", cache_err);
                return Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: false,
                    message: format!(
                        "Failed to save state to data availability layer: {}",
                        cache_err
                    ),
                    tx_hash: String::new(),
                }));
            }
        };

        // Update block state data availability with the descriptor string
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        let descriptor_str = descriptor.to_string();
        match sui_interface
            .update_block_state_data_availability(
                &agent_job.app_instance,
                req.block_number,
                descriptor_str.clone(),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "✅ UpdateBlockStateDataAvailability successful, block: {}, descriptor: {}, tx: {}",
                    req.block_number, descriptor_str, tx_hash
                );
                Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: true,
                    message: format!(
                        "Block state DA updated successfully with descriptor: {}",
                        descriptor_str
                    ),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to update block state DA: {}", e);
                Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: false,
                    message: format!("Failed to update block state DA: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn update_block_proof_data_availability(
        &self,
        request: Request<UpdateBlockProofDataAvailabilityRequest>,
    ) -> Result<Response<UpdateBlockProofDataAvailabilityResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            "Received UpdateBlockProofDataAvailability request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!(
                    "UpdateBlockProofDataAvailability request for unknown job_id: {}",
                    req.job_id
                );
                return Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Prepare metadata for the block proof
        let mut metadata = ProofMetadata::default();
        metadata
            .data
            .insert("job_id".to_string(), req.job_id.clone());
        metadata
            .data
            .insert("session_id".to_string(), req.session_id.clone());
        metadata.data.insert(
            "app_instance_id".to_string(),
            agent_job.app_instance.clone(),
        );
        metadata
            .data
            .insert("block_number".to_string(), req.block_number.to_string());
        metadata
            .data
            .insert("type".to_string(), "block_proof".to_string());
        metadata
            .data
            .insert("project".to_string(), "silvana".to_string());
        metadata.data.insert(
            "sui_chain".to_string(),
            std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string()),
        );

        // Save current proof to cache:public
        debug!(
            "Saving block proof to cache DA for block {}",
            req.block_number
        );
        let storage_start = std::time::Instant::now();

        let descriptor = match self
            .proof_storage
            .store_proof(
                "cache",
                "public",
                &req.proof_data_availability,
                Some(metadata.clone()),
            )
            .await
        {
            Ok(desc) => {
                let storage_duration = storage_start.elapsed();
                info!(
                    "Successfully saved block proof to cache with descriptor: {} (took {}ms)",
                    desc.to_string(),
                    storage_duration.as_millis()
                );
                desc
            }
            Err(cache_err) => {
                error!("Failed to save block proof to cache: {}", cache_err);
                return Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: false,
                    message: format!(
                        "Failed to save proof to data availability layer: {}",
                        cache_err
                    ),
                    tx_hash: String::new(),
                }));
            }
        };

        // Check if we should create a quilt for every N blocks
        let quilt_descriptor_str = if req.block_number % WALRUS_QUILT_BLOCK_INTERVAL == 0
            && req.block_number > 0
        {
            info!(
                "Block {} is a quilt checkpoint, fetching last {} blocks for quilt storage",
                req.block_number, WALRUS_QUILT_BLOCK_INTERVAL
            );

            // Fetch app_instance to get block data
            match sui::fetch::fetch_app_instance(&agent_job.app_instance).await {
                Ok(app_instance) => {
                    // Fetch the last N blocks including current
                    let start_block = req
                        .block_number
                        .saturating_sub(WALRUS_QUILT_BLOCK_INTERVAL - 1);
                    let mut quilt_data = Vec::new();

                    for block_num in start_block..=req.block_number {
                        // Special handling for current block - use the proof we just stored
                        if block_num == req.block_number {
                            // Add the current block's proof that we just stored
                            quilt_data
                                .push((block_num.to_string(), req.proof_data_availability.clone()));
                            debug!("Added current block {} proof to quilt", block_num);
                            continue;
                        }

                        match fetch_block_info(&app_instance, block_num).await {
                            Ok(Some(block)) => {
                                if let Some(proof_da) = block.proof_data_availability {
                                    if !proof_da.is_empty() {
                                        // For cache descriptors, we need to fetch the actual proof data
                                        if proof_da.starts_with("cache:") {
                                            match crate::proofs_storage::ProofDescriptor::parse(
                                                &proof_da,
                                            ) {
                                                Ok(desc) => {
                                                    match self
                                                        .proof_storage
                                                        .get_proof(&desc.to_string())
                                                        .await
                                                    {
                                                        Ok((proof_data, _)) => {
                                                            quilt_data.push((
                                                                block_num.to_string(),
                                                                proof_data,
                                                            ));
                                                            debug!(
                                                                "Added block {} proof to quilt",
                                                                block_num
                                                            );
                                                        }
                                                        Err(e) => {
                                                            warn!(
                                                                "Failed to fetch proof for block {}: {}",
                                                                block_num, e
                                                            );
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        "Failed to parse descriptor for block {}: {}",
                                                        block_num, e
                                                    );
                                                }
                                            }
                                        } else {
                                            // For other storage types, store the descriptor itself
                                            quilt_data.push((block_num.to_string(), proof_da));
                                            debug!("Added block {} descriptor to quilt", block_num);
                                        }
                                    }
                                } else {
                                    warn!("Block {} has no proof_data_availability", block_num);
                                }
                            }
                            Ok(None) => {
                                info!("Block {} not found", block_num);
                            }
                            Err(e) => {
                                warn!("Failed to fetch block {}: {}", block_num, e);
                            }
                        }
                    }

                    // Store the quilt if we have data
                    if !quilt_data.is_empty() {
                        info!(
                            "Creating quilt with {} block proofs for blocks {}-{}",
                            quilt_data.len(),
                            start_block,
                            req.block_number
                        );

                        // Clone the original quilt data for cache fallback (without test entries)
                        let quilt_data_for_cache = quilt_data.clone();

                        // If WALRUS_TEST is enabled, add 580 test entries to simulate 600 proofs
                        // (Walrus has a maximum of 600 pieces per quilt)
                        // We only add these to the Walrus version, not the cache version
                        if WALRUS_TEST {
                            warn!(
                                "WALRUS_TEST mode enabled: Adding test entries to reach 600 proofs (Walrus max)"
                            );

                            let available_proofs = quilt_data.len();
                            let test_entries_to_add = 600 - available_proofs; // Add enough to reach 600 total

                            if test_entries_to_add > 0 {
                                // Use full proof data for Walrus testing (clone existing proofs)
                                for i in 1..=test_entries_to_add {
                                    // Use proof data from existing proofs in a round-robin fashion
                                    let source_index = (i - 1) % available_proofs;
                                    let test_identifier = format!("test{}", i);
                                    let test_data = quilt_data[source_index].1.clone();

                                    quilt_data.push((test_identifier, test_data));

                                    // Log progress every 100 entries
                                    if i % 100 == 0 {
                                        debug!(
                                            "Added {} test entries to quilt for Walrus testing",
                                            i
                                        );
                                    }
                                }

                                warn!(
                                    "Total quilt pieces for Walrus: {} (original: {}, test: {})",
                                    quilt_data.len(),
                                    available_proofs,
                                    test_entries_to_add
                                );
                            } else {
                                warn!(
                                    "Already have {} pieces, no test entries needed",
                                    available_proofs
                                );
                            }
                        }

                        // Determine Walrus network based on SUI chain
                        let walrus_network = if std::env::var("SUI_CHAIN")
                            .unwrap_or_else(|_| "devnet".to_string())
                            == "mainnet"
                        {
                            "mainnet"
                        } else {
                            "testnet"
                        };

                        // Try to store quilt to Walrus first, fallback to cache if it fails
                        let result = match self
                            .proof_storage
                            .store_quilt(
                                "walrus",
                                walrus_network,
                                quilt_data.clone(),
                                Some(metadata.clone()),
                            )
                            .await
                        {
                            Ok(result) => {
                                // Success - quilt stored to Walrus
                                info!("✅ Quilt stored to Walrus successfully:");
                                info!("  Descriptor: {}", result.descriptor.to_string());
                                info!("  Blob ID: {}", result.blob_id);
                                if let Some(tx) = &result.tx_digest {
                                    info!("  Tx Digest: {}", tx);
                                }
                                if !result.quilt_patches.is_empty() {
                                    info!("  Quilt patches: {}", result.quilt_patches.len());
                                    for (i, patch) in result.quilt_patches.iter().enumerate() {
                                        if i < 5 {
                                            // Show first 5 patches
                                            info!(
                                                "    - {}: {}",
                                                patch.identifier, patch.quilt_patch_id
                                            );
                                        }
                                    }
                                    if result.quilt_patches.len() > 5 {
                                        info!(
                                            "    ... and {} more",
                                            result.quilt_patches.len() - 5
                                        );
                                    }
                                }
                                Ok(result)
                            }
                            Err(walrus_err) => {
                                // Walrus failed, try cache as fallback with smaller data (without test entries)
                                warn!(
                                    "Failed to store quilt to Walrus: {}, trying cache as fallback",
                                    walrus_err
                                );

                                match self
                                    .proof_storage
                                    .store_quilt(
                                        "cache",
                                        "public",
                                        quilt_data_for_cache,
                                        Some(metadata.clone()),
                                    )
                                    .await
                                {
                                    Ok(result) => {
                                        info!(
                                            "✅ Quilt stored to cache successfully (fallback without test entries):"
                                        );
                                        info!("  Descriptor: {}", result.descriptor.to_string());
                                        info!("  Blob ID: {}", result.blob_id);
                                        Ok(result)
                                    }
                                    Err(cache_err) => {
                                        error!(
                                            "Failed to store quilt to both Walrus and cache: walrus_err={}, cache_err={}",
                                            walrus_err, cache_err
                                        );
                                        Err(cache_err)
                                    }
                                }
                            }
                        };

                        match result {
                            Ok(res) => Some(res.descriptor.to_string()),
                            Err(_) => None,
                        }
                    } else {
                        warn!("No valid proof data found for quilt creation");
                        None
                    }
                }
                Err(e) => {
                    error!("Failed to parse app_instance: {}", e);
                    warn!("Continuing without quilt storage due to parse error");
                    None
                }
            }
        } else {
            None
        };

        // Update block proof data availability with the descriptor string
        // Use quilt descriptor for blocks where block_number % 10 == 0
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        let final_descriptor_str = if let Some(quilt_desc) = quilt_descriptor_str {
            // For quilt checkpoint blocks, use the quilt descriptor
            info!(
                "Using quilt descriptor for block {}: {}",
                req.block_number, quilt_desc
            );
            quilt_desc
        } else {
            descriptor.to_string()
        };

        match sui_interface
            .update_block_proof_data_availability(
                &agent_job.app_instance,
                req.block_number,
                final_descriptor_str.clone(),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "✅ UpdateBlockProofDataAvailability successful, block: {}, descriptor: {}, tx: {}",
                    req.block_number, final_descriptor_str, tx_hash
                );
                Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: true,
                    message: format!(
                        "Block proof DA updated successfully with descriptor: {}",
                        final_descriptor_str
                    ),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to update block proof DA: {}", e);
                Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: false,
                    message: format!("Failed to update block proof DA: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn update_block_settlement_tx_hash(
        &self,
        request: Request<UpdateBlockSettlementTxHashRequest>,
    ) -> Result<Response<UpdateBlockSettlementTxHashResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            "Received UpdateBlockSettlementTxHash request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!(
                    "UpdateBlockSettlementTxHash request for unknown job_id: {}",
                    req.job_id
                );
                return Ok(Response::new(UpdateBlockSettlementTxHashResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Update block settlement tx hash
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .update_block_settlement_tx_hash(
                &agent_job.app_instance,
                req.block_number,
                req.chain.clone(),
                req.settlement_tx_hash,
            )
            .await
        {
            Ok(tx_hash) => {
                info!("✅ UpdateBlockSettlementTxHash successful, tx: {}", tx_hash);
                Ok(Response::new(UpdateBlockSettlementTxHashResponse {
                    success: true,
                    message: "Block settlement tx hash updated successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to update block settlement tx hash: {}", e);
                Ok(Response::new(UpdateBlockSettlementTxHashResponse {
                    success: false,
                    message: format!("Failed to update block settlement tx hash: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
    }

    async fn update_block_settlement_tx_included_in_block(
        &self,
        request: Request<UpdateBlockSettlementTxIncludedInBlockRequest>,
    ) -> Result<Response<UpdateBlockSettlementTxIncludedInBlockResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            settled_at = %req.settled_at,
            "Received UpdateBlockSettlementTxIncludedInBlock request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!(
                    "UpdateBlockSettlementTxIncludedInBlock request for unknown job_id: {}",
                    req.job_id
                );
                return Ok(Response::new(
                    UpdateBlockSettlementTxIncludedInBlockResponse {
                        success: false,
                        message: format!("Job not found: {}", req.job_id),
                        tx_hash: String::new(),
                    },
                ));
            }
        };

        // Update block settlement included in block
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .update_block_settlement_tx_included_in_block(
                &agent_job.app_instance,
                req.block_number,
                req.chain.clone(),
                req.settled_at,
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "✅ UpdateBlockSettlementTxIncludedInBlock successful, tx: {}",
                    tx_hash
                );
                Ok(Response::new(
                    UpdateBlockSettlementTxIncludedInBlockResponse {
                        success: true,
                        message: "Block settlement included in block updated successfully"
                            .to_string(),
                        tx_hash,
                    },
                ))
            }
            Err(e) => {
                error!("Failed to update block settlement included in block: {}", e);
                Ok(Response::new(
                    UpdateBlockSettlementTxIncludedInBlockResponse {
                        success: false,
                        message: format!("Failed to update block settlement included: {}", e),
                        tx_hash: String::new(),
                    },
                ))
            }
        }
    }

    async fn create_app_job(
        &self,
        request: Request<CreateAppJobRequest>,
    ) -> Result<Response<CreateAppJobResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            method_name = %req.method_name,
            "Received CreateAppJob request"
        );

        // Get job from agent database to validate it exists and get app_instance info
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!("CreateAppJob request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(CreateAppJobResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                    job_sequence: 0,
                }));
            }
        };

        // Create app job
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .create_app_job(
                &agent_job.app_instance,
                req.method_name.clone(),
                req.job_description,
                req.block_number,
                Some(req.sequences),
                Some(req.sequences1),
                Some(req.sequences2),
                req.data,
                req.interval_ms,
                req.next_scheduled_at,
                req.settlement_chain,
            )
            .await
        {
            Ok(tx_hash) => {
                info!("✅ CreateAppJob successful, tx: {}", tx_hash);
                // TODO: Parse event to get job sequence
                Ok(Response::new(CreateAppJobResponse {
                    success: true,
                    message: format!("App job '{}' created successfully", req.method_name),
                    tx_hash,
                    job_sequence: 0, // TODO: Get from event
                }))
            }
            Err(e) => {
                error!("Failed to create app job: {}", e);
                Ok(Response::new(CreateAppJobResponse {
                    success: false,
                    message: format!("Failed to create app job: {}", e),
                    tx_hash: String::new(),
                    job_sequence: 0,
                }))
            }
        }
    }

    async fn get_block(
        &self,
        request: Request<GetBlockRequest>,
    ) -> Result<Response<GetBlockResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            "Received GetBlock request"
        );

        // Get agent job to find app_instance
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                return Ok(Response::new(GetBlockResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    block: None,
                }));
            }
        };

        // Fetch app instance
        let app_instance =
            match sui::fetch::app_instance::fetch_app_instance(&agent_job.app_instance).await {
                Ok(ai) => ai,
                Err(e) => {
                    return Ok(Response::new(GetBlockResponse {
                        success: false,
                        message: format!("Failed to fetch app instance: {}", e),
                        block: None,
                    }));
                }
            };

        // Fetch block info
        match sui::fetch::block::fetch_block_info(&app_instance, req.block_number).await {
            Ok(Some(block_info)) => {
                info!("✅ GetBlock successful for block {}", req.block_number);

                // Commitments are already Vec<u8> from the fetch function
                let block = Block {
                    id: format!("block_{}", req.block_number), // Generate ID from block number
                    name: block_info.name,
                    block_number: block_info.block_number,
                    start_sequence: block_info.start_sequence,
                    end_sequence: block_info.end_sequence,
                    actions_commitment: block_info.actions_commitment,
                    state_commitment: block_info.state_commitment,
                    time_since_last_block: block_info.time_since_last_block,
                    number_of_transactions: block_info.number_of_transactions,
                    start_actions_commitment: block_info.start_actions_commitment,
                    end_actions_commitment: block_info.end_actions_commitment,
                    state_data_availability: block_info.state_data_availability,
                    proof_data_availability: block_info.proof_data_availability,
                    // Settlement fields removed - now handled in Settlement per chain
                    created_at: block_info.created_at,
                    state_calculated_at: block_info.state_calculated_at,
                    proved_at: block_info.proved_at,
                };

                Ok(Response::new(GetBlockResponse {
                    success: true,
                    message: format!("Block {} retrieved successfully", req.block_number),
                    block: Some(block),
                }))
            }
            Ok(None) => {
                info!("Block {} not found", req.block_number);
                Ok(Response::new(GetBlockResponse {
                    success: false,
                    message: format!("Block {} not found", req.block_number),
                    block: None,
                }))
            }
            Err(e) => {
                error!("Failed to fetch block {}: {}", req.block_number, e);
                Ok(Response::new(GetBlockResponse {
                    success: false,
                    message: format!("Failed to fetch block: {}", e),
                    block: None,
                }))
            }
        }
    }

    async fn get_block_settlement(
        &self,
        request: Request<GetBlockSettlementRequest>,
    ) -> Result<Response<GetBlockSettlementResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            chain = %req.chain,
            "Received GetBlockSettlement request"
        );

        // Get the app instance from the job
        let agent_job = match self
            .state
            .get_agent_job_db()
            .get_job_by_id(&req.job_id)
            .await
        {
            Some(job) => job,
            None => {
                warn!(
                    "GetBlockSettlement request for unknown job_id: {}",
                    req.job_id
                );
                return Ok(Response::new(GetBlockSettlementResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    block_settlement: None,
                    chain: req.chain.clone(),
                }));
            }
        };

        // Fetch the app instance and get settlement info for the specific chain
        match sui::fetch::fetch_app_instance(&agent_job.app_instance).await {
            Ok(app_instance) => {
                // Get the settlement for the specified chain
                if let Some(settlement) = app_instance.settlements.get(&req.chain) {
                    // Fetch the block settlement from the ObjectTable
                    match sui::fetch::fetch_block_settlement(settlement, req.block_number).await {
                        Ok(Some(block_settlement)) => {
                            let response_settlement = BlockSettlement {
                                block_number: block_settlement.block_number,
                                settlement_tx_hash: block_settlement.settlement_tx_hash.clone(),
                                settlement_tx_included_in_block: block_settlement
                                    .settlement_tx_included_in_block,
                                sent_to_settlement_at: block_settlement.sent_to_settlement_at,
                                settled_at: block_settlement.settled_at,
                            };

                            Ok(Response::new(GetBlockSettlementResponse {
                                success: true,
                                message: format!(
                                    "Block settlement {} for chain {} retrieved successfully",
                                    req.block_number, req.chain
                                ),
                                block_settlement: Some(response_settlement),
                                chain: req.chain.clone(),
                            }))
                        }
                        Ok(None) => Ok(Response::new(GetBlockSettlementResponse {
                            success: false,
                            message: format!(
                                "Block settlement {} not found for chain {}",
                                req.block_number, req.chain
                            ),
                            block_settlement: None,
                            chain: req.chain.clone(),
                        })),
                        Err(e) => {
                            error!("Failed to fetch block settlement: {}", e);
                            Ok(Response::new(GetBlockSettlementResponse {
                                success: false,
                                message: format!("Failed to fetch block settlement: {}", e),
                                block_settlement: None,
                                chain: req.chain.clone(),
                            }))
                        }
                    }
                } else {
                    Ok(Response::new(GetBlockSettlementResponse {
                        success: false,
                        message: format!("Chain {} not found in settlements", req.chain),
                        block_settlement: None,
                        chain: req.chain.clone(),
                    }))
                }
            }
            Err(e) => {
                error!("Failed to fetch app instance: {}", e);
                Ok(Response::new(GetBlockSettlementResponse {
                    success: false,
                    message: format!("Failed to fetch app instance: {}", e),
                    block_settlement: None,
                    chain: req.chain.clone(),
                }))
            }
        }
    }

    async fn update_block_settlement(
        &self,
        request: Request<UpdateBlockSettlementRequest>,
    ) -> Result<Response<UpdateBlockSettlementResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            block_number = %req.block_number,
            chain = %req.chain,
            "Received UpdateBlockSettlement request"
        );

        // For now, we'll just return a placeholder response
        // In the future, this would update the settlement status on the blockchain
        Ok(Response::new(UpdateBlockSettlementResponse {
            success: true,
            message: format!(
                "Block settlement update for block {} on chain {} acknowledged",
                req.block_number, req.chain
            ),
            tx_hash: String::new(), // Placeholder for now
        }))
    }
}

pub async fn start_grpc_server(
    socket_path: &str,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Start both UDS and TCP servers concurrently
    let uds_state = state.clone();
    let tcp_state = state.clone();

    let uds_server = async {
        // Remove existing socket file if it exists
        if Path::new(socket_path).exists() {
            std::fs::remove_file(socket_path)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = Path::new(socket_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        }

        let uds = UnixListener::bind(socket_path)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        let uds_stream = UnixListenerStream::new(uds);

        info!("Starting gRPC server on Unix socket: {}", socket_path);

        Server::builder()
            .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(
                uds_state,
            )))
            .serve_with_incoming(uds_stream)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    };

    let tcp_server = async {
        let addr = "0.0.0.0:50051"
            .parse()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        info!("Starting gRPC server on TCP: {}", addr);

        Server::builder()
            .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(
                tcp_state,
            )))
            .serve(addr)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    };

    // Run both servers concurrently
    tokio::try_join!(uds_server, tcp_server)?;

    Ok(())
}
