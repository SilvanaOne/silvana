//! Configuration structures for multiple coordination layers

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main configuration structure containing all coordination layers
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoordinatorConfig {
    /// General coordinator settings
    #[serde(default)]
    pub coordinator: GeneralConfig,

    /// Sui coordination layer (always present for registry)
    pub sui: SuiConfig,

    /// Multiple Private coordination layers
    #[serde(default)]
    pub private: Vec<PrivateConfig>,

    /// Multiple Ethereum coordination layers
    #[serde(default)]
    pub ethereum: Vec<EthereumConfig>,
}

/// General coordinator configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeneralConfig {
    /// List of enabled layer IDs
    #[serde(default = "default_enable_layers")]
    pub enable_layers: Vec<String>,

    /// Maximum parallel jobs across all layers
    #[serde(default = "default_max_parallel_jobs")]
    pub max_parallel_jobs: usize,

    /// Reconciliation interval in seconds
    #[serde(default = "default_reconciliation_interval")]
    pub reconciliation_interval: u64,

    /// Whether to enable metrics reporting
    #[serde(default = "default_enable_metrics")]
    pub enable_metrics: bool,

    /// Metrics reporting interval in seconds
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            enable_layers: default_enable_layers(),
            max_parallel_jobs: default_max_parallel_jobs(),
            reconciliation_interval: default_reconciliation_interval(),
            enable_metrics: default_enable_metrics(),
            metrics_interval: default_metrics_interval(),
        }
    }
}

/// Sui coordination layer configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SuiConfig {
    /// Unique layer identifier
    pub layer_id: String,

    /// Sui RPC URL
    pub rpc_url: String,

    /// Coordination package ID
    pub package_id: String,

    /// Registry package ID
    pub registry_id: String,

    /// Operation mode (should be "multicall" for Sui)
    #[serde(default = "default_multicall_mode")]
    pub operation_mode: String,

    /// Multicall batching interval in seconds
    #[serde(default = "default_multicall_interval")]
    pub multicall_interval_secs: u64,

    /// Maximum operations per multicall batch
    #[serde(default = "default_multicall_max")]
    pub multicall_max_operations: usize,

    /// Optional app instance filter
    pub app_instance_filter: Option<String>,
}

/// Private coordination layer configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrivateConfig {
    /// Unique layer identifier
    pub layer_id: String,

    /// TiDB database URL
    pub database_url: String,

    /// JWT secret for authentication
    pub jwt_secret: String,

    /// Maximum database connections
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Operation mode (should be "direct" for Private)
    #[serde(default = "default_direct_mode")]
    pub operation_mode: String,

    /// Optional app instance filter
    pub app_instance_filter: Option<String>,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,

    /// Enable connection pooling
    #[serde(default = "default_enable_pooling")]
    pub enable_pooling: bool,
}

/// Ethereum coordination layer configuration (for future implementation)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EthereumConfig {
    /// Unique layer identifier
    pub layer_id: String,

    /// Ethereum RPC URL
    pub rpc_url: String,

    /// WebSocket URL for event streaming (optional)
    pub ws_url: Option<String>,

    /// Coordination contract address
    pub contract_address: String,

    /// JobManager contract address (optional - will be fetched from coordination contract if not provided)
    pub job_manager_address: Option<String>,

    /// Environment variable containing private key
    pub private_key_env: String,

    /// Operation mode ("multicall" or "direct" for private networks)
    #[serde(default = "default_multicall_mode")]
    pub operation_mode: String,

    /// Gas price multiplier for transactions
    #[serde(default = "default_gas_multiplier")]
    pub gas_price_multiplier: f64,

    /// Maximum gas per transaction
    #[serde(default = "default_max_gas")]
    pub max_gas: u64,

    /// Chain ID (for EIP-155)
    pub chain_id: Option<u64>,

    /// Multicall batching interval in seconds (if multicall mode)
    #[serde(default = "default_multicall_interval")]
    pub multicall_interval_secs: u64,

    /// Maximum operations per multicall batch (if multicall mode)
    #[serde(default = "default_multicall_max")]
    pub multicall_max_operations: usize,

    /// Optional app instance filter
    pub app_instance_filter: Option<String>,
}

/// Layer type enumeration for runtime identification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerType {
    Sui,
    Private,
    Ethereum,
}

/// Common layer information
pub struct LayerInfo {
    #[allow(dead_code)]
    pub layer_id: String,
    pub layer_type: LayerType,
    pub operation_mode: OperationMode,
    pub app_instance_filter: Option<String>,
}

/// Operation mode for coordination layers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationMode {
    /// Direct execution without batching
    Direct,
    /// Multicall batching for gas optimization
    Multicall,
}

