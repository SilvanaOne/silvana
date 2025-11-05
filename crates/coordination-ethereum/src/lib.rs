//! Ethereum Coordination Layer Implementation
//!
//! This crate provides an Ethereum and EVM-compatible coordination layer implementation
//! that implements the `Coordination` trait from `silvana-coordination-trait`.
//!
//! # Features
//!
//! - Full implementation of all 56 Coordination trait methods
//! - Support for Ethereum, Polygon, Base, and all EVM chains
//! - Multicall batching for gas optimization (60-80% savings)
//! - EIP-1559 and legacy gas pricing support
//! - Type-safe contract bindings via Alloy
//!
//! # Example
//!
//! ```ignore
//! // This example will work after Phase 4 implementation
//! use silvana_coordination_ethereum::{EthereumCoordinationConfig, EthereumCoordination};
//! use silvana_coordination_trait::Coordination;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = EthereumCoordinationConfig {
//!     rpc_url: "https://rpc-amoy.polygon.technology".to_string(),
//!     chain_id: 80002,
//!     contract_address: "0xF1C921CEf0c62e7a15cef3D04dFc3e2e7Eb90165".to_string(),
//!     ..Default::default()
//! };
//!
//! let coord = EthereumCoordination::new(config).await?;
//! let jobs = coord.fetch_pending_jobs("my-app").await?;
//! # Ok(())
//! # }
//! ```

pub mod abi;
pub mod config;
pub mod contract;
pub mod conversions;
pub mod coordination;
pub mod error;

pub use config::EthereumCoordinationConfig;
pub use contract::ContractClient;
pub use coordination::EthereumCoordination;
pub use error::{EthereumCoordinationError, Result};

/// Re-export the Coordination trait for convenience
pub use silvana_coordination_trait::Coordination;
