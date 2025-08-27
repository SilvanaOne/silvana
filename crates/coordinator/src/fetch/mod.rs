// Re-export everything from sui::fetch (including block module)
pub use sui::fetch::*;

// Re-export functions that were moved to settlement module
pub use crate::settlement::{fetch_pending_job_from_instances, fetch_all_pending_jobs};