impl From<&str> for OperationMode {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "direct" => OperationMode::Direct,
            "multicall" => OperationMode::Multicall,
            _ => OperationMode::Direct,
        }
    }
}

// Default value functions for serde
fn default_enable_layers() -> Vec<String> {
    vec!["sui-mainnet".to_string()]
}

fn default_max_parallel_jobs() -> usize {
    10
}

fn default_reconciliation_interval() -> u64 {
    30
}

fn default_enable_metrics() -> bool {
    true
}

fn default_metrics_interval() -> u64 {
    60
}

fn default_multicall_mode() -> String {
    "multicall".to_string()
}

fn default_direct_mode() -> String {
    "direct".to_string()
}

fn default_multicall_interval() -> u64 {
    3
}

fn default_multicall_max() -> usize {
    100
}

fn default_max_connections() -> u32 {
    50
}

fn default_connection_timeout() -> u64 {
    30
}

fn default_enable_pooling() -> bool {
    true
}

fn default_gas_multiplier() -> f64 {
    1.1
}

fn default_max_gas() -> u64 {
    10_000_000
}

impl CoordinatorConfig {
    /// Load configuration from a TOML file
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Load configuration from environment variables and/or default values
    #[allow(dead_code)]
    pub fn from_env() -> anyhow::Result<Self> {
        // Start with minimal default configuration
        let sui_config = SuiConfig {
            layer_id: "sui-mainnet".to_string(),
            rpc_url: std::env::var("SUI_RPC_URL")
                .unwrap_or_else(|_| "https://fullnode.mainnet.sui.io".to_string()),
            package_id: std::env::var("SUI_PACKAGE_ID")
                .unwrap_or_else(|_| "0x0".to_string()),
            registry_id: std::env::var("SUI_REGISTRY_ID")
                .unwrap_or_else(|_| "0x0".to_string()),
            operation_mode: default_multicall_mode(),
            multicall_interval_secs: default_multicall_interval(),
            multicall_max_operations: default_multicall_max(),
            app_instance_filter: std::env::var("APP_INSTANCE_FILTER").ok(),
        };

        let mut config = CoordinatorConfig {
            coordinator: GeneralConfig::default(),
            sui: sui_config,
            private: Vec::new(),
            ethereum: Vec::new(),
        };

        // Check for private layer configuration in environment
        if let Ok(db_url) = std::env::var("PRIVATE_DATABASE_URL") {
            config.private.push(PrivateConfig {
                layer_id: "private-default".to_string(),
                database_url: db_url,
                jwt_secret: std::env::var("PRIVATE_JWT_SECRET")
                    .unwrap_or_else(|_| "default-secret".to_string()),
                max_connections: default_max_connections(),
                operation_mode: default_direct_mode(),
                app_instance_filter: None,
                connection_timeout: default_connection_timeout(),
                enable_pooling: default_enable_pooling(),
            });
        }

        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        // Ensure all layer IDs are unique
        let mut layer_ids = HashMap::new();

        layer_ids.insert(&self.sui.layer_id, "sui");

        for private in &self.private {
            if layer_ids.contains_key(&private.layer_id) {
                anyhow::bail!("Duplicate layer_id: {}", private.layer_id);
            }
            layer_ids.insert(&private.layer_id, "private");
        }

        for ethereum in &self.ethereum {
            if layer_ids.contains_key(&ethereum.layer_id) {
                anyhow::bail!("Duplicate layer_id: {}", ethereum.layer_id);
            }
            layer_ids.insert(&ethereum.layer_id, "ethereum");
        }

        // Validate that enabled layers exist
        for enabled in &self.coordinator.enable_layers {
            if !layer_ids.contains_key(enabled) {
                anyhow::bail!("Enabled layer '{}' not found in configuration", enabled);
            }
        }

        Ok(())
    }

    /// Get all configured layer IDs
    #[allow(dead_code)]
    pub fn all_layer_ids(&self) -> Vec<String> {
        let mut ids = vec![self.sui.layer_id.clone()];

        for private in &self.private {
            ids.push(private.layer_id.clone());
        }

        for ethereum in &self.ethereum {
            ids.push(ethereum.layer_id.clone());
        }

        ids
    }

    /// Check if a layer is enabled
    pub fn is_layer_enabled(&self, layer_id: &str) -> bool {
        self.coordinator.enable_layers.contains(&layer_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        let config = CoordinatorConfig::from_env().unwrap();
        assert_eq!(config.sui.layer_id, "sui-mainnet");
    }

    #[test]
    fn test_operation_mode_from_string() {
        assert_eq!(OperationMode::from("direct"), OperationMode::Direct);
        assert_eq!(OperationMode::from("multicall"), OperationMode::Multicall);
        assert_eq!(OperationMode::from("unknown"), OperationMode::Direct);
    }
}