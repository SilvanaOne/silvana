use anyhow::Result;
use proto::silvana_rpc_service_client::SilvanaRpcServiceClient;
use std::sync::Once;
use tonic::transport::Channel;
use tracing::{error, info};

// Re-export proto types for convenience
pub use proto::{
    Event, EventWithRelevance, GetConfigRequest, GetConfigResponse,
    GetEventsByAppInstanceSequenceRequest, GetEventsByAppInstanceSequenceResponse, GetProofRequest,
    GetProofResponse, GetSessionByJobIdRequest, GetSessionByJobIdResponse,
    GetSessionFinishedEventRequest, GetSessionFinishedEventResponse, ReadBinaryRequest,
    ReadBinaryResponse, RetrieveSecretRequest, RetrieveSecretResponse, SearchEventsRequest,
    SearchEventsResponse, SecretReference, StoreSecretRequest, StoreSecretResponse,
    SubmitEventRequest, SubmitEventResponse, SubmitEventsRequest, SubmitEventsResponse,
    SubmitProofRequest, SubmitProofResponse, WriteBinaryRequest, WriteBinaryResponse,
    WriteConfigRequest, WriteConfigResponse,
};

// Re-export the proto module
pub use proto;

// Shared client module
pub mod shared;

// Initialize rustls crypto provider once
static INIT: Once = Once::new();

