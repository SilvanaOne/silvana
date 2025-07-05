//! Proto definitions for Silvana
//!
//! This crate contains all the protobuf definitions and generated types
//! that are shared across different Silvana components.

pub mod events {
    tonic::include_proto!("silvana.events.v1");
}

// Re-export commonly used types for convenience
pub use events::*;
