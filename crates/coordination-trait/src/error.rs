//! Error types for coordination operations

use std::fmt;
use thiserror::Error;

/// Result type for coordination operations
pub type CoordinationResult<T> = Result<T, CoordinationError>;

/// Main error type for coordination operations
#[derive(Error, Debug)]
pub enum CoordinationError {
    /// Operation not supported by this coordination layer
    #[error("Operation not supported by {layer} coordination layer")]
    NotSupported { layer: String },

    /// Network or RPC connection error
    #[error("Connection error: {message}")]
    ConnectionError { message: String },

    /// Transaction failed
    #[error("Transaction failed: {message}")]
    TransactionError {
        message: String,
        tx_hash: Option<String>,
    },

    /// Resource not found
    #[error("{resource} not found: {id}")]
    NotFound { resource: String, id: String },

    /// Invalid input parameters
    #[error("Invalid {parameter}: {message}")]
    InvalidParameter { parameter: String, message: String },

    /// Authentication or authorization failure
    #[error("Authorization failed: {message}")]
    AuthorizationError { message: String },

    /// Timeout waiting for operation
    #[error("Operation timed out after {seconds} seconds")]
    Timeout { seconds: u64 },

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded { message: String },

    /// Insufficient resources (e.g., gas, balance)
    #[error("Insufficient {resource}: {message}")]
    InsufficientResources { resource: String, message: String },

    /// Data serialization/deserialization error
    #[error("Serialization error: {message}")]
    SerializationError { message: String },

    /// Storage error (for private layer)
    #[error("Storage error: {message}")]
    StorageError { message: String },

    /// Generic internal error
    #[error("Internal error: {message}")]
    InternalError { message: String },

    /// Wrapper for other error types
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CoordinationError {
    /// Create a not supported error for the given coordination layer
    pub fn not_supported(layer: impl fmt::Display) -> Self {
        Self::NotSupported {
            layer: layer.to_string(),
        }
    }

    /// Create a connection error
    pub fn connection(message: impl Into<String>) -> Self {
        Self::ConnectionError {
            message: message.into(),
        }
    }

    /// Create a transaction error
    pub fn transaction(message: impl Into<String>, tx_hash: Option<String>) -> Self {
        Self::TransactionError {
            message: message.into(),
            tx_hash,
        }
    }

    /// Create a not found error
    pub fn not_found(resource: impl Into<String>, id: impl fmt::Display) -> Self {
        Self::NotFound {
            resource: resource.into(),
            id: id.to_string(),
        }
    }

    /// Create an invalid parameter error
    pub fn invalid_parameter(parameter: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidParameter {
            parameter: parameter.into(),
            message: message.into(),
        }
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::InternalError {
            message: message.into(),
        }
    }
}