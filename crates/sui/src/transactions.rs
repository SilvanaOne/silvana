use anyhow::{Result, Context, anyhow};
use std::str::FromStr;
use sui_rpc::field::FieldMask;
use sui_rpc::proto::sui::rpc::v2beta2 as proto;
use sui_sdk_types as sui;
use sui_crypto::SuiSigner;
use tracing::{debug, warn, error, info};
use tokio::time::{sleep, Duration};

use crate::chain::get_reference_gas_price;
use crate::coin::fetch_coin;
use crate::object_lock::get_object_lock_manager;
use crate::state::SharedSuiState;
use crate::error::SilvanaSuiInterfaceError;

/// Helper function to check transaction effects for errors
fn check_transaction_effects(tx_resp: &proto::ExecuteTransactionResponse, operation: &str) -> Result<()> {
    // Get transaction digest first (available even for failed transactions)
    let tx_digest = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.digest.as_ref())
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    // Check for errors in transaction effects
    if let Some(ref transaction) = tx_resp.transaction {
        if let Some(ref effects) = transaction.effects {
            debug!("{} effects status: {:?}", operation, effects.status);
            if let Some(ref status) = effects.status {
                if status.error.is_some() {
                    let error_msg = status.error.as_ref().unwrap();
                    let error_str = format!("{:?}", error_msg);
                    
                    // Clean up common error messages for better readability
                    let clean_error = if error_str.contains("MoveAbort") {
                        // Extract abort code and location for Move aborts
                        let mut parts = vec![];
                        if error_str.contains("abort_code: Some(") {
                            if let Some(start) = error_str.find("abort_code: Some(") {
                                let code_start = start + "abort_code: Some(".len();
                                if let Some(end) = error_str[code_start..].find(')') {
                                    let abort_code_str = &error_str[code_start..code_start+end];
                                    // Try to parse the abort code as u64 and format as hex
                                    if let Ok(abort_code) = abort_code_str.parse::<u64>() {
                                        parts.push(format!("abort_code: 0x{:016X}", abort_code));
                                    } else {
                                        parts.push(format!("abort_code: {}", abort_code_str));
                                    }
                                }
                            }
                        }
                        if error_str.contains("function_name: Some(") {
                            if let Some(start) = error_str.find("function_name: Some(\"") {
                                let name_start = start + "function_name: Some(\"".len();
                                if let Some(end) = error_str[name_start..].find("\"") {
                                    parts.push(format!("function: {}", &error_str[name_start..name_start+end]));
                                }
                            }
                        }
                        if !parts.is_empty() {
                            format!("MoveAbort: {}", parts.join(", "))
                        } else {
                            "Move execution aborted".to_string()
                        }
                    } else {
                        error_str
                    };
                    
                    // Log as info for expected race conditions (multiple coordinators competing)
                    if clean_error.contains("reserve_proof") || 
                       clean_error.contains("start_job") || 
                       clean_error.contains("start_proving") {
                        info!("{} transaction failed (normal for multiple coordinators): {} (tx: {})", operation, clean_error, tx_digest);
                    } else {
                        error!("{} transaction failed: {} (tx: {})", operation, clean_error, tx_digest);
                    }
                    
                    return Err(SilvanaSuiInterfaceError::TransactionError {
                        message: format!("{} transaction failed: {}", operation, clean_error),
                        tx_digest: Some(tx_digest.clone()),
                    }.into());
                }
            }
        }
    }
    
    // Check transaction was successful
    if tx_resp.finality.is_none() {
        error!("{} transaction did not achieve finality (tx: {})", operation, tx_digest);
        return Err(SilvanaSuiInterfaceError::TransactionError {
            message: format!("{} transaction did not achieve finality", operation),
            tx_digest: Some(tx_digest),
        }.into());
    }
    
    // Check for transaction success in effects
    let tx_successful = tx_resp.transaction
        .as_ref()
        .and_then(|t| t.effects.as_ref())
        .and_then(|e| e.status.as_ref())
        .map(|s| s.error.is_none())
        .unwrap_or(false);
        
    if !tx_successful {
        error!("{} transaction failed despite being executed", operation);
        return Err(anyhow!("{} transaction failed despite being executed", operation));
    }
    
    Ok(())
}

