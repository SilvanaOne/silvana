//! Coordinator gRPC Client
//!
//! This module provides a Rust client for the Silvana Coordinator gRPC service.
//! It mirrors the functionality of the TypeScript @silvana-one/agent package.

use anyhow::{Context, Result};
use log::{debug, error, info};

// Include the generated protobuf code
pub mod coordinator {
    tonic::include_proto!("silvana.coordinator.v1");
}

use coordinator::coordinator_service_client::CoordinatorServiceClient;
use coordinator::*;
use tonic::transport::Channel;

/// Coordinator client with session management
pub struct CoordinatorClient {
    client: CoordinatorServiceClient<Channel>,
    session_id: String,
    developer: String,
    agent: String,
    agent_method: String,
    current_job_id: Option<String>,
}

impl CoordinatorClient {
    /// Create a new coordinator client
    pub async fn new(
        coordinator_url: &str,
        session_id: String,
        developer: String,
        agent: String,
        agent_method: String,
    ) -> Result<Self> {
        info!("Connecting to coordinator at {}...", coordinator_url);

        let client = CoordinatorServiceClient::connect(coordinator_url.to_string())
            .await
            .context("Failed to connect to coordinator service")?;

        info!("✓ Connected to coordinator");

        Ok(Self {
            client,
            session_id,
            developer,
            agent,
            agent_method,
            current_job_id: None,
        })
    }

    /// Get a job from the coordinator
    pub async fn get_job(&mut self) -> Result<Option<Job>> {
        debug!("Requesting job from coordinator...");

        let request = GetJobRequest {
            developer: self.developer.clone(),
            agent: self.agent.clone(),
            agent_method: self.agent_method.clone(),
            session_id: self.session_id.clone(),
        };

        let response = self
            .client
            .get_job(request)
            .await
            .context("Failed to get job from coordinator")?
            .into_inner();

        if !response.success {
            debug!("No job available: {}", response.message);
            return Ok(None);
        }

        if let Some(job) = response.job {
            self.current_job_id = Some(job.job_id.clone());
            info!(
                "Received job: ID={}, sequence={}, method={}",
                job.job_id, job.job_sequence, job.app_instance_method
            );
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    /// Complete the current job
    pub async fn complete_job(&mut self) -> Result<()> {
        let job_id = self
            .current_job_id
            .take()
            .context("No active job to complete")?;

        debug!("Completing job {}...", job_id);

        let request = CompleteJobRequest {
            job_id: job_id.clone(),
            session_id: self.session_id.clone(),
        };

        let response = self
            .client
            .complete_job(request)
            .await
            .context("Failed to complete job")?
            .into_inner();

        if !response.success {
            error!("Failed to complete job {}: {}", job_id, response.message);
            anyhow::bail!("Complete job failed: {}", response.message);
        }

        info!("✓ Job {} completed successfully", job_id);
        Ok(())
    }

    /// Fail the current job with an error message
    pub async fn fail_job(&mut self, error_message: &str) -> Result<()> {
        let job_id = self
            .current_job_id
            .take()
            .context("No active job to fail")?;

        debug!("Failing job {} with error: {}", job_id, error_message);

        let request = FailJobRequest {
            job_id: job_id.clone(),
            error_message: error_message.to_string(),
            session_id: self.session_id.clone(),
        };

        let response = self
            .client
            .fail_job(request)
            .await
            .context("Failed to mark job as failed")?
            .into_inner();

        if !response.success {
            error!(
                "Failed to mark job {} as failed: {}",
                job_id, response.message
            );
            anyhow::bail!("Fail job RPC failed: {}", response.message);
        }

        info!("✓ Job {} marked as failed", job_id);
        Ok(())
    }

    /// Send a log message to the coordinator
    pub async fn send_log(&mut self, level: LogLevel, message: &str) -> Result<()> {
        let request = AgentMessageRequest {
            job_id: self.current_job_id.clone(),
            session_id: self.session_id.clone(),
            level: level as i32,
            message: message.to_string(),
        };

        let response = self
            .client
            .agent_message(request)
            .await
            .context("Failed to send agent message")?
            .into_inner();

        if !response.success {
            debug!("Failed to send log to coordinator: {}", response.message);
        }

        Ok(())
    }

    /// Get metadata from the coordinator
    pub async fn get_metadata(&mut self, key: Option<String>) -> Result<Option<Metadata>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .context("No active job for metadata request")?;

        let request = GetMetadataRequest {
            job_id: job_id.clone(),
            session_id: self.session_id.clone(),
            key,
        };

        let response = self
            .client
            .get_metadata(request)
            .await
            .context("Failed to get metadata")?
            .into_inner();

        if !response.success {
            debug!("Failed to get metadata: {}", response.message);
            return Ok(None);
        }

        Ok(response.metadata)
    }
}

// Re-export types for convenience
pub use coordinator::{Job, LogLevel, Metadata};
