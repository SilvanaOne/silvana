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