pub(crate) fn init_rustls() {
    INIT.call_once(|| {
        // Install the default crypto provider (aws-lc-rs) - same as integration tests
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[derive(thiserror::Error, Debug)]
pub enum RpcClientError {
    #[error("gRPC error: {0}")]
    GrpcError(#[from] tonic::Status),
    #[error("Transport error: {0}")]
    TransportError(#[from] tonic::transport::Error),
    #[error("Invalid URI: {0}")]
    InvalidUri(String),
    #[error("Other error: {0}")]
    Other(String),
}

/// Type alias for the gRPC client
pub type RpcClient = SilvanaRpcServiceClient<Channel>;

/// Configuration for the RPC client
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// RPC server endpoint (e.g., "https://rpc-devnet.silvana.dev")
    pub endpoint: String,
}

impl RpcClientConfig {
    /// Create a new configuration with the given endpoint
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// Create a configuration from environment variables
    pub fn from_env() -> Self {
        // Load .env file if it exists
        dotenvy::dotenv().ok();

        // Get SILVANA_RPC_SERVER from environment, fallback to default if not set
        let endpoint = std::env::var("SILVANA_RPC_SERVER").unwrap_or_else(|_| {
            error!("SILVANA_RPC_SERVER not set, using default");
            "https://rpc-devnet.silvana.dev".to_string()
        });

        Self { endpoint }
    }
}

/// Create a new gRPC client for the Silvana RPC service
pub async fn create_rpc_client(config: RpcClientConfig) -> Result<RpcClient, RpcClientError> {
    // Initialize rustls crypto provider (required for HTTPS)
    init_rustls();

    info!("Connecting to Silvana RPC service at: {}", config.endpoint);

    // Connect using the same method as integration tests
    let client = SilvanaRpcServiceClient::connect(config.endpoint.clone())
        .await
        .map_err(RpcClientError::TransportError)?;

    info!("Successfully connected to Silvana RPC service");

    Ok(client)
}

/// Convenience function to create a client with default configuration from environment
pub async fn create_rpc_client_from_env() -> Result<RpcClient, RpcClientError> {
    create_rpc_client(RpcClientConfig::from_env()).await
}

/// Wrapper for the RPC client with additional convenience methods
#[derive(Clone)]
pub struct SilvanaRpcClient {
    inner: RpcClient,
}

impl SilvanaRpcClient {
    /// Create a new client wrapper
    pub async fn new(config: RpcClientConfig) -> Result<Self, RpcClientError> {
        let client = create_rpc_client(config).await?;
        Ok(Self { inner: client })
    }

    /// Create a new client wrapper using environment configuration
    pub async fn from_env() -> Result<Self, RpcClientError> {
        let client = create_rpc_client_from_env().await?;
        Ok(Self { inner: client })
    }

    /// Get a mutable reference to the inner client
    pub fn inner_mut(&mut self) -> &mut RpcClient {
        &mut self.inner
    }

    /// Get a reference to the inner client
    pub fn inner(&self) -> &RpcClient {
        &self.inner
    }

    /// Submit a single event
    pub async fn submit_event(
        &mut self,
        event: Event,
    ) -> Result<SubmitEventResponse, RpcClientError> {
        let request = SubmitEventRequest { event: Some(event) };
        let response = self.inner.submit_event(request).await?;
        Ok(response.into_inner())
    }

    /// Submit multiple events
    pub async fn submit_events(
        &mut self,
        events: Vec<Event>,
    ) -> Result<SubmitEventsResponse, RpcClientError> {
        let request = SubmitEventsRequest { events };
        let response = self.inner.submit_events(request).await?;
        Ok(response.into_inner())
    }

    /// Store a secret
    pub async fn store_secret(
        &mut self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        secret_value: &str,
        signature: &[u8],
    ) -> Result<StoreSecretResponse, RpcClientError> {
        let reference = SecretReference {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };

        let request = StoreSecretRequest {
            reference: Some(reference),
            secret_value: secret_value.to_string(),
            signature: signature.to_vec(),
        };

        let response = self.inner.store_secret(request).await?;
        Ok(response.into_inner())
    }

    /// Retrieve a secret
    pub async fn retrieve_secret(
        &mut self,
        developer: &str,
        agent: &str,
        app: Option<&str>,
        app_instance: Option<&str>,
        name: Option<&str>,
        signature: &[u8],
    ) -> Result<RetrieveSecretResponse, RpcClientError> {
        let reference = SecretReference {
            developer: developer.to_string(),
            agent: agent.to_string(),
            app: app.map(|s| s.to_string()),
            app_instance: app_instance.map(|s| s.to_string()),
            name: name.map(|s| s.to_string()),
        };

        let request = RetrieveSecretRequest {
            reference: Some(reference),
            signature: signature.to_vec(),
        };

        let response = self.inner.retrieve_secret(request).await?;
        Ok(response.into_inner())
    }

    /// Get configuration for a chain
    pub async fn get_config(&mut self, chain: &str) -> Result<GetConfigResponse, RpcClientError> {
        let request = GetConfigRequest {
            chain: chain.to_string(),
        };

        let response = self.inner.get_config(request).await?;
        Ok(response.into_inner())
    }

    /// Write configuration for a chain
    pub async fn write_config(
        &mut self,
        chain: &str,
        config: std::collections::HashMap<String, String>,
    ) -> Result<WriteConfigResponse, RpcClientError> {
        let request = WriteConfigRequest {
            chain: chain.to_string(),
            config,
        };

        let response = self.inner.write_config(request).await?;
        Ok(response.into_inner())
    }

    /// Search events using full-text search
    pub async fn search_events(
        &mut self,
        request: SearchEventsRequest,
    ) -> Result<SearchEventsResponse, RpcClientError> {
        let response = self.inner.search_events(request).await?;
        Ok(response.into_inner())
    }

    /// Get events by app instance and sequence
    pub async fn get_events_by_app_instance_sequence(
        &mut self,
        request: GetEventsByAppInstanceSequenceRequest,
    ) -> Result<GetEventsByAppInstanceSequenceResponse, RpcClientError> {
        let response = self
            .inner
            .get_events_by_app_instance_sequence(request)
            .await?;
        Ok(response.into_inner())
    }

    /// Submit a proof
    pub async fn submit_proof(
        &mut self,
        request: SubmitProofRequest,
    ) -> Result<SubmitProofResponse, RpcClientError> {
        let response = self.inner.submit_proof(request).await?;
        Ok(response.into_inner())
    }

    /// Get a proof
    pub async fn get_proof(
        &mut self,
        request: GetProofRequest,
    ) -> Result<GetProofResponse, RpcClientError> {
        let response = self.inner.get_proof(request).await?;
        Ok(response.into_inner())
    }

    /// Read binary file from storage
    pub async fn read_binary(
        &mut self,
        file_name: &str,
    ) -> Result<ReadBinaryResponse, RpcClientError> {
        let request = ReadBinaryRequest {
            file_name: file_name.to_string(),
        };

        let response = self.inner.read_binary(request).await?;
        Ok(response.into_inner())
    }

    /// Write binary file to storage
    pub async fn write_binary(
        &mut self,
        data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        expected_sha256: Option<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<WriteBinaryResponse, RpcClientError> {
        let request = WriteBinaryRequest {
            data,
            file_name: file_name.to_string(),
            mime_type: mime_type.to_string(),
            expected_sha256,
            metadata,
        };

        let response = self.inner.write_binary(request).await?;
        Ok(response.into_inner())
    }

    /// Get session info by job ID
    pub async fn get_session_by_job_id(
        &mut self,
        job_id: &str,
    ) -> Result<GetSessionByJobIdResponse, RpcClientError> {
        let request = GetSessionByJobIdRequest {
            job_id: job_id.to_string(),
        };

        let response = self.inner.get_session_by_job_id(request).await?;
        Ok(response.into_inner())
    }

    /// Get session finished event by session ID
    pub async fn get_session_finished_event(
        &mut self,
        session_id: &str,
    ) -> Result<GetSessionFinishedEventResponse, RpcClientError> {
        let request = GetSessionFinishedEventRequest {
            session_id: session_id.to_string(),
        };

        let response = self.inner.get_session_finished_event(request).await?;
        Ok(response.into_inner())
    }
}
