//! Private Coordination Layer Implementation
//!
//! This crate provides a private coordination layer implementation using gRPC
//! to communicate with the state service. It implements the `Coordination` trait
//! from `silvana-coordination-trait` to enable coordinators to manage jobs
//! with Ed25519 signature-based authentication.
//!

pub mod auth;
pub mod config;
pub mod coordination;
pub mod error;
pub mod jwt;

pub use config::PrivateCoordinationConfig;
pub use coordination::PrivateCoordination;
pub use error::{PrivateCoordinationError, Result};

/// Re-export the Coordination trait for convenience
pub use silvana_coordination_trait::Coordination;
