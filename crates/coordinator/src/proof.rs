use crate::merge::analyze_and_create_merge_jobs_with_blockchain_data;
use crate::settlement;
use anyhow::Result;
use silvana_coordination_trait::Coordination;
use std::collections::{HashMap, HashSet};
use sui::fetch::{
    AppInstance, Settlement, fetch_block_settlement, fetch_blocks_range,
    fetch_proof_calculations_range,
};
use tracing::{debug, error, info, warn};

// Helper function to check if a block needs settlement on any chain
async fn block_needs_settlement(
    block_number: u64,
    settlements: &HashMap<String, Settlement>,
) -> Vec<String> {
    let mut chains_needing_settlement = Vec::new();

    for (chain, settlement) in settlements.iter() {
        // Fetch the BlockSettlement from the ObjectTable
        match fetch_block_settlement(settlement, block_number).await {
            Ok(Some(block_settlement)) => {
                // Check if it needs settlement
                if block_settlement.settlement_tx_hash.is_none() {
                    chains_needing_settlement.push(chain.clone()); // No settlement tx yet
                } else if block_settlement.settlement_tx_hash.is_some()
                    && !block_settlement.settlement_tx_included_in_block
                {
                    chains_needing_settlement.push(chain.clone()); // Tx exists but not included
                }
            }
            Ok(None) => {
                // No settlement record for this block
                if block_number > settlement.last_settled_block_number {
                    // Block is after last settled block - needs settlement
                    chains_needing_settlement.push(chain.clone());
                }
                // If block_number <= last_settled_block_number, it's already settled
            }
            Err(e) => {
                warn!(
                    "Failed to fetch block settlement for block {} on chain {}: {}",
                    block_number, chain, e
                );
                // Continue checking other chains
            }
        }
    }

    chains_needing_settlement
}

