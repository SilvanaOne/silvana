use crate::transactions::{start_job_tx, complete_job_tx, fail_job_tx, terminate_job_tx, multicall_job_operations_tx, restart_failed_jobs_with_sequences_tx, submit_proof_tx, update_state_for_sequence_tx, create_app_job_tx, create_merge_job_tx, create_settle_job_tx, update_block_proof_data_availability_tx, update_block_settlement_tx_hash_tx, update_block_settlement_tx_included_in_block_tx, reject_proof_tx, start_proving_tx, try_create_block_tx};
use tracing::{info, warn, error, debug};

/// Interface for calling Sui Move functions related to job management
pub struct SilvanaSuiInterface;

impl SilvanaSuiInterface {
    pub fn new() -> Self {
        Self
    }

    /// Start a job on the Sui blockchain by calling the start_job Move function
    /// This should be called before starting the actual job processing
    /// Returns true if the transaction was successful, false if it failed
    pub async fn start_job(&mut self, app_instance: &str, job_sequence: u64) -> bool {
        debug!("Attempting to start job {} on Sui blockchain", job_sequence);
        
        match start_job_tx(app_instance, job_sequence).await {
            Ok(tx_digest) => {
                debug!("Successfully started job {} on blockchain, tx: {}", job_sequence, tx_digest);
                true
            }
            Err(e) => {
                info!("Failed to start job {} on blockchain: {}", job_sequence, e);
                false
            }
        }
    }

    /// Complete a job on the Sui blockchain by calling the complete_job Move function
    /// This should be called after successful job completion
    /// Returns the transaction hash if successful, or None if it failed
    pub async fn complete_job(&mut self, app_instance: &str, job_sequence: u64) -> Option<String> {
        debug!("Attempting to complete job {} on Sui blockchain", job_sequence);
        
        match complete_job_tx(app_instance, job_sequence).await {
            Ok(tx_digest) => {
                debug!("Successfully completed job {} on blockchain, tx: {}", job_sequence, tx_digest);
                Some(tx_digest)
            }
            Err(e) => {
                error!("Failed to complete job {} on blockchain: {}", job_sequence, e);
                None
            }
        }
    }

    /// Fail a job on the Sui blockchain by calling the fail_job Move function
    /// This should be called when job processing fails
    /// Returns the transaction hash if successful, or None if it failed
    pub async fn fail_job(&mut self, app_instance: &str, job_sequence: u64, error_message: &str) -> Option<String> {
        debug!("Attempting to fail job {} on Sui blockchain with error: {}", job_sequence, error_message);
        
        match fail_job_tx(app_instance, job_sequence, error_message).await {
            Ok(tx_digest) => {
                debug!("Successfully failed job {} on blockchain, tx: {}", job_sequence, tx_digest);
                Some(tx_digest)
            }
            Err(e) => {
                error!("Failed to fail job {} on blockchain: {}", job_sequence, e);
                None
            }
        }
    }

