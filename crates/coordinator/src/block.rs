use crate::constants::BLOCK_CREATION_MIN_INTERVAL_MS;
use sui::interface::SilvanaSuiInterface;
use anyhow::Result;
use tracing::{info, debug, error};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn settle(
    app_instance: &str,
    block_number: u64,
    proof_da_hash: String,
) -> Result<()> {
    info!(
        "Settling complete block {} (da_hash: {})",
        block_number,
        proof_da_hash
    );

    // Create a SilvanaSuiInterface to interact with the blockchain
    let mut sui_interface = SilvanaSuiInterface::new();

    // 1. First, update the block proof data availability on the blockchain
    match sui_interface.update_block_proof_data_availability(
        app_instance,
        block_number,
        proof_da_hash.clone(),
    ).await {
        Ok(tx_digest) => {
            info!(
                "✅ Successfully updated block proof DA for block {} - Transaction: {}",
                block_number, tx_digest
            );
        }
        Err(e) => {
            error!(
                "❌ Failed to update block proof DA for block {}: {}",
                block_number, e
            );
            return Err(anyhow::anyhow!("Failed to update block proof DA: {}", e));
        }
    }

    // 2. Fetch the app instance to get all configured chains
    let app_inst = match sui::fetch::fetch_app_instance(app_instance).await {
        Ok(inst) => inst,
        Err(e) => {
            error!("❌ Failed to fetch app instance {}: {}", app_instance, e);
            return Err(anyhow::anyhow!("Failed to fetch app instance: {}", e));
        }
    };
    
    // 3. Create settle jobs for chains that don't already have one
    let mut chains_needing_jobs = Vec::new();
    let mut chains_with_existing_jobs = Vec::new();
    
    for (chain, settlement) in &app_inst.settlements {
        if settlement.settlement_job.is_some() {
            chains_with_existing_jobs.push(chain.clone());
            debug!(
                "Chain {} already has settlement job ID {}",
                chain, settlement.settlement_job.unwrap()
            );
        } else {
            chains_needing_jobs.push(chain.clone());
        }
    }
    
    if !chains_with_existing_jobs.is_empty() {
        info!(
            "Skipping {} chains with existing settlement jobs: {:?}",
            chains_with_existing_jobs.len(),
            chains_with_existing_jobs
        );
    }
    
    if chains_needing_jobs.is_empty() {
        info!(
            "All {} configured chains already have settlement jobs",
            app_inst.settlements.len()
        );
        return Ok(());
    }
    
    info!(
        "Creating settlement jobs for {} chains: {:?}",
        chains_needing_jobs.len(),
        chains_needing_jobs
    );
    
    let mut success_count = 0;
    let mut failed_chains = Vec::new();
    
    for chain in chains_needing_jobs {
        let job_description = Some(format!(
            "Settle block {} on chain {} with proof DA hash",
            block_number, chain
        ));
        
        match sui_interface.create_settle_job(
            app_instance,
            block_number,
            chain.clone(),
            job_description,
        ).await {
            Ok(tx_digest) => {
                info!(
                    "✅ Successfully created settle job for block {} on chain {} - Transaction: {}",
                    block_number, chain, tx_digest
                );
                success_count += 1;
            }
            Err(e) => {
                error!(
                    "❌ Failed to create settle job for block {} on chain {}: {}",
                    block_number, chain, e
                );
                failed_chains.push(chain);
            }
        }
    }
    
    if !failed_chains.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to create settle jobs for chains: {:?}. Successfully created {} jobs.",
            failed_chains, success_count
        ));
    }

    info!("Block {} settlement complete - proof recorded with DA hash: {}", block_number, proof_da_hash);
    
    Ok(())
}


/// Try to create a new block for the app instance
/// This function checks if the conditions are met to create a new block:
/// 1. No new sequences pending (sequence != previous_block_last_sequence + 1)
/// 2. Sufficient time has passed since the last block (current_time - previous_block_timestamp > MIN_TIME_BETWEEN_BLOCKS)
pub async fn try_create_block(
    sui_interface: &mut SilvanaSuiInterface,
    app_instance_id: &str,
) -> Result<Option<(String, u64, u64)>> { // Returns Some((tx_digest, new_sequences_count, time_since_last_block)) on success
    debug!("Checking if we should create a new block for app_instance {}", app_instance_id);
    
    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id).await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;
    
    // Get current time in milliseconds
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    // Check conditions:
    // 1. There are new sequences not in blocks (sequence != previous_block_last_sequence + 1)
    // 2. Sufficient time has passed since last block
    let has_new_sequences = app_instance.sequence != app_instance.previous_block_last_sequence + 1;
    let time_since_last_block = current_time.saturating_sub(app_instance.previous_block_timestamp);
    let sufficient_time_passed = time_since_last_block > BLOCK_CREATION_MIN_INTERVAL_MS;
    
    debug!(
        "Block creation check for {}: sequence={}, prev_block_last_seq={}, prev_block_timestamp={}, current_time={}, time_since_last={}, has_new_sequences={}, sufficient_time={}",
        app_instance_id, app_instance.sequence, app_instance.previous_block_last_sequence, app_instance.previous_block_timestamp, 
        current_time, time_since_last_block, has_new_sequences, sufficient_time_passed
    );
    
    // Both conditions must be met to create a block
    if has_new_sequences && sufficient_time_passed {
        debug!(
            "Conditions met for creating block in app_instance {}: {} new sequences, {}ms since last block",
            app_instance_id, 
            app_instance.sequence - app_instance.previous_block_last_sequence - 1,
            time_since_last_block
        );
        
        // Call the blockchain to try creating the block
        match sui_interface.try_create_block(app_instance_id).await {
            Ok(tx_digest) => {
                debug!("Successfully created block for app_instance {}, tx: {}", app_instance_id, tx_digest);
                let new_sequences = app_instance.sequence - app_instance.previous_block_last_sequence - 1;
                Ok(Some((tx_digest, new_sequences, time_since_last_block)))
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("NonEntryFunctionInvoked") || error_str.contains("not an entry function") {
                    // Critical error - function not found
                    error!("CRITICAL: try_create_block function not found in Move contract: {}", error_str);
                    Err(anyhow::anyhow!("try_create_block function not found in Move contract: {}", error_str))
                } else {
                    // Expected failures (conditions not met, another coordinator created it, etc.)
                    debug!("Block not created for app_instance {} (expected): {}", app_instance_id, error_str);
                    Ok(None)
                }
            }
        }
    } else {
        debug!(
            "Conditions not met for creating block in app_instance {}: has_new_sequences={}, sufficient_time_passed={}",
            app_instance_id, has_new_sequences, sufficient_time_passed
        );
        Ok(None)
    }
}