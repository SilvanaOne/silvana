use crate::agent::AgentJob;
use crate::proof::analyze_proof_completion;
use crate::settlement::fetch_pending_job_from_instances;
use crate::state::SharedState;
use std::path::Path;
use sui::start_job_tx;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{Request, Response, Status, transport::Server};
use tracing::{debug, error, info, warn};

pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::{
    AddMetadataRequest, AddMetadataResponse, CompleteJobRequest, CompleteJobResponse,
    CreateAppJobRequest, CreateAppJobResponse, DeleteKvRequest, DeleteKvResponse,
    FailJobRequest, FailJobResponse, GetBlockProofRequest, GetBlockProofResponse,
    GetJobRequest, GetJobResponse, GetKvRequest, GetKvResponse, GetMetadataRequest,
    GetMetadataResponse, GetProofRequest, GetProofResponse, GetSequenceStatesRequest,
    GetSequenceStatesResponse, Job, PurgeSequencesBelowRequest, PurgeSequencesBelowResponse,
    ReadDataAvailabilityRequest, ReadDataAvailabilityResponse, RetrieveSecretRequest,
    RetrieveSecretResponse, SequenceState, SetKvRequest, SetKvResponse, SubmitProofRequest,
    SubmitProofResponse, SubmitStateRequest, SubmitStateResponse, TerminateJobRequest,
    TerminateJobResponse, TryCreateBlockRequest, TryCreateBlockResponse,
    UpdateBlockProofDataAvailabilityRequest, UpdateBlockProofDataAvailabilityResponse,
    UpdateBlockSettlementTxHashRequest, UpdateBlockSettlementTxHashResponse,
    UpdateBlockSettlementTxIncludedInBlockRequest, UpdateBlockSettlementTxIncludedInBlockResponse,
    UpdateBlockStateDataAvailabilityRequest, UpdateBlockStateDataAvailabilityResponse,
    coordinator_service_server::{CoordinatorService, CoordinatorServiceServer},
};

#[derive(Clone)]
pub struct CoordinatorServiceImpl {
    state: SharedState,
}

