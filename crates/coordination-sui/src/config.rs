//! Configuration types for Sui coordination layer

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for Sui coordination layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiCoordinationConfig {
    /// RPC URL for Sui node
    /// Examples:
    /// - Devnet: "https://fullnode.devnet.sui.io:443"
    /// - Testnet: "https://fullnode.testnet.sui.io:443"
    /// - Mainnet: "https://fullnode.mainnet.sui.io:443"
    pub rpc_url: String,

    /// Deployed coordination package ID
    /// Must be a valid Sui object ID (0x-prefixed hex, 64 hex chars)
    pub package_id: String,

    /// Coordination state object ID
    /// Must be a valid Sui object ID
    pub state_object_id: String,

    /// Private key for signing transactions (optional for read-only operations)
    /// Format: Base64-encoded or hex-encoded key
    pub private_key: Option<String>,

    /// Enable multicall batching (recommended for gas savings)
    pub multicall_enabled: bool,

    /// How often to flush multicall queue (seconds)
    /// Default: 3 seconds
    pub multicall_interval_secs: u64,

    /// Gas budget for transactions (in MIST, 1 SUI = 1_000_000_000 MIST)
    /// Default: 100_000_000 (0.1 SUI)
    pub gas_budget: u64,

    /// Number of confirmations to wait for (default 1)
    /// Higher values = more secure but slower
    pub confirmation_blocks: u64,

    /// Maximum retries for failed requests
    pub max_retries: u32,
}

impl Default for SuiCoordinationConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://fullnode.devnet.sui.io:443".to_string(),
            package_id: String::new(),
            state_object_id: String::new(),
            private_key: None,
            multicall_enabled: true,
            multicall_interval_secs: 3,
            gas_budget: 100_000_000, // 0.1 SUI
            confirmation_blocks: 1,
            max_retries: 3,
        }
    }
}

impl SuiCoordinationConfig {
    /// Load configuration from TOML file
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
    pub fn validate(&self) -> Result<(), String> {
        // Validate RPC URL
        if self.rpc_url.is_empty() {
            return Err("rpc_url cannot be empty".to_string());
        }

        if !self.rpc_url.starts_with("http://") && !self.rpc_url.starts_with("https://") {
            return Err("rpc_url must start with http:// or https://".to_string());
        }

        // Validate package ID
        if self.package_id.is_empty() {
            return Err("package_id cannot be empty".to_string());
        }

        if !self.package_id.starts_with("0x") {
            return Err("package_id must start with 0x".to_string());
        }

        if self.package_id.len() != 66 {
            return Err(format!(
                "package_id must be 66 characters (0x + 64 hex), got {}",
                self.package_id.len()
            ));
        }

        // Validate state object ID
        if self.state_object_id.is_empty() {
            return Err("state_object_id cannot be empty".to_string());
        }

        if !self.state_object_id.starts_with("0x") {
            return Err("state_object_id must start with 0x".to_string());
        }

        if self.state_object_id.len() != 66 {
            return Err(format!(
                "state_object_id must be 66 characters (0x + 64 hex), got {}",
                self.state_object_id.len()
            ));
        }

        // Validate multicall interval
        if self.multicall_interval_secs == 0 {
            return Err("multicall_interval_secs must be > 0".to_string());
        }

        if self.multicall_interval_secs > 3600 {
            return Err("multicall_interval_secs too large (max 1 hour)".to_string());
        }

        // Validate gas budget
        if self.gas_budget == 0 {
            return Err("gas_budget must be > 0".to_string());
        }

        if self.gas_budget > 1_000_000_000_000 {
            return Err("gas_budget suspiciously high (>1000 SUI)".to_string());
        }

        // Validate confirmation blocks
        if self.confirmation_blocks == 0 {
            return Err("confirmation_blocks must be > 0".to_string());
        }

        if self.confirmation_blocks > 100 {
            return Err("confirmation_blocks too large (max 100)".to_string());
        }

        // Validate max retries
        if self.max_retries > 100 {
            return Err("max_retries too large (max 100)".to_string());
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
        let mut config = SuiCoordinationConfig::default();
        config.package_id = "0x1234567890123456789012345678901234567890123456789012345678901234".to_string();
        config.state_object_id = "0x5678901234567890123456789012345678901234567890123456789012345678".to_string();

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_rpc_url() {
        let mut config = SuiCoordinationConfig::default();
        config.rpc_url = String::new();
        config.package_id = "0x1234567890123456789012345678901234567890123456789012345678901234".to_string();
        config.state_object_id = "0x5678901234567890123456789012345678901234567890123456789012345678".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_package_id_length() {
        let mut config = SuiCoordinationConfig::default();
        config.package_id = "0x12345".to_string();
        config.state_object_id = "0x5678901234567890123456789012345678901234567890123456789012345678".to_string();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_can_write() {
        let mut config = SuiCoordinationConfig::default();
        config.package_id = "0x1234567890123456789012345678901234567890123456789012345678901234".to_string();
        config.state_object_id = "0x5678901234567890123456789012345678901234567890123456789012345678".to_string();

        assert!(!config.can_write());

        config.private_key = Some(String::new());
        assert!(!config.can_write());

        config.private_key = Some("some_key".to_string());
        assert!(config.can_write());
    }
}
