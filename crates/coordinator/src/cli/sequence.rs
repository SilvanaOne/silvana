use crate::config::get_default_endpoint;
use crate::error::Result;
use anyhow::anyhow;
use rpc_client::{GetEventsByAppInstanceSequenceRequest, RpcClientConfig, SilvanaRpcClient};
use tracing::error;

/// Handle the sequence command - fetch and display all events for a specific sequence
pub async fn handle_sequence_command(
    rpc_url: Option<String>,
    instance: String,
    sequence: u64,
    limit: u32,
    offset: u32,
    _chain_override: Option<String>,
) -> Result<()> {
    // Get RPC URL from provided argument or use default endpoint
    let endpoint = rpc_url.unwrap_or_else(get_default_endpoint);

    // Connect to RPC server using SilvanaRpcClient (same pattern as config.rs)
    println!("Connecting to RPC server at {}...", endpoint);
    let mut client = SilvanaRpcClient::new(RpcClientConfig::new(&endpoint))
        .await
        .map_err(|e| {
            error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
            anyhow!("Failed to connect to RPC endpoint {}: {}", endpoint, e)
        })?;

    println!(
        "Fetching events for sequence {} in app instance {}...",
        sequence, instance
    );
    println!("Limit: {}, Offset: {}", limit, offset);
    println!();

    // Create request
    let request = GetEventsByAppInstanceSequenceRequest {
        app_instance_id: instance.clone(),
        sequence,
        limit: if limit > 0 { Some(limit) } else { None },
        offset: if offset > 0 { Some(offset) } else { None },
    };

    // Fetch events using the inner client
    let response = client
        .inner_mut()
        .get_events_by_app_instance_sequence(request)
        .await
        .map_err(|e| {
            error!("Failed to fetch events: {}", e);
            anyhow!("Failed to fetch events: {}", e)
        })?
        .into_inner();

    // Print response as debug format
    println!("{:#?}", response);

    Ok(())
}