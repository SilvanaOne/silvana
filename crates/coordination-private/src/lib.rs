//! Private Coordination Layer Implementation
//!
//! This crate provides a private coordination layer implementation using TiDB
//! as the backend storage. It implements the `Coordination` trait from
//! `silvana-coordination-trait` to provide a fast, cost-effective alternative
//! to blockchain-based coordination layers.
//!

pub mod auth;
pub mod config;
pub mod coordination;
pub mod database;
pub mod error;

pub use config::PrivateCoordinationConfig;
pub use coordination::PrivateCoordination;
pub use error::{PrivateCoordinationError, Result};

/// Re-export the Coordination trait for convenience
pub use silvana_coordination_trait::Coordination;
