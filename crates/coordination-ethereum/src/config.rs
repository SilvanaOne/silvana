//! Configuration types for Ethereum coordination layer

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for Ethereum coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumCoordinationConfig {
    /// RPC URL for Ethereum node
    /// Example: "https://rpc-amoy.polygon.technology"
    pub rpc_url: String,

    /// WebSocket URL for event streaming (optional)
    /// Example: "wss://polygon-amoy.g.alchemy.com/v2/YOUR-API-KEY"
    /// If not provided, event streaming will not be available
    pub ws_url: Option<String>,

    /// Chain ID (1=mainnet, 80002=Polygon Amoy, 84532=Base Sepolia, etc.)
    pub chain_id: u64,

    /// Deployed SilvanaCoordination contract address
    /// Must be a valid Ethereum address (0x-prefixed, 42 characters)
    pub contract_address: String,

    /// JobManager contract address (optional - will be fetched if not provided)
    /// Used for listening to JobCreated events
    /// Must be a valid Ethereum address (0x-prefixed, 42 characters)
    pub job_manager_address: Option<String>,

    /// Private key for signing transactions (optional for read-only operations)
    /// Format: 0x-prefixed hex string (64 hex chars + 0x prefix = 66 chars)
    pub private_key: Option<String>,

    /// Enable multicall batching (recommended for gas savings)
    pub multicall_enabled: bool,

    /// How often to flush multicall queue (seconds)
    /// Default: 3 seconds
    pub multicall_interval_secs: u64,

    /// Multiply estimated gas limit by this factor for safety
    /// Default: 1.2 (20% safety margin)
    pub gas_limit_multiplier: f64,

    /// Maximum gas price willing to pay (in gwei, optional)
    /// Transactions will fail if current gas price exceeds this
    pub max_gas_price_gwei: Option<f64>,

    /// Number of confirmations to wait for (default 1)
    /// Higher values = more secure but slower
    pub confirmation_blocks: u64,
}

impl Default for EthereumCoordinationConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            ws_url: Some("ws://localhost:8545".to_string()), // Default WebSocket for Anvil
            chain_id: 1337, // Local anvil/hardhat
            contract_address: String::new(),
            job_manager_address: None,
            private_key: None,
            multicall_enabled: true,
            multicall_interval_secs: 3,
            gas_limit_multiplier: 1.2,
            max_gas_price_gwei: None,
            confirmation_blocks: 1,
        }
    }
}

