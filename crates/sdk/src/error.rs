//! Error types for the Silvana SDK

use thiserror::Error;

/// SDK error type
#[derive(Error, Debug)]
pub enum SdkError {
    /// Environment variable not found or invalid
    #[error("Environment configuration error: {0}")]
    EnvironmentError(String),

    /// gRPC transport or communication error
    #[error("gRPC error: {0}")]
    GrpcError(#[from] tonic::Status),

    /// Connection error
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// No active job
    #[error("No active job. Call get_job() first")]
    NoActiveJob,

    /// Invalid response from coordinator
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Operation failed on coordinator
    #[error("Operation failed: {0}")]
    OperationFailed(String),

    /// Generic error with context
    #[error("SDK error: {0}")]
    Other(String),
}

/// Result type alias for SDK operations
pub type Result<T> = std::result::Result<T, SdkError>;

impl From<tonic::transport::Error> for SdkError {
    fn from(err: tonic::transport::Error) -> Self {
        SdkError::ConnectionError(err.to_string())
    }
}