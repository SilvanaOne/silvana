use crate::block::settle;
use anyhow::Result;
use sui::fetch::fetch_proof_calculation;
use sui::fetch::{Proof, ProofCalculation, ProofStatus};
use tracing::{debug, error, info, warn};

pub async fn analyze_and_create_merge_jobs_with_blockchain_data(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    da_hash: &str,
) -> Result<()> {
    debug!(
        "🔍 Analyzing proof calculation for block {} with sequences: {:?}",
        proof_calc.block_number,
        proof_calc
            .proofs
            .last()
            .map(|p| &p.sequences)
            .unwrap_or(&vec![])
    );

    // First fetch the AppInstance to get the AppInstance object
    let app_instance_obj = match sui::fetch::fetch_app_instance(app_instance).await {
        Ok(app_inst) => app_inst,
        Err(e) => {
            error!(
                "❌ Failed to fetch AppInstance {}: {}",
                app_instance, e
            );
            return Err(anyhow::anyhow!("Failed to fetch AppInstance: {}", e));
        }
    };
    
    // Fetch existing ProofCalculation for this block to get full info including start_sequence, end_sequence, is_finished
    let existing_proof_calculation = match fetch_proof_calculation(
        &app_instance_obj,
        proof_calc.block_number,
    )
    .await
    {
        Ok(Some(pc_info)) => {
            debug!(
                "🧮 Fetched ProofCalculation for block {}: {} proofs, finished={}",
                proof_calc.block_number,
                pc_info.proofs.len(),
                pc_info.is_finished
            );
            Some(pc_info)
        }
        Ok(None) => {
            debug!(
                "📋 No existing ProofCalculation found for block {}",
                proof_calc.block_number
            );
            None
        }
        Err(e) => {
            error!(
                "❌ Failed to fetch ProofCalculation for block {}: {}",
                proof_calc.block_number, e
            );
            None
        }
    };

    // Get the ProofCalculation info for this block if it exists
    // Otherwise use default values
    let start_sequence = existing_proof_calculation
        .as_ref()
        .map(|p| p.start_sequence)
        .unwrap_or_else(|| {
            // If no ProofCalculation exists yet, derive start_sequence from the proofs
            proof_calc
                .proofs
                .iter()
                .flat_map(|p| p.sequences.iter())
                .min()
                .copied()
                .unwrap_or(proof_calc.start_sequence)
        });
    let end_sequence = existing_proof_calculation.as_ref().and_then(|p| p.end_sequence);
    let is_finished = existing_proof_calculation.as_ref().map(|p| p.is_finished).unwrap_or(false);

    debug!(
        "📦 Using ProofCalculation info: block_number={}, start_sequence={}, end_sequence={:?}, is_finished={}",
        proof_calc.block_number, start_sequence, end_sequence, is_finished
    );

    // Convert to BlockProofs structure
    let mut proof_infos = Vec::new();

    // Only add existing proofs from blockchain (which already includes the current proof)
    if let Some(existing_proof_calc) = &existing_proof_calculation {
        for existing_proof in &existing_proof_calc.proofs {
            proof_infos.push(existing_proof.clone());
        }
    }

    let block_proofs = ProofCalculation {
        id: existing_proof_calculation
            .as_ref()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "temp_id".to_string()),
        block_number: proof_calc.block_number,
        start_sequence: proof_calc.start_sequence, // No Some() needed
        end_sequence: proof_calc.end_sequence,
        proofs: proof_infos,
        block_proof: proof_calc.block_proof.clone(),
        is_finished: proof_calc.is_finished,
    };

    debug!(
        "🎯 Block {} analysis: sequences {}-{}, {} proofs, finished={}",
        proof_calc.block_number,
        block_proofs.start_sequence,
        block_proofs
            .end_sequence
            .map(|e| e.to_string())
            .unwrap_or_else(|| "pending".to_string()),
        block_proofs.proofs.len(),
        proof_calc.is_finished
    );
    
    // Debug: Print detailed proof list with sequences and statuses
    if !block_proofs.proofs.is_empty() {
        debug!("📝 Detailed proof list for block {}:", proof_calc.block_number);
        for (idx, proof) in block_proofs.proofs.iter().enumerate() {
            let sequences_str = if proof.sequences.len() <= 10 {
                format!("{:?}", proof.sequences)
            } else {
                format!("[{} sequences: {}..{}]", 
                    proof.sequences.len(),
                    proof.sequences.first().unwrap_or(&0),
                    proof.sequences.last().unwrap_or(&0))
            };
            
            debug!(
                "   Proof #{}: sequences={}, status={:?}, da_hash={}, timestamp={}",
                idx + 1,
                sequences_str,
                proof.status,
                proof.da_hash.as_ref().unwrap_or(&"none".to_string()),
                proof.timestamp
            );
        }
    } else {
        debug!("📝 No proofs found for block {}", proof_calc.block_number);
    }

    // Check if the current proof covers the entire block
    if let Some(end_seq) = block_proofs.end_sequence {
        // Generate the complete sequence range for the block
        let complete_block_sequences: Vec<u64> = (block_proofs.start_sequence..=end_seq).collect();

        // Check if current proof contains all sequences for the block
        let current_sequences = proof_calc
            .proofs
            .last()
            .map(|p| p.sequences.clone())
            .unwrap_or_default();
        if current_sequences == complete_block_sequences {
            debug!(
                "🎉 Current proof covers entire block {} (sequences {}-{}) - ready to settle",
                proof_calc.block_number, block_proofs.start_sequence, end_seq
            );

            // Call settle for the complete block with the DA hash
            settle(
                app_instance,
                proof_calc.block_number,
                da_hash.to_string(),
            )
            .await?;

            return Ok(());
        }
    } else {
        debug!(
            "⏳ Block {} end_sequence not yet determined for app_instance {} (still receiving sequences)",
            proof_calc.block_number, app_instance
        );
    }

    // Try to find and create merge jobs, with up to 10 attempts
    const MAX_MERGE_ATTEMPTS: usize = 10;
    let mut attempted_merges = Vec::new();
    let mut merge_created = false;

    for attempt in 1..=MAX_MERGE_ATTEMPTS {
        // Refetch ProofCalculations on each attempt to get the latest state
        // This is important because other coordinators might have updated them
        let current_block_proofs =
            if attempt > 1 {
                debug!(
                    "🔄 Refetching ProofCalculations to get latest state (attempt {})",
                    attempt
                );

                // Fetch existing ProofCalculation for this block again
                let updated_proof_calculation =
                    match fetch_proof_calculation(&app_instance_obj, proof_calc.block_number)
                        .await
                    {
                        Ok(Some(pc)) => {
                            debug!(
                                "📊 Refetched ProofCalculation for block {}",
                                proof_calc.block_number
                            );
                            Some(pc)
                        }
                        Ok(None) => {
                            debug!(
                                "📋 Still no ProofCalculation found for block {}",
                                proof_calc.block_number
                            );
                            None
                        }
                        Err(e) => {
                            error!(
                                "❌ Failed to refetch ProofCalculation for block {}: {}",
                                proof_calc.block_number, e
                            );
                            None
                        }
                    };

                // Rebuild proof_infos with updated data (only from blockchain)
                let mut updated_proof_infos = Vec::new();

                // Only add existing proofs from refetched data with their actual status
                if let Some(existing_proof_calc) = &updated_proof_calculation {
                    for existing_proof in &existing_proof_calc.proofs {
                        updated_proof_infos.push(existing_proof.clone());
                    }
                }

                // Use the most recent ProofCalculation's start_sequence and end_sequence
                // If no updated calculations exist, use the current one's values
                let (latest_start_seq, latest_end_seq, latest_is_finished, latest_block_proof) =
                    if let Some(ref latest) = updated_proof_calculation {
                        (
                            latest.start_sequence,
                            latest.end_sequence,
                            latest.is_finished,
                            latest.block_proof.clone(),
                        )
                    } else {
                        (
                            start_sequence,
                            end_sequence,
                            is_finished,
                            proof_calc.block_proof.clone(),
                        )
                    };

                let updated_block_proofs = ProofCalculation {
                    id: updated_proof_calculation
                        .as_ref()
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| String::new()),
                    block_number: proof_calc.block_number,
                    start_sequence: latest_start_seq,
                    end_sequence: latest_end_seq,
                    proofs: updated_proof_infos,
                    block_proof: latest_block_proof,
                    is_finished: latest_is_finished,
                };
                
                // Debug: Print updated proof list after refetch
                if !updated_block_proofs.proofs.is_empty() {
                    debug!("📝 Updated proof list after refetch (attempt {}):", attempt);
                    for (idx, proof) in updated_block_proofs.proofs.iter().enumerate() {
                        let sequences_str = if proof.sequences.len() <= 10 {
                            format!("{:?}", proof.sequences)
                        } else {
                            format!("[{} sequences: {}..{}]", 
                                proof.sequences.len(),
                                proof.sequences.first().unwrap_or(&0),
                                proof.sequences.last().unwrap_or(&0))
                        };
                        
                        debug!(
                            "   Proof #{}: sequences={}, status={:?}",
                            idx + 1,
                            sequences_str,
                            proof.status
                        );
                    }
                }
                
                updated_block_proofs
            } else {
                block_proofs.clone()
            };

        // Find proofs that can be merged, excluding already attempted ones
        if let Some(merge_request) =
            find_proofs_to_merge_excluding(&current_block_proofs, &attempted_merges)
        {
            // Find the actual proof objects for more details
            let proof1_details = current_block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| format!("status={:?}", p.status))
                .unwrap_or_else(|| "not found".to_string());
                
            let proof2_details = current_block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences2)
                .map(|p| format!("status={:?}", p.status))
                .unwrap_or_else(|| "not found".to_string());
            
            debug!(
                "✨ Attempt {}/{}: Found merge opportunity - Sequences1: {:?} ({}), Sequences2: {:?} ({}), Is block proof: {}",
                attempt, MAX_MERGE_ATTEMPTS, 
                merge_request.sequences1, proof1_details,
                merge_request.sequences2, proof2_details,
                merge_request.block_proof
            );

            // Remember this attempt
            attempted_merges.push((
                merge_request.sequences1.clone(),
                merge_request.sequences2.clone(),
            ));

            // Find the proof statuses for the sequences to merge
            let proof1_status = current_block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);

            let proof2_status = current_block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences2)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);

            // Try to create merge job with proof reservation
            match create_merge_job(
                merge_request.sequences1.clone(),
                merge_request.sequences2.clone(),
                proof_calc.block_number,
                app_instance,
                proof1_status,
                proof2_status,
                &current_block_proofs,
            )
            .await
            {
                Ok(_) => {
                    debug!(
                        "✅ Successfully created and reserved merge job for block {} on attempt {}",
                        proof_calc.block_number, attempt
                    );
                    merge_created = true;
                    break; // Successfully created a merge job, stop trying
                }
                Err(e) => {
                    debug!(
                        "ℹ️ Attempt {}/{}: Could not create merge job for block {} (sequences {:?} + {:?}): {}",
                        attempt,
                        MAX_MERGE_ATTEMPTS,
                        proof_calc.block_number,
                        merge_request.sequences1,
                        merge_request.sequences2,
                        e
                    );

                    // Check if we should continue trying
                    let error_str = e.to_string();
                    if error_str.contains("already reserved") {
                        debug!("   → Looking for other merge opportunities...");
                        // Add a small delay before retrying to allow other coordinators to complete
                        if attempt < MAX_MERGE_ATTEMPTS {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }
                        // Continue to next iteration to try another merge opportunity
                    } else if error_str.contains("already Started and not timed out") 
                        || error_str.contains("Cannot merge")
                        || error_str.contains("Cannot reserve proofs - likely taken by another coordinator") {
                        // These are expected conditions in a concurrent environment, not errors
                        debug!("   → Expected condition, will try other opportunities: {}", e);
                    } else {
                        warn!("   → Unexpected error, will try other opportunities: {}", e);
                    }
                }
            }
        } else {
            debug!(
                "🚫 No more merge opportunities found for block {} after {} attempt(s)",
                proof_calc.block_number, attempt
            );
            break; // No more merge opportunities available
        }
    }

    if !merge_created && !attempted_merges.is_empty() {
        debug!(
            "ℹ️ No merge job created for block {} after {} attempts (proofs may be in progress)",
            proof_calc.block_number,
            attempted_merges.len()
        );
    } else if !merge_created {
        debug!(
            "🚫 No merge opportunities found for block {} in app_instance {}",
            proof_calc.block_number, app_instance
        );
    }

    Ok(())
}