impl EthereumCoordinationConfig {
    /// Load configuration from TOML file
    ///
    /// # Example
    ///
    /// ```no_run
    /// use silvana_coordination_ethereum::EthereumCoordinationConfig;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = EthereumCoordinationConfig::from_file("ethereum.toml")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate()
            .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;
        Ok(config)
    }

    /// Load configuration from TOML string
    pub fn from_toml_str(toml: &str) -> Result<Self, anyhow::Error> {
        let config: Self = toml::from_str(toml)?;
        config.validate()
            .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;
        Ok(config)
    }

    /// Validate configuration
    ///
    /// Returns `Ok(())` if valid, otherwise returns error message
    pub fn validate(&self) -> Result<(), String> {
        // Validate RPC URL
        if self.rpc_url.is_empty() {
            return Err("rpc_url cannot be empty".to_string());
        }

        if !self.rpc_url.starts_with("http://") && !self.rpc_url.starts_with("https://") {
            return Err("rpc_url must start with http:// or https://".to_string());
        }

        // Validate contract address
        if self.contract_address.is_empty() {
            return Err("contract_address cannot be empty".to_string());
        }

        if !self.contract_address.starts_with("0x") {
            return Err("contract_address must start with 0x".to_string());
        }

        if self.contract_address.len() != 42 {
            return Err(format!(
                "contract_address must be 42 characters (0x + 40 hex), got {}",
                self.contract_address.len()
            ));
        }

        // Validate hex chars in address
        if !self.contract_address[2..]
            .chars()
            .all(|c| c.is_ascii_hexdigit())
        {
            return Err("contract_address must contain only hex characters after 0x".to_string());
        }

        // Validate private key if provided
        if let Some(ref pk) = self.private_key {
            if !pk.is_empty() {
                if !pk.starts_with("0x") {
                    return Err("private_key must start with 0x".to_string());
                }

                if pk.len() != 66 {
                    return Err(format!(
                        "private_key must be 66 characters (0x + 64 hex), got {}",
                        pk.len()
                    ));
                }

                if !pk[2..].chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("private_key must contain only hex characters after 0x".to_string());
                }
            }
        }

        // Validate gas limit multiplier
        if self.gas_limit_multiplier < 1.0 {
            return Err("gas_limit_multiplier must be >= 1.0".to_string());
        }

        if self.gas_limit_multiplier > 10.0 {
            return Err("gas_limit_multiplier must be <= 10.0 (suspiciously high)".to_string());
        }

        // Validate max gas price
        if let Some(max_gas) = self.max_gas_price_gwei {
            if max_gas <= 0.0 {
                return Err("max_gas_price_gwei must be positive".to_string());
            }

            if max_gas > 100000.0 {
                return Err("max_gas_price_gwei suspiciously high (>100k gwei)".to_string());
            }
        }

        // Validate multicall interval
        if self.multicall_interval_secs == 0 {
            return Err("multicall_interval_secs must be > 0".to_string());
        }

        if self.multicall_interval_secs > 3600 {
            return Err("multicall_interval_secs too large (max 1 hour)".to_string());
        }

        // Validate confirmation blocks
        if self.confirmation_blocks == 0 {
            return Err("confirmation_blocks must be > 0".to_string());
        }

        if self.confirmation_blocks > 100 {
            return Err("confirmation_blocks too large (max 100)".to_string());
        }

        Ok(())
    }

    /// Check if configuration supports write operations (has private key)
    pub fn can_write(&self) -> bool {
        self.private_key.is_some() && !self.private_key.as_ref().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validation() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_rpc_url() {
        let mut config = EthereumCoordinationConfig::default();
        config.rpc_url = String::new();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_rpc_url_scheme() {
        let mut config = EthereumCoordinationConfig::default();
        config.rpc_url = "ws://localhost:8545".to_string();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_empty_contract_address() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = String::new();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_contract_address_no_prefix() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "1234567890123456789012345678901234567890".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_contract_address_length() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x12345".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_contract_address_non_hex() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x12345678901234567890123456789012345678XY".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_private_key() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();
        config.private_key = Some("0x1234567890123456789012345678901234567890123456789012345678901234".to_string());

        assert!(config.validate().is_ok());
        assert!(config.can_write());
    }

    #[test]
    fn test_invalid_private_key_length() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();
        config.private_key = Some("0x1234".to_string());

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gas_limit_multiplier_validation() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();

        config.gas_limit_multiplier = 0.5;
        assert!(config.validate().is_err());

        config.gas_limit_multiplier = 1.0;
        assert!(config.validate().is_ok());

        config.gas_limit_multiplier = 2.0;
        assert!(config.validate().is_ok());

        config.gas_limit_multiplier = 11.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_max_gas_price_validation() {
        let mut config = EthereumCoordinationConfig::default();
        config.contract_address = "0x1234567890123456789012345678901234567890".to_string();

        config.max_gas_price_gwei = Some(-1.0);
        assert!(config.validate().is_err());

        config.max_gas_price_gwei = Some(50.0);
        assert!(config.validate().is_ok());

        config.max_gas_price_gwei = Some(200000.0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_can_write() {
        let mut config = EthereumCoordinationConfig::default();

        assert!(!config.can_write());

        config.private_key = Some(String::new());
        assert!(!config.can_write());

        config.private_key = Some("0x1234567890123456789012345678901234567890123456789012345678901234".to_string());
        assert!(config.can_write());
    }

    #[test]
    fn test_from_toml_str() {
        let toml = r#"
rpc_url = "https://rpc-amoy.polygon.technology"
chain_id = 80002
contract_address = "0xF1C921CEf0c62e7a15cef3D04dFc3e2e7Eb90165"
multicall_enabled = true
multicall_interval_secs = 3
gas_limit_multiplier = 1.2
confirmation_blocks = 1
"#;

        let config = EthereumCoordinationConfig::from_toml_str(toml).unwrap();

        assert_eq!(config.rpc_url, "https://rpc-amoy.polygon.technology");
        assert_eq!(config.chain_id, 80002);
        assert_eq!(config.contract_address, "0xF1C921CEf0c62e7a15cef3D04dFc3e2e7Eb90165");
        assert!(config.multicall_enabled);
        assert_eq!(config.multicall_interval_secs, 3);
        assert_eq!(config.gas_limit_multiplier, 1.2);
        assert_eq!(config.confirmation_blocks, 1);
    }
}
