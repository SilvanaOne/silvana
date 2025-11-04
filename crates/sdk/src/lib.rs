//! # Silvana Agent SDK
//!
//! A Rust SDK for building agents that interact with the Silvana coordinator via gRPC.
//!
//! ## Features
//!
//! - Simple, ergonomic API matching the TypeScript SDK
//! - Automatic environment variable configuration
//! - Global client instance with lazy initialization
//! - Comprehensive error handling
//! - Full support for all coordinator RPC methods
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use silvana_sdk::{get_job, complete_job, agent};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize logging
//!     tracing_subscriber::fmt::init();
//!
//!     loop {
//!         // Get a job from the coordinator
//!         if let Some(job) = get_job().await? {
//!             // Log job receipt
//!             agent::info(&format!("Processing job: {}", job.job_id)).await?;
//!
//!             // Process the job...
//!
//!             // Complete the job
//!             complete_job().await?;
//!         } else {
//!             // No job available, wait before polling again
//!             tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
//!         }
//!     }
//! }
//! ```
//!
//! ## Environment Variables
//!
//! The SDK requires the following environment variables:
//!
//! - `SESSION_ID` (required): Agent session identifier
//! - `DEVELOPER` (required): Developer identifier
//! - `AGENT` (required): Agent name
//! - `AGENT_METHOD` (required): Agent method name
//! - `COORDINATOR_URL` (optional): Coordinator gRPC URL (default: `http://host.docker.internal:50051`)
//! - `CHAIN` (optional): Blockchain identifier
//! - `COORDINATOR_ID` (optional): Coordinator instance ID
//! - `SESSION_PRIVATE_KEY` (optional): Session private key
//!
//! ## Advanced Usage
//!
//! For more control, you can create and manage the client instance directly:
//!
//! ```rust,no_run
//! use silvana_sdk::{CoordinatorClient, ClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ClientConfig::from_env()?;
//!     let mut client = CoordinatorClient::new(config).await?;
//!
//!     // Use the client directly
//!     if let Some(job) = client.get_job().await? {
//!         // Process job...
//!         client.complete_job().await?;
//!     }
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Include generated proto code
#[allow(missing_docs)]
pub(crate) mod proto {
    pub mod coordinator {
        tonic::include_proto!("silvana.coordinator.v1");
    }
}

// Public modules
pub mod agent;
pub mod block;
pub mod client;
pub mod error;
pub mod kv;
pub mod state;
pub mod types;

// Re-export main types and functions for convenience
pub use client::{
    ClientConfig, CoordinatorClient,
    // Global job functions
    get_job, complete_job, fail_job, terminate_job, get_secret,
};

pub use error::{SdkError, Result};

pub use types::*;

// Re-export state management functions
pub use state::{
    get_sequence_states, submit_state, submit_proof, read_data_availability,
};

// Re-export block management functions
pub use block::{
    get_block, try_create_block, get_block_settlement, update_block_settlement,
};

// Re-export KV and metadata functions
pub use kv::{
    set_kv, get_kv, delete_kv, add_metadata, get_metadata,
    get_app_instance_info, get_app_instance, create_app_job,
};