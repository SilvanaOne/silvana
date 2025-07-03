// Application modules
pub mod buffer;
pub mod database;
#[path = "entity/mod.rs"]
pub mod entities;

// Re-export common types
pub use database::EventDatabase;

// Include the generated protobuf code
pub mod events {
    tonic::include_proto!("silvana.events");
}
