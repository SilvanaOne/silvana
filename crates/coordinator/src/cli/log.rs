use crate::config::get_default_endpoint;
use crate::error::Result;
use anyhow::anyhow;
use rpc_client::{RpcClientConfig, SilvanaRpcClient};
use tracing::error;

/// Handle the log command - fetch and display session log for a job or session
pub async fn handle_log_command(
    job_id: Option<String>,
    session_id: Option<String>,
    _chain_override: Option<String>,
) -> Result<()> {
    // Validate that at least one parameter is provided
    if job_id.is_none() && session_id.is_none() {
        return Err(anyhow!("Either --job-id or --session-id must be provided").into());
    }

    // Get RPC endpoint using default (respects chain)
    let endpoint = get_default_endpoint();

    // Connect to RPC server
    println!("Connecting to RPC server at {}...", endpoint);
    let mut client = SilvanaRpcClient::new(RpcClientConfig::new(&endpoint))
        .await
        .map_err(|e| {
            error!("Failed to connect to RPC endpoint {}: {}", endpoint, e);
            anyhow!("Failed to connect to RPC endpoint {}: {}", endpoint, e)
        })?;

    // Determine session_id - either provided directly or fetched via job_id
    let session_id = if let Some(sid) = session_id {
        println!("Using session ID: {}", sid);
        println!();
        sid
    } else if let Some(jid) = job_id {
        println!("Fetching session for job {}...", jid);
        println!();

        // Get session_id from job_id
        let session_response = client
            .get_session_by_job_id(&jid)
            .await
            .map_err(|e| {
                error!("Failed to get session by job ID: {}", e);
                anyhow!("Failed to get session by job ID: {}", e)
            })?;

        if !session_response.success {
            println!("Error: {}", session_response.message);
            return Ok(());
        }

        println!("Found session: {}", session_response.session_id);
        println!("App Instance: {}", session_response.app_instance_id);
        println!("App Method: {}", session_response.app_method);
        println!();

        session_response.session_id
    } else {
        unreachable!("Already validated that at least one parameter is provided");
    };

    // Get session finished event
    let event_response = client
        .get_session_finished_event(&session_id)
        .await
        .map_err(|e| {
            error!("Failed to get session finished event: {}", e);
            anyhow!("Failed to get session finished event: {}", e)
        })?;

    if !event_response.success {
        println!("Error: {}", event_response.message);
        return Ok(());
    }

    // Print session finished event details (log last)
    println!("=== Session Finished Event ===");
    println!("Coordinator ID: {}", event_response.coordinator_id);
    println!("Session ID: {}", event_response.session_id);
    println!("Duration: {} ms", event_response.duration);
    println!("Cost: {}", event_response.cost);

    // Format timestamp as human-readable
    let timestamp_secs = event_response.event_timestamp;
    if timestamp_secs > 0 {
        use std::time::{Duration, UNIX_EPOCH};
        let datetime = UNIX_EPOCH + Duration::from_secs(timestamp_secs);
        let datetime_str = chrono::DateTime::<chrono::Utc>::from(datetime)
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        println!("Event Timestamp: {} ({})", timestamp_secs, datetime_str);
    } else {
        println!("Event Timestamp: {}", timestamp_secs);
    }

    println!();
    println!("=== Session Log ===");
    println!("{}", event_response.session_log);

    Ok(())
}