impl CoordinatorServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
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

        debug!(
            developer = %req.developer,
            agent = %req.agent,
            agent_method = %req.agent_method,
            session_id = %req.session_id,
            "Received GetJob request"
        );

        // First check if there's a ready job in the agent database
        if let Some(agent_job) = self
            .state
            .get_agent_job_db()
            .get_ready_job(&req.developer, &req.agent, &req.agent_method)
            .await
        {
            let elapsed = start_time.elapsed();
            info!(
                "✅ GetJob: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, source=db, time={:?}",
                agent_job.app_instance,
                req.developer,
                req.agent,
                req.agent_method,
                agent_job.job_sequence,
                agent_job.pending_job.app_instance_method,
                elapsed
            );

            // Convert AgentJob to protobuf Job
            let job = Job {
                job_sequence: agent_job.job_sequence,
                description: agent_job.pending_job.description.clone(),
                developer: agent_job.developer.clone(),
                agent: agent_job.agent.clone(),
                agent_method: agent_job.agent_method.clone(),
                app: agent_job.pending_job.app.clone(),
                app_instance: agent_job.app_instance.clone(),
                app_instance_method: agent_job.pending_job.app_instance_method.clone(),
                block_number: agent_job.pending_job.block_number,
                sequences: agent_job.pending_job.sequences.clone().unwrap_or_default(),
                sequences1: agent_job.pending_job.sequences1.clone().unwrap_or_default(),
                sequences2: agent_job.pending_job.sequences2.clone().unwrap_or_default(),
                data: agent_job.pending_job.data.clone(),
                job_id: agent_job.job_id,
                attempts: agent_job.pending_job.attempts as u32,
                created_at: agent_job.pending_job.created_at,
                updated_at: agent_job.pending_job.updated_at,
            };

            return Ok(Response::new(GetJobResponse { job: Some(job) }));
        }

        // If no ready job found, check if the requested job matches the current running agent for this session
        let current_agent = self.state.get_current_agent(&req.session_id).await;

        if let Some(current) = current_agent {
            if current.developer == req.developer
                && current.agent == req.agent
                && current.agent_method == req.agent_method
            {
                // Get current_app_instances for this agent session
                let current_instances = self.state.get_current_app_instances(&req.session_id).await;

                if !current_instances.is_empty() {
                    debug!(
                        "Found {} current app_instances for this agent",
                        current_instances.len()
                    );

                    // Use index-based fetching to get the job with lowest job_sequence
                    match fetch_pending_job_from_instances(
                        &current_instances,
                        &req.developer,
                        &req.agent,
                        &req.agent_method,
                    )
                    .await
                    {
                        Ok(Some(pending_job)) => {
                            debug!("Found pending job {} using index", pending_job.job_sequence);

                            // Execute start_job transaction on Sui before returning the job
                            match start_job_tx(
                                &pending_job.app_instance,
                                pending_job.job_sequence,
                            )
                            .await
                            {
                                Ok(tx_digest) => {
                                    debug!(
                                        "Successfully started job {} with tx: {}",
                                        pending_job.job_sequence, tx_digest
                                    );

                                    // Create AgentJob and add it to agent database
                                    let agent_job = AgentJob::new(pending_job, &self.state);

                                    // Add to pending jobs (job has been started and is being returned to agent)
                                    self.state
                                        .get_agent_job_db()
                                        .add_to_pending(agent_job.clone())
                                        .await;

                                    debug!(
                                        "Added job {} to agent database with job_id: {}",
                                        agent_job.job_sequence, agent_job.job_id
                                    );

                                    let elapsed = start_time.elapsed();
                                    info!(
                                        "✅ GetJob: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, source=sui, time={:?}",
                                        agent_job.app_instance,
                                        req.developer,
                                        req.agent,
                                        req.agent_method,
                                        agent_job.job_sequence,
                                        agent_job.pending_job.app_instance_method,
                                        elapsed
                                    );

                                    // Convert AgentJob to protobuf Job
                                    let job = Job {
                                        job_sequence: agent_job.job_sequence,
                                        description: agent_job.pending_job.description.clone(),
                                        developer: agent_job.developer.clone(),
                                        agent: agent_job.agent.clone(),
                                        agent_method: agent_job.agent_method.clone(),
                                        app: agent_job.pending_job.app.clone(),
                                        app_instance: agent_job.app_instance.clone(),
                                        app_instance_method: agent_job
                                            .pending_job
                                            .app_instance_method
                                            .clone(),
                                        block_number: agent_job.pending_job.block_number,
                                        sequences: agent_job
                                            .pending_job
                                            .sequences
                                            .clone()
                                            .unwrap_or_default(),
                                        sequences1: agent_job
                                            .pending_job
                                            .sequences1
                                            .clone()
                                            .unwrap_or_default(),
                                        sequences2: agent_job
                                            .pending_job
                                            .sequences2
                                            .clone()
                                            .unwrap_or_default(),
                                        data: agent_job.pending_job.data.clone(),
                                        job_id: agent_job.job_id,
                                        attempts: agent_job.pending_job.attempts as u32,
                                        created_at: agent_job.pending_job.created_at,
                                        updated_at: agent_job.pending_job.updated_at,
                                    };

                                    return Ok(Response::new(GetJobResponse { job: Some(job) }));
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to start job {} on Sui: {}",
                                        pending_job.job_sequence, e
                                    );
                                    // Don't return the job if start_job transaction failed
                                    // Continue to check for other jobs or return None
                                }
                            }
                        }
                        Ok(None) => {
                            debug!(
                                "No pending jobs found using index for {}/{}/{}",
                                req.developer, req.agent, req.agent_method
                            );
                        }
                        Err(e) => {
                            warn!("Failed to fetch jobs using index: {}", e);
                            // If it's a not found error, remove the stale app_instance
                            if e.to_string().contains("not found")
                                || e.to_string().contains("NotFound")
                            {
                                warn!("Jobs object not found: {}", current_instances.join(", "));
                                // for instance in &current_instances {
                                //     self.state.remove_app_instance(instance).await;
                                // }
                            }
                        }
                    }

                    debug!("No pending jobs found in any current app_instances");
                } else {
                    debug!("No current app_instances for this agent");
                }
            }
        }

        // No matching job found
        let elapsed = start_time.elapsed();
        info!(
            "⭕ GetJob: dev={}, agent={}/{}, no_job_found, time={:?}",
            req.developer, req.agent, req.agent_method, elapsed
        );
        Ok(Response::new(GetJobResponse { job: None }))
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

        // Execute complete_job transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        let tx_hash = sui_interface
            .complete_job(&agent_job.app_instance, agent_job.job_sequence)
            .await;

        if let Some(tx) = tx_hash {
            // Remove job from agent database
            let removed_job = self
                .state
                .get_agent_job_db()
                .complete_job(&req.job_id)
                .await;
            if removed_job.is_some() {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ CompleteJob: app={}, dev={}, agent={}/{}, job_seq={}, app_method={}, job_id={}, tx={}, time={}ms",
                    agent_job.app_instance,
                    agent_job.developer,
                    agent_job.agent,
                    agent_job.agent_method,
                    agent_job.job_sequence,
                    agent_job.pending_job.app_instance_method,
                    req.job_id,
                    tx,
                    elapsed.as_millis()
                );
            } else {
                warn!(
                    "Job {} completed on blockchain but was not found in agent database",
                    req.job_id
                );
            }
            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {} completed successfully", req.job_id),
            }))
        } else {
            let elapsed = start_time.elapsed();
            warn!(
                "❌ CompleteJob: job_id={}, failed_on_blockchain, time={}ms",
                req.job_id,
                elapsed.as_millis()
            );
            Ok(Response::new(CompleteJobResponse {
                success: false,
                message: format!("Failed to complete job {} on blockchain", req.job_id),
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

        // Execute fail_job transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        let success = sui_interface
            .fail_job(
                &agent_job.app_instance,
                agent_job.job_sequence,
                &req.error_message,
            )
            .await;

        if success {
            // Remove job from agent database
            self.state.get_agent_job_db().fail_job(&req.job_id).await;

            info!("Successfully failed job {}", req.job_id);
            Ok(Response::new(FailJobResponse {
                success: true,
                message: format!("Job {} failed successfully", req.job_id),
            }))
        } else {
            error!("Failed to fail job {} on blockchain", req.job_id);
            Ok(Response::new(FailJobResponse {
                success: false,
                message: format!("Failed to fail job {} on blockchain", req.job_id),
            }))
        }
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

        let success = sui_interface
            .terminate_job(&agent_job.app_instance, agent_job.job_sequence)
            .await;

        if success {
            // Remove job from agent database
            self.state
                .get_agent_job_db()
                .terminate_job(&req.job_id)
                .await;

            info!("Successfully terminated job {}", req.job_id);
            Ok(Response::new(TerminateJobResponse {
                success: true,
                message: format!("Job {} terminated successfully", req.job_id),
            }))
        } else {
            error!("Failed to terminate job {} on blockchain", req.job_id);
            Ok(Response::new(TerminateJobResponse {
                success: false,
                message: format!("Failed to terminate job {} on blockchain", req.job_id),
            }))
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

        // Save proof to Walrus DA
        debug!("Saving proof to Walrus DA for job {}", req.job_id);
        let walrus_client = walrus::WalrusClient::new();

        let save_params = walrus::SaveToWalrusParams {
            data: req.proof.clone(),
            address: None,
            num_epochs: Some(53), // Maximum epochs for longer retention
        };

        let walrus_save_start = std::time::Instant::now();
        let da_hash = match walrus_client.save_to_walrus(save_params).await {
            Ok(Some(blob_id)) => {
                let walrus_save_duration = walrus_save_start.elapsed();
                debug!(
                    "Successfully saved proof to Walrus with blob_id: {} (took {}ms)",
                    blob_id,
                    walrus_save_duration.as_millis()
                );
                blob_id
            }
            Ok(None) => {
                error!("Failed to save proof to Walrus: no blob_id returned");
                return Err(Status::internal(
                    "Failed to save proof to data availability layer",
                ));
            }
            Err(e) => {
                error!("Error saving proof to Walrus: {}", e);
                return Err(Status::internal(
                    "Failed to save proof to data availability layer",
                ));
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

        // Submit proof transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        let tx_result = sui_interface
            .submit_proof(
                &agent_job.app_instance,
                req.block_number,
                sequences.clone(),
                merged_sequences_1,
                merged_sequences_2,
                req.job_id.clone(),
                da_hash.clone(),
                hardware_info.cpu_cores,
                hardware_info.prover_architecture.clone(),
                hardware_info.prover_memory,
                req.cpu_time,
            )
            .await;

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
                    da_hash,
                    tx_hash,
                    elapsed
                );
                debug!(
                    "Successfully submitted proof for job {} with tx: {}",
                    req.job_id, tx_hash
                );

                // Spawn merge analysis in background to not delay the response
                let app_instance_id = agent_job.app_instance.clone();
                let job_id_clone = req.job_id.clone();

                tokio::spawn(async move {
                    debug!(
                        "🔄 Starting background merge analysis for job {}",
                        job_id_clone
                    );

                    // Fetch the AppInstance first
                    match sui::fetch::fetch_app_instance(&app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(e) = analyze_proof_completion(&app_instance).await {
                                warn!("Failed to analyze proof completion in background: {}", e);
                            } else {
                                debug!(
                                    "✅ Background merge analysis completed for job {}",
                                    job_id_clone
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to fetch AppInstance {} for merge analysis: {}",
                                app_instance_id, e
                            );
                        }
                    }
                });

                // Return immediately without waiting for merge analysis
                Ok(Response::new(SubmitProofResponse { tx_hash, da_hash }))
            }
            Err(e) => {
                error!(
                    "Failed to submit proof for job {} on blockchain: {}",
                    req.job_id, e
                );

                // Even after failure, spawn merge analysis in background
                let app_instance_id = agent_job.app_instance.clone();
                let job_id_clone = req.job_id.clone();

                tokio::spawn(async move {
                    info!(
                        "🔄 Starting background merge analysis for failed job {}",
                        job_id_clone
                    );

                    // Fetch the AppInstance first
                    match sui::fetch::fetch_app_instance(&app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(analysis_err) = analyze_proof_completion(&app_instance).await
                            {
                                warn!(
                                    "Failed to analyze failed proof for merge opportunities: {}",
                                    analysis_err
                                );
                            } else {
                                info!(
                                    "✅ Background merge analysis completed for failed job {}",
                                    job_id_clone
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to fetch AppInstance {} for merge analysis: {}",
                                app_instance_id, e
                            );
                        }
                    }
                });

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

        // Save serialized state to Walrus DA if provided
        let da_hash = if let Some(serialized_state) = req.serialized_state {
            debug!("Saving state to Walrus DA for sequence {}", req.sequence);
            let walrus_client = walrus::WalrusClient::new();

            let save_params = walrus::SaveToWalrusParams {
                data: serialized_state,
                address: None,
                num_epochs: Some(53), // Maximum epochs for longer retention
            };

            let walrus_save_start = std::time::Instant::now();
            match walrus_client.save_to_walrus(save_params).await {
                Ok(Some(blob_id)) => {
                    let walrus_save_duration = walrus_save_start.elapsed();
                    debug!(
                        "Successfully saved state to Walrus with blob_id: {} (took {}ms)",
                        blob_id,
                        walrus_save_duration.as_millis()
                    );
                    Some(blob_id)
                }
                Ok(None) => {
                    error!("Failed to save state to Walrus: no blob_id returned");
                    return Err(Status::internal(
                        "Failed to save state to data availability layer",
                    ));
                }
                Err(e) => {
                    error!("Error saving state to Walrus: {}", e);
                    return Err(Status::internal(
                        "Failed to save state to data availability layer",
                    ));
                }
            }
        } else {
            None
        };

        // Call update_state_for_sequence on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

        let tx_result = sui_interface
            .update_state_for_sequence(
                &agent_job.app_instance,
                req.sequence,
                req.new_state_data,
                da_hash.clone(),
            )
            .await;

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
                    da_hash,
                    elapsed
                );

                Ok(Response::new(SubmitStateResponse { tx_hash, da_hash }))
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
                    "Failed to fetch AppInstance: {}", e
                )));
            }
        };
        
        // Query sequence states from the coordinator fetch module
        debug!(
            "🔍 Querying sequence states for app_instance={}, sequence={}",
            app_instance_id, req.sequence
        );
        match sui::fetch::query_sequence_states(
            &app_instance,
            req.sequence,
        )
        .await
        {
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

        // Use walrus client to read the data
        let walrus_client = walrus::WalrusClient::new();
        let read_params = walrus::ReadFromWalrusParams {
            blob_id: req.da_hash.clone(),
        };

        match walrus_client.read_from_walrus(read_params).await {
            Ok(Some(data)) => {
                info!(
                    "✅ ReadDA: session={}, da_hash={}, size={} bytes, time={:?}",
                    req.session_id,
                    req.da_hash,
                    data.len(),
                    start_time.elapsed()
                );
                Ok(Response::new(ReadDataAvailabilityResponse {
                    data: Some(data),
                    success: true,
                    message: format!("Successfully read data for hash: {}", req.da_hash),
                }))
            }
            Ok(None) => {
                warn!("No data found for da_hash: {}", req.da_hash);
                Ok(Response::new(ReadDataAvailabilityResponse {
                    data: None,
                    success: false,
                    message: format!("No data found for hash: {}", req.da_hash),
                }))
            }
            Err(e) => {
                error!(
                    "Failed to read data from Walrus for da_hash {}: {}",
                    req.da_hash, e
                );
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
                    message: Some(format!("Job not found: {}", req.job_id)),
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
                    message: Some("Session does not match job assignment".to_string()),
                }));
            }
        } else {
            warn!("GetProof request from unknown session: {}", req.session_id);
            return Ok(Response::new(GetProofResponse {
                success: false,
                proof: None,
                message: Some("Invalid session ID".to_string()),
            }));
        }

        // Validate sequences are provided
        if req.sequences.is_empty() {
            warn!("GetProof request with empty sequences");
            return Ok(Response::new(GetProofResponse {
                success: false,
                proof: None,
                message: Some("Sequences are required".to_string()),
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
                    message: Some(format!("Failed to fetch AppInstance: {}", e)),
                }));
            }
        };

        // Fetch the ProofCalculation using the existing function from sui::fetch::prover
        let proof_calculation =
            match sui::fetch::fetch_proof_calculation(&app_instance, req.block_number).await {
                Ok(Some(proof_calc)) => proof_calc,
                Ok(None) => {
                    warn!("No ProofCalculation found for block {}", req.block_number);
                    return Ok(Response::new(GetProofResponse {
                        success: false,
                        proof: None,
                        message: Some(format!(
                            "No ProofCalculation found for block {}",
                            req.block_number
                        )),
                    }));
                }
                Err(e) => {
                    error!("Failed to fetch ProofCalculation: {}", e);
                    return Ok(Response::new(GetProofResponse {
                        success: false,
                        proof: None,
                        message: Some(format!("Failed to fetch ProofCalculation: {}", e)),
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
                    message: Some(format!(
                        "No proof found for sequences {:?} in block {}",
                        req.sequences, req.block_number
                    )),
                }));
            }
        };

        debug!("Found proof with da_hash: {}", da_hash);

        // Use walrus client to read the proof data
        let walrus_client = walrus::WalrusClient::new();
        let read_params = walrus::ReadFromWalrusParams {
            blob_id: da_hash.clone(),
        };

        match walrus_client.read_from_walrus(read_params).await {
            Ok(Some(data)) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetProof: app={}, block={}, sequences={:?}, da_hash={}, proof_size={} bytes, time={:?}",
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
                    message: None,
                }))
            }
            Ok(None) => {
                let elapsed = start_time.elapsed();
                warn!(
                    "❌ GetProof: app={}, block={}, sequences={:?}, error=No data found for da_hash: {}, time={:?}",
                    app_instance_id, req.block_number, req.sequences, da_hash, elapsed
                );
                Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("No data found for hash: {}", da_hash)),
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                error!(
                    "❌ GetProof: app={}, block={}, sequences={:?}, da_hash={}, error={}, time={:?}",
                    app_instance_id, req.block_number, req.sequences, da_hash, e, elapsed
                );
                Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("Failed to read proof: {}", e)),
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
                    message: Some(format!("Job not found: {}", req.job_id)),
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
                    message: Some("Session does not match job assignment".to_string()),
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
                message: Some("Invalid session ID".to_string()),
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
                    message: Some(format!("Failed to fetch AppInstance: {}", e)),
                }));
            }
        };

        // Use the fetch_proof_calculation function from prover module
        let proof_calculation = match sui::fetch::fetch_proof_calculation(&app_instance, req.block_number).await {
            Ok(Some(calc)) => calc,
            Ok(None) => {
                warn!("No ProofCalculation found for block {}", req.block_number);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!(
                        "No ProofCalculation found for block {}",
                        req.block_number
                    )),
                }));
            }
            Err(e) => {
                error!("Failed to fetch ProofCalculation: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to fetch ProofCalculation: {}", e)),
                }));
            }
        };

        debug!("Successfully fetched ProofCalculation for block {}", req.block_number);
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
                    message: Some(format!(
                        "Block proof not available yet for block {}",
                        req.block_number
                    )),
                }));
            }
        };

        debug!("Found block proof with da_hash: {}", da_hash);

        // Use walrus client to read the proof data
        let walrus_client = walrus::WalrusClient::new();
        let read_params = walrus::ReadFromWalrusParams {
            blob_id: da_hash.clone(),
        };

        match walrus_client.read_from_walrus(read_params).await {
            Ok(Some(data)) => {
                let elapsed = start_time.elapsed();
                info!(
                    "✅ GetBlockProof: block={}, job_id={}, da_hash={}, time={}ms",
                    req.block_number,
                    req.job_id,
                    da_hash,
                    elapsed.as_millis()
                );
                Ok(Response::new(GetBlockProofResponse {
                    success: true,
                    block_proof: Some(data),
                    message: None,
                }))
            }
            Ok(None) => {
                let elapsed = start_time.elapsed();
                warn!(
                    "❌ GetBlockProof: block={}, job_id={}, da_hash={}, no_data, time={}ms",
                    req.block_number,
                    req.job_id,
                    da_hash,
                    elapsed.as_millis()
                );
                Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("No data found for hash: {}", da_hash)),
                }))
            }
            Err(e) => {
                error!(
                    "Failed to read block proof from Walrus for da_hash {}: {}",
                    da_hash, e
                );
                Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to read proof: {}", e)),
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
        let secret_reference = SecretReference {
            developer: agent_job.developer.clone(),
            agent: agent_job.agent.clone(),
            app: Some(agent_job.pending_job.app.clone()),
            app_instance: Some(agent_job.app_instance.clone()),
            name: Some(req.name.clone()),
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
        match sui::set_kv_tx(
            &agent_job.app_instance,
            req.key,
            req.value,
        )
        .await
        {
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
                    info!(
                        "GetKV for job {}: key '{}' not found",
                        req.job_id, req.key
                    );
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
        match sui::delete_kv_tx(
            &agent_job.app_instance,
            req.key,
        )
        .await
        {
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
                error!("Failed to delete key-value pair for job {}: {}", req.job_id, e);
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
            warn!("AddMetadata request from unknown session: {}", req.session_id);
            return Ok(Response::new(AddMetadataResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                tx_hash: String::new(),
            }));
        }

        // Execute the add_metadata transaction
        match sui::add_metadata_tx(
            &agent_job.app_instance,
            req.key,
            req.value,
        )
        .await
        {
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
                    value: None,
                    app_instance_id: String::new(),
                    silvana_app_name: String::new(),
                    description: None,
                    sequence: 0,
                    admin: String::new(),
                    block_number: 0,
                    previous_block_timestamp: 0,
                    previous_block_last_sequence: 0,
                    last_proved_block_number: 0,
                    last_settled_block_number: 0,
                    settlement_chain: None,
                    settlement_address: None,
                    is_paused: false,
                    created_at: 0,
                    updated_at: 0,
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
                    value: None,
                    app_instance_id: String::new(),
                    silvana_app_name: String::new(),
                    description: None,
                    sequence: 0,
                    admin: String::new(),
                    block_number: 0,
                    previous_block_timestamp: 0,
                    previous_block_last_sequence: 0,
                    last_proved_block_number: 0,
                    last_settled_block_number: 0,
                    settlement_chain: None,
                    settlement_address: None,
                    is_paused: false,
                    created_at: 0,
                    updated_at: 0,
                }));
            }
        } else {
            warn!("GetMetadata request from unknown session: {}", req.session_id);
            return Ok(Response::new(GetMetadataResponse {
                success: false,
                message: "Invalid session ID".to_string(),
                value: None,
                app_instance_id: String::new(),
                silvana_app_name: String::new(),
                description: None,
                sequence: 0,
                admin: String::new(),
                block_number: 0,
                previous_block_timestamp: 0,
                previous_block_last_sequence: 0,
                last_proved_block_number: 0,
                last_settled_block_number: 0,
                settlement_chain: None,
                settlement_address: None,
                is_paused: false,
                created_at: 0,
                updated_at: 0,
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
                    last_settled_block_number: app_instance.last_settled_block_number,
                    settlement_chain: app_instance.settlement_chain.clone(),
                    settlement_address: app_instance.settlement_address.clone(),
                    is_paused: app_instance.is_paused,
                    created_at: app_instance.created_at,
                    updated_at: app_instance.updated_at,
                }))
            }
            Err(e) => {
                error!("Failed to fetch app instance for job {}: {}", req.job_id, e);
                Ok(Response::new(GetMetadataResponse {
                    success: false,
                    message: format!("Failed to fetch app instance: {}", e),
                    value: None,
                    app_instance_id: String::new(),
                    silvana_app_name: String::new(),
                    description: None,
                    sequence: 0,
                    admin: String::new(),
                    block_number: 0,
                    previous_block_timestamp: 0,
                    previous_block_last_sequence: 0,
                    last_proved_block_number: 0,
                    last_settled_block_number: 0,
                    settlement_chain: None,
                    settlement_address: None,
                    is_paused: false,
                    created_at: 0,
                    updated_at: 0,
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
        match sui_interface.try_create_block(&agent_job.app_instance).await {
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
                warn!("UpdateBlockStateDataAvailability request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Update block state data availability
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .update_block_state_data_availability(
                &agent_job.app_instance,
                req.block_number,
                req.state_data_availability,
            )
            .await
        {
            Ok(tx_hash) => {
                info!("✅ UpdateBlockStateDataAvailability successful, tx: {}", tx_hash);
                Ok(Response::new(UpdateBlockStateDataAvailabilityResponse {
                    success: true,
                    message: "Block state DA updated successfully".to_string(),
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
                warn!("UpdateBlockProofDataAvailability request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Update block proof data availability
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .update_block_proof_data_availability(
                &agent_job.app_instance,
                req.block_number,
                req.proof_data_availability,
            )
            .await
        {
            Ok(tx_hash) => {
                info!("✅ UpdateBlockProofDataAvailability successful, tx: {}", tx_hash);
                Ok(Response::new(UpdateBlockProofDataAvailabilityResponse {
                    success: true,
                    message: "Block proof DA updated successfully".to_string(),
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
                warn!("UpdateBlockSettlementTxHash request for unknown job_id: {}", req.job_id);
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
                warn!("UpdateBlockSettlementTxIncludedInBlock request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(UpdateBlockSettlementTxIncludedInBlockResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Update block settlement included in block
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .update_block_settlement_tx_included_in_block(
                &agent_job.app_instance,
                req.block_number,
                req.settled_at,
            )
            .await
        {
            Ok(tx_hash) => {
                info!("✅ UpdateBlockSettlementTxIncludedInBlock successful, tx: {}", tx_hash);
                Ok(Response::new(UpdateBlockSettlementTxIncludedInBlockResponse {
                    success: true,
                    message: "Block settlement included in block updated successfully".to_string(),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to update block settlement included in block: {}", e);
                Ok(Response::new(UpdateBlockSettlementTxIncludedInBlockResponse {
                    success: false,
                    message: format!("Failed to update block settlement included: {}", e),
                    tx_hash: String::new(),
                }))
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
                req.is_settlement_job,
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

    async fn purge_sequences_below(
        &self,
        request: Request<PurgeSequencesBelowRequest>,
    ) -> Result<Response<PurgeSequencesBelowResponse>, Status> {
        let req = request.into_inner();
        info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            threshold_sequence = %req.threshold_sequence,
            "Received PurgeSequencesBelow request"
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
                warn!("PurgeSequencesBelow request for unknown job_id: {}", req.job_id);
                return Ok(Response::new(PurgeSequencesBelowResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                    tx_hash: String::new(),
                }));
            }
        };

        // Purge sequences below threshold
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        match sui_interface
            .purge_sequences_below(&agent_job.app_instance, req.threshold_sequence)
            .await
        {
            Ok(tx_hash) => {
                info!("✅ PurgeSequencesBelow successful, tx: {}", tx_hash);
                Ok(Response::new(PurgeSequencesBelowResponse {
                    success: true,
                    message: format!("Sequences below {} purged successfully", req.threshold_sequence),
                    tx_hash,
                }))
            }
            Err(e) => {
                error!("Failed to purge sequences: {}", e);
                Ok(Response::new(PurgeSequencesBelowResponse {
                    success: false,
                    message: format!("Failed to purge sequences: {}", e),
                    tx_hash: String::new(),
                }))
            }
        }
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
