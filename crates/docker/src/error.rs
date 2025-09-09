use thiserror::Error;

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker connection failed: {0}")]
    ConnectionError(String),
    
    #[error("Image not found: {0}")]
    ImageNotFound(String),
    
    #[error("Container operation failed: {0}")]
    ContainerError(String),
    
    #[error("Container timeout after {0} seconds")]
    Timeout(u64),
    
    #[error("Invalid configuration: {0}")]
    ConfigError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Docker API error: {0:?}")]
    BollardError(#[from] bollard::errors::Error),
    
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DockerError>;