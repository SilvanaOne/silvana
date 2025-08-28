use crate::agent::AgentJob;
use crate::settlement::fetch_pending_job_from_instances;
use crate::proof::analyze_proof_completion;
use crate::state::SharedState;
use sui::start_job_tx;
use std::path::Path;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{debug, info, warn, error};

pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::{
    coordinator_service_server::{CoordinatorService, CoordinatorServiceServer},
    GetJobRequest, GetJobResponse, Job,
    CompleteJobRequest, CompleteJobResponse,
    FailJobRequest, FailJobResponse,
    TerminateJobRequest, TerminateJobResponse,
    SubmitProofRequest, SubmitProofResponse,
    SubmitStateRequest, SubmitStateResponse,
    GetSequenceStatesRequest, GetSequenceStatesResponse, SequenceState,
    ReadDataAvailabilityRequest, ReadDataAvailabilityResponse,
    GetProofRequest, GetProofResponse,
    GetBlockProofRequest, GetBlockProofResponse,
    RetrieveSecretRequest, RetrieveSecretResponse,
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
        if let Some(agent_job) = self.state.get_agent_job_db()
            .get_ready_job(&req.developer, &req.agent, &req.agent_method).await {
            
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
                && current.agent_method == req.agent_method {
                
                // Get current_app_instances for this agent session
                let current_instances = self.state.get_current_app_instances(&req.session_id).await;
                
                if !current_instances.is_empty() {
                    debug!("Found {} current app_instances for this agent", current_instances.len());
                    
                    // Get a cloned Sui client
                    let mut client = self.state.get_sui_client();
                    
                    // Use index-based fetching to get the job with lowest job_sequence
                    match fetch_pending_job_from_instances(
                        &current_instances,
                        &req.developer,
                        &req.agent,
                        &req.agent_method,
                    ).await {
                        Ok(Some(pending_job)) => {
                            debug!("Found pending job {} using index", pending_job.job_sequence);
                            
                            // Execute start_job transaction on Sui before returning the job
                            match start_job_tx(
                                &mut client,
                                &pending_job.app_instance,
                                pending_job.job_sequence,
                            ).await {
                                Ok(tx_digest) => {
                                    debug!("Successfully started job {} with tx: {}", pending_job.job_sequence, tx_digest);
                                    
                                    // Create AgentJob and add it to agent database
                                    let agent_job = AgentJob::new(pending_job, &self.state);
                                    
                                    // Add to pending jobs (job has been started and is being returned to agent)
                                    self.state.get_agent_job_db().add_to_pending(agent_job.clone()).await;
                                    
                                    debug!("Added job {} to agent database with job_id: {}", 
                                        agent_job.job_sequence, agent_job.job_id);
                                    
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
                                Err(e) => {
                                    warn!("Failed to start job {} on Sui: {}", pending_job.job_sequence, e);
                                    // Don't return the job if start_job transaction failed
                                    // Continue to check for other jobs or return None
                                }
                            }
                        }
                        Ok(None) => {
                            debug!("No pending jobs found using index for {}/{}/{}", 
                                req.developer, req.agent, req.agent_method);
                        }
                        Err(e) => {
                            warn!("Failed to fetch jobs using index: {}", e);
                            // If it's a not found error, remove the stale app_instance
                            if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
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
            req.developer,
            req.agent, 
            req.agent_method,
            elapsed
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "CompleteJob request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Ok(Response::new(CompleteJobResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                }));
            }
        } else {
            warn!("CompleteJob request from unknown session: {}", req.session_id);
            return Ok(Response::new(CompleteJobResponse {
                success: false,
                message: "Invalid session ID".to_string(),
            }));
        }

        // Execute complete_job transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        
        let tx_hash = sui_interface.complete_job(&agent_job.app_instance, agent_job.job_sequence).await;
        
        if let Some(tx) = tx_hash {
            // Remove job from agent database
            let removed_job = self.state.get_agent_job_db().complete_job(&req.job_id).await;
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
                warn!("Job {} completed on blockchain but was not found in agent database", req.job_id);
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "FailJob request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
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
        
        let success = sui_interface.fail_job(
            &agent_job.app_instance, 
            agent_job.job_sequence, 
            &req.error_message
        ).await;
        
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => job,
            None => {
                return Ok(Response::new(TerminateJobResponse {
                    success: false,
                    message: format!("Job not found: {}", req.job_id),
                }))
            }
        };

        info!(
            "TerminateJob request for job_id: {} (sequence: {}, app_instance: {})",
            req.job_id, agent_job.job_sequence, agent_job.app_instance
        );

        // Execute terminate_job transaction on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        
        let success = sui_interface.terminate_job(&agent_job.app_instance, agent_job.job_sequence).await;

        if success {
            // Remove job from agent database
            self.state.get_agent_job_db().terminate_job(&req.job_id).await;
            
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
                return Err(Status::invalid_argument("Merged sequences 1 must be sorted"));
            }
        }
        if !req.merged_sequences_2.is_empty() {
            if !req.merged_sequences_2.windows(2).all(|w| w[0] <= w[1]) {
                return Err(Status::invalid_argument("Merged sequences 2 must be sorted"));
            }
        }

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer 
                || current.agent != agent_job.agent 
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "SubmitProof request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Err(Status::permission_denied("Job does not belong to requesting session"));
            }
        } else {
            warn!("SubmitProof request from unknown session: {}", req.session_id);
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
                debug!("Successfully saved proof to Walrus with blob_id: {} (took {}ms)", 
                    blob_id, walrus_save_duration.as_millis());
                blob_id
            }
            Ok(None) => {
                error!("Failed to save proof to Walrus: no blob_id returned");
                return Err(Status::internal("Failed to save proof to data availability layer"));
            }
            Err(e) => {
                error!("Error saving proof to Walrus: {}", e);
                return Err(Status::internal("Failed to save proof to data availability layer"));
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
        
        let tx_result = sui_interface.submit_proof(
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
        ).await;

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
                debug!("Successfully submitted proof for job {} with tx: {}", req.job_id, tx_hash);

                // Spawn merge analysis in background to not delay the response
                let app_instance_id = agent_job.app_instance.clone();
                let job_id_clone = req.job_id.clone();
                
                tokio::spawn(async move {
                    debug!("🔄 Starting background merge analysis for job {}", job_id_clone);
                    
                    // Fetch the AppInstance first
                    match sui::fetch::fetch_app_instance(&app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(e) = analyze_proof_completion(
                                &app_instance
                            ).await {
                                warn!("Failed to analyze proof completion in background: {}", e);
                            } else {
                                debug!("✅ Background merge analysis completed for job {}", job_id_clone);
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch AppInstance {} for merge analysis: {}", app_instance_id, e);
                        }
                    }
                });

                // Return immediately without waiting for merge analysis
                Ok(Response::new(SubmitProofResponse {
                    tx_hash,
                    da_hash,
                }))
            }
            Err(e) => {
                error!("Failed to submit proof for job {} on blockchain: {}", req.job_id, e);
                
                // Even after failure, spawn merge analysis in background
                let app_instance_id = agent_job.app_instance.clone();
                let job_id_clone = req.job_id.clone();
                
                tokio::spawn(async move {
                    info!("🔄 Starting background merge analysis for failed job {}", job_id_clone);
                    
                    // Fetch the AppInstance first
                    match sui::fetch::fetch_app_instance(&app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(analysis_err) = analyze_proof_completion(
                                &app_instance
                            ).await {
                                warn!("Failed to analyze failed proof for merge opportunities: {}", analysis_err);
                            } else {
                                info!("✅ Background merge analysis completed for failed job {}", job_id_clone);
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch AppInstance {} for merge analysis: {}", app_instance_id, e);
                        }
                    }
                });

                let elapsed = start_time.elapsed();
                warn!(
                    "❌ SubmitProof: job_id={}, error={}, time={:?}",
                    req.job_id,
                    e,
                    elapsed
                );
                Err(Status::internal(format!("Failed to submit proof transaction: {}", e)))
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "SubmitState request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Err(Status::permission_denied("Job does not belong to requesting session"));
            }
        } else {
            warn!("SubmitState request from unknown session: {}", req.session_id);
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
                    debug!("Successfully saved state to Walrus with blob_id: {} (took {}ms)", 
                        blob_id, walrus_save_duration.as_millis());
                    Some(blob_id)
                }
                Ok(None) => {
                    error!("Failed to save state to Walrus: no blob_id returned");
                    return Err(Status::internal("Failed to save state to data availability layer"));
                }
                Err(e) => {
                    error!("Error saving state to Walrus: {}", e);
                    return Err(Status::internal("Failed to save state to data availability layer"));
                }
            }
        } else {
            None
        };

        // Call update_state_for_sequence on Sui
        let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
        
        let tx_result = sui_interface.update_state_for_sequence(
            &agent_job.app_instance,
            req.sequence,
            req.new_state_data,
            da_hash.clone(),
        ).await;

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

                Ok(Response::new(SubmitStateResponse {
                    tx_hash,
                    da_hash,
                }))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();
                warn!(
                    "❌ SubmitState: seq={}, job_id={}, error={}, time={:?}",
                    req.sequence,
                    req.job_id,
                    e,
                    elapsed
                );
                Err(Status::internal(format!("Failed to update state transaction: {}", e)))
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => {
                debug!(
                    "Found job: job_id={}, job_sequence={}, app_instance={}, developer={}/{}/{}",
                    req.job_id, job.job_sequence, job.app_instance, job.developer, job.agent, job.agent_method
                );
                job
            }
            None => {
                warn!("GetSequenceStates request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer 
                || current.agent != agent_job.agent 
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "GetSequenceStates request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Err(Status::permission_denied("Job does not belong to requesting session"));
            }
        } else {
            warn!("GetSequenceStates request from unknown session: {}", req.session_id);
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Query sequence states from the coordinator fetch module
        debug!("🔍 Querying sequence states for app_instance={}, sequence={}", agent_job.app_instance, req.sequence);
        let mut sui_client = self.state.get_sui_client();
        
        match sui::fetch::query_sequence_states(&mut sui_client, &agent_job.app_instance, req.sequence).await {
            Ok(fetch_states) => {
                debug!("📦 Retrieved {} sequence states from query", fetch_states.len());
                // Convert fetch SequenceState to protobuf SequenceState
                let proto_states: Vec<SequenceState> = fetch_states
                    .into_iter()
                    .enumerate()
                    .map(|(i, state)| {
                        debug!("State {}: sequence={}, has_state={}, has_data_availability={}", 
                            i, state.sequence, state.state.is_some(), state.data_availability.is_some());
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
                error!("Failed to query sequence states for sequence {}: {}", req.sequence, e);
                Err(Status::internal(format!("Failed to query sequence states: {}", e)))
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
            warn!("ReadDataAvailability request from unknown session: {}", req.session_id);
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
                error!("Failed to read data from Walrus for da_hash {}: {}", req.da_hash, e);
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
            if current.developer != agent_job.developer || 
               current.agent != agent_job.agent || 
               current.agent_method != agent_job.agent_method {
                warn!(
                    "Session mismatch for GetProof: session agent={}/{}/{}, job agent={}/{}/{}",
                    current.developer, current.agent, current.agent_method,
                    agent_job.developer, agent_job.agent, agent_job.agent_method
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
        let app_instance = agent_job.app_instance.clone();

        // Fetch the ProofCalculation using the existing function from sui::fetch::prover
        let proof_calculation = match sui::fetch::fetch_proof_calculation(
            &app_instance,
            req.block_number
        ).await {
            Ok(Some(proof_calc)) => proof_calc,
            Ok(None) => {
                warn!("No ProofCalculation found for block {}", req.block_number);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("No ProofCalculation found for block {}", req.block_number)),
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

        debug!("Found ProofCalculation for block {} with {} proofs", 
            req.block_number, proof_calculation.proofs.len());

        // Find the proof with matching sequences
        let mut da_hash: Option<String> = None;
        for proof in &proof_calculation.proofs {
            debug!("Checking proof: sequences={:?}, looking for={:?}", proof.sequences, req.sequences);
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
                warn!("No proof found for sequences {:?} in block {}", req.sequences, req.block_number);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("No proof found for sequences {:?} in block {}", req.sequences, req.block_number)),
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
                    agent_job.app_instance,
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
                    agent_job.app_instance,
                    req.block_number,
                    req.sequences,
                    da_hash,
                    elapsed
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
                    agent_job.app_instance,
                    req.block_number,
                    req.sequences,
                    da_hash,
                    e,
                    elapsed
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
            if current.developer != agent_job.developer || 
               current.agent != agent_job.agent || 
               current.agent_method != agent_job.agent_method {
                warn!(
                    "Session mismatch for GetBlockProof: session agent={}/{}/{}, job agent={}/{}/{}",
                    current.developer, current.agent, current.agent_method,
                    agent_job.developer, agent_job.agent, agent_job.agent_method
                );
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some("Session does not match job assignment".to_string()),
                }));
            }
        } else {
            warn!("GetBlockProof request from unknown session: {}", req.session_id);
            return Ok(Response::new(GetBlockProofResponse {
                success: false,
                block_proof: None,
                message: Some("Invalid session ID".to_string()),
            }));
        }

        // Use the app_instance from the job
        let app_instance = agent_job.app_instance.clone();

        // Get SUI client from shared state
        let mut client = self.state.get_sui_client();

        // Fetch the AppInstance object
        let formatted_id = if app_instance.starts_with("0x") {
            app_instance.clone()
        } else {
            format!("0x{}", app_instance)
        };

        // Get the AppInstance to find proof_calculations ObjectTable
        let app_request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
            object_id: Some(formatted_id.clone()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["json".to_string()],
            }),
        };

        let app_response = match client.ledger_client().get_object(app_request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!("Failed to fetch AppInstance: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to fetch AppInstance: {}", e)),
                }));
            }
        };

        // Extract proof_calculations ObjectTable ID
        let proof_calc_table_id = if let Some(proto_object) = app_response.object {
            if let Some(json_value) = &proto_object.json {
                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                    if let Some(proofs_field) = struct_value.fields.get("proof_calculations") {
                        if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                            if let Some(table_id_field) = proofs_struct.fields.get("id") {
                                if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                    Some(table_id.clone())
                                } else { None }
                            } else { None }
                        } else { None }
                    } else { None }
                } else { None }
            } else { None }
        } else { None };

        let proof_calc_table_id = match proof_calc_table_id {
            Some(id) => id,
            None => {
                error!("Could not extract proof_calculations table ID");
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some("Failed to extract proof_calculations table".to_string()),
                }));
            }
        };

        debug!("proof_calculations table ID: {}", proof_calc_table_id);

        // List dynamic fields of proof_calculations ObjectTable to find our block
        let list_request = sui_rpc::proto::sui::rpc::v2beta2::ListDynamicFieldsRequest {
            parent: Some(proof_calc_table_id),
            page_size: Some(100),
            page_token: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["field_id".to_string(), "name_value".to_string()],
            }),
        };

        let list_response = match client.live_data_client().list_dynamic_fields(list_request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!("Failed to list proof_calculations dynamic fields: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to list dynamic fields: {}", e)),
                }));
            }
        };

        // Find the field with our block_number
        let mut proof_calc_field_id = None;
        for field in &list_response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                // The name_value is BCS-encoded u64 (block_number)
                if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                    if field_block_number == req.block_number {
                        proof_calc_field_id = field.field_id.clone();
                        break;
                    }
                }
            }
        }

        let proof_calc_field_id = match proof_calc_field_id {
            Some(id) => id,
            None => {
                warn!("No ProofCalculation found for block {}", req.block_number);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("No ProofCalculation found for block {}", req.block_number)),
                }));
            }
        };

        // Fetch the ProofCalculation object
        let proof_calc_request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
            object_id: Some(proof_calc_field_id.clone()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["json".to_string()],
            }),
        };

        let proof_calc_response = match client.ledger_client().get_object(proof_calc_request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!("Failed to fetch ProofCalculation field: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to fetch ProofCalculation: {}", e)),
                }));
            }
        };

        // Extract the actual ProofCalculation object ID from the Field wrapper
        let proof_calc_object_id = if let Some(proof_object) = proof_calc_response.object {
            if let Some(proof_json) = &proof_object.json {
                debug!("ProofCalculation field JSON structure: {:?}", proof_json);
                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &proof_json.kind {
                    if let Some(value_field) = struct_value.fields.get("value") {
                        if let Some(prost_types::value::Kind::StringValue(object_id)) = &value_field.kind {
                            Some(object_id.clone())
                        } else { None }
                    } else { None }
                } else { None }
            } else { None }
        } else { None };

        let proof_calc_object_id = match proof_calc_object_id {
            Some(id) => id,
            None => {
                error!("Could not extract ProofCalculation object ID");
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some("Failed to extract ProofCalculation object ID".to_string()),
                }));
            }
        };

        debug!("Fetching actual ProofCalculation object: {}", proof_calc_object_id);

        // Fetch the actual ProofCalculation object to get block_proof field
        let actual_proof_calc_request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
            object_id: Some(proof_calc_object_id),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["json".to_string()],
            }),
        };

        let actual_proof_calc_response = match client.ledger_client().get_object(actual_proof_calc_request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!("Failed to fetch actual ProofCalculation: {}", e);
                return Ok(Response::new(GetBlockProofResponse {
                    success: false,
                    block_proof: None,
                    message: Some(format!("Failed to fetch ProofCalculation: {}", e)),
                }));
            }
        };

        // Extract the block_proof field from ProofCalculation
        let mut block_proof: Option<String> = None;
        if let Some(proof_calc_object) = actual_proof_calc_response.object {
            if let Some(proof_calc_json) = &proof_calc_object.json {
                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &proof_calc_json.kind {
                    // Get the block_proof field (Option<String>)
                    if let Some(block_proof_field) = struct_value.fields.get("block_proof") {
                        match &block_proof_field.kind {
                            // Direct string value (Some case)
                            Some(prost_types::value::Kind::StringValue(proof)) => {
                                block_proof = Some(proof.clone());
                                debug!("Found block_proof for block {}", req.block_number);
                            }
                            // Null value (None case)
                            Some(prost_types::value::Kind::NullValue(_)) => {
                                debug!("block_proof is null for block {}", req.block_number);
                            }
                            // Struct with Some field (older format compatibility)
                            Some(prost_types::value::Kind::StructValue(option_struct)) => {
                                if let Some(some_field) = option_struct.fields.get("Some") {
                                    if let Some(prost_types::value::Kind::StringValue(proof)) = &some_field.kind {
                                        block_proof = Some(proof.clone());
                                        debug!("Found block_proof in Some variant for block {}", req.block_number);
                                    }
                                }
                            }
                            _ => {
                                debug!("block_proof field has unexpected kind");
                            }
                        }
                    } else {
                        debug!("No block_proof field found in ProofCalculation");
                    }
                } else {
                    debug!("ProofCalculation JSON is not a struct");
                }
            } else {
                debug!("ProofCalculation object has no JSON");
            }
        } else {
            debug!("No ProofCalculation object found");
        }

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
                    message: Some(format!("Block proof not available yet for block {}", req.block_number)),
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
                error!("Failed to read block proof from Walrus for da_hash {}: {}", da_hash, e);
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
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
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
                || current.agent_method != agent_job.agent_method {
                warn!(
                    "RetrieveSecret request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: "Job does not belong to requesting session".to_string(),
                    secret_value: None,
                }));
            }
        } else {
            warn!("RetrieveSecret request from unknown session: {}", req.session_id);
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
                    info!("Successfully retrieved secret '{}' for job {} via Silvana RPC", req.name, req.job_id);
                    Ok(Response::new(RetrieveSecretResponse {
                        success: true,
                        message: inner.message,
                        secret_value: Some(inner.secret_value),
                    }))
                } else {
                    warn!("Failed to retrieve secret '{}' for job {}: {}", req.name, req.job_id, inner.message);
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
}

pub async fn start_grpc_server(socket_path: &str, state: SharedState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
            .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(uds_state)))
            .serve_with_incoming(uds_stream)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    };

    let tcp_server = async {
        let addr = "0.0.0.0:50051".parse()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        info!("Starting gRPC server on TCP: {}", addr);

        Server::builder()
            .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(tcp_state)))
            .serve(addr)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    };

    // Run both servers concurrently
    tokio::try_join!(uds_server, tcp_server)?;

    Ok(())
}
