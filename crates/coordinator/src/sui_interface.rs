use sui::jobs::{start_job_tx, complete_job_tx, fail_job_tx, submit_proof_tx};
use sui_rpc::Client;
use tracing::{info, error, warn};

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
}