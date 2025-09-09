use thiserror::Error;

#[derive(Error, Debug)]
pub enum AvsOperatorError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    
    #[error("Invalid environment variable {var}: {reason}")]
    InvalidEnvVar { var: String, reason: String },
    
    #[error("Ethereum error: {0}")]
    Ethereum(String),
    
    #[error("Contract interaction error: {0}")]
    Contract(String),
    
    #[error("Transaction failed: {0}")]
    Transaction(String),
    
    #[error("Signature error: {0}")]
    Signature(String),
    
    #[error("RPC error: {0}")]
    Rpc(String),
    
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Registration failed: {0}")]
    Registration(String),
    
    #[error("Task creation failed: {0}")]
    TaskCreation(String),
    
    #[error("Task response failed: {0}")]
    TaskResponse(String),
    
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AvsOperatorError>;