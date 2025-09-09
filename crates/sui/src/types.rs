/// Operations for a specific app instance in a multicall
#[derive(Debug, Clone)]
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
    pub submit_proofs: Vec<(u64, Vec<u64>, Option<Vec<u64>>, Option<Vec<u64>>, String, String, u8, String, u64, u64)>,
    /// Create job operations: (method_name, job_description, block_number, sequences, sequences1, sequences2, data, interval_ms, next_scheduled_at, settlement_chain)
    pub create_jobs: Vec<(String, Option<String>, Option<u64>, Option<Vec<u64>>, Option<Vec<u64>>, Option<Vec<u64>>, Vec<u8>, Option<u64>, Option<u64>, Option<String>)>,
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

    /// Validate that related arrays have consistent lengths
    pub fn validate(&self) -> Result<(), String> {
        if self.fail_job_sequences.len() != self.fail_errors.len() {
            return Err(format!(
                "fail_job_sequences and fail_errors must have the same length: {} != {}",
                self.fail_job_sequences.len(),
                self.fail_errors.len()
            ));
        }

        if self.start_job_sequences.len() != self.start_job_memory_requirements.len() {
            return Err(format!(
                "start_job_sequences and start_job_memory_requirements must have the same length: {} != {}",
                self.start_job_sequences.len(),
                self.start_job_memory_requirements.len()
            ));
        }

        Ok(())
    }
}

/// Result of a multicall operation containing details about which operations succeeded
#[derive(Debug, Clone)]
pub struct MulticallResult {
    /// Transaction digest
    pub tx_digest: String,
    
    /// Jobs that were attempted to be started and their results
    pub start_jobs: Vec<u64>,
    pub start_results: Vec<bool>,
    
    /// Jobs that were attempted to be completed and their results
    pub complete_jobs: Vec<u64>,
    pub complete_results: Vec<bool>,
    
    /// Jobs that were attempted to be failed and their results
    pub fail_jobs: Vec<u64>,
    pub fail_results: Vec<bool>,
    
    /// Jobs that were attempted to be terminated and their results
    pub terminate_jobs: Vec<u64>,
    pub terminate_results: Vec<bool>,
    
    /// Timestamp of the multicall execution
    pub timestamp: u64,
}

impl MulticallResult {
    /// Get successfully started jobs with their sequences
    pub fn successful_start_jobs(&self) -> Vec<u64> {
        self.start_jobs
            .iter()
            .zip(&self.start_results)
            .filter_map(|(job, success)| if *success { Some(*job) } else { None })
            .collect()
    }
    
    /// Get failed start jobs with their sequences
    pub fn failed_start_jobs(&self) -> Vec<u64> {
        self.start_jobs
            .iter()
            .zip(&self.start_results)
            .filter_map(|(job, success)| if !*success { Some(*job) } else { None })
            .collect()
    }
}