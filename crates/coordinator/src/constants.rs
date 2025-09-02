//! Centralized constants for the coordinator crate
//! 
//! This module contains all configurable constants for max jobs, delays, timeouts, etc.

// Job and concurrency limits
pub const MAX_CONCURRENT_AGENTS: usize = 10;
pub const JOB_POOL_SIZE: usize = 50;
pub const MAX_PER_INSTANCE: usize = 100;

// Delays and intervals (in milliseconds)
pub const MAX_JOB_START_DELAY_MS: u64 = 10000; // 10 seconds
pub const MIN_TIME_BETWEEN_BLOCKS: u64 = 60000; // 60 seconds
pub const PERIODIC_JOB_BUFFER_MS: u64 = 10000; // 10 seconds

// Timeouts (in milliseconds)
pub const TIMEOUT_MS: u64 = 2 * 60 * 1000; // 2 minutes
pub const STARTED_TIMEOUT_MS: u64 = 10 * 60 * 1000; // 10 minutes for Started status

// Retry configuration
pub const INITIAL_RETRY_DELAY_SECS: u64 = 5;
pub const MAX_RETRY_DELAY_SECS: u64 = 300; // Cap at 5 minutes
pub const MAX_RETRIES: usize = 100; // Increased from 5 to 100 for better resilience
pub const STREAM_TIMEOUT_SECS: u64 = 30;
pub const MAX_MERGE_ATTEMPTS: usize = 10;

// System requirements
pub const MIN_SYSTEM_MEMORY_GB: u64 = 2;

// Shutdown configuration
pub const SHUTDOWN_TIMEOUT_SECS: u64 = 600; // 10 minutes - Time to wait for graceful shutdown before forcing
pub const CONTAINER_FORCE_STOP_TIMEOUT_SECS: u64 = 5; // Timeout when force-stopping containers