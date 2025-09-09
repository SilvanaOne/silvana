use crate::error::{AvsOperatorError, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub operator_private_key: String,
    pub operator_address: String,
    pub admin_private_key: String,
    pub admin_address: String,
    pub operator_response_percentage: f64,
    pub rpc_url: String,
    pub ws_url: String,
    pub chain_id: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        // Try to load .env file, but don't fail if it doesn't exist
        let _ = dotenvy::dotenv();

        // Note: The env var has a typo (OPERSTOR instead of OPERATOR) - matching what's in .env
        let operator_private_key = env::var("AVS_OPERATOR_PRIVATE_KEY")
            .or_else(|_| env::var("AVS_OPERATOR_PRIVATE_KEY")) // Also try correct spelling
            .map_err(|_| {
                AvsOperatorError::MissingEnvVar(
                    "AVS_OPERATOR_PRIVATE_KEY or AVS_OPERATOR_PRIVATE_KEY".to_string(),
                )
            })?;

        let operator_address = env::var("AVS_OPERATOR_ADDRESS")
            .or_else(|_| env::var("AVS_OPERATOR_ADDRESS")) // Also try correct spelling
            .map_err(|_| {
                AvsOperatorError::MissingEnvVar(
                    "AVS_OPERATOR_ADDRESS or AVS_OPERATOR_ADDRESS".to_string(),
                )
            })?;

        let admin_private_key = env::var("ADMIN_PRIVATE_KEY")
            .map_err(|_| AvsOperatorError::MissingEnvVar("ADMIN_PRIVATE_KEY".to_string()))?;

        let admin_address = env::var("ADMIN_ADDRESS")
            .map_err(|_| AvsOperatorError::MissingEnvVar("ADMIN_ADDRESS".to_string()))?;

        let operator_response_percentage = env::var("OPERATOR_RESPONSE_PERCENTAGE")
            .map_err(|_| {
                AvsOperatorError::MissingEnvVar("OPERATOR_RESPONSE_PERCENTAGE".to_string())
            })?
            .parse::<f64>()
            .map_err(|e| AvsOperatorError::InvalidEnvVar {
                var: "OPERATOR_RESPONSE_PERCENTAGE".to_string(),
                reason: e.to_string(),
            })?;

        let rpc_url = env::var("RPC_URL")
            .map_err(|_| AvsOperatorError::MissingEnvVar("RPC_URL".to_string()))?;

        let ws_url = env::var("WS_URL")
            .map_err(|_| AvsOperatorError::MissingEnvVar("WS_URL".to_string()))?;

        // Default to Holesky testnet chain ID
        let chain_id = env::var("CHAIN_ID")
            .unwrap_or_else(|_| "17000".to_string())
            .parse::<u64>()
            .map_err(|e| AvsOperatorError::InvalidEnvVar {
                var: "CHAIN_ID".to_string(),
                reason: e.to_string(),
            })?;

        Ok(Config {
            operator_private_key,
            operator_address,
            admin_private_key,
            admin_address,
            operator_response_percentage,
            rpc_url,
            ws_url,
            chain_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentAddresses {
    pub delegation_manager: String,
    pub avs_directory: String,
    pub silvana_service_manager: String,
    pub stake_registry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDeployment {
    pub addresses: CoreAddresses,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreAddresses {
    #[serde(rename = "delegationManager")]
    pub delegation_manager: String,
    #[serde(rename = "avsDirectory")]
    pub avs_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilvanaDeployment {
    pub addresses: SilvanaAddresses,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilvanaAddresses {
    #[serde(rename = "silvanaServiceManager")]
    pub silvana_service_manager: String,
    #[serde(rename = "stakeRegistry")]
    pub stake_registry: String,
}

pub fn load_deployment_data(chain_id: u64) -> Result<DeploymentAddresses> {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Load core deployment
    let core_path = base_path.join(format!("deployments/core/{}.json", chain_id));
    let core_data = std::fs::read_to_string(&core_path).map_err(|e| AvsOperatorError::Io(e))?;
    let core_deployment: CoreDeployment = serde_json::from_str(&core_data)?;

    // Load silvana deployment
    let silvana_path = base_path.join(format!("deployments/silvana/{}.json", chain_id));
    let silvana_data =
        std::fs::read_to_string(&silvana_path).map_err(|e| AvsOperatorError::Io(e))?;
    let silvana_deployment: SilvanaDeployment = serde_json::from_str(&silvana_data)?;

    Ok(DeploymentAddresses {
        delegation_manager: core_deployment.addresses.delegation_manager,
        avs_directory: core_deployment.addresses.avs_directory,
        silvana_service_manager: silvana_deployment.addresses.silvana_service_manager,
        stake_registry: silvana_deployment.addresses.stake_registry,
    })
}

pub fn load_abi(abi_name: &str) -> Result<serde_json::Value> {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let abi_path = base_path.join(format!("abis/{}", abi_name));
    let abi_data = std::fs::read_to_string(&abi_path).map_err(|e| AvsOperatorError::Io(e))?;
    let abi: serde_json::Value = serde_json::from_str(&abi_data)?;
    Ok(abi)
}

pub static GLOBAL_CONFIG: Lazy<Result<Config>> = Lazy::new(Config::from_env);

/// Load configuration from environment variables
pub fn load_config() -> Result<Config> {
    Config::from_env()
}
