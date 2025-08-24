use crate::coordination::{ProofCalculation, ProofMergeData};
use crate::fetch::{fetch_block_info, fetch_proof_calculations};
use anyhow::Result;
use tracing::{info, warn, error};

#[derive(Debug, Clone)]
pub struct ProofInfo {
    pub sequences: Vec<u64>,
    pub status: ProofStatus,
    pub da_hash: Option<String>,
    pub timestamp: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProofStatus {
    Started,
    Calculated,
    Used,
    Reserved,
    Failed,
}

#[derive(Debug, Clone)]
pub struct BlockProofs {
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

    // Find proofs that can be merged using the same logic as TypeScript
    if let Some(merge_request) = find_proofs_to_merge(&block_proofs) {
        info!("✨ Found merge opportunity:");
        info!("  Sequences1: {:?}", merge_request.sequences1);
        info!("  Sequences2: {:?}", merge_request.sequences2);
        info!("  Is block proof: {}", merge_request.block_proof);
        
        create_merge_job(
            merge_request.sequences1,
            merge_request.sequences2,
            proof_calc.block_number
        ).await?;
    } else {
        info!("🚫 No merge opportunities found for block {}", proof_calc.block_number);
    }

    Ok(())
}

// Main entry point - always use blockchain data
pub async fn analyze_and_create_merge_jobs(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    client: &mut sui_rpc::Client,
) -> Result<()> {
    analyze_and_create_merge_jobs_with_blockchain_data(proof_calc, app_instance, client).await
}

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
    pub block_proof: bool,
}

const TIMEOUT_MS: u64 = 2 * 60 * 1000; // 2 minutes

pub fn find_proofs_to_merge(block_proofs: &BlockProofs) -> Option<MergeRequest> {
    if block_proofs.is_finished {
        return None;
    }

    // First priority: Try to create block proofs (complete range from start to end)
    if let (Some(start_seq), Some(end_seq)) = (block_proofs.start_sequence, block_proofs.end_sequence) {
        for i in (start_seq + 1)..=end_seq {
            let sequence1: Vec<u64> = (start_seq..i).collect();
            let sequence2: Vec<u64> = (i..=end_seq).collect();
            
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


async fn create_merge_job(sequences1: Vec<u64>, sequences2: Vec<u64>, block_number: u64) -> Result<()> {
    // Create ProofMergeData
    let merge_data = ProofMergeData {
        block_number,
        sequences1: sequences1.clone(),
        sequences2: sequences2.clone(),
    };

    // Serialize the merge data
    let serialized_data = serde_json::to_vec(&merge_data)?;
    
    info!(
        "Creating merge job for block {} with sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

    // TODO: Create actual Sui transaction to create merge job
    // This would involve:
    // 1. Creating a job with the serialized ProofMergeData
    // 2. Setting proper metadata (developer, agent, agent_method, etc.)
    // 3. Submitting to the blockchain
    
    // For now, just log the action
    info!("Merge job data prepared (size: {} bytes)", serialized_data.len());

    Ok(())
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