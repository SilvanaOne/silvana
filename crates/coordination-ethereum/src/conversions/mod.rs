//! Type conversions between Solidity contract types and Rust coordination trait types
//!
//! This module provides bidirectional conversions between:
//! - Solidity contract types (from ABI bindings)
//! - Rust coordination trait types (from silvana-coordination-trait)
//!
//! # Usage
//!
//! ```ignore
//! use silvana_coordination_ethereum::conversions::*;
//!
//! // Convert Solidity JobStatus to Rust
//! let status = convert_job_status(0); // JobStatus::Pending
//!
//! // Create a Block from Solidity data
//! let block = create_block(...);
//! ```

pub mod app;
pub mod block;
pub mod helpers;
pub mod job;
pub mod proof;
pub mod sequence;
pub mod settlement;
pub mod solidity;

// Re-export commonly used conversion functions
pub use app::*;
pub use block::*;
pub use helpers::*;
pub use job::*;
pub use proof::*;
pub use sequence::*;
pub use settlement::*;
pub use solidity::*;
