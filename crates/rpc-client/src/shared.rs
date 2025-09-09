use proto::silvana_events_service_client::SilvanaEventsServiceClient;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tonic::transport::Channel;
use tracing::info;

/// Shared gRPC client instance for thread-safe access across crates
static SHARED_CLIENT: OnceCell<Arc<SilvanaEventsServiceClient<Channel>>> = OnceCell::const_new();

/// Get or create a shared gRPC client instance
/// This ensures only one connection is created and reused across all components
pub async fn get_shared_client() -> Result<SilvanaEventsServiceClient<Channel>, crate::RpcClientError> {
    // Try to get the existing client
    if let Some(client) = SHARED_CLIENT.get() {
        return Ok((**client).clone());
    }

    // If not initialized, create a new one
    let config = crate::RpcClientConfig::from_env();
    
    // Initialize rustls crypto provider (required for HTTPS)
    crate::init_rustls();
    
    info!("Creating shared gRPC client connection to: {}", config.endpoint);
    
    // Create the client
    let client = SilvanaEventsServiceClient::connect(config.endpoint.clone())
        .await
        .map_err(crate::RpcClientError::TransportError)?;
    
    info!("Successfully created shared gRPC client connection");
    
    // Store in the OnceCell
    let arc_client = Arc::new(client.clone());
    let _ = SHARED_CLIENT.set(arc_client);
    
    Ok(client)
}

/// Reset the shared client (useful for testing or reconnection)
#[allow(dead_code)]
pub async fn reset_shared_client() {
    // OnceCell doesn't have a reset method, but we can work around this in tests
    // by using a different approach if needed
}