// Helper function to analyze proof completion and determine next action
pub async fn analyze_proof_completion(
    app_instance: &AppInstance,
    state: &crate::state::SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let analysis_start = std::time::Instant::now();
    debug!(
        "🔍 Starting proof completion analysis for app: {}",
        app_instance.silvana_app_name
    );

    let last_proved_block_number = app_instance.last_proved_block_number;
    // Use the AppInstance's last_settled_block_number which is already the minimum across all chains
    let last_settled_block_number = app_instance.last_settled_block_number;
    let current_block_number = app_instance.block_number;
    let previous_block_last_sequence = app_instance.previous_block_last_sequence;
    let current_sequence = app_instance.sequence;
    let app_instance_id = &app_instance.id;

    debug!(
        "📊 AppInstance status: last_proved_block={}, last_settled_block={}, current_block={}, prev_block_last_seq={}, current_seq={}",
        last_proved_block_number,
        last_settled_block_number,
        current_block_number,
        previous_block_last_sequence,
        current_sequence
    );

    // Check if we're at the start of a new block with no sequences processed yet
    if last_proved_block_number + 1 == current_block_number
        && previous_block_last_sequence + 1 == current_sequence
        && last_settled_block_number == last_proved_block_number
    {
        // Only skip analysis if we're at a fresh block AND all proved blocks are settled
        let analysis_duration = analysis_start.elapsed();
        info!(
            "✅ We're at the start of block {} with no sequences processed yet and all blocks settled - took {:.2}s",
            current_block_number,
            analysis_duration.as_secs_f64()
        );
        return Ok(());
    }

    // Track statistics
    let mut blocks_checked_for_settlement = 0;
    let mut blocks_fetched = 0;
    let mut proofs_fetched = 0;

    // Check for settlement opportunities (skip block 0 as it cannot be settled)
    let mut chains_with_settlement_opportunities = HashSet::new();

    // Only check for settlement if there are proved blocks > 0 that haven't been settled
    // Block 0 is the genesis/initial state and cannot be settled
    if last_proved_block_number > 0 && last_proved_block_number > last_settled_block_number {
        let start_block = std::cmp::max(1, last_settled_block_number + 1);
        debug!(
            "🔍 Checking for settlement opportunities from block {} to {}",
            start_block, last_proved_block_number
        );

        // Fetch all blocks and proof calculations in parallel
        let fetch_start = std::time::Instant::now();

        // Run both fetches concurrently
        let (blocks_result, proofs_result) = tokio::join!(
            fetch_blocks_range(app_instance, start_block, last_proved_block_number),
            fetch_proof_calculations_range(app_instance, start_block, last_proved_block_number)
        );

        let fetch_duration = fetch_start.elapsed();

        // Process blocks result
        let blocks_map = match blocks_result {
            Ok(blocks) => {
                blocks_fetched = blocks.len();
                debug!("Fetched {} blocks", blocks.len());
                blocks
            }
            Err(e) => {
                warn!("Failed to fetch blocks range: {}", e);
                Default::default()
            }
        };

        // Process proofs result
        let proofs_map = match proofs_result {
            Ok(proofs) => {
                proofs_fetched = proofs.len();
                debug!("Fetched {} proof calculations", proofs.len());
                proofs
            }
            Err(e) => {
                warn!("Failed to fetch proof calculations range: {}", e);
                Default::default()
            }
        };

        debug!(
            "Fetched {} blocks and {} proofs in parallel in {:.2}s",
            blocks_fetched,
            proofs_fetched,
            fetch_duration.as_secs_f64()
        );

        // Check each block for settlement opportunities
        for block_number in start_block..=last_proved_block_number {
            blocks_checked_for_settlement += 1;
            if let Some(block_details) = blocks_map.get(&block_number) {
                let proof_calc = proofs_map.get(&block_number);

                let has_block_proof = proof_calc
                    .and_then(|pc| pc.block_proof.as_ref())
                    .map(|bp| !bp.is_empty())
                    .unwrap_or(false);

                // Check settlement opportunities using the new settlement structure
                let proof_available =
                    block_details.proof_data_availability.is_some() || has_block_proof;

                if proof_available {
                    // Check which chains need settlement for this block
                    let chains_needing_settlement =
                        block_needs_settlement(block_number, &app_instance.settlements).await;

                    if !chains_needing_settlement.is_empty() {
                        for chain_name in chains_needing_settlement {
                            debug!(
                                "Settlement opportunity found for block {} on chain {}",
                                block_number, chain_name
                            );
                            chains_with_settlement_opportunities.insert(chain_name);
                        }
                        // Continue checking all blocks to find all chains with opportunities
                    }
                }
            }
        }
    } else {
        debug!(
            "📊 No valid blocks to settle yet (last_proved={}, last_settled={}). Block 0 cannot be settled.",
            last_proved_block_number, last_settled_block_number
        );
    }

    // Check for existing settlement jobs across all chains
    let existing_settlement_jobs =
        sui::fetch::app_instance::get_settlement_job_ids_for_instance(app_instance)
            .await
            .unwrap_or_default();

    if !chains_with_settlement_opportunities.is_empty() {
        // Create settlement jobs only for chains that have settlement opportunities
        debug!(
            "📝 Found settlement opportunities for {} chains",
            chains_with_settlement_opportunities.len()
        );

        for chain_name in &chains_with_settlement_opportunities {
            // Check if a job already exists for this chain
            if let Some(existing_job_id) = existing_settlement_jobs.get(chain_name.as_str()) {
                debug!(
                    "  - Chain {}: job ID {} already exists",
                    chain_name, existing_job_id
                );
            } else {
                debug!(
                    "  - Creating settle job for chain {} (blocks ready to settle)",
                    chain_name
                );
                if let Err(e) =
                    settlement::create_periodic_settle_job(app_instance, chain_name, state).await
                {
                    warn!(
                        "Failed to create settle job for chain {}: {}",
                        chain_name, e
                    );
                } else {
                    debug!(
                        "✅ Successfully created periodic settle job for chain {}",
                        chain_name
                    );
                }
            }
        }
    } else {
        // No settlement opportunities for any chain - terminate all existing settlement jobs
        if !existing_settlement_jobs.is_empty() {
            debug!(
                "🚫 No valid blocks to settle on any chain, terminating {} settlement jobs",
                existing_settlement_jobs.len()
            );
            for (chain, job_id) in &existing_settlement_jobs {
                debug!("  - Terminating job {} for chain {}", job_id, chain);
                let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
                if let Err(e) = sui_interface
                    .terminate_app_job(&app_instance.id, *job_id)
                    .await
                {
                    warn!(
                        "Failed to terminate settlement job {} for chain {}: {}",
                        job_id, chain, e
                    );
                } else {
                    debug!(
                        "✅ Successfully terminated settlement job {} for chain {}",
                        job_id, chain
                    );
                }
            }
        }
    }

    // Also terminate jobs for chains that no longer have settlement opportunities
    for (chain, job_id) in &existing_settlement_jobs {
        if !chains_with_settlement_opportunities.contains(chain.as_str()) {
            debug!(
                "🚫 No more blocks to settle on chain {}, terminating job {}",
                chain, job_id
            );
            let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
            if let Err(e) = sui_interface
                .terminate_app_job(&app_instance.id, *job_id)
                .await
            {
                warn!(
                    "Failed to terminate settlement job {} for chain {}: {}",
                    job_id, chain, e
                );
            } else {
                debug!(
                    "✅ Successfully terminated settlement job {} for chain {}",
                    job_id, chain
                );
            }
        }
    }

    // Step 2: Process blocks in order from last_proved_block_number + 1 to current_block_number for merge opportunities
    // But limit to MAX_BLOCK_LOOKAHEAD blocks ahead to prevent creating too many future jobs
    let max_block_to_analyze = std::cmp::min(
        current_block_number,
        last_proved_block_number + crate::constants::MAX_BLOCK_LOOKAHEAD,
    );

    let blocks_to_analyze = max_block_to_analyze.saturating_sub(last_proved_block_number);
    debug!(
        "🔄 Processing {} blocks from {} to {} for merge opportunities (limited by lookahead of {})",
        blocks_to_analyze,
        last_proved_block_number + 1,
        max_block_to_analyze,
        crate::constants::MAX_BLOCK_LOOKAHEAD
    );

    let mut analyzed_blocks = 0;
    for block_number in (last_proved_block_number + 1)..=max_block_to_analyze {
        analyzed_blocks += 1;
        let block_start = std::time::Instant::now();
        debug!(
            "📦 Analyzing block {} for merge opportunities",
            block_number
        );

        // Fetch the full ProofCalculation with proofs for this block
        let proof_calc = match sui::fetch::fetch_proof_calculation(app_instance, block_number).await
        {
            Ok(Some(proof_calc)) => {
                debug!(
                    "📊 Fetched ProofCalculation with {} proofs for block {}",
                    proof_calc.proofs.len(),
                    block_number
                );
                proof_calc
            }
            Ok(None) => {
                debug!(
                    "⏭️ No ProofCalculation found for block {}, skipping",
                    block_number
                );
                continue;
            }
            Err(e) => {
                warn!(
                    "Failed to fetch ProofCalculation for block {}: {}, skipping",
                    block_number, e
                );
                continue;
            }
        };

        // Analyze this block for merge opportunities
        // Use empty da_hash since we're just looking for merge opportunities
        if let Err(e) =
            analyze_and_create_merge_jobs_with_blockchain_data(&proof_calc, app_instance_id, state)
                .await
        {
            let block_duration = block_start.elapsed();
            warn!(
                "Failed to analyze block {} for merges in {:.2}s: {}",
                block_number,
                block_duration.as_secs_f64(),
                e
            );
            // Continue to next block even if this one fails
        } else {
            let block_duration = block_start.elapsed();
            debug!(
                "✅ Successfully analyzed block {} for merge opportunities in {:.2}s",
                block_number,
                block_duration.as_secs_f64()
            );
        }
    }

    // Check for purge opportunity
    // The Move contract purges when last_purged_sequence < last_settled_sequence
    if app_instance.last_purged_sequence < app_instance.last_settled_sequence {
        let sequences_to_purge = crate::constants::DEFAULT_PURGE_SEQUENCES;
        let initial_purgeable_sequences =
            app_instance.last_settled_sequence - app_instance.last_purged_sequence;

        debug!(
            "🗑️ Initial purge opportunity detected: {} sequences can be purged (last_purged: {}, last_settled: {})",
            initial_purgeable_sequences,
            app_instance.last_purged_sequence,
            app_instance.last_settled_sequence
        );

        // Get the coordination manager from state
        let manager = match state.get_coordination_manager() {
            Some(mgr) => mgr,
            None => {
                warn!("No coordination manager available for purge check");
                return Ok(());
            }
        };

        // Get the correct coordination layer for this app instance
        let (_layer_id, coordination) = match manager.get_layer_for_app(&app_instance.id).await {
            Ok(layer) => layer,
            Err(e) => {
                warn!("Failed to find coordination layer for purge check: {}", e);
                return Ok(());
            }
        };

        // Refetch the app instance to ensure the condition is still valid
        // For now, still use Sui-specific types for purge operation
        let fresh_app_instance = match coordination.fetch_app_instance(&app_instance.id).await {
            Ok(_trait_app_instance) => {
                // Still need Sui-specific type for purge operations
                match sui::fetch::fetch_app_instance(&app_instance.id).await {
                    Ok(sui_app_inst) => sui_app_inst,
                    Err(_) => {
                        debug!("Skipping purge for non-Sui app instance {}", app_instance.id);
                        return Ok(());
                    }
                }
            },
            Err(e) => {
                warn!("Failed to refetch app instance for purge: {}", e);
                return Ok(());
            }
        };

        {
            let fresh_app_instance = fresh_app_instance;
            {
                // Re-check the condition with fresh data
                if fresh_app_instance.last_purged_sequence
                    < fresh_app_instance.last_settled_sequence
                {
                    let fresh_purgeable_sequences = fresh_app_instance.last_settled_sequence
                        - fresh_app_instance.last_purged_sequence;

                    debug!(
                        "🗑️ Confirmed purge opportunity after refetch: {} sequences can be purged (last_purged: {}, last_settled: {})",
                        fresh_purgeable_sequences,
                        fresh_app_instance.last_purged_sequence,
                        fresh_app_instance.last_settled_sequence
                    );

                    // Create a Sui interface and call purge
                    let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

                    // Progressive retry mechanism: divide by 2 on each retry
                    // With DEFAULT_PURGE_SEQUENCES=32: 32 -> 16 -> 8 -> 4 -> 2 -> 1
                    let mut current_sequences = sequences_to_purge;
                    let mut attempt_count = 0;
                    let mut purge_succeeded = false;

                    loop {
                        attempt_count += 1;

                        let max_computation_cost = if current_sequences > 1 {
                            Some(crate::constants::MAX_COMPUTATION_COST_MIST)
                        } else {
                            None
                        };
                        match sui_interface
                            .purge(&app_instance.id, current_sequences, None, max_computation_cost)
                            .await
                        {
                            Ok(tx_digest) => {
                                info!(
                                    "✅ Successfully initiated purge of up to {} sequences (tx: {}, attempt: {})",
                                    current_sequences, tx_digest, attempt_count
                                );
                                purge_succeeded = true;
                                break;
                            }
                            Err((error_msg, tx_digest)) => {
                                // Check if it's a gas budget issue or other failure
                                let is_gas_issue = error_msg.contains("Insufficient gas budget")
                                    || error_msg.contains("InsufficientGas")
                                    || error_msg.contains("Computation cost")
                                    || error_msg.contains("gas");

                                let tx_info =
                                    tx_digest.map_or(String::new(), |d| format!(" (tx: {})", d));

                                // Calculate next retry sequences by dividing by 2
                                let next_sequences = if current_sequences > 1 {
                                    // Divide by 2, but ensure at least 1
                                    (current_sequences / 2).max(1)
                                } else {
                                    // Already tried with 1, no more retries
                                    error!(
                                        "❌ Failed to purge even with sequences=1: {}{}",
                                        error_msg, tx_info
                                    );
                                    break;
                                };

                                // Don't retry if we're already at 1 and it failed
                                if current_sequences == 1 {
                                    error!(
                                        "❌ Failed to purge even with sequences=1: {}{}",
                                        error_msg, tx_info
                                    );
                                    break;
                                }

                                if is_gas_issue {
                                    warn!(
                                        "⚠️ Failed to purge {} sequences due to gas constraints: {}{}. Retrying with {} sequences...",
                                        current_sequences, error_msg, tx_info, next_sequences
                                    );
                                } else {
                                    warn!(
                                        "⚠️ Failed to purge {} sequences: {}{}. Retrying with {} sequences...",
                                        current_sequences, error_msg, tx_info, next_sequences
                                    );
                                }

                                // Update for next attempt
                                current_sequences = next_sequences;
                            }
                        }
                    }

                    if !purge_succeeded && attempt_count > 1 {
                        error!(
                            "❌ Failed to purge sequences after {} attempts with progressively smaller batches",
                            attempt_count
                        );
                    }
                } else {
                    debug!(
                        "🔍 Purge opportunity no longer valid after refetch (last_purged: {}, last_settled: {})",
                        fresh_app_instance.last_purged_sequence,
                        fresh_app_instance.last_settled_sequence
                    );
                }
            }
        }
    } else {
        debug!(
            "🔍 No purge needed (last_purged: {}, last_settled: {})",
            app_instance.last_purged_sequence, app_instance.last_settled_sequence
        );
    }

    let analysis_duration = analysis_start.elapsed();

    // Prepare comprehensive stats
    let settlement_status = if !chains_with_settlement_opportunities.is_empty() {
        if !existing_settlement_jobs.is_empty() {
            format!(
                "settle_jobs_exist(count={})",
                existing_settlement_jobs.len()
            )
        } else {
            "settle_job_created".to_string()
        }
    } else if !existing_settlement_jobs.is_empty() {
        "settle_jobs_terminated".to_string()
    } else {
        "no_settlement_needed".to_string()
    };

    // Always log comprehensive stats at info level
    info!(
        "📊 Proof analysis complete | app: {} | current_block: {} | last_proved: {} | last_settled: {} | blocks_checked_settlement: {} | blocks_fetched: {} | proofs_fetched: {} | blocks_analyzed_merge: {} | settlement: {} | duration: {:.3}s",
        app_instance.silvana_app_name,
        current_block_number,
        last_proved_block_number,
        last_settled_block_number,
        blocks_checked_for_settlement,
        blocks_fetched,
        proofs_fetched,
        analyzed_blocks,
        settlement_status,
        analysis_duration.as_secs_f64()
    );

    Ok(())
}