/// Wait for a transaction to be available in the ledger
/// This polls GetTransaction until the transaction is found or timeout occurs
/// Optionally takes a CoinLockGuard to ensure the coin stays locked until confirmation
async fn wait_for_transaction(
    tx_digest: &str,
    max_wait_ms: Option<u64>,
    _gas_guard: Option<crate::coin::CoinLockGuard>,
) -> Result<()> {
    let timeout = max_wait_ms.unwrap_or(5000); // Default to 5000ms if not specified
    let start = std::time::Instant::now();
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut ledger = client.ledger_client();
    
    debug!("Waiting for transaction {} to be available in ledger (max {}ms)", tx_digest, timeout);
    
    loop {
        // Check if we've exceeded the maximum wait time
        if start.elapsed().as_millis() > timeout as u128 {
            return Err(anyhow!("Timeout waiting for transaction {} after {}ms", tx_digest, timeout));
        }
        
        // Try to get the transaction - just check if it exists
        let req = proto::GetTransactionRequest {
            digest: Some(tx_digest.to_string()),
            read_mask: Some(FieldMask { 
                paths: vec!["digest".into()] // Just request minimal data to check existence
            }),
        };
        
        match ledger.get_transaction(req).await {
            Ok(_) => {
                // Transaction found! It's available in the ledger
                debug!("Transaction {} is now available in ledger (took {}ms)", 
                    tx_digest, start.elapsed().as_millis());
                return Ok(());
            }
            Err(e) => {
                // Transaction not found yet, this is expected while we wait
                debug!("Transaction {} not yet available: {}", tx_digest, e);
            }
        }
        
        // Wait before polling again
        sleep(Duration::from_millis(200)).await;
    }
}

/// Get the app instance object ID from the job data
fn get_app_instance_id(app_instance_str: &str) -> Result<sui::Address> {
    let app_instance_id = if app_instance_str.starts_with("0x") {
        app_instance_str.to_string()
    } else {
        format!("0x{}", app_instance_str)
    };
    Ok(sui::Address::from_str(&app_instance_id)?)
}

/// Get the clock object ID (0x6 for system clock)
fn get_clock_object_id() -> sui::Address {
    sui::Address::from_str("0x0000000000000000000000000000000000000000000000000000000000000006")
        .expect("Valid clock object ID")
}


