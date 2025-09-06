//! Centralized constants for the coordinator crate
//!
//! This module contains all configurable constants for max jobs, delays, timeouts, etc.

// =============================================================================
// Job and Concurrency Limits
// =============================================================================

/// Maximum number of agent Docker containers that can run simultaneously.
/// This limits the total parallel processing capacity of the coordinator.
pub const MAX_CONCURRENT_AGENT_CONTAINERS: usize = 10;

/// Size of the job selection pool buffer.
/// The coordinator maintains a pool of jobs to choose from when scheduling work.
/// Larger pool size allows better job prioritization but increases memory usage.
pub const JOB_SELECTION_POOL_SIZE: usize = 30;

/// Maximum number of jobs to process per app instance in a single batch.
/// This prevents any single app instance from monopolizing processing resources.
pub const MAX_JOBS_PER_INSTANCE_BATCH: usize = 100;

// =============================================================================
// Delays and Intervals (in milliseconds)
// =============================================================================

/// Maximum random delay before starting a job (in milliseconds).
/// A random delay between 0 and this value is added before starting each job
/// to prevent thundering herd problems when multiple coordinators start simultaneously.
pub const JOB_START_JITTER_MAX_MS: u64 = 10000; // 10 seconds

/// Base delay per running container for job acquisition (in milliseconds).
/// Each running/loading container adds this delay to prioritize coordinators with fewer containers.
/// For example, with 500ms per container:
/// - 0 containers: 0ms delay
/// - 5 containers: 2500ms delay  
/// - 10 containers: 5000ms delay
pub const JOB_ACQUISITION_DELAY_PER_CONTAINER_MS: u64 = 1000; // 1000ms per container

/// Maximum delay for job acquisition regardless of container count (in milliseconds).
/// This caps the delay to prevent excessive waiting even with many containers.
pub const JOB_ACQUISITION_MAX_DELAY_MS: u64 = 10000; // 10 seconds max

/// Minimum time that must pass between creating consecutive blocks (in milliseconds).
/// Blocks will not be created more frequently than this interval, even if new
/// sequences are available. This ensures blocks have a reasonable time window.
pub const BLOCK_CREATION_MIN_INTERVAL_MS: u64 = 60000; // 60 seconds

/// Time buffer for periodic jobs (in milliseconds).
/// Settlement nodes use this value directly, while regular nodes use 2x this value.
/// This gives settlement nodes priority in picking up periodic jobs.
pub const PERIODIC_JOB_EXECUTION_BUFFER_MS: u64 = 10000; // 10 seconds

// =============================================================================
// Proof Status Timeouts (in milliseconds)
// =============================================================================

/// Timeout for RESERVED proof status (in milliseconds).
/// If a proof remains in RESERVED status longer than this timeout,
/// it can be reassigned to another agent.
pub const PROOF_RESERVED_TIMEOUT_MS: u64 = 2 * 60 * 1000; // 2 minutes

/// Timeout for STARTED proof status (in milliseconds).
/// If a proof remains in STARTED status longer than this timeout,
/// it's considered stalled and will be rejected/reassigned.
pub const PROOF_STARTED_TIMEOUT_MS: u64 = 10 * 60 * 1000; // 10 minutes

// =============================================================================
// Retry and Recovery Configuration
// =============================================================================

/// Initial delay for retry backoff strategy (in seconds).
/// After a failure, the first retry will wait this many seconds.
pub const RETRY_INITIAL_DELAY_SECS: u64 = 5;

/// Maximum delay cap for exponential backoff (in seconds).
/// Retry delays will not exceed this value, even with exponential growth.
pub const RETRY_MAX_DELAY_SECS: u64 = 300; // 5 minutes

/// Maximum number of retry attempts for failed operations.
/// After this many failures, the operation is considered permanently failed.
pub const RETRY_MAX_ATTEMPTS: usize = 100;

/// Timeout for gRPC stream operations (in seconds).
/// If no data is received from a stream within this time, it's considered failed.
pub const GRPC_STREAM_TIMEOUT_SECS: u64 = 30;

/// Maximum number of attempts to merge proofs into a single block.
/// If merging fails this many times, the block creation is abandoned.
pub const PROOF_MERGE_MAX_ATTEMPTS: usize = 10;

// =============================================================================
// System Requirements and Resource Limits
// =============================================================================

/// Minimum system memory required to run an agent container (in GB).
/// The coordinator will not start agents if available memory is below this threshold.
pub const AGENT_MIN_MEMORY_REQUIREMENT_GB: u64 = 2;

// =============================================================================
// Shutdown and Cleanup Configuration
// =============================================================================

/// Timeout for graceful shutdown (in seconds).
/// During shutdown, the coordinator waits this long for running jobs to complete
/// before forcefully terminating them.
pub const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 600; // 10 minutes

