//! Core coordinator client implementation
//!
//! This module mirrors the TypeScript SDK pattern where:
//! - The client instance is stored as a global singleton (initialized once)
//! - Environment variables are cached once on first access (matching TypeScript SDK)
//! - All env vars are set by coordinator at container startup and don't change
//! - The client is wrapped in Arc<RwLock<>> to allow safe concurrent mutation

use crate::error::{Result, SdkError};
use crate::proto::coordinator::*;
use crate::proto::coordinator::coordinator_service_client::CoordinatorServiceClient;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::env;
use std::sync::Arc;
use tonic::transport::Channel;
use tracing::{debug, error, info};

/// Global coordinator client instance
static COORDINATOR_CLIENT: OnceCell<Arc<RwLock<CoordinatorClient>>> = OnceCell::new();

/// Cached environment variables (initialized once on first access, matching TypeScript SDK pattern)
/// All these values are set by the coordinator at container startup and cached for performance
static SESSION_ID: OnceCell<String> = OnceCell::new();
static CHAIN: OnceCell<Option<String>> = OnceCell::new();
static COORDINATOR_ID: OnceCell<Option<String>> = OnceCell::new();
static SESSION_PRIVATE_KEY: OnceCell<Option<String>> = OnceCell::new();
static DEVELOPER: OnceCell<String> = OnceCell::new();
static AGENT_NAME: OnceCell<String> = OnceCell::new();
static AGENT_METHOD: OnceCell<String> = OnceCell::new();
static COORDINATOR_URL: OnceCell<String> = OnceCell::new();

/// Environment configuration for the coordinator client
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Agent session identifier (always provided by coordinator)
    pub session_id: String,
    /// Blockchain identifier (always provided by coordinator)
    pub chain: Option<String>,
    /// Coordinator instance ID (always provided by coordinator)
    pub coordinator_id: Option<String>,
    /// Session private key (always provided by coordinator)
    pub session_private_key: Option<String>,
    /// Developer identifier (always provided by coordinator)
    pub developer: String,
    /// Agent name (always provided by coordinator)
    pub agent: String,
    /// Agent method name (always provided by coordinator)
    pub agent_method: String,
    /// Coordinator gRPC URL
    pub coordinator_url: String,
}

impl ClientConfig {
    /// Load configuration from environment variables
    /// Caches immutable variables (session_id, developer, etc.) on first access
    /// Reads mutable variables (chain) fresh each time
    pub fn from_env() -> Result<Self> {
        // Cached variables (initialized once, matching TypeScript SDK)
        let session_id = SESSION_ID.get_or_try_init(|| {
            env::var("SESSION_ID")
                .map_err(|_| SdkError::EnvironmentError("SESSION_ID is required".into()))
        })?;

        let coordinator_id = COORDINATOR_ID.get_or_init(|| env::var("COORDINATOR_ID").ok());

        let session_private_key = SESSION_PRIVATE_KEY.get_or_init(|| {
            env::var("SESSION_PRIVATE_KEY").ok()
        });

        let developer = DEVELOPER.get_or_try_init(|| {
            env::var("DEVELOPER")
                .map_err(|_| SdkError::EnvironmentError("DEVELOPER is required".into()))
        })?;

        let agent_name = AGENT_NAME.get_or_try_init(|| {
            env::var("AGENT")
                .map_err(|_| SdkError::EnvironmentError("AGENT is required".into()))
        })?;

        let agent_method_val = AGENT_METHOD.get_or_try_init(|| {
            env::var("AGENT_METHOD")
                .map_err(|_| SdkError::EnvironmentError("AGENT_METHOD is required".into()))
        })?;

        let coordinator_url = COORDINATOR_URL.get_or_init(|| {
            env::var("COORDINATOR_URL")
                .unwrap_or_else(|_| "http://host.docker.internal:50051".into())
        });

        // Cache CHAIN as well - coordinator always provides it at startup
        let chain = CHAIN.get_or_init(|| env::var("CHAIN").ok());

        Ok(ClientConfig {
            session_id: session_id.clone(),
            chain: chain.clone(),
            coordinator_id: coordinator_id.clone(),
            session_private_key: session_private_key.clone(),
            developer: developer.clone(),
            agent: agent_name.clone(),
            agent_method: agent_method_val.clone(),
            coordinator_url: coordinator_url.clone(),
        })
    }
}

/// Coordinator client with session management
pub struct CoordinatorClient {
    pub(crate) client: CoordinatorServiceClient<Channel>,
    pub(crate) config: ClientConfig,
    pub(crate) current_job_id: Option<String>,
}

impl CoordinatorClient {
    /// Create a new coordinator client with the given configuration
    pub async fn new(config: ClientConfig) -> Result<Self> {
        info!("Connecting to coordinator at {}...", config.coordinator_url);

        let client = CoordinatorServiceClient::connect(config.coordinator_url.clone())
            .await?;

        info!("✓ Connected to coordinator");

        Ok(Self {
            client,
            config,
            current_job_id: None,
        })
    }

