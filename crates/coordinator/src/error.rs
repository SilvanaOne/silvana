use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoordinatorError {
    #[error("RPC connection failed: {0}")]
    RpcConnectionError(String),

    #[error("Docker error: {0}")]
    DockerError(#[from] docker::DockerError),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Tonic/gRPC error: {0}")]
    TonicError(#[from] tonic::Status),

    #[error("Coordination error: {0}")]
    Coordination(#[from] crate::coordination_layer::CoordinationError),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, CoordinatorError>;
