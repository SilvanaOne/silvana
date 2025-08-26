use crate::agent::AgentJob;
use crate::coordination::ProofCalculation;
use crate::fetch::fetch_pending_job_from_instances;
use crate::fetch::app_instance::AppInstance;
use crate::merge::analyze_and_create_merge_jobs_with_blockchain_data;
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
    SubmitProofRequest, SubmitProofResponse,
    SubmitStateRequest, SubmitStateResponse,
    GetSequenceStatesRequest, GetSequenceStatesResponse, SequenceState,
    ReadDataAvailabilityRequest, ReadDataAvailabilityResponse,
    GetProofRequest, GetProofResponse,
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
        let req = request.into_inner();
        
        info!(
            developer = %req.developer,
            agent = %req.agent,
            agent_method = %req.agent_method,
            session_id = %req.session_id,
            "Received GetJob request"
        );

        // First check if there's a ready job in the agent database
        if let Some(agent_job) = self.state.get_agent_job_db()
            .get_ready_job(&req.developer, &req.agent, &req.agent_method).await {
            
            info!(
                "Returning ready job {} from agent database",
                agent_job.job_id
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
                sequences: agent_job.pending_job.sequences.clone().unwrap_or_default(),
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
                    info!("Found {} current app_instances for this agent", current_instances.len());
                    
                    // Get a cloned Sui client
                    let mut client = self.state.get_sui_client();
                    
                    // Use index-based fetching to get the job with lowest job_sequence
                    match fetch_pending_job_from_instances(
                        &mut client,
                        &current_instances,
                        &req.developer,
                        &req.agent,
                        &req.agent_method,
                    ).await {
                        Ok(Some(pending_job)) => {
                            info!("Found pending job {} using index", pending_job.job_sequence);
                            
                            // Execute start_job transaction on Sui before returning the job
                            match start_job_tx(
                                &mut client,
                                &pending_job.app_instance,
                                pending_job.job_sequence,
                            ).await {
                                Ok(tx_digest) => {
                                    info!("Successfully started job {} with tx: {}", pending_job.job_sequence, tx_digest);
                                    
                                    // Create AgentJob and add it to agent database
                                    let agent_job = AgentJob::new(pending_job, &self.state);
                                    
                                    // Add to pending jobs (job has been started and is being returned to agent)
                                    self.state.get_agent_job_db().add_to_pending(agent_job.clone()).await;
                                    
                                    info!("Added job {} to agent database with job_id: {}", 
                                        agent_job.job_sequence, agent_job.job_id);
                                    
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
                                        sequences: agent_job.pending_job.sequences.clone().unwrap_or_default(),
                                        data: agent_job.pending_job.data.clone(),
                                        job_id: agent_job.job_id,
                                        attempts: agent_job.pending_job.attempts as u32,
                                        created_at: agent_job.pending_job.created_at,
                                        updated_at: agent_job.pending_job.updated_at,
                                    };
                                    
                                    return Ok(Response::new(GetJobResponse { job: Some(job) }));
                                }
                                Err(e) => {
                                    error!("Failed to start job {} on Sui: {}", pending_job.job_sequence, e);
                                    // Don't return the job if start_job transaction failed
                                    // Continue to check for other jobs or return None
                                }
                            }
                        }
                        Ok(None) => {
                            info!("No pending jobs found using index for {}/{}/{}", 
                                req.developer, req.agent, req.agent_method);
                        }
                        Err(e) => {
                            error!("Failed to fetch jobs using index: {}", e);
                            // If it's a not found error, remove the stale app_instance
                            if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                                error!("Jobs object not found: {}", current_instances.join(", "));
                                // for instance in &current_instances {
                                //     self.state.remove_app_instance(instance).await;
                                // }
                            }
                        }
                    }
                    
                    info!("No pending jobs found in any current app_instances");
                } else {
                    info!("No current app_instances for this agent");
                }
            }
        }

        // No matching job found
        info!("No matching job for {}/{}/{}", req.developer, req.agent, req.agent_method);
        Ok(Response::new(GetJobResponse { job: None }))
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let req = request.into_inner();
        
        info!(
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
        let sui_client = self.state.get_sui_client();
        let mut sui_interface = crate::sui_interface::SuiJobInterface::new(sui_client);
        
        let success = sui_interface.complete_job(&agent_job.app_instance, agent_job.job_sequence).await;
        
        if success {
            // Remove job from agent database
            let removed_job = self.state.get_agent_job_db().complete_job(&req.job_id).await;
            if removed_job.is_some() {
                info!("Successfully completed and removed job {} from agent database", req.job_id);
            } else {
                warn!("Job {} completed on blockchain but was not found in agent database", req.job_id);
            }
            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {} completed successfully", req.job_id),
            }))
        } else {
            error!("Failed to complete job {} on blockchain", req.job_id);
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
        let sui_client = self.state.get_sui_client();
        let mut sui_interface = crate::sui_interface::SuiJobInterface::new(sui_client);
        
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

    async fn submit_proof(
        &self,
        request: Request<SubmitProofRequest>,
    ) -> Result<Response<SubmitProofResponse>, Status> {
        let req = request.into_inner();
        
        // Get job early to log app_instance
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => job,
            None => {
                warn!("SubmitProof request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };
        
        info!(
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
        info!("Saving proof to Walrus DA for job {}", req.job_id);
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
                info!("Successfully saved proof to Walrus with blob_id: {} (took {}ms)", 
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
        let mut sui_fetch_client = self.state.get_sui_client();
        let proof_calculations = crate::fetch::fetch_proof_calculations(
            &mut sui_fetch_client,
            &agent_job.app_instance,
            req.block_number
        ).await
        .map_err(|e| Status::internal(format!("Failed to fetch ProofCalculation: {}", e)))?;
        
        // Get the first ProofCalculation for this block (should exist)
        let proof_calc_info = proof_calculations.first()
            .ok_or_else(|| Status::internal(format!("No ProofCalculation found for block {}", req.block_number)))?;
        
        // Create ProofCalculation with real data from blockchain
        let _proof_calc = ProofCalculation {
            block_number: req.block_number,
            start_sequence: proof_calc_info.start_sequence,
            end_sequence: proof_calc_info.end_sequence,
            proofs: vec![],  // Individual proofs will be fetched later in merge analysis
            block_proof: proof_calc_info.block_proof.clone(),
            is_finished: proof_calc_info.is_finished,
        };

        // Submit proof transaction on Sui
        let sui_client = self.state.get_sui_client();
        let mut sui_interface = crate::sui_interface::SuiJobInterface::new(sui_client);
        
        let tx_result = sui_interface.submit_proof(
            &agent_job.app_instance,
            req.block_number,
            sequences,
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
                info!("Successfully submitted proof for job {} with tx: {}", req.job_id, tx_hash);

                // Spawn merge analysis in background to not delay the response
                let app_instance_id = agent_job.app_instance.clone();
                let mut client_clone = self.state.get_sui_client();
                let job_id_clone = req.job_id.clone();
                
                tokio::spawn(async move {
                    info!("🔄 Starting background merge analysis for job {}", job_id_clone);
                    
                    // Fetch the AppInstance first
                    match crate::fetch::fetch_app_instance(&mut client_clone, &app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(e) = analyze_proof_completion(
                                &app_instance,
                                &mut client_clone
                            ).await {
                                warn!("Failed to analyze proof completion in background: {}", e);
                            } else {
                                info!("✅ Background merge analysis completed for job {}", job_id_clone);
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
                let mut client_clone = self.state.get_sui_client();
                let job_id_clone = req.job_id.clone();
                
                tokio::spawn(async move {
                    info!("🔄 Starting background merge analysis for failed job {}", job_id_clone);
                    
                    // Fetch the AppInstance first
                    match crate::fetch::fetch_app_instance(&mut client_clone, &app_instance_id).await {
                        Ok(app_instance) => {
                            if let Err(analysis_err) = analyze_proof_completion(
                                &app_instance,
                                &mut client_clone
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

                Err(Status::internal(format!("Failed to submit proof transaction: {}", e)))
            }
        }
    }

    async fn submit_state(
        &self,
        request: Request<SubmitStateRequest>,
    ) -> Result<Response<SubmitStateResponse>, Status> {
        let req = request.into_inner();
        
        info!(
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
            info!("Saving state to Walrus DA for sequence {}", req.sequence);
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
                    info!("Successfully saved state to Walrus with blob_id: {} (took {}ms)", 
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
        let sui_client = self.state.get_sui_client();
        let mut sui_interface = crate::sui_interface::SuiJobInterface::new(sui_client);
        
        let tx_result = sui_interface.update_state_for_sequence(
            &agent_job.app_instance,
            req.sequence,
            req.new_state_data,
            da_hash.clone(),
        ).await;

        match tx_result {
            Ok(tx_hash) => {
                info!("Successfully updated state for sequence {} with tx: {}", req.sequence, tx_hash);

                Ok(Response::new(SubmitStateResponse {
                    tx_hash,
                    da_hash,
                }))
            }
            Err(e) => {
                error!("Failed to update state for sequence {} on blockchain: {}", req.sequence, e);
                Err(Status::internal(format!("Failed to update state transaction: {}", e)))
            }
        }
    }

    async fn get_sequence_states(
        &self,
        request: Request<GetSequenceStatesRequest>,
    ) -> Result<Response<GetSequenceStatesResponse>, Status> {
        let req = request.into_inner();
        
        info!(
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
        
        match crate::fetch::query_sequence_states(&mut sui_client, &agent_job.app_instance, req.sequence).await {
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

                info!("✅ Successfully retrieved {} sequence states for sequence {}", proto_states.len(), req.sequence);
                
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
        let req = request.into_inner();
        
        info!(
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
                info!("Successfully read data from Walrus for da_hash: {}", req.da_hash);
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
        let req = request.into_inner();
        
        info!(
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
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
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
                error!("Failed to extract proof_calculations table ID from AppInstance");
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some("Failed to extract proof_calculations table ID".to_string()),
                }));
            }
        };

        debug!("Found proof_calculations table ID: {}", proof_calc_table_id);

        // Fetch the ProofCalculation for the given block_number
        // First, list dynamic fields to find the field for our block_number
        let list_request = sui_rpc::proto::sui::rpc::v2beta2::ListDynamicFieldsRequest {
            parent: Some(proof_calc_table_id.clone()),
            page_size: Some(100),
            page_token: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["field_id".to_string(), "name_value".to_string()],
            }),
        };

        let list_response = match client.live_data_client().list_dynamic_fields(list_request).await {
            Ok(resp) => resp.into_inner(),
            Err(e) => {
                error!("Failed to list dynamic fields: {}", e);
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
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
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
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
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
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
                        } else { 
                            debug!("value field is not a string: {:?}", value_field);
                            None 
                        }
                    } else { 
                        debug!("No 'value' field found in struct");
                        None 
                    }
                } else { None }
            } else { None }
        } else { None };

        let proof_calc_object_id = match proof_calc_object_id {
            Some(id) => {
                debug!("Extracted ProofCalculation object ID: {}", id);
                id
            },
            None => {
                error!("Failed to extract ProofCalculation object ID");
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some("Failed to extract ProofCalculation object ID".to_string()),
                }));
            }
        };

        // Fetch the actual ProofCalculation object
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
                return Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("Failed to fetch ProofCalculation: {}", e)),
                }));
            }
        };

        // Extract the proofs VecMap and find our proof by sequences
        let mut da_hash: Option<String> = None;
        if let Some(proof_calc_object) = actual_proof_calc_response.object {
            if let Some(proof_calc_json) = &proof_calc_object.json {
                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &proof_calc_json.kind {
                    // Get the proofs VecMap
                    if let Some(proofs_field) = struct_value.fields.get("proofs") {
                        if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                            if let Some(contents) = proofs_struct.fields.get("contents") {
                                if let Some(prost_types::value::Kind::ListValue(list_value)) = &contents.kind {
                                    debug!("Found {} proofs in ProofCalculation", list_value.values.len());
                                    // Search through the VecMap entries for matching sequences
                                    'outer: for (i, entry) in list_value.values.iter().enumerate() {
                                        if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &entry.kind {
                                            // Check the key (sequences)
                                            if let Some(key_field) = entry_struct.fields.get("key") {
                                                if let Some(prost_types::value::Kind::ListValue(key_list)) = &key_field.kind {
                                                    let mut key_sequences = Vec::new();
                                                    for seq_val in &key_list.values {
                                                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &seq_val.kind {
                                                            if let Ok(seq) = seq_str.parse::<u64>() {
                                                                key_sequences.push(seq);
                                                            }
                                                        } else if let Some(prost_types::value::Kind::NumberValue(seq_num)) = &seq_val.kind {
                                                            key_sequences.push(seq_num.round() as u64);
                                                        }
                                                    }
                                                    
                                                    debug!("Proof {}: sequences={:?}, looking for={:?}", i, key_sequences, req.sequences);
                                                    
                                                    // Check if sequences match
                                                    if key_sequences == req.sequences {
                                                        info!("Found matching proof for sequences {:?}", req.sequences);
                                                        // Found our proof! Extract da_hash
                                                        if let Some(value_field) = entry_struct.fields.get("value") {
                                                            if let Some(prost_types::value::Kind::StructValue(proof_struct)) = &value_field.kind {
                                                                // Don't check job_id - the proof may have been calculated by a different job
                                                                // We only care that app_instance, block_number, and sequences match
                                                                
                                                                // Get da_hash from the Proof (Option<String> field)
                                                                if let Some(da_hash_field) = proof_struct.fields.get("da_hash") {
                                                                    match &da_hash_field.kind {
                                                                        // Direct string value (Some case)
                                                                        Some(prost_types::value::Kind::StringValue(hash)) => {
                                                                            da_hash = Some(hash.clone());
                                                                            info!("Found da_hash: {}", hash);
                                                                            break 'outer;
                                                                        }
                                                                        // Null value (None case)
                                                                        Some(prost_types::value::Kind::NullValue(_)) => {
                                                                            debug!("da_hash is null for this proof");
                                                                            // Continue searching for another proof with da_hash
                                                                        }
                                                                        // Struct with Some field (older format compatibility)
                                                                        Some(prost_types::value::Kind::StructValue(option_struct)) => {
                                                                            if let Some(some_field) = option_struct.fields.get("Some") {
                                                                                if let Some(prost_types::value::Kind::StringValue(hash)) = &some_field.kind {
                                                                                    da_hash = Some(hash.clone());
                                                                                    info!("Found da_hash in Some variant: {}", hash);
                                                                                    break 'outer;
                                                                                }
                                                                            }
                                                                        }
                                                                        _ => {
                                                                            warn!("Unexpected da_hash field type: {:?}", da_hash_field.kind);
                                                                        }
                                                                    }
                                                                } else {
                                                                    warn!("da_hash field not found in proof struct");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

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
                info!("Successfully read proof from Walrus for da_hash: {}", da_hash);
                Ok(Response::new(GetProofResponse {
                    success: true,
                    proof: Some(data),
                    message: None,
                }))
            }
            Ok(None) => {
                warn!("No data found for da_hash: {}", da_hash);
                Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
                    message: Some(format!("No data found for hash: {}", da_hash)),
                }))
            }
            Err(e) => {
                error!("Failed to read proof from Walrus for da_hash {}: {}", da_hash, e);
                Ok(Response::new(GetProofResponse {
                    success: false,
                    proof: None,
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

// Helper function to analyze proof completion and determine next action
// Refactored to take AppInstance struct with all necessary data
pub async fn analyze_proof_completion(
    app_instance: &AppInstance,
    client: &mut sui_rpc::Client,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let analysis_start = std::time::Instant::now();
    info!("🔍 Starting proof completion analysis for app: {}", app_instance.silvana_app_name);
    
    let last_proved_block_number = app_instance.last_proved_block_number;
    let current_block_number = app_instance.block_number;
    let previous_block_last_sequence = app_instance.previous_block_last_sequence;
    let current_sequence = app_instance.sequence;
    let app_instance_id = &app_instance.id;
    
    info!("📊 AppInstance status: last_proved_block={}, current_block={}, prev_block_last_seq={}, current_seq={}", 
        last_proved_block_number, current_block_number, previous_block_last_sequence, current_sequence);
    
    // Check if we're at the start of a new block with no sequences processed yet
    if last_proved_block_number + 1 == current_block_number && 
       previous_block_last_sequence + 1 == current_sequence {
        let analysis_duration = analysis_start.elapsed();
        info!("✅ We're at the start of block {} with no sequences processed yet (waiting for sequences) - took {:.2}s", 
            current_block_number, analysis_duration.as_secs_f64());
        return Ok(());
    }
    
    
    // Step 2: Process blocks in order from last_proved_block_number + 1 to current_block_number
    let blocks_to_analyze = (current_block_number - last_proved_block_number).saturating_sub(0);
    info!("🔄 Processing {} blocks from {} to {} for merge opportunities", 
        blocks_to_analyze, last_proved_block_number + 1, current_block_number);
    
    let mut analyzed_blocks = 0;
    for block_number in (last_proved_block_number + 1)..=current_block_number {
        analyzed_blocks += 1;
        let block_start = std::time::Instant::now();
        info!("📦 Analyzing block {} for merge opportunities", block_number);
        
        // Fetch ProofCalculations for this block
        let proof_calculations = match crate::fetch::fetch_proof_calculations(
            client,
            app_instance_id,
            block_number
        ).await {
            Ok(proofs) => {
                if proofs.is_empty() {
                    info!("⏭️ No ProofCalculations found for block {}, skipping", block_number);
                    continue;
                }
                info!("📊 Found {} ProofCalculation(s) for block {}", proofs.len(), block_number);
                proofs
            }
            Err(e) => {
                warn!("Failed to fetch ProofCalculations for block {}: {}, skipping", block_number, e);
                continue;
            }
        };
        
        // Use the first ProofCalculation for this block
        let proof_calc_info = &proof_calculations[0];
        
        // Create a ProofCalculation struct for analysis
        let proof_calc = ProofCalculation {
            block_number,
            start_sequence: proof_calc_info.start_sequence,
            end_sequence: proof_calc_info.end_sequence,
            proofs: vec![], // Will be populated by merge analysis
            block_proof: proof_calc_info.block_proof.clone(),
            is_finished: proof_calc_info.is_finished,
        };
        
        // Analyze this block for merge opportunities
        // Use empty da_hash since we're just looking for merge opportunities
        if let Err(e) = analyze_and_create_merge_jobs_with_blockchain_data(
            &proof_calc,
            app_instance_id,
            client,
            "", // No specific da_hash for general analysis
        ).await {
            let block_duration = block_start.elapsed();
            warn!("Failed to analyze block {} for merges in {:.2}s: {}", 
                block_number, block_duration.as_secs_f64(), e);
            // Continue to next block even if this one fails
        } else {
            let block_duration = block_start.elapsed();
            info!("✅ Successfully analyzed block {} for merge opportunities in {:.2}s", 
                block_number, block_duration.as_secs_f64());
        }
    }
    
    let analysis_duration = analysis_start.elapsed();
    info!("🎉 Completed proof completion analysis for {} blocks in {:.2}s", 
        analyzed_blocks, analysis_duration.as_secs_f64());
    Ok(())
}