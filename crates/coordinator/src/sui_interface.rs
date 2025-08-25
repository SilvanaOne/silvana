use sui::jobs::{start_job_tx, complete_job_tx, fail_job_tx, submit_proof_tx, update_state_for_sequence_tx, create_app_job_tx, create_merge_job_tx, reject_proof_tx, start_proving_tx, try_create_block_tx};
use sui_rpc::Client;
use tracing::{info, warn, error, debug};

/// Interface for calling Sui Move functions related to job management
pub struct SuiJobInterface {
    client: Client,
}

impl SuiJobInterface {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Start a job on the Sui blockchain by calling the start_job Move function
    /// This should be called before starting the actual job processing
    /// Returns true if the transaction was successful, false if it failed
    pub async fn start_job(&mut self, app_instance: &str, job_sequence: u64) -> bool {
        info!("Attempting to start job {} on Sui blockchain", job_sequence);
        
        match start_job_tx(&mut self.client, app_instance, job_sequence).await {
            Ok(tx_digest) => {
                info!("Successfully started job {} on blockchain, tx: {}", job_sequence, tx_digest);
                true
            }
            Err(e) => {
                error!("Failed to start job {} on blockchain: {}", job_sequence, e);
                false
            }
        }
    }

    /// Complete a job on the Sui blockchain by calling the complete_job Move function
    /// This should be called after successful job completion
    /// Returns true if the transaction was successful, false if it failed
    pub async fn complete_job(&mut self, app_instance: &str, job_sequence: u64) -> bool {
        info!("Attempting to complete job {} on Sui blockchain", job_sequence);
        
        match complete_job_tx(&mut self.client, app_instance, job_sequence).await {
            Ok(tx_digest) => {
                info!("Successfully completed job {} on blockchain, tx: {}", job_sequence, tx_digest);
                true
            }
            Err(e) => {
                error!("Failed to complete job {} on blockchain: {}", job_sequence, e);
                false
            }
        }
    }

    /// Fail a job on the Sui blockchain by calling the fail_job Move function
    /// This should be called when job processing fails
    /// Returns true if the transaction was successful, false if it failed
    pub async fn fail_job(&mut self, app_instance: &str, job_sequence: u64, error_message: &str) -> bool {
        info!("Attempting to fail job {} on Sui blockchain with error: {}", job_sequence, error_message);
        
        match fail_job_tx(&mut self.client, app_instance, job_sequence, error_message).await {
            Ok(tx_digest) => {
                info!("Successfully failed job {} on blockchain, tx: {}", job_sequence, tx_digest);
                true
            }
            Err(e) => {
                error!("Failed to fail job {} on blockchain: {}", job_sequence, e);
                false
            }
        }
    }

    /// Try to start a job with retries, ensuring only one coordinator can start the same job
    /// This prevents race conditions when multiple coordinators are running
    pub async fn try_start_job_with_retry(&mut self, app_instance: &str, job_sequence: u64, max_retries: u32) -> bool {
        for attempt in 1..=max_retries {
            info!("Attempting to start job {} (attempt {}/{})", job_sequence, attempt, max_retries);
            
            if self.start_job(app_instance, job_sequence).await {
                return true;
            }
            
            if attempt < max_retries {
                warn!("Failed to start job {} on attempt {}, retrying...", job_sequence, attempt);
                // Small delay between retries to avoid overwhelming the network
                tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempt as u64)).await;
            }
        }
        
        error!("Failed to start job {} after {} attempts", job_sequence, max_retries);
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
        sequences: Option<Vec<u64>>,
        data: Vec<u8>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Attempting to create app job for method '{}' on Sui blockchain (data size: {} bytes)",
            method_name, data.len()
        );

        match create_app_job_tx(
            &mut self.client,
            app_instance,
            method_name.clone(),
            job_description,
            sequences,
            data,
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
            &mut self.client,
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
        info!(
            "Attempting to submit proof for block {} job {} on Sui blockchain",
            block_number, job_id
        );

        match submit_proof_tx(
            &mut self.client,
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
                info!(
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
            &mut self.client,
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
            &mut self.client,
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
        info!(
            "🔄 Attempting to update state for sequence {} on Sui blockchain (app_instance={})",
            sequence, app_instance
        );
        
        info!(
            "State update details: has_state_data={}, has_da_hash={}", 
            new_state_data.is_some(), 
            new_data_availability_hash.is_some()
        );

        match update_state_for_sequence_tx(
            &mut self.client,
            app_instance,
            sequence,
            new_state_data,
            new_data_availability_hash.clone(),
        )
        .await
        {
            Ok(tx_digest) => {
                info!(
                    "✅ Successfully updated state for sequence {} on blockchain, tx: {} - waiting for propagation",
                    sequence, tx_digest
                );
                
                // Add a small delay to allow blockchain state to propagate
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                info!("State update transaction {} should now be propagated", tx_digest);
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

        match try_create_block_tx(&mut self.client, app_instance).await {
            Ok(tx_digest) => {
                info!(
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
}