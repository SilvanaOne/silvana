use tonic::transport::Channel;
use tracing::{info, error};

// Re-export the events module from the proto crate
pub use proto::events;
use proto::events::silvana_events_service_client::SilvanaEventsServiceClient;

pub type RpcClient = SilvanaEventsServiceClient<Channel>;

/// Create a new gRPC client for the Silvana RPC service
pub async fn create_rpc_client() -> Result<RpcClient, Box<dyn std::error::Error + Send + Sync>> {
    // Load .env file if it exists
    dotenv::dotenv().ok();
    
    // Get SILVANA_RPC_SERVER from environment, fallback to default if not set
    let server_addr = std::env::var("SILVANA_RPC_SERVER")
        .unwrap_or_else(|_| {
            error!("SILVANA_RPC_SERVER not set, using default");
            "https://rpc.silvana.dev".to_string()
        });
    
    info!("Connecting to Silvana RPC service at: {}", server_addr);
    
    let client = SilvanaEventsServiceClient::connect(server_addr).await?;
    
    info!("Successfully connected to Silvana RPC service");
    
    Ok(client)
}