    /// Initialize the global client instance from environment variables
    pub async fn init() -> Result<()> {
        let config = ClientConfig::from_env()?;
        let client = CoordinatorClient::new(config).await?;
        COORDINATOR_CLIENT
            .set(Arc::new(RwLock::new(client)))
            .map_err(|_| SdkError::Other("Client already initialized".into()))?;
        Ok(())
    }

    /// Get the global client instance, initializing if necessary
    pub async fn global() -> Result<Arc<RwLock<CoordinatorClient>>> {
        if COORDINATOR_CLIENT.get().is_none() {
            Self::init().await?;
        }
        Ok(COORDINATOR_CLIENT
            .get()
            .expect("Client should be initialized")
            .clone())
    }

    /// Get a job from the coordinator
    pub async fn get_job(&mut self) -> Result<Option<Job>> {
        debug!("Requesting job from coordinator...");

        let request = GetJobRequest {
            developer: self.config.developer.clone(),
            agent: self.config.agent.clone(),
            agent_method: self.config.agent_method.clone(),
            session_id: self.config.session_id.clone(),
        };

        let response = self
            .client
            .get_job(request)
            .await?
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
            .ok_or(SdkError::NoActiveJob)?;

        debug!("Completing job {}...", job_id);

        let request = CompleteJobRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
        };

        let response = self
            .client
            .complete_job(request)
            .await?
            .into_inner();

        if !response.success {
            error!("Failed to complete job {}: {}", job_id, response.message);
            return Err(SdkError::OperationFailed(response.message));
        }

        info!("✓ Job {} completed successfully", job_id);
        Ok(())
    }

    /// Fail the current job with an error message
    pub async fn fail_job(&mut self, error_message: &str) -> Result<()> {
        let job_id = self
            .current_job_id
            .take()
            .ok_or(SdkError::NoActiveJob)?;

        debug!("Failing job {} with error: {}", job_id, error_message);

        let request = FailJobRequest {
            job_id: job_id.clone(),
            error_message: error_message.to_string(),
            session_id: self.config.session_id.clone(),
        };

        let response = self
            .client
            .fail_job(request)
            .await?
            .into_inner();

        if !response.success {
            error!(
                "Failed to mark job {} as failed: {}",
                job_id, response.message
            );
            return Err(SdkError::OperationFailed(response.message));
        }

        info!("✓ Job {} marked as failed", job_id);
        Ok(())
    }

    /// Terminate the current job
    pub async fn terminate_job(&mut self) -> Result<()> {
        let job_id = self
            .current_job_id
            .take()
            .ok_or(SdkError::NoActiveJob)?;

        debug!("Terminating job {}...", job_id);

        let request = TerminateJobRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
        };

        let response = self
            .client
            .terminate_job(request)
            .await?
            .into_inner();

        if !response.success {
            return Err(SdkError::OperationFailed(response.message));
        }

        info!("✓ Job {} terminated", job_id);
        Ok(())
    }

    /// Retrieve a secret value
    pub async fn retrieve_secret(&mut self, name: &str) -> Result<Option<String>> {
        let job_id = self
            .current_job_id
            .as_ref()
            .ok_or(SdkError::NoActiveJob)?;

        debug!("Retrieving secret: {}", name);

        let request = RetrieveSecretRequest {
            job_id: job_id.clone(),
            session_id: self.config.session_id.clone(),
            name: name.to_string(),
        };

        let response = self
            .client
            .retrieve_secret(request)
            .await?
            .into_inner();

        if response.success {
            info!("✓ Successfully retrieved secret: {}", name);
            Ok(response.secret_value)
        } else {
            debug!("Failed to retrieve secret: {} - {}", name, response.message);
            Ok(None)
        }
    }

    /// Send an agent message with log level
    pub async fn agent_message(&mut self, level: LogLevel, message: &str) -> Result<()> {
        let request = AgentMessageRequest {
            job_id: self.current_job_id.clone(),
            session_id: self.config.session_id.clone(),
            level: level as i32,
            message: message.to_string(),
        };

        let response = self
            .client
            .agent_message(request)
            .await?
            .into_inner();

        if !response.success {
            debug!("Failed to send log to coordinator: {}", response.message);
        }

        Ok(())
    }

    /// Get the current job ID
    pub fn current_job_id(&self) -> Option<&str> {
        self.current_job_id.as_deref()
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}

// Convenience functions for global client access

/// Get a job from the coordinator using the global client
pub async fn get_job() -> Result<Option<Job>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.get_job().await
}

/// Complete the current job using the global client
pub async fn complete_job() -> Result<()> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.complete_job().await
}

/// Fail the current job using the global client
pub async fn fail_job(error_message: &str) -> Result<()> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.fail_job(error_message).await
}

/// Terminate the current job using the global client
pub async fn terminate_job() -> Result<()> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.terminate_job().await
}

/// Retrieve a secret using the global client
pub async fn get_secret(name: &str) -> Result<Option<String>> {
    let client = CoordinatorClient::global().await?;
    let mut client = client.write();
    client.retrieve_secret(name).await
}