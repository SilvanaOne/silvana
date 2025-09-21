use anyhow::{Result, Context};
use sui_rpc::proto::sui::rpc::v2 as proto;
use tracing::{debug, warn};
use crate::state::SharedSuiState;
use std::env;

/// Format a number with thousands separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}

/// Information about the current network/epoch
#[derive(Debug)]
pub struct NetworkInfo {
    pub epoch: u64,
    pub protocol_version: u64,
    pub reference_gas_price: u64,
    pub epoch_start_timestamp_ms: u64,
    pub epoch_duration_ms: u64,
    pub safe_mode: bool,
    pub validator_count: usize,
    pub stake_subsidy_start_epoch: u64,
    pub min_validator_count: u64,
    pub max_validator_count: u64,
    pub chain_name: Option<String>,
    pub chain_id: Option<String>,
}

/// Service information from the RPC node
#[derive(Debug)]
pub struct ServiceInfo {
    pub chain_name: Option<String>,
    pub chain_id: Option<String>,
    pub epoch: Option<u64>,
    pub checkpoint_height: Option<u64>,
    pub server_version: Option<String>,
}

/// Get service info (chain name and ID) from the Sui network
pub async fn get_service_info() -> Result<(Option<String>, Option<String>)> {
    let full_info = get_service_info_full().await?;
    Ok((full_info.chain_name, full_info.chain_id))
}

/// Get full service info from the Sui network
pub async fn get_service_info_full() -> Result<ServiceInfo> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let mut ledger = client.ledger_client();
    
    let req = proto::GetServiceInfoRequest::default();
    
    debug!("Fetching service info from gRPC...");
    let resp = ledger.get_service_info(req).await
        .context("Failed to get service info")?;
    
    let info = resp.into_inner();
    
    // Remove debug output after debugging
    // println!("DEBUG GetServiceInfoResponse: {:?}", info);
    
    // If chain name is "unknown", don't use it
    let chain_name = info.chain.filter(|c| c != "unknown");
    
    Ok(ServiceInfo {
        chain_name,
        chain_id: info.chain_id,
        epoch: info.epoch,
        checkpoint_height: info.checkpoint_height,
        server_version: info.server,
    })
}

/// Verify that SUI_CHAIN env var matches the actual chain
pub async fn verify_chain_config() -> Result<()> {
    let env_chain = env::var("SUI_CHAIN").unwrap_or_else(|_| "unknown".to_string());
    let (actual_chain, chain_id) = get_service_info().await?;
    
    if let Some(ref chain) = actual_chain {
        // Only warn if RPC returns a meaningful chain name (not "unknown")
        if chain != &env_chain && chain != "unknown" {
            warn!(
                "⚠️ Chain mismatch: SUI_CHAIN env var is '{}' but RPC reports '{}'",
                env_chain, chain
            );
        } else if chain == &env_chain {
            debug!("✅ Chain verified: {} (ID: {})", chain, chain_id.as_ref().unwrap_or(&"unknown".to_string()));
        } else {
            // RPC returned "unknown", use env var
            debug!("Chain from env: {} (RPC did not provide chain name)", env_chain);
        }
    }
    
    Ok(())
}

