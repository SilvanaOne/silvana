//! Error types for Sui coordination layer

use thiserror::Error;

/// Result type alias for Sui coordination operations
pub type Result<T> = std::result::Result<T, SuiCoordinationError>;

/// Errors that can occur in the Sui coordination layer
#[derive(Debug, Error)]
pub enum SuiCoordinationError {
    /// Configuration validation error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// RPC connection or network error
    #[error("RPC error: {0}")]
    Rpc(String),

    /// Transaction execution failed
    #[error("Transaction failed: {0}")]
    Transaction(String),

    /// Checkpoint query failed
    #[error("Checkpoint error: {0}")]
    Checkpoint(String),

    /// Insufficient gas for transaction
    #[error("Insufficient gas: required {required} MIST, available {available} MIST")]
    InsufficientGas {
        /// Required gas amount
        required: u64,
        /// Available gas amount
        available: u64,
    },

    /// Resource not found (job, checkpoint, object, etc.)
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid job ID format
    #[error("Invalid job ID: {0}")]
    InvalidJobId(String),

    /// Invalid Sui address or object ID format
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

    /// Serialization/deserialization error (BCS or JSON)
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Generic error from anyhow
    #[error(transparent)]
    Other(#[from] anyhow::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// No private key configured for write operations
    #[error("No private key configured - write operations require private_key in config")]
    NoPrivateKey,

    /// Invalid private key format
    #[error("Invalid private key: {0}")]
    InvalidPrivateKey(String),

    /// Object parsing error
    #[error("Failed to parse object: {0}")]
    ObjectParse(String),

    /// State management error
    #[error("State error: {0}")]
    StateError(String),

    /// Coin management error
    #[error("Coin error: {0}")]
    CoinError(String),

    /// Sui utilities error
    #[error("Sui utilities error: {0}")]
    SuiUtilities(String),
}

impl SuiCoordinationError {
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
                | Self::Checkpoint(_)
                | Self::CoinError(_)
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
        let rpc_error = SuiCoordinationError::Rpc("connection failed".to_string());
        assert!(rpc_error.is_retriable());

        let timeout_error = SuiCoordinationError::TransactionTimeout(60);
        assert!(timeout_error.is_retriable());

        let gas_error = SuiCoordinationError::InsufficientGas {
            required: 100000,
            available: 50000,
        };
        assert!(gas_error.is_retriable());

        let config_error = SuiCoordinationError::Configuration("invalid".to_string());
        assert!(!config_error.is_retriable());

        let not_found_error = SuiCoordinationError::NotFound("job".to_string());
        assert!(!not_found_error.is_retriable());
    }

    #[test]
    fn test_is_configuration_error() {
        let config_error = SuiCoordinationError::Configuration("test".to_string());
        assert!(config_error.is_configuration_error());

        let no_key_error = SuiCoordinationError::NoPrivateKey;
        assert!(no_key_error.is_configuration_error());

        let multicall_error = SuiCoordinationError::MulticallNotEnabled;
        assert!(multicall_error.is_configuration_error());

        let rpc_error = SuiCoordinationError::Rpc("test".to_string());
        assert!(!rpc_error.is_configuration_error());
    }

    #[test]
    fn test_is_network_error() {
        let rpc_error = SuiCoordinationError::Rpc("test".to_string());
        assert!(rpc_error.is_network_error());

        let timeout_error = SuiCoordinationError::TransactionTimeout(30);
        assert!(timeout_error.is_network_error());

        let config_error = SuiCoordinationError::Configuration("test".to_string());
        assert!(!config_error.is_network_error());
    }

    #[test]
    fn test_error_display() {
        let gas_error = SuiCoordinationError::InsufficientGas {
            required: 100000,
            available: 50000,
        };
        assert_eq!(
            gas_error.to_string(),
            "Insufficient gas: required 100000 MIST, available 50000 MIST"
        );
    }
}