/// Create and submit a transaction to start a job
pub async fn start_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
) -> Result<String> {
    debug!("Creating start_app_job transaction for job_sequence: {}", job_sequence);
    
    execute_app_instance_function(
        app_instance_str,
        "start_app_job",
        move |tb, app_instance_arg, clock_arg| {
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to complete a job
pub async fn complete_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
) -> Result<String> {
    debug!("Creating complete_app_job transaction for job_sequence: {}", job_sequence);
    
    execute_app_instance_function(
        app_instance_str,
        "complete_app_job",
        move |tb, app_instance_arg, clock_arg| {
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to fail a job
pub async fn fail_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
    error_message: &str,
) -> Result<String> {
    debug!("Creating fail_app_job transaction for job_sequence: {} with error: {}", job_sequence, error_message);
    
    // Debug: Query current job state before attempting to fail it
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID for debug query")?;
    match query_job_status(app_instance_id, job_sequence).await {
        Ok(status) => {
            debug!("Current job {} status before fail attempt: {:?}", job_sequence, status);
        }
        Err(e) => {
            warn!("Failed to query job {} status before fail: {}", job_sequence, e);
        }
    }
    
    let error_msg = error_message.to_string();
    execute_app_instance_function(
        app_instance_str,
        "fail_app_job",
        move |tb, app_instance_arg, clock_arg| {
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            let error_arg = tb.input(sui_transaction_builder::Serialized(&error_msg));
            vec![app_instance_arg, job_sequence_arg, error_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to terminate a job
pub async fn terminate_job_tx(
    app_instance_str: &str,
    job_sequence: u64,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!("Creating terminate_app_job transaction for job_sequence: {}", job_sequence);
    
    execute_app_instance_function_with_gas(
        app_instance_str,
        "terminate_app_job",
        gas_budget,
        move |tb, app_instance_arg, clock_arg| {
            let job_sequence_arg = tb.input(sui_transaction_builder::Serialized(&job_sequence));
            vec![app_instance_arg, job_sequence_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to restart failed jobs (with optional specific job sequences)
pub async fn restart_failed_jobs_with_sequences_tx(
    app_instance_str: &str,
    job_sequences: Option<Vec<u64>>,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!("Creating restart_failed_app_jobs transaction with job_sequences: {:?}", job_sequences);
    
    // Default to 1 SUI for this potentially heavy operation if not specified
    let gas = gas_budget.or(Some(1_000_000_000));
    
    execute_app_instance_function_with_gas(
        app_instance_str,
        "restart_failed_app_jobs",
        gas,
        move |tb, app_instance_arg, clock_arg| {
            let job_sequences_arg = tb.input(sui_transaction_builder::Serialized(&job_sequences));
            vec![app_instance_arg, job_sequences_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to remove failed jobs (with optional specific job sequences)
pub async fn remove_failed_jobs_tx(
    app_instance_str: &str,
    job_sequences: Option<Vec<u64>>,
    gas_budget: Option<u64>,
) -> Result<String> {
    debug!("Creating remove_failed_app_jobs transaction with job_sequences: {:?}", job_sequences);
    
    execute_app_instance_function_with_gas(
        app_instance_str,
        "remove_failed_app_jobs",
        gas_budget,
        move |tb, app_instance_arg, clock_arg| {
            let job_sequences_arg = tb.input(sui_transaction_builder::Serialized(&job_sequences));
            vec![app_instance_arg, job_sequences_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to submit a proof
pub async fn submit_proof_tx(
    app_instance_str: &str,
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
) -> Result<String> {
    debug!("Creating submit_proof transaction for block_number: {}, job_id: {}", block_number, job_id);
    
    execute_app_instance_function(
        app_instance_str,
        "submit_proof",
        move |tb, app_instance_arg, clock_arg| {
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let merged_sequences_1_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
            let merged_sequences_2_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));
            let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));
            let da_hash_arg = tb.input(sui_transaction_builder::Serialized(&da_hash));
            let cpu_cores_arg = tb.input(sui_transaction_builder::Serialized(&cpu_cores));
            let prover_architecture_arg = tb.input(sui_transaction_builder::Serialized(&prover_architecture));
            let prover_memory_arg = tb.input(sui_transaction_builder::Serialized(&prover_memory));
            let cpu_time_arg = tb.input(sui_transaction_builder::Serialized(&cpu_time));
            
            vec![
                app_instance_arg,
                block_number_arg,
                sequences_arg,
                merged_sequences_1_arg,
                merged_sequences_2_arg,
                job_id_arg,
                da_hash_arg,
                cpu_cores_arg,
                prover_architecture_arg,
                prover_memory_arg,
                cpu_time_arg,
                clock_arg,
            ]
        },
    ).await
}

/// Create and submit a transaction to update state for a sequence
pub async fn update_state_for_sequence_tx(
    app_instance_str: &str,
    sequence: u64,
    new_state_data: Option<Vec<u8>>,
    new_data_availability_hash: Option<String>,
) -> Result<String> {
    debug!("Creating update_state_for_sequence transaction for sequence: {}", sequence);
    
    execute_app_instance_function(
        app_instance_str,
        "update_state_for_sequence",
        move |tb, app_instance_arg, clock_arg| {
            let sequence_arg = tb.input(sui_transaction_builder::Serialized(&sequence));
            let new_state_data_arg = tb.input(sui_transaction_builder::Serialized(&new_state_data));
            let new_data_availability_hash_arg = tb.input(sui_transaction_builder::Serialized(&new_data_availability_hash));
            
            vec![
                app_instance_arg,
                sequence_arg,
                new_state_data_arg,
                new_data_availability_hash_arg,
                clock_arg,
            ]
        },
    ).await
}

/// Create and submit a transaction to create an app job
/// This is a general function that can create any type of job by specifying method_name and data
pub async fn create_app_job_tx(
    app_instance_str: &str,
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
) -> Result<String> {
    debug!("Creating app job transaction for method: {}, data size: {} bytes", 
        method_name, data.len());
    
    let method_name_clone = method_name.clone();
    execute_app_instance_function(
        app_instance_str,
        "create_app_job",
        move |tb, app_instance_arg, clock_arg| {
            let method_name_arg = tb.input(sui_transaction_builder::Serialized(&method_name));
            let job_description_arg = tb.input(sui_transaction_builder::Serialized(&job_description));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let sequences1_arg = tb.input(sui_transaction_builder::Serialized(&sequences1));
            let sequences2_arg = tb.input(sui_transaction_builder::Serialized(&sequences2));
            let data_arg = tb.input(sui_transaction_builder::Serialized(&data));
            // Add the periodic job parameters and settlement_chain
            let interval_ms_arg = tb.input(sui_transaction_builder::Serialized(&interval_ms));
            let next_scheduled_at_arg = tb.input(sui_transaction_builder::Serialized(&next_scheduled_at));
            let settlement_chain_arg = tb.input(sui_transaction_builder::Serialized(&settlement_chain));
            
            vec![
                app_instance_arg,
                method_name_arg,
                job_description_arg,
                block_number_arg,
                sequences_arg,
                sequences1_arg,
                sequences2_arg,
                data_arg,
                interval_ms_arg,
                next_scheduled_at_arg,
                settlement_chain_arg,
                clock_arg,
            ]
        },
    ).await.map_err(|e| anyhow!("Failed to create app job for method '{}': {}", method_name_clone, e))
}

/// Create and submit a transaction to reject a proof
pub async fn reject_proof_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
) -> Result<String> {
    debug!("Creating reject_proof transaction for block_number: {}, sequences: {:?}", block_number, sequences);
    
    execute_app_instance_function(
        app_instance_str,
        "reject_proof",
        move |tb, app_instance_arg, clock_arg| {
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            vec![app_instance_arg, block_number_arg, sequences_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to start proving (reserve proofs)
pub async fn start_proving_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences: Vec<u64>,
    merged_sequences_1: Option<Vec<u64>>,
    merged_sequences_2: Option<Vec<u64>>,
    job_id: String,
) -> Result<String> {
    debug!("Creating start_proving transaction for block_number: {}, sequences: {:?}", block_number, sequences);
    
    // Use the helper but handle expected errors specially
    execute_app_instance_function(
        app_instance_str,
        "start_proving",
        move |tb, app_instance_arg, clock_arg| {
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let sequences_arg = tb.input(sui_transaction_builder::Serialized(&sequences));
            let merged_sequences_1_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_1));
            let merged_sequences_2_arg = tb.input(sui_transaction_builder::Serialized(&merged_sequences_2));
            let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));
            
            vec![
                app_instance_arg, 
                block_number_arg, 
                sequences_arg, 
                merged_sequences_1_arg,
                merged_sequences_2_arg,
                job_id_arg,
                clock_arg
            ]
        },
    ).await.map_err(|e| {
        // Special handling for expected errors
        let error_str = e.to_string();
        if error_str.contains("Transaction failed") {
            info!("start_proving transaction failed (proofs may be already reserved): {}", error_str);
            anyhow!("Failed to reserve proofs - may be already reserved by another coordinator")
        } else {
            e
        }
    })
}

/// Convenience function to create a merge job
pub async fn create_merge_job_tx(
    app_instance_str: &str,
    block_number: u64,
    sequences1: Vec<u64>,
    sequences2: Vec<u64>,
    job_description: Option<String>,
) -> Result<String> {
    // Combine and sort sequences from both proofs
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();
    combined_sequences.dedup(); // Remove any duplicates
    
    debug!("Combined sequences for merge job: {:?}", combined_sequences);
    debug!("Block number: {}, sequences1: {:?}, sequences2: {:?}", block_number, sequences1, sequences2);

    // Call the general create_app_job_tx function with the new fields
    create_app_job_tx(
        app_instance_str,
        "merge".to_string(),
        job_description,
        Some(block_number),         // Pass block_number
        Some(combined_sequences),   // Pass the combined sequences
        Some(sequences1),           // Pass sequences1
        Some(sequences2),           // Pass sequences2
        vec![],                     // Empty data since we're using the Job fields now
        None,                       // No interval for merge jobs
        None,                       // No scheduled time for merge jobs
        None,                       // Not a settlement job
    ).await
}

/// Update block proof data availability
pub async fn update_block_proof_data_availability_tx(
    app_instance_str: &str,
    block_number: u64,
    proof_data_availability: String,
) -> Result<String> {
    debug!("Creating update_block_proof_data_availability transaction for block_number: {}", block_number);
    
    execute_app_instance_function(
        app_instance_str,
        "update_block_proof_data_availability",
        move |tb, app_instance_arg, clock_arg| {
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let proof_da_arg = tb.input(sui_transaction_builder::Serialized(&proof_data_availability));
            
            vec![app_instance_arg, block_number_arg, proof_da_arg, clock_arg]
        },
    ).await
}

/// Update block state data availability
pub async fn update_block_state_data_availability_tx(
    app_instance_str: &str,
    block_number: u64,
    state_data_availability: String,
) -> Result<String> {
    debug!("Creating update_block_state_data_availability transaction for block_number: {}", block_number);
    
    execute_app_instance_function(
        app_instance_str,
        "update_block_state_data_availability",
        move |tb, app_instance_arg, clock_arg| {
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let state_da_arg = tb.input(sui_transaction_builder::Serialized(&state_data_availability));
            
            vec![app_instance_arg, block_number_arg, state_da_arg, clock_arg]
        },
    ).await
}

/// Create and submit a transaction to increase sequence (add new action)
pub async fn increase_sequence_tx(
    app_instance_str: &str,
    optimistic_state: Vec<u8>,
    transition_data: Vec<u8>,
) -> Result<String> {
    debug!("Creating increase_sequence transaction with optimistic_state size: {} bytes, transition_data size: {} bytes", 
        optimistic_state.len(), transition_data.len());
    
    execute_app_instance_function(
        app_instance_str,
        "increase_sequence",
        move |tb, app_instance_arg, clock_arg| {
            let optimistic_state_arg = tb.input(sui_transaction_builder::Serialized(&optimistic_state));
            let transition_data_arg = tb.input(sui_transaction_builder::Serialized(&transition_data));
            
            vec![app_instance_arg, optimistic_state_arg, transition_data_arg, clock_arg]
        },
    ).await
}

/// Convenience function to create a settle job
pub async fn create_settle_job_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    job_description: Option<String>,
) -> Result<String> {
    // Call the general create_app_job_tx function with settle method
    // Settlement jobs are periodic with 1 minute interval
    let interval_ms = 60000u64; // 1 minute in milliseconds
    let next_scheduled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    create_app_job_tx(
        app_instance_str,
        "settle".to_string(),
        job_description,
        Some(block_number),         // Pass block_number
        None,                       // No sequences for settle
        None,                       // No sequences1 for settle
        None,                       // No sequences2 for settle
        vec![],                     // Empty data
        Some(interval_ms),          // 1 minute interval for periodic settle jobs
        Some(next_scheduled_at),    // Start now
        Some(chain),                // This is a settlement job for the specified chain
    ).await
}

/// Terminate an app job
pub async fn terminate_app_job_tx(
    app_instance_str: &str,
    job_id: u64,
) -> Result<String> {
    debug!("Creating terminate_app_job transaction for job_id: {}", job_id);
    
    execute_app_instance_function(
        app_instance_str,
        "terminate_app_job",
        move |tb, app_instance_arg, clock_arg| {
            let job_id_arg = tb.input(sui_transaction_builder::Serialized(&job_id));
            
            vec![
                app_instance_arg,
                job_id_arg,
                clock_arg,
            ]
        },
    ).await
}

/// Update block settlement transaction hash
pub async fn update_block_settlement_tx_hash_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    settlement_tx_hash: String,
) -> Result<String> {
    debug!("Creating update_block_settlement_tx_hash transaction for block_number: {} on chain: {}", block_number, chain);
    
    execute_app_instance_function(
        app_instance_str,
        "update_block_settlement_tx_hash",
        move |tb, app_instance_arg, clock_arg| {
            // Order must match Move function signature: chain, block_number, settlement_tx_hash
            let chain_arg = tb.input(sui_transaction_builder::Serialized(&chain));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let settlement_tx_hash_arg = tb.input(sui_transaction_builder::Serialized(&settlement_tx_hash));
            
            vec![app_instance_arg, chain_arg, block_number_arg, settlement_tx_hash_arg, clock_arg]
        },
    ).await
}

/// Update block settlement transaction included in block
pub async fn update_block_settlement_tx_included_in_block_tx(
    app_instance_str: &str,
    block_number: u64,
    chain: String,
    settled_at: u64,
) -> Result<String> {
    debug!("Creating update_block_settlement_tx_included_in_block transaction for block_number: {} on chain: {}", block_number, chain);
    
    execute_app_instance_function(
        app_instance_str,
        "update_block_settlement_tx_included_in_block",
        move |tb, app_instance_arg, clock_arg| {
            // Order must match Move function signature: chain, block_number, settled_at
            let chain_arg = tb.input(sui_transaction_builder::Serialized(&chain));
            let block_number_arg = tb.input(sui_transaction_builder::Serialized(&block_number));
            let settled_at_arg = tb.input(sui_transaction_builder::Serialized(&settled_at));
            
            vec![app_instance_arg, chain_arg, block_number_arg, settled_at_arg, clock_arg]
        },
    ).await
}

/// Debug function to query job status from the blockchain
async fn query_job_status(
    app_instance_id: sui::Address,
    job_sequence: u64,
) -> Result<String> {
    use crate::fetch::app_instance::fetch_app_instance;
    
    // Use the existing fetch_app_instance function which handles formatting correctly
    match fetch_app_instance(&app_instance_id.to_string()).await {
        Ok(app_instance) => {
            // Check if this is a settlement job for any chain
            for (chain, settlement) in &app_instance.settlements {
                if let Some(settlement_job) = settlement.settlement_job {
                    if settlement_job == job_sequence {
                        debug!("Job {} is the settlement job for chain {}", job_sequence, chain);
                        return Ok(format!("Job {} is settlement job for chain {}", job_sequence, chain));
                    }
                }
            }
            
            debug!("Found app_instance for job {} check", job_sequence);
            return Ok(format!("Job {} status check completed", job_sequence));
        }
        Err(e) => {
            // Don't warn for fetch errors as they're expected for some job types
            debug!("Could not fetch app_instance for job {} status check: {}", job_sequence, e);
            return Ok(format!("Job {} status check skipped", job_sequence));
        }
    }
}

/// Get object details including ownership information and initial_shared_version
async fn get_object_details(
    object_id: sui::Address,
) -> Result<(sui::ObjectReference, Option<u64>)> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut ledger = client.ledger_client();
    
    let response = ledger
        .get_object(proto::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec![
                    "object_id".to_string(), 
                    "version".to_string(), 
                    "digest".to_string(),
                    "owner".to_string(),
                ],
            }),
        })
        .await
        .context("Failed to get object")?
        .into_inner();

    if let Some(object) = response.object {
        let id = object.object_id
            .context("Missing object_id")?
            .parse()
            .context("Failed to parse object_id")?;
        let version = object.version
            .context("Missing version")?;
        let digest = object.digest
            .context("Missing digest")?
            .parse()
            .context("Failed to parse digest")?;

        let obj_ref = sui::ObjectReference::new(id, version, digest);
        
        // Extract initial_shared_version from owner information
        let initial_shared_version = object.owner
            .and_then(|owner| {
                // For shared objects, the owner.version contains the initial_shared_version
                // and address should be empty/None
                if owner.address.is_none() || owner.address == Some("".to_string()) {
                    // This is likely a shared object, return the version as initial_shared_version
                    owner.version
                } else {
                    // This is an owned object
                    None
                }
            });
        Ok((obj_ref, initial_shared_version))
    } else {
        Err(anyhow!("Object not found: {}", object_id))
    }
}

/// Common helper function to execute app instance transactions
/// This reduces code duplication while maintaining all error detection and features
/// Returns the transaction digest and keeps the coin lock guard alive
async fn execute_app_instance_function<F>(
    app_instance_str: &str,
    function_name: &str,
    build_args: F,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        sui_sdk_types::Argument, // app_instance_arg
        sui_sdk_types::Argument, // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    execute_app_instance_function_with_gas(app_instance_str, function_name, None, build_args).await
}

/// Common helper function to execute app instance transactions with custom gas budget
/// This reduces code duplication while maintaining all error detection and features
/// Returns the transaction digest and keeps the coin lock guard alive
/// Includes retry logic for Object version conflicts (up to 3 retries)
async fn execute_app_instance_function_with_gas<F>(
    app_instance_str: &str,
    function_name: &str,
    custom_gas_budget: Option<u64>,
    build_args: F,
) -> Result<String>
where
    F: Fn(
        &mut sui_transaction_builder::TransactionBuilder,
        sui_sdk_types::Argument, // app_instance_arg
        sui_sdk_types::Argument, // clock_arg
    ) -> Vec<sui_sdk_types::Argument>,
{
    const MAX_RETRIES: u32 = 3;
    let mut retry_count = 0;
    
    debug!("Creating {} transaction for app_instance: {}", function_name, app_instance_str);
    
    // Get shared state and client
    let shared_state = SharedSuiState::get_instance();
    let sender = shared_state.get_sui_address();
    let sk = shared_state.get_sui_private_key().clone();
    let package_id = shared_state.get_coordination_package_id();
    let app_instance_id = get_app_instance_id(app_instance_str)
        .context("Failed to parse app instance ID")?;
    let clock_object_id = get_clock_object_id();
    
    debug!("Package ID: {}", package_id);
    debug!("App instance ID: {}", app_instance_id);
    debug!("Sender: {}", sender);
    
    // Use custom gas budget or default to 0.1 SUI
    let gas_budget = custom_gas_budget.unwrap_or(100_000_000);
    debug!("Gas budget: {} MIST ({} SUI)", gas_budget, gas_budget as f64 / 1_000_000_000.0);

    // Lock the app_instance object BEFORE fetching its version
    // This prevents race conditions where multiple threads fetch the same version
    let object_lock_manager = get_object_lock_manager();
    let app_instance_guard = object_lock_manager
        .lock_object_with_retry(app_instance_id, 50)
        .await
        .context("Failed to lock app_instance object")?;
    debug!("Locked app_instance object: {}", app_instance_id);
    
    // Variables that will be reused across retries
    let mut gas_guard: Option<crate::coin::CoinLockGuard> = None;
    
    loop {
        // Get a fresh client for each attempt
        let mut client = shared_state.get_sui_client();
        
        // Build transaction using TransactionBuilder
        let mut tb = sui_transaction_builder::TransactionBuilder::new();
        tb.set_sender(sender);
        tb.set_gas_budget(gas_budget);

        // Get gas price
        let gas_price = get_reference_gas_price(&mut client).await?;
        tb.set_gas_price(gas_price);
        debug!("Gas price: {}", gas_price);

        // Release old coin if we're retrying
        if retry_count > 0 {
            if let Some(old_guard) = gas_guard.take() {
                info!("Retry {}/{}: Releasing old gas coin {} due to version conflict", 
                    retry_count, MAX_RETRIES, old_guard.coin_id());
                drop(old_guard);
                // Small delay to allow the coin to be released
                sleep(Duration::from_millis(100)).await;
            }
        }

        // Select new gas coin for this attempt
        let (gas_coin, new_gas_guard) = match fetch_coin(&mut client, sender, gas_budget).await? {
            Some((coin, guard)) => (coin, guard),
            None => {
                error!("No available coins with sufficient balance for gas");
                return Err(anyhow!("No available coins with sufficient balance for gas"));
            }
        };
        gas_guard = Some(new_gas_guard);
        
        let gas_input = sui_transaction_builder::unresolved::Input::owned(
            gas_coin.object_id(),
            gas_coin.object_ref.version(),
            *gas_coin.object_ref.digest(),
        );
        tb.add_gas_objects(vec![gas_input]);
        debug!("Gas coin selected: id={} ver={} digest={} balance={}", 
            gas_coin.object_id(), gas_coin.object_ref.version(), gas_coin.object_ref.digest(), gas_coin.balance);

        // Fetch the current version and ownership info of app_instance object
        // Do this fresh for each retry to get the latest version
        let (app_instance_ref, initial_shared_version) = get_object_details(app_instance_id).await
            .context("Failed to get app instance details")?;
        
        // Create input based on whether object is shared or owned
        let app_instance_input = if let Some(shared_version) = initial_shared_version {
            if retry_count > 0 {
                info!("Retry {}/{}: Using updated shared object version {} for app_instance", 
                    retry_count, MAX_RETRIES, shared_version);
            }
            debug!("Using shared object input for app_instance ({}) with initial_shared_version={}", 
                function_name, shared_version);
            sui_transaction_builder::unresolved::Input::shared(
                app_instance_id,
                shared_version,
                true // mutable
            )
        } else {
            debug!("Using owned object input for app_instance ({})", function_name);
            sui_transaction_builder::unresolved::Input::owned(
                *app_instance_ref.object_id(),
                app_instance_ref.version(),
                *app_instance_ref.digest(),
            )
        };
        let app_instance_arg = tb.input(app_instance_input);

        // Clock object (shared)
        let clock_input = sui_transaction_builder::unresolved::Input::shared(clock_object_id, 1, false);
        let clock_arg = tb.input(clock_input);

        // Build function-specific arguments
        // The closure can be called multiple times since it's Fn, not FnOnce
        let args = build_args(&mut tb, app_instance_arg, clock_arg);

        // Function call
        let func = sui_transaction_builder::Function::new(
            package_id,
            "app_instance".parse()
                .map_err(|e| anyhow!("Failed to parse module name 'app_instance': {}", e))?,
            function_name.parse()
                .map_err(|e| anyhow!("Failed to parse function name '{}': {}", function_name, e))?,
            vec![],
        );
        tb.move_call(func, args);

        // Finalize and sign
        let tx = tb.finish()?;
        let sig = sk.sign_transaction(&tx)?;

        // Execute transaction via gRPC
        let mut exec = client.execution_client();
        let req = proto::ExecuteTransactionRequest {
            transaction: Some(tx.into()),
            signatures: vec![sig.into()],
            read_mask: Some(FieldMask { paths: vec!["finality".into(), "transaction".into()] }),
        };

        debug!("Sending {} transaction (attempt {}/{})...", function_name, retry_count + 1, MAX_RETRIES + 1);
        let tx_start = std::time::Instant::now();
        let exec_result = exec.execute_transaction(req).await;
        let tx_elapsed_ms = tx_start.elapsed().as_millis();

        let resp = match exec_result {
            Ok(r) => r,
            Err(e) => {
                let error_str = e.to_string();
                
                // Clean up error message - remove binary details
                let clean_error = if error_str.contains("Object ID") && error_str.contains("is not available for consumption") {
                    // Extract just the relevant object version conflict info
                    if let Some(obj_start) = error_str.find("Object ID") {
                        if let Some(version_info) = error_str.find("current version:") {
                            let end_idx = error_str[version_info..].find('.').unwrap_or(50) + version_info;
                            format!("Object version conflict - {}", &error_str[obj_start..end_idx])
                        } else {
                            "Object version conflict - transaction inputs are outdated".to_string()
                        }
                    } else {
                        "Transaction failed due to outdated object versions".to_string()
                    }
                } else if error_str.contains("details: [") {
                    // Remove binary details array
                    if let Some(idx) = error_str.find(", details: [") {
                        error_str[..idx].to_string()
                    } else {
                        error_str
                    }
                } else {
                    error_str
                };
                
                // Check if this is a version conflict that we should retry
                if (clean_error.contains("version conflict") || clean_error.contains("not available for consumption")) 
                    && retry_count < MAX_RETRIES {
                    retry_count += 1;
                    info!("Transaction {} failed with version conflict on attempt {}/{}. Retrying with fresh object version. Version conflict details: {}", 
                        function_name, retry_count, MAX_RETRIES + 1, clean_error);
                    
                    // Add exponential backoff delay before retry
                    let delay = Duration::from_millis(1000 * (2_u64.pow(retry_count - 1)));
                    sleep(delay).await;
                    
                    continue; // Retry the transaction
                }
                
                // Log as warning for expected race conditions, error for unexpected issues
                if clean_error.contains("version conflict") || clean_error.contains("not available for consumption") {
                    error!("Transaction {} failed after {} retries with version conflict: {}", 
                        function_name, retry_count + 1, clean_error);
                } else {
                    error!("Transaction {} failed: {}", function_name, clean_error);
                }
                
                return Err(anyhow!("Failed to execute {} transaction: {}", function_name, clean_error));
            }
        };
        let tx_resp = resp.into_inner();

        // Check transaction effects for errors
        check_transaction_effects(&tx_resp, function_name)?;

        let tx_digest = tx_resp
            .transaction
            .as_ref()
            .and_then(|t| t.digest.as_ref())
            .context("Failed to get transaction digest")?
            .to_string();

        if retry_count > 0 {
            info!("{} transaction succeeded after {} retries: {} (took {}ms)",
                function_name, retry_count, tx_digest, tx_elapsed_ms);
        } else {
            debug!("{} transaction executed successfully: {} (took {}ms)",
                function_name, tx_digest, tx_elapsed_ms);
        }

        // Wait for the transaction to be available in the ledger
        // Pass the gas_guard to keep the coin locked until transaction is confirmed
        if let Err(e) = wait_for_transaction(&tx_digest, None, gas_guard).await {
            warn!("Failed to wait for {} transaction to be available: {}", function_name, e);
            // Continue anyway, the transaction was successful
        }
        
        // Release the app_instance lock after transaction is confirmed
        drop(app_instance_guard);
        debug!("Released app_instance lock: {}", app_instance_id);

        return Ok(tx_digest);
    }
}

/// Try to create a new block for the app instance
/// This function calls the try_create_block Move function on the blockchain
/// which will check if conditions are met to create a new block
pub async fn try_create_block_tx(
    app_instance_str: &str,
) -> Result<String> {
    execute_app_instance_function(
        app_instance_str,
        "try_create_block",
        |_tb, app_instance_arg, clock_arg| {
            vec![app_instance_arg, clock_arg]
        },
    ).await
}

/// Set a key-value pair in the app instance KV store
pub async fn set_kv_tx(
    app_instance_str: &str,
    key: String,
    value: String,
) -> Result<String> {
    debug!("Creating set_kv transaction for key: {}", key);
    
    execute_app_instance_function(
        app_instance_str,
        "set_kv",
        move |tb, app_instance_arg, _clock_arg| {
            let key_arg = tb.input(sui_transaction_builder::Serialized(&key));
            let value_arg = tb.input(sui_transaction_builder::Serialized(&value));
            vec![app_instance_arg, key_arg, value_arg]
        },
    ).await
}

/// Delete a key-value pair from the app instance KV store
pub async fn delete_kv_tx(
    app_instance_str: &str,
    key: String,
) -> Result<String> {
    debug!("Creating delete_kv transaction for key: {}", key);
    
    execute_app_instance_function(
        app_instance_str,
        "delete_kv",
        move |tb, app_instance_arg, _clock_arg| {
            let key_arg = tb.input(sui_transaction_builder::Serialized(&key));
            vec![app_instance_arg, key_arg]
        },
    ).await
}

/// Add metadata to the app instance (write-once)
pub async fn add_metadata_tx(
    app_instance_str: &str,
    key: String,
    value: String,
) -> Result<String> {
    debug!("Creating add_metadata transaction for key: {}", key);
    
    execute_app_instance_function(
        app_instance_str,
        "add_metadata",
        move |tb, app_instance_arg, _clock_arg| {
            let key_arg = tb.input(sui_transaction_builder::Serialized(&key));
            let value_arg = tb.input(sui_transaction_builder::Serialized(&value));
            vec![app_instance_arg, key_arg, value_arg]
        },
    ).await
}

