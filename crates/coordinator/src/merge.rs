use crate::coordination::{ProofCalculation, ProofInfo, ProofStatus};
use crate::fetch::fetch_proof_calculations;
use crate::block::settle;
use anyhow::Result;
use tracing::{debug, info, warn, error};

pub async fn analyze_and_create_merge_jobs_with_blockchain_data(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    client: &mut sui_rpc::Client,
    da_hash: &str,
) -> Result<()> {
    info!("🔍 Analyzing proof calculation for block {} with sequences: {:?}", 
        proof_calc.block_number, proof_calc.proofs.last().map(|p| &p.sequences).unwrap_or(&vec![]));

    // Fetch all existing ProofCalculations for this block to get full info including start_sequence, end_sequence, is_finished
    let existing_proof_calculations = match fetch_proof_calculations(client, app_instance, proof_calc.block_number).await {
        Ok(proofs) => {
            info!("🧮 Fetched {} existing ProofCalculations for block {}:", 
                proofs.len(), proof_calc.block_number);
            for (i, pc_info) in proofs.iter().enumerate() {
                info!("  ProofCalculation {}: block={}, start_seq={}, end_seq={:?}, finished={}, individual_proofs={}",
                    i + 1, pc_info.block_number, pc_info.start_sequence, 
                    pc_info.end_sequence, pc_info.is_finished, pc_info.individual_proofs.len());
                for (j, proof) in pc_info.individual_proofs.iter().enumerate() {
                    info!("    Individual proof {}: sequences={:?}, status={}, job_id={}", 
                        j + 1, proof.sequences, proof.status, proof.job_id);
                }
            }
            proofs
        }
        Err(e) => {
            error!("❌ Failed to fetch ProofCalculations for block {}: {}", proof_calc.block_number, e);
            vec![]
        }
    };
    
    // Get the ProofCalculation info for this block (should be at least one)
    // Use the first one or create default values if none exist
    let proof_calc_info = existing_proof_calculations.first();
    let start_sequence = proof_calc_info.map(|p| p.start_sequence).unwrap_or_else(|| {
        // If no ProofCalculation exists yet, derive start_sequence from the proofs
        proof_calc.proofs.iter()
            .flat_map(|p| p.sequences.iter())
            .min()
            .copied()
            .unwrap_or(proof_calc.start_sequence)
    });
    let end_sequence = proof_calc_info.and_then(|p| p.end_sequence);
    let is_finished = proof_calc_info.map(|p| p.is_finished).unwrap_or(false);
    
    info!("📦 Using ProofCalculation info: block_number={}, start_sequence={}, end_sequence={:?}, is_finished={}", 
        proof_calc.block_number, start_sequence, end_sequence, is_finished);

    // Convert to BlockProofs structure
    let mut proof_infos = Vec::new();
    
    // Only add existing proofs from blockchain (which already includes the current proof)
    for existing_proof_calc in &existing_proof_calculations {
        for existing_proof in &existing_proof_calc.individual_proofs {
            // Convert u8 status to ProofStatus enum
            // From Move: 1=STARTED, 2=CALCULATED, 3=REJECTED, 4=RESERVED, 5=USED
            let status = match existing_proof.status {
                1 => ProofStatus::Started,
                2 => ProofStatus::Calculated,
                3 => ProofStatus::Rejected,
                4 => ProofStatus::Reserved,
                5 => ProofStatus::Used,
                _ => ProofStatus::Rejected, // Unknown status defaults to Rejected
            };
            
            proof_infos.push(ProofInfo {
                sequences: existing_proof.sequences.clone(),
                status,
                da_hash: existing_proof.da_hash.clone(),
                timestamp: Some(existing_proof.timestamp),
            });
        }
    }

    let block_proofs = ProofCalculation {
        block_number: proof_calc.block_number,
        start_sequence: proof_calc.start_sequence,  // No Some() needed
        end_sequence: proof_calc.end_sequence,
        proofs: proof_infos,
        block_proof: proof_calc.block_proof.clone(),
        is_finished: proof_calc.is_finished,
    };

    info!("🎯 Block analysis summary for app_instance {}:", app_instance);
    info!("  Block {}: sequences {}-{}", 
        proof_calc.block_number, 
        block_proofs.start_sequence,  // Direct access, no unwrap needed
        block_proofs.end_sequence.map(|e| e.to_string()).unwrap_or_else(|| "pending".to_string()));
    info!("  Total proofs available: {}", block_proofs.proofs.len());
    
    // List all available proofs with their sequences
    for (i, proof) in block_proofs.proofs.iter().enumerate() {
        let status_str = match proof.status {
            ProofStatus::Started => "Started",
            ProofStatus::Calculated => "Calculated",
            ProofStatus::Rejected => "Rejected", 
            ProofStatus::Reserved => "Reserved",
            ProofStatus::Used => "Used",
        };
        info!("    Proof {}: sequences {:?} (status: {})", i + 1, proof.sequences, status_str);
    }
    
    info!("  Block is finished: {}", proof_calc.is_finished);

    // Check if the current proof covers the entire block
    if let Some(end_seq) = block_proofs.end_sequence {
        // Generate the complete sequence range for the block
        let complete_block_sequences: Vec<u64> = (block_proofs.start_sequence..=end_seq).collect();
        
        // Check if current proof contains all sequences for the block
        let current_sequences = proof_calc.proofs.last()
            .map(|p| p.sequences.clone())
            .unwrap_or_default();
        if current_sequences == complete_block_sequences {
            info!("🎉 Current proof covers the entire block {} (sequences {} to {})", 
                proof_calc.block_number, block_proofs.start_sequence, end_seq);
            info!("🔒 Settling block {} as complete for app_instance {}", proof_calc.block_number, app_instance);
            
            // Call settle for the complete block with the DA hash
            settle(app_instance, proof_calc.block_number, da_hash.to_string(), client).await?;
            
            return Ok(());
        }
    } else {
        info!("⏳ Block {} end_sequence not yet determined for app_instance {} (still receiving sequences)", proof_calc.block_number, app_instance);
    }

    // Try to find and create merge jobs, with up to 10 attempts
    const MAX_MERGE_ATTEMPTS: usize = 10;
    let mut attempted_merges = Vec::new();
    let mut merge_created = false;
    
    for attempt in 1..=MAX_MERGE_ATTEMPTS {
        // Refetch ProofCalculations on each attempt to get the latest state
        // This is important because other coordinators might have updated them
        let current_block_proofs = if attempt > 1 {
            info!("🔄 Refetching ProofCalculations to get latest state (attempt {})", attempt);
            
            // Fetch all existing ProofCalculations for this block again
            let updated_proof_calculations = match fetch_proof_calculations(client, app_instance, proof_calc.block_number).await {
                Ok(proofs) => {
                    info!("📊 Refetched {} ProofCalculations for block {}", proofs.len(), proof_calc.block_number);
                    proofs
                }
                Err(e) => {
                    error!("❌ Failed to refetch ProofCalculations for block {}: {}", proof_calc.block_number, e);
                    vec![]
                }
            };
            
            // Rebuild proof_infos with updated data (only from blockchain)
            let mut updated_proof_infos = Vec::new();
            
            // Only add existing proofs from refetched data with their actual status
            for existing_proof_calc in &updated_proof_calculations {
                for existing_proof in &existing_proof_calc.individual_proofs {
                    // Convert u8 status to ProofStatus enum
                    // From Move: 1=STARTED, 2=CALCULATED, 3=REJECTED, 4=RESERVED, 5=USED
                    let status = match existing_proof.status {
                        1 => ProofStatus::Started,
                        2 => ProofStatus::Calculated,
                        3 => ProofStatus::Rejected,
                        4 => ProofStatus::Reserved,
                        5 => ProofStatus::Used,
                        _ => ProofStatus::Rejected, // Unknown status defaults to Rejected
                    };
                    
                    updated_proof_infos.push(ProofInfo {
                        sequences: existing_proof.sequences.clone(),
                        status,
                        da_hash: existing_proof.da_hash.clone(),
                        timestamp: Some(existing_proof.timestamp),
                    });
                }
            }
            
            // Use the most recent ProofCalculation's start_sequence and end_sequence
            // If no updated calculations exist, use the current one's values
            let (latest_start_seq, latest_end_seq, latest_is_finished, latest_block_proof) = 
                if let Some(latest) = updated_proof_calculations.iter().max_by_key(|p| p.individual_proofs.len()) {
                    (latest.start_sequence, latest.end_sequence, latest.is_finished, latest.block_proof.clone())
                } else {
                    (start_sequence, end_sequence, is_finished, proof_calc.block_proof.clone())
                };
            
            ProofCalculation {
                block_number: proof_calc.block_number,
                start_sequence: latest_start_seq,
                end_sequence: latest_end_seq,
                proofs: updated_proof_infos,
                block_proof: latest_block_proof,
                is_finished: latest_is_finished,
            }
        } else {
            block_proofs.clone()
        };
        
        // Find proofs that can be merged, excluding already attempted ones
        if let Some(merge_request) = find_proofs_to_merge_excluding(&current_block_proofs, &attempted_merges) {
            info!("✨ Attempt {}/{}: Found merge opportunity:", attempt, MAX_MERGE_ATTEMPTS);
            info!("  Sequences1: {:?}", merge_request.sequences1);
            info!("  Sequences2: {:?}", merge_request.sequences2);
            info!("  Is block proof: {}", merge_request.block_proof);
            
            // Remember this attempt
            attempted_merges.push((merge_request.sequences1.clone(), merge_request.sequences2.clone()));
            
            // Find the proof statuses for the sequences to merge
            let proof1_status = current_block_proofs.proofs.iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);
                
            let proof2_status = current_block_proofs.proofs.iter()
                .find(|p| p.sequences == merge_request.sequences2)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);
            
            // Try to create merge job with proof reservation
            match create_merge_job(
                merge_request.sequences1.clone(),
                merge_request.sequences2.clone(),
                proof_calc.block_number,
                app_instance,
                client,
                proof1_status,
                proof2_status,
                &current_block_proofs,
            ).await {
                Ok(_) => {
                    info!("✅ Successfully created and reserved merge job for block {} on attempt {}", 
                        proof_calc.block_number, attempt);
                    merge_created = true;
                    break; // Successfully created a merge job, stop trying
                }
                Err(e) => {
                    info!("ℹ️ Attempt {}/{}: Could not create merge job for block {} (sequences {:?} + {:?}): {}", 
                        attempt, MAX_MERGE_ATTEMPTS, proof_calc.block_number, 
                        merge_request.sequences1, merge_request.sequences2, e);
                    
                    // Check if we should continue trying
                    if e.to_string().contains("already reserved") {
                        info!("   → Looking for other merge opportunities...");
                        // Add a small delay before retrying to allow other coordinators to complete
                        if attempt < MAX_MERGE_ATTEMPTS {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }
                        // Continue to next iteration to try another merge opportunity
                    } else {
                        warn!("   → Unexpected error, will try other opportunities: {}", e);
                    }
                }
            }
        } else {
            debug!("🚫 No more merge opportunities found for block {} after {} attempt(s)", 
                proof_calc.block_number, attempt);
            break; // No more merge opportunities available
        }
    }
    
    if !merge_created && !attempted_merges.is_empty() {
        warn!("❌ Failed to create any merge job for block {} after {} attempts", 
            proof_calc.block_number, attempted_merges.len());
    } else if !merge_created {
        info!("🚫 No merge opportunities found for block {} in app_instance {}", proof_calc.block_number, app_instance);
    }

    Ok(())
}

