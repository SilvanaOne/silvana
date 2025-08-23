use crate::agent::AgentJob;
use crate::fetch::fetch_pending_job_from_instances;
use crate::state::SharedState;
use sui::start_job_tx;
use std::path::Path;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{transport::Server, Request, Response, Status};
use tracing::debug;

pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::{
    coordinator_service_server::{CoordinatorService, CoordinatorServiceServer},
    GetJobRequest, GetJobResponse, Job,
    CompleteJobRequest, CompleteJobResponse,
    FailJobRequest, FailJobResponse,
    SubmitProofRequest, SubmitProofResponse,
    GetSequenceStatesRequest, GetSequenceStatesResponse, SequenceState,
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
        
        tracing::info!(
            developer = %req.developer,
            agent = %req.agent,
            agent_method = %req.agent_method,
            session_id = %req.session_id,
            "Received GetJob request"
        );

        // First check if there's a ready job in the agent database
        if let Some(agent_job) = self.state.get_agent_job_db()
            .get_ready_job(&req.developer, &req.agent, &req.agent_method).await {
            
            tracing::info!(
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
                    tracing::info!("Found {} current app_instances for this agent", current_instances.len());
                    
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
                            tracing::info!("Found pending job {} using index", pending_job.job_sequence);
                            
                            // Execute start_job transaction on Sui before returning the job
                            match start_job_tx(
                                &mut client,
                                &pending_job.app_instance,
                                pending_job.job_sequence,
                            ).await {
                                Ok(tx_digest) => {
                                    tracing::info!("Successfully started job {} with tx: {}", pending_job.job_sequence, tx_digest);
                                    
                                    // Create AgentJob and add it to agent database
                                    let agent_job = AgentJob::new(pending_job, &self.state);
                                    
                                    // Add to pending jobs (job has been started and is being returned to agent)
                                    self.state.get_agent_job_db().add_to_pending(agent_job.clone()).await;
                                    
                                    tracing::info!("Added job {} to agent database with job_id: {}", 
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
                                    tracing::error!("Failed to start job {} on Sui: {}", pending_job.job_sequence, e);
                                    // Don't return the job if start_job transaction failed
                                    // Continue to check for other jobs or return None
                                }
                            }
                        }
                        Ok(None) => {
                            tracing::info!("No pending jobs found using index for {}/{}/{}", 
                                req.developer, req.agent, req.agent_method);
                        }
                        Err(e) => {
                            tracing::error!("Failed to fetch jobs using index: {}", e);
                            // If it's a not found error, remove the stale app_instance
                            if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                                tracing::warn!("Jobs object not found, removing stale app_instances from tracking");
                                for instance in &current_instances {
                                    self.state.remove_app_instance(instance).await;
                                }
                            }
                        }
                    }
                    
                    tracing::info!("No pending jobs found in any current app_instances");
                } else {
                    tracing::info!("No current app_instances for this agent");
                }
            }
        }

        // No matching job found
        tracing::info!("No matching job for {}/{}/{}", req.developer, req.agent, req.agent_method);
        Ok(Response::new(GetJobResponse { job: None }))
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(
            job_id = %req.job_id,
            session_id = %req.session_id,
            "Received CompleteJob request"
        );

        // Get job from agent database
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => job,
            None => {
                tracing::warn!("CompleteJob request for unknown job_id: {}", req.job_id);
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
                tracing::warn!(
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
            tracing::warn!("CompleteJob request from unknown session: {}", req.session_id);
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
                tracing::info!("Successfully completed and removed job {} from agent database", req.job_id);
            } else {
                tracing::warn!("Job {} completed on blockchain but was not found in agent database", req.job_id);
            }
            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {} completed successfully", req.job_id),
            }))
        } else {
            tracing::error!("Failed to complete job {} on blockchain", req.job_id);
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
        
        tracing::info!(
            job_id = %req.job_id,
            error = %req.error_message,
            session_id = %req.session_id,
            "Received FailJob request"
        );

        // Get job from agent database
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => job,
            None => {
                tracing::warn!("FailJob request for unknown job_id: {}", req.job_id);
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
                tracing::warn!(
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
            tracing::warn!("FailJob request from unknown session: {}", req.session_id);
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
            
            tracing::info!("Successfully failed job {}", req.job_id);
            Ok(Response::new(FailJobResponse {
                success: true,
                message: format!("Job {} failed successfully", req.job_id),
            }))
        } else {
            tracing::error!("Failed to fail job {} on blockchain", req.job_id);
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
        
        tracing::info!(
            session_id = %req.session_id,
            block_number = %req.block_number,
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

        // Get job from agent database to validate it exists and get app_instance
        let agent_job = match self.state.get_agent_job_db().get_job_by_id(&req.job_id).await {
            Some(job) => job,
            None => {
                tracing::warn!("SubmitProof request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer 
                || current.agent != agent_job.agent 
                || current.agent_method != agent_job.agent_method {
                tracing::warn!(
                    "SubmitProof request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Err(Status::permission_denied("Job does not belong to requesting session"));
            }
        } else {
            tracing::warn!("SubmitProof request from unknown session: {}", req.session_id);
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Save proof to Walrus DA
        tracing::info!("Saving proof to Walrus DA for job {}", req.job_id);
        let walrus_client = walrus::WalrusClient::new();
        
        let save_params = walrus::SaveToWalrusParams {
            data: req.proof.clone(),
            address: None,
            num_epochs: Some(53), // Maximum epochs for longer retention
        };

        let da_hash = match walrus_client.save_to_walrus(save_params).await {
            Ok(Some(blob_id)) => {
                tracing::info!("Successfully saved proof to Walrus with blob_id: {}", blob_id);
                blob_id
            }
            Ok(None) => {
                tracing::error!("Failed to save proof to Walrus: no blob_id returned");
                return Err(Status::internal("Failed to save proof to data availability layer"));
            }
            Err(e) => {
                tracing::error!("Error saving proof to Walrus: {}", e);
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
                tracing::info!("Successfully submitted proof for job {} with tx: {}", req.job_id, tx_hash);
                
                // Remove job from agent database after successful proof submission
                let removed_job = self.state.get_agent_job_db().complete_job(&req.job_id).await;
                if removed_job.is_some() {
                    tracing::info!("Successfully completed and removed job {} from agent database after proof submission", req.job_id);
                } else {
                    tracing::warn!("Job {} completed on blockchain but was not found in agent database", req.job_id);
                }

                Ok(Response::new(SubmitProofResponse {
                    tx_hash,
                    da_hash,
                }))
            }
            Err(e) => {
                tracing::error!("Failed to submit proof for job {} on blockchain: {}", req.job_id, e);
                Err(Status::internal(format!("Failed to submit proof transaction: {}", e)))
            }
        }
    }

    async fn get_sequence_states(
        &self,
        request: Request<GetSequenceStatesRequest>,
    ) -> Result<Response<GetSequenceStatesResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(
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
                tracing::warn!("GetSequenceStates request for unknown job_id: {}", req.job_id);
                return Err(Status::not_found(format!("Job not found: {}", req.job_id)));
            }
        };

        // Validate that the requesting session matches the current agent for this job
        let current_agent = self.state.get_current_agent(&req.session_id).await;
        if let Some(current) = current_agent {
            if current.developer != agent_job.developer 
                || current.agent != agent_job.agent 
                || current.agent_method != agent_job.agent_method {
                tracing::warn!(
                    "GetSequenceStates request from mismatched session: job belongs to {}/{}/{}, session is {}/{}/{}",
                    agent_job.developer, agent_job.agent, agent_job.agent_method,
                    current.developer, current.agent, current.agent_method
                );
                return Err(Status::permission_denied("Job does not belong to requesting session"));
            }
        } else {
            tracing::warn!("GetSequenceStates request from unknown session: {}", req.session_id);
            return Err(Status::unauthenticated("Invalid session ID"));
        }

        // Query sequence states from the coordinator fetch module
        debug!("Querying sequence states for app_instance={}, sequence={}", agent_job.app_instance, req.sequence);
        let mut sui_client = self.state.get_sui_client();
        
        match crate::fetch::query_sequence_states(&mut sui_client, &agent_job.app_instance, req.sequence).await {
            Ok(fetch_states) => {
                // Convert fetch SequenceState to protobuf SequenceState
                let proto_states: Vec<SequenceState> = fetch_states
                    .into_iter()
                    .map(|state| SequenceState {
                        sequence: state.sequence,
                        state: state.state,
                        data_availability: state.data_availability,
                        optimistic_state: state.optimistic_state,
                        transition_data: state.transition_data,
                    })
                    .collect();

                tracing::info!("Successfully retrieved {} sequence states for sequence {}", proto_states.len(), req.sequence);
                
                Ok(Response::new(GetSequenceStatesResponse {
                    states: proto_states,
                }))
            }
            Err(e) => {
                tracing::error!("Failed to query sequence states for sequence {}: {}", req.sequence, e);
                Err(Status::internal(format!("Failed to query sequence states: {}", e)))
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

        tracing::info!("Starting gRPC server on Unix socket: {}", socket_path);

        Server::builder()
            .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(uds_state)))
            .serve_with_incoming(uds_stream)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    };

    let tcp_server = async {
        let addr = "0.0.0.0:50051".parse()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        tracing::info!("Starting gRPC server on TCP: {}", addr);

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