// Main entry point - always use blockchain data
#[allow(dead_code)]
pub async fn analyze_and_create_merge_jobs(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    da_hash: &str,
) -> Result<()> {
    analyze_and_create_merge_jobs_with_blockchain_data(proof_calc, app_instance, da_hash)
        .await
}

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
    pub block_proof: bool,
}

const TIMEOUT_MS: u64 = 2 * 60 * 1000; // 2 minutes

fn find_proofs_to_merge_excluding(
    block_proofs: &ProofCalculation,
    excluded: &[(Vec<u64>, Vec<u64>)],
) -> Option<MergeRequest> {
    if block_proofs.is_finished {
        return None;
    }

    // First priority: Try to create block proofs (complete range from start to end)
    if let Some(end_seq) = block_proofs.end_sequence {
        let start_seq = block_proofs.start_sequence;
        for i in (start_seq + 1)..=end_seq {
            let sequence1: Vec<u64> = (start_seq..i).collect();
            let sequence2: Vec<u64> = (i..=end_seq).collect();

            // Check if this pair is in the excluded list
            if excluded
                .iter()
                .any(|(s1, s2)| s1 == &sequence1 && s2 == &sequence2)
            {
                continue; // Skip this pair as it was already attempted
            }

            let proof1 = block_proofs
                .proofs
                .iter()
                .find(|p| arrays_equal(&p.sequences, &sequence1));
            let proof2 = block_proofs
                .proofs
                .iter()
                .find(|p| arrays_equal(&p.sequences, &sequence2));

            if let (Some(proof1), Some(proof2)) = (proof1, proof2) {
                // For block proofs, only check CALCULATED or USED status (not RESERVED)
                // This matches the TypeScript logic exactly
                if (proof1.status == ProofStatus::Calculated || proof1.status == ProofStatus::Used)
                    && (proof2.status == ProofStatus::Calculated
                        || proof2.status == ProofStatus::Used)
                {
                    debug!(
                        "Merging proofs to create block proof: sequences1={:?}, sequences2={:?}",
                        sequence1, sequence2
                    );
                    return Some(MergeRequest {
                        sequences1: sequence1,
                        sequences2: sequence2,
                        block_proof: true,
                    });
                }
            }
        }
    }

    // Second priority: Find adjacent proofs that can be merged
    let current_time = chrono::Utc::now().timestamp() as u64 * 1000; // Convert to milliseconds

    for (i, proof1) in block_proofs.proofs.iter().enumerate() {
        // Check if proof1 is available for merging
        if !is_proof_available(proof1, current_time) {
            continue;
        }

        for (j, proof2) in block_proofs.proofs.iter().enumerate() {
            if i == j {
                continue;
            }

            // Check if proof2 is available for merging
            if !is_proof_available(proof2, current_time) {
                continue;
            }

            // Check if this pair is in the excluded list
            if excluded
                .iter()
                .any(|(s1, s2)| s1 == &proof1.sequences && s2 == &proof2.sequences)
            {
                continue; // Skip this pair as it was already attempted
            }

            // Condition 1: Check if proof1.sequences[last] + 1 == proof2.sequences[first]
            if let (Some(proof1_last), Some(proof2_first)) =
                (proof1.sequences.last(), proof2.sequences.first())
            {
                if *proof1_last + 1 == *proof2_first {
                    // Construct combined sequences
                    let combined: Vec<u64> =
                        [&proof1.sequences[..], &proof2.sequences[..]].concat();

                    // Condition 2: Ensure no proof already exists with the same combined sequence
                    let already_exists = block_proofs.proofs.iter().any(|p| {
                        arrays_equal(&p.sequences, &combined)
                            && is_proof_active_or_completed(p, current_time)
                    });

                    if !already_exists {
                        return Some(MergeRequest {
                            sequences1: proof1.sequences.clone(),
                            sequences2: proof2.sequences.clone(),
                            block_proof: false,
                        });
                    }
                }
            }
        }
    }

    None
}

