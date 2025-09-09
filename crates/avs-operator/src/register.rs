use crate::config::{Config, load_deployment_data};
use crate::error::{AvsOperatorError, Result};
use ethereum::{AvsConfig, AvsContracts, AvsOperatorClient, avs_parse_address as parse_address};
use tracing::{error, info};

/// Register an operator to both EigenLayer and AVS
pub async fn register_operator(config: &Config) -> Result<()> {
    info!("Starting operator registration process");

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

    let operator_address = client.address();
    info!("Operator address: {}", operator_address);

    // Check registration status
    info!("Checking registration status...");

    let is_eigenlayer_registered = client
        .is_operator(operator_address)
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;

    // Display current status
    info!("═══════════════════════════════════════════════════════════");
    info!("Current Registration Status:");
    info!(
        "  EigenLayer: {}",
        if is_eigenlayer_registered {
            "✅ Registered"
        } else {
            "❌ Not registered"
        }
    );
    info!("═══════════════════════════════════════════════════════════");

    // Check if already fully registered
    if is_eigenlayer_registered {
        info!("✅ Operator is already fully registered to EigenLayer!");
        info!("No further action needed.");
        return Ok(());
    }

    // Step 1: Register to EigenLayer if needed
    if !is_eigenlayer_registered {
        info!("Step 1: Registering as operator in EigenLayer...");
        match client.register_as_operator().await {
            Ok(tx_hash) => {
                info!(
                    "✅ Operator registered to Core EigenLayer contracts. Tx: {}",
                    tx_hash
                );
            }
            Err(e) => {
                error!("❌ Failed to register as operator: {}", e);
                return Err(AvsOperatorError::Registration(format!(
                    "Failed to register as operator: {}",
                    e
                )));
            }
        }
    } else {
        info!("Step 1: ✅ Already registered in EigenLayer (skipping)");
    }

    info!("Operator registration completed successfully!");
    Ok(())
}

/// Check if an operator is already registered
pub async fn is_operator_registered(config: &Config) -> Result<bool> {
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
    let client = AvsOperatorClient::new(avs_config, avs_contracts)
        .await
        .map_err(|e| AvsOperatorError::Other(e))?;

    client
        .is_operator(client.address())
        .await
        .map_err(|e| AvsOperatorError::Other(e))
}
