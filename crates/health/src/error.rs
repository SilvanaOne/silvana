//! Custom error types for health metrics crate

use thiserror::Error;

/// Errors that can occur in the health metrics crate
#[derive(Error, Debug)]
pub enum HealthError {
    #[error("Environment variable error: {0}")]
    EnvVar(String),

    #[error("JWT error: {0}")]
    Jwt(String),

    #[error("HTTP client error: {0}")]
    HttpClient(String),

    #[error("HTTP request error: {0}")]
    HttpRequest(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

impl From<std::env::VarError> for HealthError {
    fn from(err: std::env::VarError) -> Self {
        HealthError::EnvVar(err.to_string())
    }
}
