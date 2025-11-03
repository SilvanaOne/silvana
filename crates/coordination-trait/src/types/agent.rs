//! Agent and AgentMethod types
//!
//! These types represent the agent registry data stored on Sui blockchain.
//! The registry is always fetched from Sui, regardless of which coordination layer is being used.

use serde::{Deserialize, Serialize};

/// Agent method configuration (Docker execution requirements)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMethod {
    /// Docker image to use for this agent method
    pub docker_image: String,

    /// Optional SHA256 hash of the Docker image for verification
    pub docker_sha256: Option<String>,

    /// Minimum memory required in GB
    pub min_memory_gb: u16,

    /// Minimum CPU cores required
    pub min_cpu_cores: u16,

    /// Whether this agent requires a Trusted Execution Environment (TEE)
    pub requires_tee: bool,
}

/// Agent registered in the Sui registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// On-chain object ID
    pub id: String,

    /// Agent name
    pub name: String,

    /// Optional agent image/logo URL
    pub image: Option<String>,

    /// Optional description
    pub description: Option<String>,

    /// Optional website URL
    pub site: Option<String>,

    /// Supported blockchain chains
    pub chains: Vec<String>,

    /// Available methods for this agent (method_name -> AgentMethod)
    pub methods: Vec<(String, AgentMethod)>,

    /// Optional default method
    pub default_method: Option<AgentMethod>,

    /// Creation timestamp (milliseconds)
    pub created_at: u64,

    /// Last update timestamp (milliseconds)
    pub updated_at: u64,

    /// Version number
    pub version: u64,
}

/// App method configuration (maps app instance method to agent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMethod {
    /// Optional description
    pub description: Option<String>,

    /// Developer name
    pub developer: String,

    /// Agent name
    pub agent: String,

    /// Agent method name
    pub agent_method: String,
}

impl AppMethod {
    /// Create a new AppMethod
    pub fn new(
        description: Option<String>,
        developer: String,
        agent: String,
        agent_method: String,
    ) -> Self {
        Self {
            description,
            developer,
            agent,
            agent_method,
        }
    }
}