    /// Terminate a job on the Sui blockchain by calling the terminate_job Move function
    /// This permanently removes a job (useful for canceling periodic jobs)
    /// Returns Ok(tx_digest) if successful, Err((error_msg, optional_tx_digest)) if failed
    pub async fn terminate_job(&mut self, app_instance: &str, job_sequence: u64, gas_budget_sui: f64) -> Result<String, (String, Option<String>)> {
        debug!("Attempting to terminate job {} on Sui blockchain", job_sequence);
        
        let gas_budget_mist = (gas_budget_sui * 1_000_000_000.0) as u64;
        match terminate_job_tx(app_instance, job_sequence, Some(gas_budget_mist)).await {
            Ok(tx_digest) => {
                debug!("Successfully terminated job {} on blockchain, tx: {}", job_sequence, tx_digest);
                Ok(tx_digest)
            }
            Err(e) => {
                let error_msg = e.to_string();
                error!("Failed to terminate job {} on blockchain: {}", job_sequence, error_msg);
                
                // Extract tx digest from error if it's a TransactionError
                let tx_digest = if let Some(sui_error) = e.downcast_ref::<crate::error::SilvanaSuiInterfaceError>() {
                    if let crate::error::SilvanaSuiInterfaceError::TransactionError { tx_digest, .. } = sui_error {
                        tx_digest.clone()
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                Err((error_msg, tx_digest))
            }
        }
    }

    /// Restart a failed job on the Sui blockchain by calling the restart_failed_app_jobs Move function
    /// This moves a job from the failed_jobs table back to the active jobs table
    /// Returns Ok(tx_digest) if successful, Err((error_msg, optional_tx_digest)) if failed
    pub async fn restart_failed_job(&mut self, app_instance: &str, job_sequence: u64, gas_budget_sui: f64) -> Result<String, (String, Option<String>)> {
        debug!("Attempting to restart failed job {} on Sui blockchain", job_sequence);
        
        let gas_budget_mist = (gas_budget_sui * 1_000_000_000.0) as u64;
        // Restart a single specific failed job by passing it in the sequences vector
        match restart_failed_jobs_with_sequences_tx(app_instance, Some(vec![job_sequence]), Some(gas_budget_mist)).await {
            Ok(tx_digest) => {
                debug!("Successfully restarted failed job {} on blockchain, tx: {}", job_sequence, tx_digest);
                Ok(tx_digest)
            }
            Err(e) => {
                let error_msg = e.to_string();
                error!("Failed to restart failed job {} on blockchain: {}", job_sequence, error_msg);
                
                // Extract tx digest from error if it's a TransactionError
                let tx_digest = if let Some(sui_error) = e.downcast_ref::<crate::error::SilvanaSuiInterfaceError>() {
                    if let crate::error::SilvanaSuiInterfaceError::TransactionError { tx_digest, .. } = sui_error {
                        tx_digest.clone()
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                Err((error_msg, tx_digest))
            }
        }
    }

    /// Restart all failed jobs on the Sui blockchain by calling the restart_failed_app_jobs Move function
    /// This moves all jobs from the failed_jobs table back to the active jobs table
    /// Returns Ok(tx_digest) if successful, Err((error_msg, optional_tx_digest)) if failed
    pub async fn restart_failed_jobs(&mut self, app_instance: &str, gas_budget_sui: f64) -> Result<String, (String, Option<String>)> {
        debug!("Attempting to restart all failed jobs on Sui blockchain");
        
        let gas_budget_mist = (gas_budget_sui * 1_000_000_000.0) as u64;
        // Restart all failed jobs by passing None for job_sequences
        match restart_failed_jobs_with_sequences_tx(app_instance, None, Some(gas_budget_mist)).await {
            Ok(tx_digest) => {
                debug!("Successfully restarted all failed jobs on blockchain, tx: {}", tx_digest);
                Ok(tx_digest)
            }
            Err(e) => {
                let error_msg = e.to_string();
                error!("Failed to restart all failed jobs on blockchain: {}", error_msg);
                
                // Extract tx digest from error if it's a TransactionError
                let tx_digest = if let Some(sui_error) = e.downcast_ref::<crate::error::SilvanaSuiInterfaceError>() {
                    if let crate::error::SilvanaSuiInterfaceError::TransactionError { tx_digest, .. } = sui_error {
                        tx_digest.clone()
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                Err((error_msg, tx_digest))
            }
        }
    }

    /// Execute multiple job operations in a single transaction (multicall)
    /// Operations are executed in order: complete, fail, terminate, then start
    /// Returns Ok(tx_digest) if successful, Err((error_msg, optional_tx_digest)) if failed
    pub async fn multicall_job_operations(
        &mut self,
        app_instance: &str,
        complete_job_sequences: Vec<u64>,
        fail_job_sequences: Vec<u64>,
        fail_errors: Vec<String>,
        terminate_job_sequences: Vec<u64>,
        start_job_sequences: Vec<u64>,
        start_job_memory_requirements: Vec<u64>,
        available_memory: u64,
        gas_budget_sui: Option<f64>,
    ) -> Result<String, (String, Option<String>)> {
        // Validate that fail arrays have same length
        if fail_job_sequences.len() != fail_errors.len() {
            return Err((
                format!(
                    "fail_job_sequences and fail_errors must have the same length: {} != {}",
                    fail_job_sequences.len(),
                    fail_errors.len()
                ),
                None,
            ));
        }
        
        // Validate that start job arrays have same length
        if start_job_sequences.len() != start_job_memory_requirements.len() {
            return Err((
                format!(
                    "start_job_sequences and start_job_memory_requirements must have the same length: {} != {}",
                    start_job_sequences.len(),
                    start_job_memory_requirements.len()
                ),
                None,
            ));
        }

        debug!(
            "Attempting multicall job operations: {} complete, {} fail, {} terminate, {} start (available memory: {})",
            complete_job_sequences.len(),
            fail_job_sequences.len(),
            terminate_job_sequences.len(),
            start_job_sequences.len(),
            available_memory
        );

        let gas_budget_mist = gas_budget_sui.map(|sui| (sui * 1_000_000_000.0) as u64);
        
        match multicall_job_operations_tx(
            app_instance,
            complete_job_sequences.clone(),
            fail_job_sequences.clone(),
            fail_errors,
            terminate_job_sequences.clone(),
            start_job_sequences.clone(),
            start_job_memory_requirements.clone(),
            available_memory,
            gas_budget_mist,
        ).await {
            Ok(tx_digest) => {
                debug!(
                    "Successfully executed multicall job operations on blockchain, tx: {}",
                    tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                let error_msg = e.to_string();
                error!("Failed to execute multicall job operations on blockchain: {}", error_msg);
                
                // Extract tx digest from error if it's a TransactionError
                let tx_digest = if let Some(sui_error) = e.downcast_ref::<crate::error::SilvanaSuiInterfaceError>() {
                    if let crate::error::SilvanaSuiInterfaceError::TransactionError { tx_digest, .. } = sui_error {
                        tx_digest.clone()
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                Err((error_msg, tx_digest))
            }
        }
    }

    /// Try to start a job with retries, ensuring only one coordinator can start the same job
    /// This prevents race conditions when multiple coordinators are running
    pub async fn try_start_job_with_retry(&mut self, app_instance: &str, job_sequence: u64, max_retries: u32) -> bool {
        for attempt in 1..=max_retries {
            debug!("Attempting to start job {} (attempt {}/{})", job_sequence, attempt, max_retries);
            
            if self.start_job(app_instance, job_sequence).await {
                return true;
            }
            
            if attempt < max_retries {
                info!("Failed to start job {} on attempt {}, retrying...", job_sequence, attempt);
                // Small delay between retries to avoid overwhelming the network
                tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempt as u64)).await;
            }
        }
        
        info!("Failed to start job {} after {} attempts", job_sequence, max_retries);
        false
    }

    /// Create an app job on the Sui blockchain by calling the create_app_job Move function  
    /// This is a general method that can create any type of job
    /// Returns the transaction hash if successful, or an error if it failed
    #[allow(dead_code)]
    pub async fn create_app_job(
        &mut self,
        app_instance: &str,
        method_name: String,
        job_description: Option<String>,
        block_number: Option<u64>,
        sequences: Option<Vec<u64>>,
        sequences1: Option<Vec<u64>>,
        sequences2: Option<Vec<u64>>,
        data: Vec<u8>,
        interval_ms: Option<u64>,
        next_scheduled_at: Option<u64>,
        settlement_chain: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to create app job for method '{}' on Sui blockchain (data size: {} bytes)",
            method_name, data.len()
        );

        match create_app_job_tx(
            app_instance,
            method_name.clone(),
            job_description,
            block_number,
            sequences,
            sequences1,
            sequences2,
            data,
            interval_ms,
            next_scheduled_at,
            settlement_chain,
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully created app job for method '{}' on blockchain, tx: {}",
                    method_name, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to create app job for method '{}' on blockchain: {}",
                    method_name, e
                );
                Err(e.into())
            }
        }
    }

    /// Create a merge job on the Sui blockchain (convenience method)
    /// Returns the transaction hash if successful, or an error if it failed  
    pub async fn create_merge_job(
        &mut self,
        app_instance: &str,
        block_number: u64,
        sequences1: Vec<u64>,
        sequences2: Vec<u64>,
        job_description: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to create merge job for block {} with sequences {:?} + {:?} on Sui blockchain",
            block_number, sequences1, sequences2
        );

        match create_merge_job_tx(
            app_instance,
            block_number,
            sequences1.clone(),
            sequences2.clone(),
            job_description,
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully created merge job for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to create merge job for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Create a settle job on the Sui blockchain
    pub async fn create_settle_job(
        &mut self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        job_description: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to create settle job for block {} on chain {} on Sui blockchain",
            block_number, chain
        );

        match create_settle_job_tx(
            app_instance,
            block_number,
            chain,
            job_description,
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully created settle job for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to create settle job for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Update block proof data availability on the Sui blockchain
    pub async fn update_block_proof_data_availability(
        &mut self,
        app_instance: &str,
        block_number: u64,
        proof_data_availability: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to update block proof DA for block {} on Sui blockchain",
            block_number
        );

        match update_block_proof_data_availability_tx(
            app_instance,
            block_number,
            proof_data_availability.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully updated block proof DA for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to update block proof DA for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Update block settlement transaction hash on the Sui blockchain
    pub async fn update_block_settlement_tx_hash(
        &mut self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settlement_tx_hash: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to update block settlement tx hash for block {} on Sui blockchain",
            block_number
        );

        match update_block_settlement_tx_hash_tx(
            app_instance,
            block_number,
            chain.clone(),
            settlement_tx_hash.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully updated block settlement tx hash for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to update block settlement tx hash for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Update block settlement included in block on the Sui blockchain
    pub async fn update_block_settlement_tx_included_in_block(
        &mut self,
        app_instance: &str,
        block_number: u64,
        chain: String,
        settled_at: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to update block settlement included for block {} on Sui blockchain",
            block_number
        );

        match update_block_settlement_tx_included_in_block_tx(
            app_instance,
            block_number,
            chain.clone(),
            settled_at,
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully updated block settlement included for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to update block settlement included for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Submit a proof on the Sui blockchain by calling the submit_proof Move function
    /// Returns the transaction hash if successful, or None if it failed
    pub async fn submit_proof(
        &mut self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
        job_id: String,
        da_hash: String,
        cpu_cores: u8,
        prover_architecture: String,
        prover_memory: u64,
        cpu_time: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Attempting to submit proof for block {} job {} on Sui blockchain",
            block_number, job_id
        );

        match submit_proof_tx(
            app_instance,
            block_number,
            sequences,
            merged_sequences_1,
            merged_sequences_2,
            job_id.clone(),
            da_hash.clone(),
            cpu_cores,
            prover_architecture,
            prover_memory,
            cpu_time,
        )
        .await
        {
            Ok(tx_digest) => {
                debug!(
                    "Successfully submitted proof for job {} on blockchain, tx: {}",
                    job_id, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!("Failed to submit proof for job {} on blockchain: {}", job_id, e);
                Err(format!("Failed to submit proof: {}", e).into())
            }
        }
    }

    /// Terminate an app job on the Sui blockchain
    pub async fn terminate_app_job(
        &mut self,
        app_instance: &str,
        job_id: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to terminate app job {} on Sui blockchain",
            job_id
        );

        match crate::transactions::terminate_app_job_tx(
            app_instance,
            job_id,
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully terminated app job {} on blockchain, tx: {}",
                    job_id, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to terminate app job {} on blockchain: {}",
                    job_id, e
                );
                Err(e.into())
            }
        }
    }

    /// Reject a proof on the Sui blockchain by calling the reject_proof Move function
    /// Returns the transaction hash if successful, or an error if it failed
    pub async fn reject_proof(
        &mut self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to reject proof for block {} sequences {:?} on Sui blockchain",
            block_number, sequences
        );

        match reject_proof_tx(
            app_instance,
            block_number,
            sequences.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully rejected proof for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to reject proof for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

    /// Start proving (reserve proofs) on the Sui blockchain by calling the start_proving Move function
    /// Returns the transaction hash if successful, or an error if it failed
    /// Note: This may fail if another coordinator has already reserved the proofs
    pub async fn start_proving(
        &mut self,
        app_instance: &str,
        block_number: u64,
        sequences: Vec<u64>,
        merged_sequences_1: Option<Vec<u64>>,
        merged_sequences_2: Option<Vec<u64>>,
        job_id: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to reserve proofs for block {} sequences {:?} on Sui blockchain",
            block_number, sequences
        );

        match start_proving_tx(
            app_instance,
            block_number,
            sequences.clone(),
            merged_sequences_1,
            merged_sequences_2,
            job_id.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully reserved proofs for block {} job {} on blockchain, tx: {}",
                    block_number, job_id, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                // This is expected to fail if another coordinator already reserved the proofs
                // Log as info instead of warn to reduce noise
                info!(
                    "Could not reserve proofs for block {} job {} - likely already reserved by another coordinator: {}",
                    block_number, job_id, e
                );
                Err(e.into())
            }
        }
    }

    /// Update state for a sequence on the Sui blockchain by calling the update_state_for_sequence Move function
    /// Returns the transaction hash if successful, or an error if it failed
    pub async fn update_state_for_sequence(
        &mut self,
        app_instance: &str,
        sequence: u64,
        new_state_data: Option<Vec<u8>>,
        new_data_availability_hash: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "🔄 Attempting to update state for sequence {} on Sui blockchain (app_instance={})",
            sequence, app_instance
        );
        
        debug!(
            "State update details: has_state_data={}, has_da_hash={}", 
            new_state_data.is_some(), 
            new_data_availability_hash.is_some()
        );

        match update_state_for_sequence_tx(
            app_instance,
            sequence,
            new_state_data,
            new_data_availability_hash.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                debug!(
                    "✅ Successfully updated state for sequence {} on blockchain, tx: {}",
                    sequence, tx_digest
                );
                
                // Add a small delay to allow blockchain state to propagate
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                debug!("State update transaction {} should now be propagated", tx_digest);
                Ok(tx_digest)
            }
            Err(e) => {
                error!("❌ Failed to update state for sequence {} on blockchain: {}", sequence, e);
                Err(format!("Failed to update state: {}", e).into())
            }
        }
    }

    /// Try to create a new block for the app instance
    /// This calls the try_create_block Move function which checks if conditions are met
    /// Returns the transaction hash if successful, or an error if it failed
    pub async fn try_create_block(
        &mut self,
        app_instance: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Attempting to create block for app_instance {} on Sui blockchain",
            app_instance
        );

        match try_create_block_tx(app_instance).await {
            Ok(tx_digest) => {
                debug!(
                    "Successfully created block for app_instance {} on blockchain, tx: {}",
                    app_instance, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("NonEntryFunctionInvoked") || error_str.contains("not an entry function") {
                    // This is a critical error - the function doesn't exist or isn't entry
                    error!(
                        "CRITICAL: try_create_block is not an entry function for app_instance {}: {}",
                        app_instance, e
                    );
                } else if error_str.contains("conditions not met") {
                    // This is expected - conditions for block creation aren't met
                    debug!(
                        "Block creation conditions not met for app_instance {}: {}",
                        app_instance, e
                    );
                } else {
                    // Some other error
                    warn!(
                        "Failed to create block for app_instance {}: {}",
                        app_instance, e
                    );
                }
                Err(e.into())
            }
        }
    }

    /// Update block state data availability on the Sui blockchain
    pub async fn update_block_state_data_availability(
        &mut self,
        app_instance: &str,
        block_number: u64,
        state_data_availability: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to update block state DA for block {} on Sui blockchain",
            block_number
        );

        match crate::transactions::update_block_state_data_availability_tx(
            app_instance,
            block_number,
            state_data_availability.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "Successfully updated block state DA for block {} on blockchain, tx: {}",
                    block_number, tx_digest
                );
                Ok(tx_digest)
            }
            Err(e) => {
                error!(
                    "Failed to update block state DA for block {} on blockchain: {}",
                    block_number, e
                );
                Err(e.into())
            }
        }
    }

}