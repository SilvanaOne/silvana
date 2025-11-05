//! Contract client for interacting with Ethereum coordination contracts
//!
//! This module provides a high-level client for managing contract instances
//! and provides unified access to all coordination contract functionality.

use crate::config::EthereumCoordinationConfig;
use crate::error::{EthereumCoordinationError, Result};
use alloy::network::EthereumWallet;
use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use std::str::FromStr;

/// Contract client that manages contract connection details
///
/// This is a placeholder for Phase 2. Full contract instance management
/// will be implemented in Phase 4 when we implement the Coordination trait.
pub struct ContractClient {
    /// Contract address
    pub contract_address: Address,
    /// RPC URL
    pub rpc_url: String,
    /// Optional private key for signing
    pub private_key: Option<String>,
    /// Configuration
    pub config: EthereumCoordinationConfig,
}

impl ContractClient {
    /// Creates a new contract client from configuration
    ///
    /// # Arguments
    ///
    /// * `config` - The Ethereum coordination configuration
    ///
    /// # Returns
    ///
    /// A new `ContractClient` instance
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Contract address is invalid
    /// - Private key is invalid (if provided)
    pub fn new(config: EthereumCoordinationConfig) -> Result<Self> {
        // Parse and validate contract address
        let contract_address = Address::from_str(&config.contract_address).map_err(|e| {
            EthereumCoordinationError::Configuration(format!(
                "Invalid contract address '{}': {}",
                config.contract_address, e
            ))
        })?;

        // Validate private key if provided
        if let Some(ref private_key) = config.private_key {
            let _ = private_key.parse::<PrivateKeySigner>().map_err(|e| {
                EthereumCoordinationError::Configuration(format!("Invalid private key: {}", e))
            })?;
        }

        Ok(Self {
            contract_address,
            rpc_url: config.rpc_url.clone(),
            private_key: config.private_key.clone(),
            config,
        })
    }

    /// Returns the contract address
    pub fn contract_address(&self) -> &Address {
        &self.contract_address
    }

    /// Returns the chain ID from configuration
    pub fn chain_id(&self) -> u64 {
        self.config.chain_id
    }

    /// Checks if the client has a wallet for signing transactions
    pub fn has_wallet(&self) -> bool {
        self.private_key.is_some()
    }

    /// Returns the RPC URL
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Create a read-only provider for contract calls
    ///
    /// This creates a new provider instance for making read-only contract calls.
    /// The provider is not cached - a new one is created for each call.
    ///
    /// # Returns
    ///
    /// A provider that can be used for contract `call()` operations
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC URL is invalid
    pub fn create_provider(&self) -> Result<impl Provider> {
        let rpc_url = self
            .rpc_url
            .parse()
            .map_err(|e| EthereumCoordinationError::ProviderError(format!("Invalid RPC URL: {}", e)))?;

        Ok(ProviderBuilder::new().connect_http(rpc_url))
    }

    /// Create a provider with wallet for sending transactions
    ///
    /// This creates a new provider instance with a wallet/signer attached,
    /// allowing it to send transactions. The provider is not cached - a new
    /// one is created for each transaction.
    ///
    /// # Returns
    ///
    /// A provider with wallet that can be used for contract `send()` operations
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No private key is configured
    /// - The private key is invalid
    /// - The RPC URL is invalid
    pub fn create_provider_with_signer(&self) -> Result<impl Provider> {
        let private_key = self
            .private_key
            .as_ref()
            .ok_or(EthereumCoordinationError::NoPrivateKey)?;

        let signer = private_key.parse::<PrivateKeySigner>().map_err(|e| {
            EthereumCoordinationError::WalletError(format!("Invalid private key: {}", e))
        })?;

        let wallet = EthereumWallet::from(signer);

        let rpc_url = self
            .rpc_url
            .parse()
            .map_err(|e| EthereumCoordinationError::ProviderError(format!("Invalid RPC URL: {}", e)))?;

        Ok(ProviderBuilder::new().wallet(wallet).connect_http(rpc_url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EthereumCoordinationConfig {
        EthereumCoordinationConfig {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 31337,
            contract_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string(),
            private_key: None,
            multicall_enabled: true,
            multicall_interval_secs: 1,
            gas_limit_multiplier: 1.2,
            max_gas_price_gwei: Some(100.0),
            confirmation_blocks: 1,
        }
    }

    #[test]
    fn test_contract_client_creation() {
        let config = test_config();
        let client = ContractClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_invalid_contract_address() {
        let mut config = test_config();
        config.contract_address = "invalid".to_string();
        let client = ContractClient::new(config);
        assert!(client.is_err());
    }

    #[test]
    fn test_contract_client_methods() {
        let config = test_config();
        let client = ContractClient::new(config).unwrap();
        assert_eq!(client.chain_id(), 31337);
        assert!(!client.has_wallet());
        assert_eq!(client.rpc_url(), "http://localhost:8545");
    }
}
