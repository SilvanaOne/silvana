use anyhow::Result;
use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use std::sync::Once;
use tonic::transport::Channel;
use tracing::{error, info};

// Re-export proto types for convenience
pub use proto::{
    AgentMessageEventWithId, AgentTransactionEventWithId, CoordinatorMessageEventWithRelevance,
    Event, GetAgentMessageEventsBySequenceRequest, GetAgentMessageEventsBySequenceResponse,
    GetAgentTransactionEventsBySequenceRequest, GetAgentTransactionEventsBySequenceResponse,
    RetrieveSecretRequest, RetrieveSecretResponse, SearchCoordinatorMessageEventsRequest,
    SearchCoordinatorMessageEventsResponse, SecretReference, StoreSecretRequest,
    StoreSecretResponse, SubmitEventRequest, SubmitEventResponse, SubmitEventsRequest,
    SubmitEventsResponse,
};

// Re-export the proto events module
pub use proto::events;

// Initialize rustls crypto provider once
static INIT: Once = Once::new();

fn init_rustls() {
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
pub type RpcClient = SilvanaEventsServiceClient<Channel>;

/// Configuration for the RPC client
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// RPC server endpoint (e.g., "https://rpc.silvana.dev")
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
            "https://rpc.silvana.dev".to_string()
        });

        Self { endpoint }
    }
}

/// Create a new gRPC client for the Silvana RPC service
pub async fn create_rpc_client(
    config: RpcClientConfig,
) -> Result<RpcClient, RpcClientError> {
    // Initialize rustls crypto provider (required for HTTPS)
    init_rustls();

    info!("Connecting to Silvana RPC service at: {}", config.endpoint);

    // Connect using the same method as integration tests
    let client = SilvanaEventsServiceClient::connect(config.endpoint.clone())
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
    pub async fn submit_event(&mut self, event: Event) -> Result<SubmitEventResponse, RpcClientError> {
        let request = SubmitEventRequest { event: Some(event) };
        let response = self.inner.submit_event(request).await?;
        Ok(response.into_inner())
    }

    /// Submit multiple events
    pub async fn submit_events(&mut self, events: Vec<Event>) -> Result<SubmitEventsResponse, RpcClientError> {
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

    /// Search coordinator message events
    pub async fn search_coordinator_messages(
        &mut self,
        request: SearchCoordinatorMessageEventsRequest,
    ) -> Result<SearchCoordinatorMessageEventsResponse, RpcClientError> {
        let response = self.inner.search_coordinator_message_events(request).await?;
        Ok(response.into_inner())
    }

    /// Get agent message events by sequence
    pub async fn get_agent_messages_by_sequence(
        &mut self,
        request: GetAgentMessageEventsBySequenceRequest,
    ) -> Result<GetAgentMessageEventsBySequenceResponse, RpcClientError> {
        let response = self.inner.get_agent_message_events_by_sequence(request).await?;
        Ok(response.into_inner())
    }

    /// Get agent transaction events by sequence
    pub async fn get_agent_transactions_by_sequence(
        &mut self,
        request: GetAgentTransactionEventsBySequenceRequest,
    ) -> Result<GetAgentTransactionEventsBySequenceResponse, RpcClientError> {
        let response = self.inner.get_agent_transaction_events_by_sequence(request).await?;
        Ok(response.into_inner())
    }
}