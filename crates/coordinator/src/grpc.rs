use crate::fetch::fetch_pending_job_from_instances;
use crate::state::SharedState;
use std::path::Path;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{transport::Server, Request, Response, Status};

pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::{
    coordinator_service_server::{CoordinatorService, CoordinatorServiceServer},
    GetJobRequest, GetJobResponse, Job,
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
            "Received GetJob request"
        );

        // Check if the requested job matches the current running agent
        let current_agent = self.state.get_current_agent().await;
        
        if let Some(current) = current_agent {
            if current.developer == req.developer 
                && current.agent == req.agent 
                && current.agent_method == req.agent_method {
                
                // Get current_app_instances for this agent
                let current_instances = self.state.get_current_app_instances().await;
                
                if !current_instances.is_empty() {
                    tracing::info!("Found {} current app_instances for this agent", current_instances.len());
                    
                    // Get a cloned Sui client
                    let mut client = self.state.get_sui_client();
                    
                    // Use index-based fetching to get the job with lowest job_id
                    match fetch_pending_job_from_instances(
                        &mut client,
                        &current_instances,
                        &req.developer,
                        &req.agent,
                        &req.agent_method,
                    ).await {
                        Ok(Some(pending_job)) => {
                            tracing::info!("Found pending job {} using index", pending_job.job_id);
                            
                            // Convert PendingJob to protobuf Job
                            let job = Job {
                                job_id: pending_job.job_id,
                                description: pending_job.description.clone(),
                                developer: pending_job.developer.clone(),
                                agent: pending_job.agent.clone(),
                                agent_method: pending_job.agent_method.clone(),
                                app: pending_job.app.clone(),
                                app_instance: pending_job.app_instance.clone(),
                                app_instance_method: pending_job.app_instance_method.clone(),
                                sequences: pending_job.sequences.clone().unwrap_or_default(),
                                data: pending_job.data.clone(),
                                attempts: pending_job.attempts as u32,
                                created_at: pending_job.created_at,
                                updated_at: pending_job.updated_at,
                            };
                            
                            return Ok(Response::new(GetJobResponse { job: Some(job) }));
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
}

pub async fn start_grpc_server(socket_path: &str, state: SharedState) -> Result<(), Box<dyn std::error::Error>> {
    // Remove existing socket file if it exists
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }

    // Create parent directory if it doesn't exist
    if let Some(parent) = Path::new(socket_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let uds = UnixListener::bind(socket_path)?;
    let uds_stream = UnixListenerStream::new(uds);

    tracing::info!("Starting gRPC server on Unix socket: {}", socket_path);

    Server::builder()
        .add_service(CoordinatorServiceServer::new(CoordinatorServiceImpl::new(state)))
        .serve_with_incoming(uds_stream)
        .await?;

    Ok(())
}