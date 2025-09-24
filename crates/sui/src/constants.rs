// Multicall operation limits

/// Maximum number of operations that can be passed to multicall_job_operations in a single call.
/// This limit ensures the Move function doesn't exceed gas or computational limits.
pub const MAX_OPERATIONS_PER_MULTICALL: usize = 8;

/// Get the maximum number of operations per multicall
pub fn get_max_operations_per_multicall() -> usize {
    MAX_OPERATIONS_PER_MULTICALL
}

// Object locking configuration

/// Timeout for object locks (in seconds).
/// Maximum time to hold locks on shared objects like app_instance.
pub const OBJECT_LOCK_TIMEOUT_SECS: u64 = 120; // 2 minutes

// Faucet configuration

/// Amount of SUI to request from faucet when no coins are available for gas
pub const FAUCET_REQUEST_AMOUNT_SUI: f64 = 10.0;

// Gas budget configuration
// 1 SUI = 1,000,000,000 MIST

/// Maximum gas budget allowed by Sui network
pub const SIMULATION_GAS_BUDGET_MIST: u64 = 5_000_000_000;

/// Minimum gas budget for any transaction
pub const MIN_GAS_BUDGET_MIST: u64 = 5_000_000;

/// Maximum gas budget for any transaction
pub const MAX_GAS_BUDGET_MIST: u64 = 1_000_000_000;

/// Maximum computation cost for any transaction
pub const MAX_COMPUTATION_COST_MIST: u64 = 500_000_000;

/// Fallback gas budget when simulation fails or no custom budget provided
pub const FALLBACK_GAS_BUDGET_MIST: u64 = 100_000_000;