// Main entry point - always use blockchain data
#[allow(dead_code)]
pub async fn analyze_and_create_merge_jobs(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    client: &mut sui_rpc::Client,
    da_hash: &str,
) -> Result<()> {
    analyze_and_create_merge_jobs_with_blockchain_data(proof_calc, app_instance, client, da_hash).await
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
    excluded: &[(Vec<u64>, Vec<u64>)]
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
            if excluded.iter().any(|(s1, s2)| s1 == &sequence1 && s2 == &sequence2) {
                continue; // Skip this pair as it was already attempted
            }
            
            let proof1 = block_proofs.proofs.iter().find(|p| arrays_equal(&p.sequences, &sequence1));
            let proof2 = block_proofs.proofs.iter().find(|p| arrays_equal(&p.sequences, &sequence2));
            
            if let (Some(proof1), Some(proof2)) = (proof1, proof2) {
                // For block proofs, only check CALCULATED or USED status (not RESERVED)
                // This matches the TypeScript logic exactly
                if (proof1.status == ProofStatus::Calculated || proof1.status == ProofStatus::Used) &&
                   (proof2.status == ProofStatus::Calculated || proof2.status == ProofStatus::Used) {
                    info!("Merging proofs to create block proof: sequences1={:?}, sequences2={:?}", 
                        sequence1, sequence2);
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
            if excluded.iter().any(|(s1, s2)| s1 == &proof1.sequences && s2 == &proof2.sequences) {
                continue; // Skip this pair as it was already attempted
            }
            
            // Condition 1: Check if proof1.sequences[last] + 1 == proof2.sequences[first]
            if let (Some(proof1_last), Some(proof2_first)) = (proof1.sequences.last(), proof2.sequences.first()) {
                if *proof1_last + 1 == *proof2_first {
                    // Construct combined sequences
                    let combined: Vec<u64> = [&proof1.sequences[..], &proof2.sequences[..]].concat();
                    
                    // Condition 2: Ensure no proof already exists with the same combined sequence
                    let already_exists = block_proofs.proofs.iter().any(|p| {
                        arrays_equal(&p.sequences, &combined) && is_proof_active_or_completed(p, current_time)
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

fn is_proof_available(proof: &ProofInfo, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated => true,
        ProofStatus::Reserved => {
            // Check if timeout has expired (RESERVED proofs can be used if timed out)
            if let Some(timestamp) = proof.timestamp {
                current_time > timestamp + TIMEOUT_MS
            } else {
                false
            }
        },
        ProofStatus::Started => false, // Started proofs are still being calculated, not available for merging
        ProofStatus::Used => false, // USED proofs are NOT available for merging
        ProofStatus::Rejected => false, // REJECTED proofs are not available for merging
    }
}

fn is_proof_active_or_completed(proof: &ProofInfo, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated | ProofStatus::Used | ProofStatus::Reserved => true,
        ProofStatus::Started => {
            // Check if it's still within timeout
            if let Some(timestamp) = proof.timestamp {
                current_time <= timestamp + TIMEOUT_MS
            } else {
                false
            }
        },
        _ => false,
    }
}


async fn create_merge_job(
    sequences1: Vec<u64>, 
    sequences2: Vec<u64>, 
    block_number: u64,
    app_instance: &str,
    client: &mut sui_rpc::Client,
    _proof1_status: &ProofStatus,  // Currently unused, but kept for future use
    _proof2_status: &ProofStatus,  // Currently unused, but kept for future use
    block_proofs: &ProofCalculation,    // Add this to check if combined proof exists
) -> Result<()> {
    info!(
        "Attempting to create merge job for block {} with sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

    // Create SilvanaSuiInterface following the same pattern as submit_proof in grpc.rs
    let sui_client = client.clone(); // Clone the client  
    let mut sui_interface = sui::interface::SilvanaSuiInterface::new(sui_client);

    // Generate a unique job ID for this merge operation
    let job_id = format!("merge_{}_{}_{}", block_number, sequences1[0], sequences2[0]);

    // Combine sequences for reservation
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();
    
    // Step 1: Check if we need to reject an existing combined proof
    // Look for an existing proof with the combined sequences
    let combined_proof = block_proofs.proofs.iter().find(|p| p.sequences == combined_sequences);
    
    if let Some(proof) = combined_proof {
        // Check if the proof needs to be rejected based on its status
        let should_reject = match proof.status {
            ProofStatus::Started => {
                // Only reject Started proofs if they've timed out (10+ minutes)
                if let Some(timestamp) = proof.timestamp {
                    let current_time = chrono::Utc::now().timestamp() as u64 * 1000; // Convert to milliseconds
                    let time_since_start = current_time.saturating_sub(timestamp);
                    const STARTED_TIMEOUT_MS: u64 = 10 * 60 * 1000; // 10 minutes for Started status
                    
                    if time_since_start > STARTED_TIMEOUT_MS {
                        info!("⏰ Started proof has timed out ({} ms > {} ms), will reject", 
                            time_since_start, STARTED_TIMEOUT_MS);
                        true
                    } else {
                        info!("⏳ Started proof is still active ({} ms < {} ms), skipping merge", 
                            time_since_start, STARTED_TIMEOUT_MS);
                        // Return early - don't try to merge with an active Started proof
                        return Err(anyhow::anyhow!(
                            "Cannot merge: combined proof with sequences {:?} is already Started and not timed out", 
                            combined_sequences
                        ));
                    }
                } else {
                    // No timestamp, can't determine timeout, skip
                    false
                }
            },
            ProofStatus::Reserved => {
                // Reserved proofs can be rejected if timed out (2 minutes)
                if let Some(timestamp) = proof.timestamp {
                    let current_time = chrono::Utc::now().timestamp() as u64 * 1000;
                    current_time > timestamp + TIMEOUT_MS
                } else {
                    false
                }
            },
            ProofStatus::Rejected => true,  // Already rejected, can try again
            ProofStatus::Calculated | ProofStatus::Used => false, // Don't reject completed proofs
        };
        
        if should_reject {
            info!("📝 Found existing combined proof for block {} sequences {:?} with status {:?}, attempting to reject", 
                block_number, combined_sequences, proof.status);
            
            match sui_interface.reject_proof(
                app_instance,
                block_number,
                combined_sequences.clone(),
            ).await {
                Ok(tx_digest) => {
                    info!("✅ Successfully rejected combined proof for block {}, tx: {}", block_number, tx_digest);
                    // Small delay to let the blockchain state propagate
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    // Even though we found it in our data, it might have been modified by another coordinator
                    if e.to_string().contains("not available for consumption") {
                        info!("ℹ️ Combined proof already modified by another coordinator - continuing anyway");
                    } else {
                        warn!("⚠️ Failed to reject existing combined proof for block {}: {}", block_number, e);
                    }
                }
            }
        } else {
            info!("ℹ️ Combined proof exists with status {:?} but doesn't need rejection", proof.status);
        }
    } else {
        // The combined proof doesn't exist yet, no need to reject
        info!("ℹ️ No existing combined proof found for sequences {:?} - skipping rejection", combined_sequences);
    }

    // Step 2: Try to reserve the proofs with start_proving
    info!("🔒 Attempting to reserve proofs for merge job: {}", job_id);
    match sui_interface.start_proving(
        app_instance,
        block_number,
        combined_sequences.clone(),
        Some(sequences1.clone()),
        Some(sequences2.clone()),
        job_id.clone(),
    ).await {
        Ok(tx_digest) => {
            info!("✅ Successfully reserved proofs for block {}, tx: {}", block_number, tx_digest);
            
            // The start_proving_tx function now waits for the transaction to be available in the ledger
            // before returning, so we can proceed immediately to create the merge job
            
            // Step 3: Create the merge job only if reservation succeeded
            let job_description = Some(format!(
                "Merge proof job for block {} - merging sequences {:?} with {:?}", 
                block_number, sequences1, sequences2
            ));

            match sui_interface.create_merge_job(
                app_instance,
                block_number,
                sequences1.clone(),
                sequences2.clone(),
                job_description,
            ).await {
                Ok(tx_digest) => {
                    info!(
                        "✅ Successfully created merge job for block {} - Transaction: {}",
                        block_number, tx_digest
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
            warn!(
                "⚠️ Failed to reserve proofs for block {} - another coordinator may have already reserved them: {}",
                block_number, e
            );
            // Don't create merge job if we couldn't reserve the proofs
            Err(anyhow::anyhow!("Failed to reserve proofs for merge: {}", e))
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
            block_number: 1,
            start_sequence: 1,  // No longer Option
            end_sequence: Some(4),
            proofs: vec![
                ProofInfo {
                    sequences: vec![1, 2],
                    status: ProofStatus::Calculated,
                    da_hash: Some("hash1".to_string()),
                    timestamp: Some(1000),
                },
                ProofInfo {
                    sequences: vec![3, 4],
                    status: ProofStatus::Calculated,
                    da_hash: Some("hash2".to_string()),
                    timestamp: Some(1000),
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