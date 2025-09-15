// Application modules - explicitly public for external access
pub mod adapters;
pub mod database;
pub mod database_search;
pub mod storage;

// Service implementations
pub mod rpc;

pub use rpc::SilvanaRpcServiceImpl;

// Re-export common types
pub use database::EventDatabase;

// Re-export proto types from the proto crate
pub use proto;

// Re-export database entities from the tidb crate
pub use tidb as entities;

// Re-export buffer functionality
pub use buffer;

// Re-export adapter functionality for binary target
pub use adapters::create_event_buffer;
