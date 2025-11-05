//! Error types for Ethereum coordination layer

use thiserror::Error;

/// Result type alias for Ethereum coordination operations
pub type Result<T> = std::result::Result<T, EthereumCoordinationError>;

/// Errors that can occur in the Ethereum coordination layer
#[derive(Debug, Error)]
pub enum EthereumCoordinationError {
    /// Configuration validation error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Contract call (read operation) failed
    #[error("Contract call failed: {0}")]
    ContractCall(String),

    /// Transaction (write operation) failed
    #[error("Transaction failed: {0}")]
    Transaction(String),

    /// RPC connection or network error
    #[error("RPC error: {0}")]
    Rpc(String),

    /// Insufficient gas for transaction
    #[error("Insufficient gas: required {required} wei, available {available} wei")]
    InsufficientGas {
        /// Required gas amount
        required: u64,
        /// Available gas amount
        available: u64,
    },

    /// Gas price exceeds configured maximum
    #[error("Gas price too high: current {current} gwei exceeds max {max} gwei")]
    GasPriceTooHigh {
        /// Current gas price in gwei
        current: f64,
        /// Maximum allowed gas price in gwei
        max: f64,
    },

    /// Resource not found (job, block, sequence state, etc.)
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid job ID format
    #[error("Invalid job ID: {0}")]
    InvalidJobId(String),

    /// Invalid Ethereum address format
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Multicall not enabled in configuration
    #[error("Multicall not enabled - configure multicall_enabled=true")]
    MulticallNotEnabled,

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// Transaction timeout waiting for confirmation
    #[error("Transaction timeout after {0} seconds")]
    TransactionTimeout(u64),

    /// Type conversion error
    #[error("Conversion error: {0}")]
    Conversion(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Hex encoding/decoding error
    #[error("Hex error: {0}")]
    Hex(#[from] hex::FromHexError),

    /// Generic error from anyhow
    #[error(transparent)]
    Other(#[from] anyhow::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    /// No private key configured for write operations
    #[error("No private key configured - write operations require private_key in config")]
    NoPrivateKey,

    /// Invalid private key format
    #[error("Invalid private key: {0}")]
    InvalidPrivateKey(String),

    /// Contract event parsing error
    #[error("Failed to parse contract event: {0}")]
    EventParse(String),

    /// ABI encoding/decoding error
    #[error("ABI error: {0}")]
    Abi(String),

    /// Wallet/signer error
    #[error("Wallet error: {0}")]
    WalletError(String),

    /// Provider creation or connection error
    #[error("Provider error: {0}")]
    ProviderError(String),

    /// Event parsing error
    #[error("Event parsing error: {0}")]
    EventParsing(String),

    /// Connection error (WebSocket, network, etc.)
    #[error("Connection error: {0}")]
    Connection(String),
}

impl EthereumCoordinationError {
    /// Check if this error is retriable
    ///
    /// Some errors are transient and can be retried, while others
    /// indicate permanent failures.
    ///
    /// # Returns
    ///
    /// `true` if the operation should be retried, `false` otherwise
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            Self::Rpc(_)
                | Self::TransactionTimeout(_)
                | Self::InsufficientGas { .. }
                | Self::GasPriceTooHigh { .. }
                | Self::ContractCall(_)
        )
    }

    /// Check if this error indicates a configuration problem
    pub fn is_configuration_error(&self) -> bool {
        matches!(
            self,
            Self::Configuration(_)
                | Self::NoPrivateKey
                | Self::InvalidPrivateKey(_)
                | Self::InvalidAddress(_)
                | Self::MulticallNotEnabled
        )
    }

    /// Check if this error indicates a network problem
    pub fn is_network_error(&self) -> bool {
        matches!(
            self,
            Self::Rpc(_) | Self::TransactionTimeout(_) | Self::Io(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retriable() {
        let rpc_error = EthereumCoordinationError::Rpc("connection failed".to_string());
        assert!(rpc_error.is_retriable());

        let timeout_error = EthereumCoordinationError::TransactionTimeout(60);
        assert!(timeout_error.is_retriable());

        let gas_error = EthereumCoordinationError::InsufficientGas {
            required: 100000,
            available: 50000,
        };
        assert!(gas_error.is_retriable());

        let config_error = EthereumCoordinationError::Configuration("invalid".to_string());
        assert!(!config_error.is_retriable());

        let not_found_error = EthereumCoordinationError::NotFound("job".to_string());
        assert!(!not_found_error.is_retriable());
    }

    #[test]
    fn test_is_configuration_error() {
        let config_error = EthereumCoordinationError::Configuration("test".to_string());
        assert!(config_error.is_configuration_error());

        let no_key_error = EthereumCoordinationError::NoPrivateKey;
        assert!(no_key_error.is_configuration_error());

        let multicall_error = EthereumCoordinationError::MulticallNotEnabled;
        assert!(multicall_error.is_configuration_error());

        let rpc_error = EthereumCoordinationError::Rpc("test".to_string());
        assert!(!rpc_error.is_configuration_error());
    }

    #[test]
    fn test_is_network_error() {
        let rpc_error = EthereumCoordinationError::Rpc("test".to_string());
        assert!(rpc_error.is_network_error());

        let timeout_error = EthereumCoordinationError::TransactionTimeout(30);
        assert!(timeout_error.is_network_error());

        let config_error = EthereumCoordinationError::Configuration("test".to_string());
        assert!(!config_error.is_network_error());
    }

    #[test]
    fn test_error_display() {
        let gas_error = EthereumCoordinationError::InsufficientGas {
            required: 100000,
            available: 50000,
        };
        assert_eq!(
            gas_error.to_string(),
            "Insufficient gas: required 100000 wei, available 50000 wei"
        );

        let gas_price_error = EthereumCoordinationError::GasPriceTooHigh {
            current: 150.0,
            max: 100.0,
        };
        assert_eq!(
            gas_price_error.to_string(),
            "Gas price too high: current 150 gwei exceeds max 100 gwei"
        );
    }
}
