//! Error types for Private Coordination layer

use thiserror::Error;

/// Error type for Private Coordination layer operations
#[derive(Error, Debug)]
pub enum PrivateCoordinationError {
    /// gRPC transport error
    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status error
    #[error("gRPC status error: {0}")]
    Status(#[from] tonic::Status),

    /// Coordinator not authorized
    #[error("Coordinator not authorized: {0}")]
    NotAuthorized(String),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization error
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Not found error
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Operation not implemented
    #[error("Operation not implemented: {0}")]
    NotImplemented(String),

    /// Generic error
    #[error("Error: {0}")]
    Other(#[from] anyhow::Error),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// UUID parsing error
    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    /// Hex decoding error
    #[error("Hex decode error: {0}")]
    HexDecode(#[from] hex::FromHexError),
}

/// Result type alias for Private Coordination operations
pub type Result<T> = std::result::Result<T, PrivateCoordinationError>;
