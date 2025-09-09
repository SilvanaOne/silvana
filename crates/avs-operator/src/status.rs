use crate::config::{Config, load_deployment_data};
use crate::error::{AvsOperatorError, Result};
use ethereum::{
    AvsConfig, AvsContracts, AvsOperatorClient, avs_parse_address as parse_address,
    get_account_info,
};
use tracing::info;

/// Get operator status information
pub async fn get_operator_status(config: &Config, operator_address: Option<String>) -> Result<()> {
    info!("Fetching operator status...");

    // Load deployment data
    let deployment = load_deployment_data(config.chain_id)?;

    // Create AVS config and contracts
    let avs_config = AvsConfig {
        rpc_url: config.rpc_url.clone(),
        ws_url: config.ws_url.clone(),
        private_key: config.operator_private_key.clone(),
        chain_id: config.chain_id,
    };

    let avs_contracts = AvsContracts {
        delegation_manager: parse_address(&deployment.delegation_manager).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid delegation manager address: {}", e))
        })?,
        avs_directory: parse_address(&deployment.avs_directory).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid AVS directory address: {}", e))
        })?,
        silvana_service_manager: parse_address(&deployment.silvana_service_manager).map_err(
            |e| AvsOperatorError::Parse(format!("Invalid service manager address: {}", e)),
        )?,
        stake_registry: parse_address(&deployment.stake_registry).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid stake registry address: {}", e))
        })?,
    };

    // Create operator client
    let client = AvsOperatorClient::new(avs_config, avs_contracts.clone())
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;

    // Use provided address or derive from private key
    let address = if let Some(addr_str) = operator_address {
        parse_address(&addr_str)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid operator address: {}", e)))?
    } else {
        client.address()
    };

    info!("═══════════════════════════════════════════════════════════");
    info!("Operator Status Report");
    info!("═══════════════════════════════════════════════════════════");

    // Basic info
    info!("Chain ID: {}", config.chain_id);
    info!("RPC URL: {}", config.rpc_url);
    info!("Operator Address: {}", address);

    // Get balance and account info
    // Determine network name from chain_id
    let network_name = match config.chain_id {
        1 => "mainnet",
        11155111 => "sepolia",
        17000 => "holesky",
        _ => "unknown",
    };

    match get_account_info(&format!("{:?}", address), network_name).await {
        Ok(info) => {
            info!("ETH Balance: {} ETH", info.balance);
            info!("Nonce: {}", info.nonce);
        }
        Err(_) => {
            // If network name doesn't work, try direct RPC call
            info!("ETH Balance: Checking...");
        }
    }

    info!("───────────────────────────────────────────────────────────");
    info!("Contract Addresses:");
    info!("  Delegation Manager: {}", avs_contracts.delegation_manager);
    info!("  AVS Directory: {}", avs_contracts.avs_directory);
    info!(
        "  Service Manager: {}",
        avs_contracts.silvana_service_manager
    );
    info!("  Stake Registry: {}", avs_contracts.stake_registry);

    info!("───────────────────────────────────────────────────────────");
    info!("Registration Status:");

    // Check EigenLayer registration
    match client.is_operator(address).await {
        Ok(is_registered) => {
            if is_registered {
                info!("  ✅ Registered in EigenLayer");
            } else {
                info!("  ❌ Not registered in EigenLayer");
            }
        }
        Err(e) => {
            info!("  ⚠️  Failed to check EigenLayer registration: {}", e);
        }
    }

    // Get current block
    match client.get_current_block().await {
        Ok(block) => {
            info!("───────────────────────────────────────────────────────────");
            info!("Network Status:");
            info!("  Current Block: {}", block);
        }
        Err(e) => {
            info!("  ⚠️  Failed to get current block: {}", e);
        }
    }

    // Check if we can access the service manager
    info!("───────────────────────────────────────────────────────────");
    info!("Service Manager Status:");

    // Try to get the latest task number (if available)
    match get_latest_task_info(&client, &avs_contracts).await {
        Ok(Some((task_num, block_num))) => {
            info!("  Latest Task Number: {}", task_num);
            info!("  Created at Block: {}", block_num);
        }
        Ok(None) => {
            info!("  No tasks found");
        }
        Err(e) => {
            info!("  ⚠️  Failed to query tasks: {}", e);
        }
    }

    info!("═══════════════════════════════════════════════════════════");

    Ok(())
}

/// Get latest task information from the service manager
async fn get_latest_task_info(
    _client: &AvsOperatorClient,
    _avs_contracts: &AvsContracts,
) -> Result<Option<(u32, u32)>> {
    // This would require additional contract methods to be implemented
    // For now, return None as a placeholder
    Ok(None)
}

/// Get detailed operator information including delegations
pub async fn get_operator_details(config: &Config, operator_address: Option<String>) -> Result<()> {
    info!("Fetching detailed operator information...");

    // Load deployment data
    let deployment = load_deployment_data(config.chain_id)?;

    // Create AVS config and contracts
    let avs_config = AvsConfig {
        rpc_url: config.rpc_url.clone(),
        ws_url: config.ws_url.clone(),
        private_key: config.operator_private_key.clone(),
        chain_id: config.chain_id,
    };

    let avs_contracts = AvsContracts {
        delegation_manager: parse_address(&deployment.delegation_manager).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid delegation manager address: {}", e))
        })?,
        avs_directory: parse_address(&deployment.avs_directory).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid AVS directory address: {}", e))
        })?,
        silvana_service_manager: parse_address(&deployment.silvana_service_manager).map_err(
            |e| AvsOperatorError::Parse(format!("Invalid service manager address: {}", e)),
        )?,
        stake_registry: parse_address(&deployment.stake_registry).map_err(|e| {
            AvsOperatorError::Parse(format!("Invalid stake registry address: {}", e))
        })?,
    };

    // Create operator client
    let client = AvsOperatorClient::new(avs_config, avs_contracts.clone())
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;

    // Use provided address or derive from private key
    let address = if let Some(addr_str) = operator_address {
        parse_address(&addr_str)
            .map_err(|e| AvsOperatorError::Parse(format!("Invalid operator address: {}", e)))?
    } else {
        client.address()
    };

    info!("Operator Address: {}", address);

    // Get registration status
    let is_eigenlayer_registered = client.is_operator(address).await.unwrap_or(false);

    info!(
        "EigenLayer Registration: {}",
        if is_eigenlayer_registered {
            "✅ Yes"
        } else {
            "❌ No"
        }
    );

    if !is_eigenlayer_registered {
        info!("");
        info!("To register as an operator, run:");
        info!("  avs-operator register");
    }

    Ok(())
}
