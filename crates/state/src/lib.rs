//! State management crate for Silvana
//!
//! This crate provides:
//! - gRPC StateService implementation
//! - TiDB database entities and operations
//! - S3 storage integration for large objects
//! - Optimistic and pessimistic concurrency control
//! - JWT Ed25519 authentication
//!
//! Can be used as a library or standalone binary

pub mod auth;
pub mod concurrency;
pub mod coordinator_auth;
pub mod database;
pub mod entity;
pub mod handlers;
pub mod service;
pub mod storage;

// Re-export commonly used types
pub use auth::{verify_jwt, AuthInfo};
pub use concurrency::ConcurrencyController;
pub use coordinator_auth::{verify_and_authorize, verify_coordinator_access, verify_coordinator_signature};
pub use database::Database;
pub use handlers::StateServiceImpl;
pub use service::{ServiceConfig, StateServiceRunner};
pub use storage::PrivateStateStorage;

// Re-export proto types
pub mod proto {
    tonic::include_proto!("silvana.state.v1");

    // File descriptor set for gRPC reflection
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("state_descriptor");
}

// Descriptor pool for prost-reflect
use once_cell::sync::Lazy;
use prost_reflect::DescriptorPool;

pub static DESCRIPTOR_POOL: Lazy<DescriptorPool> =
    Lazy::new(|| DescriptorPool::decode(proto::FILE_DESCRIPTOR_SET.as_ref()).unwrap());