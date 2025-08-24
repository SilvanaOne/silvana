use crate::coordination::ProofCalculation;
use crate::fetch::{fetch_block_info, fetch_proof_calculations};
use crate::block::settle;
use anyhow::Result;
use tracing::{info, warn, error};

#[derive(Debug, Clone)]
pub struct ProofInfo {
    pub sequences: Vec<u64>,
    pub status: ProofStatus,
    #[allow(dead_code)]
    pub da_hash: Option<String>,
    pub timestamp: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ProofStatus {
    Started,
    Calculated,
    Used,
    Reserved,
    Failed,
}

#[derive(Debug, Clone)]
pub struct BlockProofs {
    #[allow(dead_code)]
    pub block_number: u64,
    pub start_sequence: Option<u64>,
    pub end_sequence: Option<u64>,
    pub proofs: Vec<ProofInfo>,
    pub is_finished: bool,
}

pub async fn analyze_and_create_merge_jobs_with_blockchain_data(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    client: &mut sui_rpc::Client,
    da_hash: &str,
) -> Result<()> {
    info!("🔍 Analyzing proof calculation for block {} with sequences: {:?}", 
        proof_calc.block_number, proof_calc.sequences);

    // Fetch Block information from blockchain
    let block_info = match fetch_block_info(client, app_instance, proof_calc.block_number).await {
        Ok(Some(block)) => {
            info!("📦 Fetched Block info: block_number={}, start_sequence={}, end_sequence={}, name={}", 
                block.block_number, block.start_sequence, block.end_sequence, block.name);
            Some(block)
        }
        Ok(None) => {
            warn!("⚠️ No Block found for block_number {}", proof_calc.block_number);
            None
        }
        Err(e) => {
            error!("❌ Failed to fetch Block info for block {}: {}", proof_calc.block_number, e);
            None
        }
    };

    // Fetch all existing ProofCalculations for this block
    let existing_proof_calculations = match fetch_proof_calculations(client, app_instance, proof_calc.block_number).await {
        Ok(proofs) => {
            info!("🧮 Fetched {} existing ProofCalculations for block {}:", 
                proofs.len(), proof_calc.block_number);
            for (i, proof_calc) in proofs.iter().enumerate() {
                info!("  ProofCalculation {}: block={}, start_seq={}, end_seq={:?}, finished={}, individual_proofs={}",
                    i + 1, proof_calc.block_number, proof_calc.start_sequence, 
                    proof_calc.end_sequence, proof_calc.is_finished, proof_calc.individual_proofs.len());
                for (j, proof) in proof_calc.individual_proofs.iter().enumerate() {
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

    // Convert to BlockProofs structure
    let mut proof_infos = Vec::new();
    
    // Add the current proof
    proof_infos.push(ProofInfo {
        sequences: proof_calc.sequences.clone(),
        status: ProofStatus::Calculated,
        da_hash: None, // TODO: Get actual DA hash from submission
        timestamp: Some(chrono::Utc::now().timestamp() as u64 * 1000),
    });

    // Add existing proofs (assume they are all calculated)
    for existing_proof_calc in &existing_proof_calculations {
        for existing_proof in &existing_proof_calc.individual_proofs {
            proof_infos.push(ProofInfo {
                sequences: existing_proof.sequences.clone(),
                status: ProofStatus::Calculated,
                da_hash: existing_proof.da_hash.clone(),
                timestamp: Some(existing_proof.timestamp),
            });
        }
    }

    let block_proofs = BlockProofs {
        block_number: proof_calc.block_number,
        start_sequence: block_info.as_ref().map(|b| b.start_sequence),
        end_sequence: block_info.as_ref().map(|b| b.end_sequence),
        proofs: proof_infos,
        is_finished: false,
    };

    info!("🎯 Block analysis summary:");
    info!("  Block {}: sequences {}-{}", 
        proof_calc.block_number, 
        block_proofs.start_sequence.unwrap_or(0), 
        block_proofs.end_sequence.unwrap_or(0));
    info!("  Total proofs available: {} (including current)", block_proofs.proofs.len());
    info!("  Current proof covers: sequences {:?}", proof_calc.sequences);

    // Check if the current proof covers the entire block
    if let (Some(start_seq), Some(end_seq)) = (block_proofs.start_sequence, block_proofs.end_sequence) {
        // Generate the complete sequence range for the block
        let complete_block_sequences: Vec<u64> = (start_seq..=end_seq).collect();
        
        // Check if current proof contains all sequences for the block
        if proof_calc.sequences == complete_block_sequences {
            info!("🎉 Current proof covers the entire block {} (sequences {} to {})", 
                proof_calc.block_number, start_seq, end_seq);
            info!("🔒 Settling block {} as complete", proof_calc.block_number);
            
            // Call settle for the complete block with the DA hash
            settle(proof_calc.clone(), da_hash.to_string()).await?;
            
            return Ok(());
        }
    }

    // Try to find and create merge jobs, with up to 10 attempts
    const MAX_MERGE_ATTEMPTS: usize = 10;
    let mut attempted_merges = Vec::new();
    let mut merge_created = false;
    
    for attempt in 1..=MAX_MERGE_ATTEMPTS {
        // Find proofs that can be merged, excluding already attempted ones
        if let Some(merge_request) = find_proofs_to_merge_excluding(&block_proofs, &attempted_merges) {
            info!("✨ Attempt {}/{}: Found merge opportunity:", attempt, MAX_MERGE_ATTEMPTS);
            info!("  Sequences1: {:?}", merge_request.sequences1);
            info!("  Sequences2: {:?}", merge_request.sequences2);
            info!("  Is block proof: {}", merge_request.block_proof);
            
            // Remember this attempt
            attempted_merges.push((merge_request.sequences1.clone(), merge_request.sequences2.clone()));
            
            // Find the proof statuses for the sequences to merge
            let proof1_status = block_proofs.proofs.iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);
                
            let proof2_status = block_proofs.proofs.iter()
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
            ).await {
                Ok(_) => {
                    info!("✅ Successfully created and reserved merge job for block {} on attempt {}", 
                        proof_calc.block_number, attempt);
                    merge_created = true;
                    break; // Successfully created a merge job, stop trying
                }
                Err(e) => {
                    warn!("⚠️ Attempt {}/{}: Failed to create merge job for block {}: {}. Looking for other opportunities...", 
                        attempt, MAX_MERGE_ATTEMPTS, proof_calc.block_number, e);
                    // Continue to next iteration to try another merge opportunity
                }
            }
        } else {
            info!("🚫 No more merge opportunities found for block {} after {} attempt(s)", 
                proof_calc.block_number, attempt);
            break; // No more merge opportunities available
        }
    }
    
    if !merge_created && !attempted_merges.is_empty() {
        warn!("❌ Failed to create any merge job for block {} after {} attempts", 
            proof_calc.block_number, attempted_merges.len());
    } else if !merge_created {
        info!("🚫 No merge opportunities found for block {}", proof_calc.block_number);
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
    block_proofs: &BlockProofs, 
    excluded: &[(Vec<u64>, Vec<u64>)]
) -> Option<MergeRequest> {
    if block_proofs.is_finished {
        return None;
    }

    // First priority: Try to create block proofs (complete range from start to end)
    if let (Some(start_seq), Some(end_seq)) = (block_proofs.start_sequence, block_proofs.end_sequence) {
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
pub fn find_proofs_to_merge(block_proofs: &BlockProofs) -> Option<MergeRequest> {
    find_proofs_to_merge_excluding(block_proofs, &[])
}

fn arrays_equal(a: &[u64], b: &[u64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

fn is_proof_available(proof: &ProofInfo, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated => true,
        ProofStatus::Reserved => {
            // Check if timeout has expired
            if let Some(timestamp) = proof.timestamp {
                current_time > timestamp + TIMEOUT_MS
            } else {
                false
            }
        },
        _ => false,
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
    proof1_status: &ProofStatus,
    proof2_status: &ProofStatus,
) -> Result<()> {
    info!(
        "Attempting to create merge job for block {} with sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

    // Create SuiJobInterface following the same pattern as submit_proof in grpc.rs
    let sui_client = client.clone(); // Clone the client  
    let mut sui_interface = crate::sui_interface::SuiJobInterface::new(sui_client);

    // Generate a unique job ID for this merge operation
    let job_id = format!("merge_{}_{}_{}", block_number, sequences1[0], sequences2[0]);

    // Combine sequences for reservation
    let mut combined_sequences = sequences1.clone();
    combined_sequences.extend(sequences2.clone());
    combined_sequences.sort();
    
    // Step 1: Reject proofs if they're in certain states (following TypeScript logic)
    // From TypeScript: status === ProofStatus.CALCULATED || USED || RESERVED || STARTED
    let should_reject = matches!(
        (proof1_status, proof2_status),
        (ProofStatus::Calculated, _) | (ProofStatus::Used, _) | 
        (ProofStatus::Reserved, _) | (ProofStatus::Started, _) |
        (_, ProofStatus::Calculated) | (_, ProofStatus::Used) | 
        (_, ProofStatus::Reserved) | (_, ProofStatus::Started)
    );

    if should_reject {
        info!("📝 Rejecting proofs before merge for block {} sequences {:?}", block_number, combined_sequences);
        match sui_interface.reject_proof(
            app_instance,
            block_number,
            combined_sequences.clone(),
        ).await {
            Ok(tx_digest) => {
                info!("✅ Successfully rejected proofs for block {}, tx: {}", block_number, tx_digest);
            }
            Err(e) => {
                // Continue even if rejection fails, as the proofs might already be in the right state
                warn!("⚠️ Failed to reject proofs for block {}: {}", block_number, e);
            }
        }
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
        let block_proofs = BlockProofs {
            block_number: 1,
            start_sequence: Some(1),
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