//! Configuration for Private Coordination layer

use serde::{Deserialize, Serialize};

/// Configuration for Private Coordination layer (gRPC client)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateCoordinationConfig {
    /// gRPC endpoint for private state server
    pub grpc_endpoint: String,

    /// Coordinator's Ed25519 private key (hex format)
    /// Should be loaded from SUI_SECRET_KEY environment variable
    #[serde(skip)]
    pub coordinator_private_key: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Chain ID (defaults to "private")
    #[serde(default = "default_chain_id")]
    pub chain_id: String,

    /// TLS configuration
    #[serde(default)]
    pub tls_enabled: bool,

    /// Path to TLS CA certificate
    pub tls_ca_cert: Option<String>,
}

impl Default for PrivateCoordinationConfig {
    fn default() -> Self {
        Self {
            grpc_endpoint: "http://localhost:50051".to_string(),
            coordinator_private_key: None,
            request_timeout_secs: default_request_timeout(),
            chain_id: default_chain_id(),
            tls_enabled: false,
            tls_ca_cert: None,
        }
    }
}

fn default_request_timeout() -> u64 {
    30
}

fn default_chain_id() -> String {
    "private".to_string()
}