/// Get current network information from the Sui network
pub async fn get_network_info() -> Result<NetworkInfo> {
    let shared_state = SharedSuiState::get_instance();
    let mut client = shared_state.get_sui_client();
    let mut ledger = client.ledger_client();
    
    // Get service info first
    let (chain_name, chain_id) = get_service_info().await.unwrap_or((None, None));
    
    // Request current epoch info with system state
    let mut req = proto::GetEpochRequest::default();
    req.epoch = None; // None means current epoch
    req.read_mask = Some(sui_rpc::field::FieldMask {
        paths: vec![
            "epoch".into(),
            "system_state".into(),
            "system_state.epoch".into(),
            "system_state.protocol_version".into(),
            "system_state.reference_gas_price".into(),
            "system_state.epoch_start_timestamp_ms".into(),
            "system_state.safe_mode".into(),
            "system_state.validators".into(),
            "system_state.parameters".into(),
            "system_state.parameters.epoch_duration_ms".into(),
            "system_state.parameters.stake_subsidy_start_epoch".into(),
                "system_state.parameters.min_validator_count".into(),
                "system_state.parameters.max_validator_count".into(),
                "system_state.stake_subsidy".into(),
            ],
        });
    
    debug!("Fetching network info from gRPC...");
    let resp = ledger.get_epoch(req).await
        .context("Failed to get epoch info")?;
    
    let epoch_data = resp.into_inner()
        .epoch
        .context("No epoch data in response")?;
    
    let system_state = epoch_data
        .system_state
        .context("No system state in epoch data")?;
    
    let parameters = system_state
        .parameters
        .context("No system parameters in system state")?;
    
    // Count validators if available
    let validator_count = system_state
        .validators
        .as_ref()
        .map(|v| v.active_validators.len())
        .unwrap_or(0);
    
    Ok(NetworkInfo {
        epoch: system_state.epoch.unwrap_or(0),
        protocol_version: system_state.protocol_version.unwrap_or(0),
        reference_gas_price: system_state.reference_gas_price.unwrap_or(0),
        epoch_start_timestamp_ms: system_state.epoch_start_timestamp_ms.unwrap_or(0),
        epoch_duration_ms: parameters.epoch_duration_ms.unwrap_or(0),
        safe_mode: system_state.safe_mode.unwrap_or(false),
        validator_count,
        stake_subsidy_start_epoch: parameters.stake_subsidy_start_epoch.unwrap_or(0),
        min_validator_count: parameters.min_validator_count.unwrap_or(0),
        max_validator_count: parameters.max_validator_count.unwrap_or(0),
        chain_name,
        chain_id,
    })
}

/// Print network information
pub async fn print_network_info() -> Result<()> {
    // First verify chain config
    let _ = verify_chain_config().await;
    
    let info = get_network_info().await?;
    
    // Also get service info for additional details
    let service_info = get_service_info_full().await.ok();
    
    println!("\n📊 Network Statistics:");
    
    // Show chain info
    let env_chain = env::var("SUI_CHAIN").unwrap_or_else(|_| "not set".to_string());
    if let Some(ref chain_name) = info.chain_name {
        // RPC provided a meaningful chain name
        println!("  Chain: {} (env: {})", chain_name, env_chain);
        
        // Warn if mismatch
        if chain_name != &env_chain && env_chain != "not set" {
            println!("  ⚠️ Warning: Chain mismatch - env var says '{}' but RPC reports '{}'", env_chain, chain_name);
        }
    } else {
        // RPC didn't provide chain name or returned "unknown"
        println!("  Chain: {} (from env, RPC did not provide chain name)", env_chain);
    }
    
    if let Some(ref chain_id) = info.chain_id {
        // Show full chain ID
        println!("  Chain ID: {}", chain_id);
    }
    
    // Show server version if available
    if let Some(ref svc_info) = service_info {
        if let Some(ref server) = svc_info.server_version {
            println!("  Server: {}", server);
        }
        if let Some(height) = svc_info.checkpoint_height {
            // Format with thousands separator
            let formatted_height = format_number(height);
            println!("  Checkpoint Height: {}", formatted_height);
        }
    }
    
    println!("  Current Epoch: {}", info.epoch);
    println!("  Protocol Version: {}", info.protocol_version);
    println!("  Reference Gas Price: {} MIST", info.reference_gas_price);
    println!("  Active Validators: {}", info.validator_count);
    println!("  Validator Limits: {}-{}", info.min_validator_count, info.max_validator_count);
    
    // Convert epoch start to human readable
    if info.epoch_start_timestamp_ms > 0 {
        let start = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(info.epoch_start_timestamp_ms as i64)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| info.epoch_start_timestamp_ms.to_string());
        println!("  Epoch Started: {}", start);
    }
    
    // Convert epoch duration to human readable
    if info.epoch_duration_ms > 0 {
        let hours = info.epoch_duration_ms / 3_600_000;
        let minutes = (info.epoch_duration_ms % 3_600_000) / 60_000;
        println!("  Epoch Duration: {}h {}m", hours, minutes);
    }
    
    if info.safe_mode {
        println!("  ⚠️ Network is in SAFE MODE");
    }
    
    Ok(())
}

/// Get a summary string of network info
pub async fn get_network_summary() -> Result<String> {
    let info = get_network_info().await?;
    Ok(format!(
        "Epoch {} | Protocol v{} | {} validators | Gas: {} MIST",
        info.epoch,
        info.protocol_version,
        info.validator_count,
        info.reference_gas_price
    ))
}