/// Timeout when force-stopping Docker containers (in seconds).
/// If a container doesn't stop gracefully, wait this long before killing it.
pub const DOCKER_CONTAINER_FORCE_STOP_TIMEOUT_SECS: u64 = 5;

// =============================================================================
// Task Scheduling and Intervals
// =============================================================================

/// Interval for reconciliation task (in seconds).
/// The coordinator reconciles its state with the blockchain at this interval.
pub const RECONCILIATION_INTERVAL_SECS: u64 = 600; // 10 minutes

/// Interval for proof analysis task (in seconds).
/// Periodic analysis of proof completion and block creation.
pub const PROOF_ANALYSIS_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Interval for block creation check (in seconds).
/// How often to check if new blocks can be created.
pub const BLOCK_CREATION_CHECK_INTERVAL_SECS: u64 = 60; // 1 minute

/// Interval for balance check task (in seconds).
/// How often to check the coordinator's SUI balance.
pub const BALANCE_CHECK_INTERVAL_SECS: u64 = 1800; // 30 minutes

/// Interval for metrics reporting (in seconds).
/// How often to send metrics to monitoring systems.
pub const METRICS_REPORTING_INTERVAL_SECS: u64 = 30;

/// Interval for showing shutdown progress (in seconds).
/// How often to display progress messages during shutdown.
pub const SHUTDOWN_PROGRESS_INTERVAL_SECS: u64 = 10;

// =============================================================================
// Job Processing Timeouts and Delays
// =============================================================================

/// Timeout for stuck jobs (in seconds).
/// Jobs running longer than this are considered stuck and will be failed.
pub const STUCK_JOB_TIMEOUT_SECS: u64 = 600; // 10 minutes

/// Delay before initial reconciliation on startup (in seconds).
/// Allows the system to initialize before the first reconciliation.
pub const STARTUP_RECONCILIATION_DELAY_SECS: u64 = 60;

/// Timeout for job searcher shutdown (in seconds).
/// Maximum time to wait for job searcher to finish gracefully.
pub const JOB_SEARCHER_SHUTDOWN_TIMEOUT_SECS: u64 = 5;

// =============================================================================
// Processing Delays and Intervals (in milliseconds)
// =============================================================================

/// Delay between job processing checks (in milliseconds).
/// Small delay to prevent tight loops when checking for jobs.
#[cfg(test)]
pub const JOB_PROCESSING_CHECK_DELAY_MS: u64 = 10;

/// Delay after merge attempt failure (in milliseconds).
/// Wait time before retrying a failed merge operation.
pub const MERGE_RETRY_DELAY_MS: u64 = 500;

/// Delay for blockchain state propagation (in milliseconds).
/// Wait time to allow blockchain state changes to propagate.
pub const BLOCKCHAIN_PROPAGATION_DELAY_MS: u64 = 100;

/// Delay for shutdown cleanup (in milliseconds).
/// Small delay during shutdown to ensure clean state.
pub const SHUTDOWN_CLEANUP_DELAY_MS: u64 = 100;

// =============================================================================
// Container Management
// =============================================================================

/// Interval for checking container status (in seconds).
/// How often to check if a container has finished running.
pub const CONTAINER_STATUS_CHECK_INTERVAL_SECS: u64 = 1;

/// Periodic container health check interval (in seconds).
/// Safety check to ensure containers are still running properly.
pub const CONTAINER_HEALTH_CHECK_INTERVAL_SECS: u64 = 5;

/// Delay when waiting for resources (in seconds).
/// Wait time when system resources are insufficient.
pub const RESOURCE_WAIT_DELAY_SECS: u64 = 2;

/// Delay after resource check error (in seconds).
/// Wait time before retrying after a resource check fails.
pub const RESOURCE_ERROR_RETRY_DELAY_SECS: u64 = 1;

/// Delay between job availability checks (in seconds).
/// How often to check if new jobs are available.
pub const JOB_AVAILABILITY_CHECK_DELAY_SECS: u64 = 1;

/// Delay for waiting for pending jobs (in milliseconds).
/// Wait time when checking for pending jobs state changes.
pub const PENDING_JOBS_CHECK_DELAY_MS: u64 = 1000;

// =============================================================================
// Data Availability Configuration
// =============================================================================

/// Number of blocks to store as a quilt in Walrus for proof data availability.
/// When block_number % this value == 0, the last N block proofs are stored as a quilt.
pub const WALRUS_QUILT_BLOCK_INTERVAL: u64 = 100;

/// Test mode for Walrus quilts - when true, adds 580 test entries to simulate 600 proofs
/// This is used for testing Walrus's ability to handle large quilts with many pieces
/// (Walrus has a maximum of 600 pieces per quilt)
pub const WALRUS_TEST: bool = false;

// =============================================================================
// Testing and CLI
// =============================================================================

/// Delay after transaction submission in CLI (in seconds).
/// Wait time for transaction processing in CLI commands.
pub const CLI_TRANSACTION_WAIT_SECS: u64 = 5;
