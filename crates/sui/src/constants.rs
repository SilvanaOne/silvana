// Multicall operation limits

/// Maximum number of Move calls that can be created in a single transaction.
/// This limit prevents transaction size from exceeding blockchain limits.
pub const MAX_MOVE_CALLS_PER_TRANSACTION: usize = 100;

/// Maximum number of operations (complete, fail, terminate, start) that can be
/// passed to multicall_job_operations in a single call.
/// This limit ensures the Move function doesn't exceed gas or computational limits.
pub const MAX_OPERATIONS_PER_MULTICALL: usize = 100;

/// Get the maximum number of Move calls per transaction
pub fn get_max_move_calls_per_transaction() -> usize {
    MAX_MOVE_CALLS_PER_TRANSACTION
}

/// Get the maximum number of operations per multicall
pub fn get_max_operations_per_multicall() -> usize {
    MAX_OPERATIONS_PER_MULTICALL
}

// Object locking configuration

/// Timeout for object locks (in seconds).
/// Maximum time to hold locks on shared objects like app_instance.
pub const OBJECT_LOCK_TIMEOUT_SECS: u64 = 120; // 2 minutes
