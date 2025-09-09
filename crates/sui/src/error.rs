use thiserror::Error;

#[derive(Error, Debug)]
pub enum SilvanaSuiInterfaceError {
    #[error("RPC connection failed: {0}")]
    RpcConnectionError(String),

    #[error("Transaction failed: {message}{}", tx_digest.as_ref().map(|d| format!(" (tx: {})", d)).unwrap_or_default())]
    TransactionError {
        message: String,
        tx_digest: Option<String>,
    },

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Invalid object format: {0}")]
    InvalidObjectFormat(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Sui SDK error: {0}")]
    SuiSdkError(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SilvanaSuiInterfaceError>;