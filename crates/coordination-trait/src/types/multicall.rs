//! Multicall operation types for batching

use serde::{Deserialize, Serialize};

/// Operations for a specific app instance in a multicall batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticallOperations {
    /// App instance identifier
    pub app_instance: String,

    /// Jobs to complete
    pub complete_job_sequences: Vec<u64>,

    /// Jobs to fail with their error messages
    pub fail_job_sequences: Vec<u64>,
    pub fail_errors: Vec<String>,

    /// Jobs to terminate
    pub terminate_job_sequences: Vec<u64>,

    /// Jobs to start with their memory requirements
    pub start_job_sequences: Vec<u64>,
    pub start_job_memory_requirements: Vec<u64>,

    /// Available memory for this app instance
    pub available_memory: u64,

    /// Update state operations: (sequence, new_state_data, new_data_availability_hash)
    pub update_state_for_sequences: Vec<(u64, Option<Vec<u8>>, Option<String>)>,

    /// Submit proof operations: (block_number, sequences, merged_sequences_1, merged_sequences_2, job_id, da_hash, cpu_cores, prover_architecture, prover_memory, cpu_time)
    pub submit_proofs: Vec<(
        u64,
        Vec<u64>,
        Option<Vec<u64>>,
        Option<Vec<u64>>,
        String,
        String,
        u8,
        String,
        u64,
        u64,
    )>,

    /// Create job operations: (method_name, job_description, block_number, sequences, sequences1, sequences2, data, interval_ms, next_scheduled_at, settlement_chain)
    pub create_jobs: Vec<(
        String,
        Option<String>,
        Option<u64>,
        Option<Vec<u64>>,
        Option<Vec<u64>>,
        Option<Vec<u64>>,
        Vec<u8>,
        Option<u64>,
        Option<u64>,
        Option<String>,
    )>,

    /// Create merge job operations: (block_number, sequences, sequences1, sequences2, job_description)
    pub create_merge_jobs: Vec<(u64, Vec<u64>, Vec<u64>, Vec<u64>, Option<String>)>,
}

impl MulticallOperations {
    /// Create a new MulticallOperations instance
    pub fn new(app_instance: String, available_memory: u64) -> Self {
        Self {
            app_instance,
            complete_job_sequences: Vec::new(),
            fail_job_sequences: Vec::new(),
            fail_errors: Vec::new(),
            terminate_job_sequences: Vec::new(),
            start_job_sequences: Vec::new(),
            start_job_memory_requirements: Vec::new(),
            available_memory,
            update_state_for_sequences: Vec::new(),
            submit_proofs: Vec::new(),
            create_jobs: Vec::new(),
            create_merge_jobs: Vec::new(),
        }
    }

    /// Get total count of operations for this app instance
    pub fn total_operations(&self) -> usize {
        self.complete_job_sequences.len()
            + self.fail_job_sequences.len()
            + self.terminate_job_sequences.len()
            + self.start_job_sequences.len()
            + self.update_state_for_sequences.len()
            + self.submit_proofs.len()
            + self.create_jobs.len()
            + self.create_merge_jobs.len()
    }

    /// Check if there are any operations to execute
    pub fn has_operations(&self) -> bool {
        self.total_operations() > 0
    }

    /// Add a job completion operation
    pub fn add_complete_job(&mut self, sequence: u64) {
        self.complete_job_sequences.push(sequence);
    }

    /// Add a job failure operation
    pub fn add_fail_job(&mut self, sequence: u64, error: String) {
        self.fail_job_sequences.push(sequence);
        self.fail_errors.push(error);
    }

    /// Add a job start operation
    pub fn add_start_job(&mut self, sequence: u64, memory_requirement: u64) {
        self.start_job_sequences.push(sequence);
        self.start_job_memory_requirements.push(memory_requirement);
    }

    /// Add a state update operation
    pub fn add_update_state(
        &mut self,
        sequence: u64,
        state_data: Option<Vec<u8>>,
        da_hash: Option<String>,
    ) {
        self.update_state_for_sequences.push((sequence, state_data, da_hash));
    }
}

/// Result of a multicall execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticallResult {
    /// Transaction hash of the multicall
    pub tx_hash: String,

    /// Number of operations successfully executed
    pub operations_executed: usize,

    /// Any errors encountered (for partial failures in some layers)
    pub errors: Vec<String>,
}

impl MulticallResult {
    /// Create a successful multicall result
    pub fn success(tx_hash: String, operations_executed: usize) -> Self {
        Self {
            tx_hash,
            operations_executed,
            errors: Vec::new(),
        }
    }

    /// Create a multicall result with errors
    pub fn with_errors(tx_hash: String, operations_executed: usize, errors: Vec<String>) -> Self {
        Self {
            tx_hash,
            operations_executed,
            errors,
        }
    }

    /// Check if the multicall was fully successful
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }
}