use std::sync::{Arc, OnceLock};

// Module declarations
pub mod secrets_storage;

// Global static to store the DynamoDB client configuration (shared across modules)
static DYNAMODB_CLIENT: OnceLock<Arc<aws_sdk_dynamodb::Client>> = OnceLock::new();