#[allow(dead_code)]
pub fn find_proofs_to_merge(block_proofs: &ProofCalculation) -> Option<MergeRequest> {
    find_proofs_to_merge_excluding(block_proofs, &[])
}

fn arrays_equal(a: &[u64], b: &[u64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

fn is_proof_available(proof: &Proof, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated => true,
        ProofStatus::Reserved => {
            // Check if timeout has expired (RESERVED proofs can be used if timed out)
            current_time > proof.timestamp + TIMEOUT_MS
        }
        ProofStatus::Started => false, // Started proofs are still being calculated, not available for merging
        ProofStatus::Used => false,    // USED proofs are NOT available for merging
        ProofStatus::Rejected => false, // REJECTED proofs are not available for merging
    }
}

fn is_proof_active_or_completed(proof: &Proof, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated | ProofStatus::Used | ProofStatus::Reserved => true,
        ProofStatus::Started => {
            // Check if it's still within timeout
            current_time <= proof.timestamp + TIMEOUT_MS
        }
        _ => false,
    }
}

async fn create_merge_job(
    sequences1: Vec<u64>,
    sequences2: Vec<u64>,
    block_number: u64,
    app_instance: &str,
    _proof1_status: &ProofStatus, // Currently unused, but kept for future use
    _proof2_status: &ProofStatus, // Currently unused, but kept for future use
    block_proofs: &ProofCalculation, // Add this to check if combined proof exists
) -> Result<()> {
    debug!(
        "Attempting to create merge job for block {} with sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

    // Create SilvanaSuiInterface following the same pattern as submit_proof in grpc.rs
    let mut sui_interface = sui::interface::SilvanaSuiInterface::new();

    // Generate a unique job ID for this merge operation
    let job_id = format!("merge_{}_{}_{}", block_number, sequences1[0], sequences2[0]);

    // Combine sequences for reservation
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();

    // Step 1: Check if we need to reject an existing combined proof
    // Look for an existing proof with the combined sequences
    let combined_proof = block_proofs
        .proofs
        .iter()
        .find(|p| p.sequences == combined_sequences);

    if let Some(proof) = combined_proof {
        // Check if the proof needs to be rejected based on its status
        let should_reject = match proof.status {
            ProofStatus::Started => {
                // Only reject Started proofs if they've timed out (10+ minutes)
                let current_time = chrono::Utc::now().timestamp() as u64 * 1000; // Convert to milliseconds
                let time_since_start = current_time.saturating_sub(proof.timestamp);
                const STARTED_TIMEOUT_MS: u64 = 10 * 60 * 1000; // 10 minutes for Started status

                if time_since_start > STARTED_TIMEOUT_MS {
                    debug!(
                        "⏰ Started proof has timed out ({} ms > {} ms), will reject",
                        time_since_start, STARTED_TIMEOUT_MS
                    );
                    true
                } else {
                    debug!(
                        "⏳ Started proof is still active ({} ms < {} ms), skipping merge",
                        time_since_start, STARTED_TIMEOUT_MS
                    );
                    // Return early - don't try to merge with an active Started proof
                    return Err(anyhow::anyhow!(
                        "Cannot merge: combined proof with sequences {:?} is already Started and not timed out",
                        combined_sequences
                    ));
                }
            }
            ProofStatus::Reserved => {
                // Reserved proofs can be rejected if timed out (2 minutes)
                let current_time = chrono::Utc::now().timestamp() as u64 * 1000;
                current_time > proof.timestamp + TIMEOUT_MS
            }
            ProofStatus::Rejected => true, // Already rejected, can try again
            ProofStatus::Calculated | ProofStatus::Used => false, // Don't reject completed proofs
        };

        if should_reject {
            debug!(
                "📝 Found existing combined proof for block {} sequences {:?} with status {:?}, attempting to reject",
                block_number, combined_sequences, proof.status
            );

            match sui_interface
                .reject_proof(app_instance, block_number, combined_sequences.clone())
                .await
            {
                Ok(tx_digest) => {
                    debug!(
                        "✅ Successfully rejected combined proof for block {}, tx: {}",
                        block_number, tx_digest
                    );
                    // Small delay to let the blockchain state propagate
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    // Even though we found it in our data, it might have been modified by another coordinator
                    if e.to_string().contains("not available for consumption") {
                        debug!(
                            "ℹ️ Combined proof already modified by another coordinator - continuing anyway"
                        );
                    } else {
                        warn!(
                            "⚠️ Failed to reject existing combined proof for block {}: {}",
                            block_number, e
                        );
                    }
                }
            }
        } else {
            debug!(
                "ℹ️ Combined proof exists with status {:?} but doesn't need rejection",
                proof.status
            );
        }
    } else {
        // The combined proof doesn't exist yet, no need to reject
        debug!(
            "ℹ️ No existing combined proof found for sequences {:?} - skipping rejection",
            combined_sequences
        );
    }

    // Step 2: Check if proofs can be reserved before attempting
    // Fetch fresh proof calculation state to check if proofs are available
    debug!("🔍 Fetching fresh proof calculation state before reservation attempt");
    let fresh_app_instance = match sui::fetch::app_instance::fetch_app_instance(app_instance).await {
        Ok(instance) => instance,
        Err(e) => {
            warn!("Failed to fetch fresh app instance: {}", e);
            return Err(anyhow::anyhow!("Failed to fetch app instance: {}", e));
        }
    };
    
    let fresh_proof_calc = match sui::fetch::prover::fetch_proof_calculation(&fresh_app_instance, block_number).await {
        Ok(Some(calc)) => calc,
        Ok(None) => {
            debug!("No proof calculation found for block {} - cannot reserve", block_number);
            return Err(anyhow::anyhow!("No proof calculation found for block {}", block_number));
        }
        Err(e) => {
            warn!("Failed to fetch fresh proof calculation: {}", e);
            return Err(anyhow::anyhow!("Failed to fetch proof calculation: {}", e));
        }
    };
    
    // Check the combined proof status - if it exists, it must be REJECTED to proceed
    let combined_proof = fresh_proof_calc.proofs.iter().find(|p| arrays_equal(&p.sequences, &combined_sequences));
    if let Some(proof) = combined_proof {
        if proof.status != ProofStatus::Rejected {
            debug!("❌ Combined proof (sequences {:?}) already exists with status {:?} - cannot start", combined_sequences, proof.status);
            return Err(anyhow::anyhow!("Combined proof already exists with status {:?}", proof.status));
        }
        debug!("✅ Combined proof exists but is REJECTED - can proceed");
    } else {
        debug!("✅ Combined proof does not exist yet - can create new");
    }
    
    // Check that sequence1 and sequence2 proofs exist and are CALCULATED
    let proof1 = fresh_proof_calc.proofs.iter().find(|p| arrays_equal(&p.sequences, &sequences1));
    let proof2 = fresh_proof_calc.proofs.iter().find(|p| arrays_equal(&p.sequences, &sequences2));
    
    let is_block_proof = combined_sequences.len() == (fresh_proof_calc.end_sequence.unwrap_or(0) - fresh_proof_calc.start_sequence + 1) as usize;
    
    let can_reserve = match (proof1, proof2) {
        (Some(p1), Some(p2)) => {
            // For regular proofs: must be CALCULATED
            // For block proofs: can be CALCULATED, USED, or RESERVED
            let p1_ok = p1.status == ProofStatus::Calculated || 
                       (is_block_proof && (p1.status == ProofStatus::Used || p1.status == ProofStatus::Reserved));
            let p2_ok = p2.status == ProofStatus::Calculated || 
                       (is_block_proof && (p2.status == ProofStatus::Used || p2.status == ProofStatus::Reserved));
            
            if !p1_ok {
                debug!("❌ Proof1 (sequences {:?}) cannot be reserved - status: {:?}", sequences1, p1.status);
            }
            if !p2_ok {
                debug!("❌ Proof2 (sequences {:?}) cannot be reserved - status: {:?}", sequences2, p2.status);
            }
            
            p1_ok && p2_ok
        }
        (None, _) => {
            debug!("❌ Proof1 (sequences {:?}) not found in proof calculation", sequences1);
            false
        }
        (_, None) => {
            debug!("❌ Proof2 (sequences {:?}) not found in proof calculation", sequences2);
            false
        }
    };
    
    if !can_reserve {
        debug!("⚠️ Proofs cannot be reserved for merge job {} - skipping transaction", job_id);
        return Err(anyhow::anyhow!("Proofs not in reservable state for sequences {:?} and {:?}", sequences1, sequences2));
    }
    
    // Step 3: Try to reserve the proofs with start_proving
    debug!("🔒 Proofs are reservable, attempting to reserve for merge job: {}", job_id);
    match sui_interface
        .start_proving(
            app_instance,
            block_number,
            combined_sequences.clone(),
            Some(sequences1.clone()),
            Some(sequences2.clone()),
            job_id.clone(),
        )
        .await
    {
        Ok(tx_digest) => {
            debug!(
                "✅ Successfully reserved proofs for block {}, tx: {}",
                block_number, tx_digest
            );

            // The start_proving_tx function now waits for the transaction to be available in the ledger
            // before returning, so we can proceed immediately to create the merge job

            // Step 3: Create the merge job only if reservation succeeded
            let job_description = Some(format!(
                "Merge proof job for block {} - merging sequences {:?} with {:?}",
                block_number, sequences1, sequences2
            ));

            match sui_interface
                .create_merge_job(
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
                        "✅ Merge job: block={}, sequences={:?}+{:?}, tx={}",
                        block_number, sequences1, sequences2, tx_digest
                    );
                    Ok(())
                }
                Err(e) => {
                    error!(
                        "❌ Failed to create merge job for block {} after successful reservation: {}",
                        block_number, e
                    );
                    Err(anyhow::anyhow!("Failed to create merge job: {}", e))
                }
            }
        }
        Err(e) => {
            // This is expected when multiple coordinators are running
            let error_str = e.to_string();
            if error_str.contains("already reserved") || error_str.contains("MoveAbort") || error_str.contains("version conflict") {
                debug!(
                    "Proofs for block {} already reserved by another coordinator (expected race condition)",
                    block_number
                );
            } else {
                warn!(
                    "⚠️ Failed to reserve proofs for block {}: {}",
                    block_number, e
                );
            }
            // Don't create merge job if we couldn't reserve the proofs
            Err(anyhow::anyhow!("Cannot reserve proofs - likely taken by another coordinator"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrays_equal() {
        assert!(arrays_equal(&[1, 2, 3], &[1, 2, 3]));
        assert!(!arrays_equal(&[1, 2, 3], &[1, 2, 4]));
        assert!(!arrays_equal(&[1, 2], &[1, 2, 3]));
    }

    #[test]
    fn test_find_proofs_to_merge() {
        let block_proofs = ProofCalculation {
            id: "test_id".to_string(),
            block_number: 1,
            start_sequence: 1, // No longer Option
            end_sequence: Some(4),
            proofs: vec![
                Proof {
                    sequences: vec![1, 2],
                    status: ProofStatus::Calculated,
                    da_hash: Some("hash1".to_string()),
                    sequence1: None,
                    sequence2: None,
                    rejected_count: 0,
                    timestamp: 1000,
                    prover: "0x1234".to_string(),
                    user: None,
                    job_id: "job1".to_string(),
                },
                Proof {
                    sequences: vec![3, 4],
                    status: ProofStatus::Calculated,
                    da_hash: Some("hash2".to_string()),
                    sequence1: None,
                    sequence2: None,
                    rejected_count: 0,
                    timestamp: 1000,
                    prover: "0x1234".to_string(),
                    user: None,
                    job_id: "job2".to_string(),
                },
            ],
            block_proof: None,
            is_finished: false,
        };

        let merge_request = find_proofs_to_merge(&block_proofs);
        assert!(merge_request.is_some());

        let request = merge_request.unwrap();
        assert_eq!(request.sequences1, vec![1, 2]);
        assert_eq!(request.sequences2, vec![3, 4]);
        assert!(request.block_proof); // Should be block proof since it covers full range
    }
}

/// Start periodic block creation task
pub async fn start_periodic_block_creation(state: crate::state::SharedState) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::interval;
    
    info!("📦 Starting periodic block creation task (runs every minute)");
    
    let is_running = Arc::new(AtomicBool::new(false));
    let mut check_interval = interval(Duration::from_secs(60)); // Every minute
    check_interval.tick().await; // Skip first immediate tick
    
    loop {
        // Check for shutdown
        if state.is_shutting_down() {
            info!("Block creation task shutting down...");
            break;
        }
        
        check_interval.tick().await;
        
        if state.is_shutting_down() {
            break;
        }
        
        // Skip if already running
        if is_running.load(Ordering::Acquire) {
            debug!("Block creation already in progress, skipping...");
            continue;
        }
        
        is_running.store(true, Ordering::Release);
        
        debug!("Running periodic block creation check...");
        
        // Get all app instances
        let app_instances = state.get_app_instances().await;
        
        if !app_instances.is_empty() {
            debug!("Checking {} app instances for block creation", app_instances.len());
            
            for app_instance_id in app_instances {
                if state.is_shutting_down() {
                    break;
                }
                
                // Try to create a block for this app instance
                let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
                match crate::block::try_create_block(&mut sui_interface, &app_instance_id).await {
                    Ok(Some((tx_digest, sequences, time_since))) => {
                        info!("✅ Created block for app instance {} - tx: {}, sequences: {}, time since last: {}s", 
                            app_instance_id, tx_digest, sequences, time_since);
                    }
                    Ok(None) => {
                        debug!("No block needed for app instance {}", app_instance_id);
                    }
                    Err(e) => {
                        error!("Failed to create block for app instance {}: {}", app_instance_id, e);
                    }
                }
            }
        }
        
        is_running.store(false, Ordering::Release);
    }
}

/// Start periodic proof analysis task
pub async fn start_periodic_proof_analysis(state: crate::state::SharedState) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::interval;
    
    info!("🔬 Starting periodic proof analysis task (runs every 5 minutes)");
    
    let is_running = Arc::new(AtomicBool::new(false));
    let mut check_interval = interval(Duration::from_secs(300)); // Every 5 minutes
    check_interval.tick().await; // Skip first immediate tick
    
    loop {
        // Check for shutdown
        if state.is_shutting_down() {
            info!("Proof analysis task shutting down...");
            break;
        }
        
        check_interval.tick().await;
        
        if state.is_shutting_down() {
            break;
        }
        
        // Skip if already running
        if is_running.load(Ordering::Acquire) {
            debug!("Proof analysis already in progress, skipping...");
            continue;
        }
        
        is_running.store(true, Ordering::Release);
        
        debug!("Running periodic proof completion analysis...");
        
        // Get all app instances
        let app_instances = state.get_app_instances().await;
        
        if !app_instances.is_empty() {
            debug!("Analyzing proof completion for {} app instances", app_instances.len());
            
            let mut analyzed_count = 0;
            
            for app_instance_id in app_instances {
                if state.is_shutting_down() {
                    break;
                }
                
                // Fetch the app instance first
                match sui::fetch::fetch_app_instance(&app_instance_id).await {
                    Ok(app_instance) => {
                        // Analyze proof completion for this app instance
                        match crate::proof::analyze_proof_completion(&app_instance).await {
                            Ok(()) => {
                                analyzed_count += 1;
                                debug!("Completed proof analysis for app instance {}", app_instance_id);
                            }
                            Err(e) => {
                                error!("Failed to analyze proofs for app instance {}: {}", app_instance_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch app instance {}: {}", app_instance_id, e);
                    }
                }
            }
            
            if analyzed_count > 0 {
                info!("✅ Proof analysis complete: {} instances analyzed", analyzed_count);
            }
        }
        
        is_running.store(false, Ordering::Release);
    }
}
