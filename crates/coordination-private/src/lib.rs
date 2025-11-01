//! Private Coordination Layer Implementation
//!
//! This crate provides a private coordination layer implementation using TiDB
//! as the backend storage. It implements the `Coordination` trait from
//! `silvana-coordination-trait` to provide a fast, cost-effective alternative
//! to blockchain-based coordination layers.
//!
//! ## Features
//!
//! - JWT-based authentication with Ed25519 signatures
//! - Direct execution mode (no batching/multicall needed)
//! - Full support for jobs, KV storage, and sequence management
//! - Partial support for blocks, proofs, and settlements (requires schema updates)
//!
//! ## Implementation Status
//!
//! ### Fully Implemented (34/56 methods)
//! - All Job Management methods (14/14)
//! - All String KV operations (5/5)
//! - All Binary KV operations (4/4)
//! - All Sequence state operations (4/4)
//! - App instance data methods (4/4)
//! - Identification methods (2/2)
//! - Purge operation (1/1)
//!
//! ### Not Implemented (22/56 methods)
//! - Block Management (5 methods) - requires `blocks` table
//! - Proof Management (5 methods) - requires `proof_calculations` table
//! - Settlement operations (7 methods) - requires `settlements` and `block_settlements` tables
//! - Metadata operations (3 methods) - requires `app_instance_metadata` table
//! - Multicall operations (2 methods) - not needed for Private layer

pub mod auth;
pub mod config;
pub mod database;
pub mod error;
pub mod coordination;

pub use config::PrivateCoordinationConfig;
pub use coordination::PrivateCoordination;
pub use error::{PrivateCoordinationError, Result};

/// Re-export the Coordination trait for convenience
pub use silvana_coordination_trait::Coordination;