use crate::constants::{
    BLOCK_CREATION_CHECK_INTERVAL_SECS, PROOF_ANALYSIS_INTERVAL_SECS,
    PROOF_RESERVED_TIMEOUT_MS, PROOF_STARTED_TIMEOUT_MS,
    PROOF_USED_MERGE_TIMEOUT_MS,
};
use anyhow::Result;
use sui::fetch::fetch_proof_calculation;
use sui::fetch::{Proof, ProofCalculation, ProofStatus};
use tracing::{debug, error, info, warn};

pub async fn analyze_and_create_merge_jobs_with_blockchain_data(
    proof_calc: &ProofCalculation,
    app_instance: &str,
    state: &crate::state::SharedState,
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
            error!("❌ Failed to fetch AppInstance {}: {}", app_instance, e);
            return Err(anyhow::anyhow!("Failed to fetch AppInstance: {}", e));
        }
    };

    // Check if this block is too far ahead of the last proved block
    // Only create merge jobs for blocks within MAX_BLOCK_LOOKAHEAD blocks of last_proved_block
    if proof_calc.block_number > app_instance_obj.last_proved_block_number + crate::constants::MAX_BLOCK_LOOKAHEAD {
        debug!(
            "⏭️ Skipping merge job creation for block {} (too far ahead, last_proved={})",
            proof_calc.block_number,
            app_instance_obj.last_proved_block_number
        );
        return Ok(());
    }

    // Fetch existing ProofCalculation for this block to get full info including start_sequence, end_sequence, is_finished
    let existing_proof_calculation =
        match fetch_proof_calculation(&app_instance_obj, proof_calc.block_number).await {
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
    let end_sequence = existing_proof_calculation
        .as_ref()
        .and_then(|p| p.end_sequence);
    let is_finished = existing_proof_calculation
        .as_ref()
        .map(|p| p.is_finished)
        .unwrap_or(false);

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
        debug!(
            "📝 Detailed proof list for block {}:",
            proof_calc.block_number
        );
        for (idx, proof) in block_proofs.proofs.iter().enumerate() {
            let sequences_str = if proof.sequences.len() <= 10 {
                format!("{:?}", proof.sequences)
            } else {
                format!(
                    "[{} sequences: {}..{}]",
                    proof.sequences.len(),
                    proof.sequences.first().unwrap_or(&0),
                    proof.sequences.last().unwrap_or(&0)
                )
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

            return Ok(());
        }
    } else {
        debug!(
            "⏳ Block {} end_sequence not yet determined for app_instance {} (still receiving sequences)",
            proof_calc.block_number, app_instance
        );
    }

    // Find ALL merge opportunities at once
    let merge_opportunities = find_all_proofs_to_merge(&block_proofs);

    if merge_opportunities.is_empty() {
        debug!(
            "🚫 No merge opportunities found for block {} in app_instance {}",
            proof_calc.block_number, app_instance
        );
    } else {
        debug!(
            "✨ Found {} merge opportunities for block {}",
            merge_opportunities.len(),
            proof_calc.block_number
        );

        let mut successful_merges = 0;
        let mut failed_merges = 0;

        // Try to create merge jobs for ALL opportunities
        for (idx, merge_request) in merge_opportunities.iter().enumerate() {
            // Find the actual proof objects for more details
            let proof1_details = block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| format!("status={:?}", p.status))
                .unwrap_or_else(|| "not found".to_string());

            let proof2_details = block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences2)
                .map(|p| format!("status={:?}", p.status))
                .unwrap_or_else(|| "not found".to_string());

            debug!(
                "  Merge opportunity #{}: Sequences1: {:?} ({}), Sequences2: {:?} ({}), Is block proof: {}",
                idx + 1,
                merge_request.sequences1,
                proof1_details,
                merge_request.sequences2,
                proof2_details,
                merge_request.block_proof
            );

            // Find the proof statuses for the sequences to merge
            let proof1_status = block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences1)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);

            let proof2_status = block_proofs
                .proofs
                .iter()
                .find(|p| p.sequences == merge_request.sequences2)
                .map(|p| &p.status)
                .unwrap_or(&ProofStatus::Calculated);

            // Special logging for block 180
            if proof_calc.block_number == 180 {
                info!("🔨 Block 180: Attempting to create merge job for {:?} + {:?}",
                      merge_request.sequences1, merge_request.sequences2);
            }

            // Try to create merge job with proof reservation
            match create_merge_job(
                merge_request.sequences1.clone(),
                merge_request.sequences2.clone(),
                proof_calc.block_number,
                app_instance,
                proof1_status,
                proof2_status,
                &block_proofs,
                state,
            )
            .await
            {
                Ok(_) => {
                    if proof_calc.block_number == 180 {
                        info!("✅ Block 180: Successfully created merge job");
                    }
                    debug!(
                        "✅ Successfully created merge job #{} for block {}",
                        idx + 1, proof_calc.block_number
                    );
                    successful_merges += 1;
                }
                Err(e) => {
                    let error_str = e.to_string();
                    if proof_calc.block_number == 180 {
                        info!("❌ Block 180: Failed to create merge job: {}", e);
                    }

                    // Log based on error type
                    if error_str.contains("already reserved")
                        || error_str.contains("already Started and not timed out")
                        || error_str.contains("Cannot merge")
                        || error_str.contains("Combined proof already exists")
                        || error_str.contains("Cannot reserve proofs - likely taken by another coordinator")
                        || error_str.contains("Proofs not in reservable state")
                    {
                        // These are expected conditions in a concurrent environment
                        debug!(
                            "ℹ️ Merge job #{} for block {} skipped (expected condition): {}",
                            idx + 1, proof_calc.block_number, e
                        );
                    } else {
                        warn!(
                            "⚠️ Failed to create merge job #{} for block {}: {}",
                            idx + 1, proof_calc.block_number, e
                        );
                    }
                    failed_merges += 1;
                }
            }
        }

        // Log summary
        if successful_merges > 0 {
            info!(
                "📊 Block {} merge jobs: {} created successfully, {} skipped/failed",
                proof_calc.block_number, successful_merges, failed_merges
            );
        } else if failed_merges > 0 {
            debug!(
                "ℹ️ Block {} merge jobs: all {} opportunities were skipped (proofs may be in progress)",
                proof_calc.block_number, failed_merges
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub sequences1: Vec<u64>,
    pub sequences2: Vec<u64>,
    pub block_proof: bool,
}

// Helper function to check if a number is a power of 2
fn is_power_of_two(n: u64) -> bool {
    n > 0 && (n & (n - 1)) == 0
}

// Helper function to get the largest power of 2 less than n
fn largest_power_of_two_below(n: u64) -> u64 {
    if n <= 1 {
        return 0;
    }
    let mut power = 1u64;
    while power * 2 < n {
        power *= 2;
    }
    power
}

// Find the deterministic split point for a range when end_sequence is known
fn find_binary_split_point(start: u64, end: u64) -> Option<u64> {
    // Cannot split a single element or invalid range
    if start >= end {
        return None;
    }

    let total_sequences = end - start + 1;

    if is_power_of_two(total_sequences) {
        // For power of 2, split in the middle
        Some(start + total_sequences / 2)
    } else {
        // For non-power of 2, split at the largest power of 2
        let split_size = largest_power_of_two_below(total_sequences);
        if split_size > 0 {
            Some(start + split_size)
        } else {
            None
        }
    }
}

// Find merge for building towards a specific target range using binary tree pattern
fn find_merge_for_target_range(
    start: u64,
    end: u64,
    block_proofs: &ProofCalculation,
    current_time: u64,
    excluded: &[(Vec<u64>, Vec<u64>)],
) -> Option<MergeRequest> {
    // Base case: single sequence, cannot merge
    if start == end {
        return None;
    }

    let full_range: Vec<u64> = (start..=end).collect();

    // Check if the full range proof already exists
    let full_proof_exists = block_proofs.proofs.iter()
        .any(|p| arrays_equal(&p.sequences, &full_range) && is_proof_active_or_completed(p, current_time));

    if full_proof_exists {
        return None; // Already have this proof
    }

    // Find the binary split point
    if let Some(split) = find_binary_split_point(start, end) {
        // Validate split point to prevent infinite recursion
        if split <= start || split > end {
            return None;
        }

        let left_range: Vec<u64> = (start..split).collect();
        let right_range: Vec<u64> = (split..=end).collect();

        // Check if this merge is excluded
        if excluded.iter().any(|(s1, s2)|
            (arrays_equal(s1, &left_range) && arrays_equal(s2, &right_range)) ||
            (arrays_equal(s1, &right_range) && arrays_equal(s2, &left_range))
        ) {
            return None;
        }

        // Check if both parts exist and are available
        let left_proof = block_proofs.proofs.iter()
            .find(|p| arrays_equal(&p.sequences, &left_range));
        let right_proof = block_proofs.proofs.iter()
            .find(|p| arrays_equal(&p.sequences, &right_range));

        if let (Some(left), Some(right)) = (left_proof, right_proof) {
            if is_proof_available(left, current_time) && is_proof_available(right, current_time) {
                // Both parts are available, we can merge them
                return Some(MergeRequest {
                    sequences1: left_range,
                    sequences2: right_range,
                    block_proof: false,
                });
            }
        }

        // If parts don't exist, recursively try to create them
        // Try to create the left part first
        if start < split - 1 { // Only recurse if there's actually a range to process
            if left_proof.is_none() || (left_proof.is_some() && !is_proof_available(left_proof.unwrap(), current_time)) {
                if let Some(merge) = find_merge_for_target_range(start, split - 1, block_proofs, current_time, excluded) {
                    return Some(merge);
                }
            }
        }

        // Then try to create the right part
        if split <= end { // Only recurse if there's actually a range to process
            if right_proof.is_none() || (right_proof.is_some() && !is_proof_available(right_proof.unwrap(), current_time)) {
                if let Some(merge) = find_merge_for_target_range(split, end, block_proofs, current_time, excluded) {
                    return Some(merge);
                }
            }
        }
    }

    None
}

// Find the next merge following the deterministic binary tree pattern
fn find_proofs_to_merge_excluding(
    block_proofs: &ProofCalculation,
    excluded: &[(Vec<u64>, Vec<u64>)],
) -> Option<MergeRequest> {
    if block_proofs.is_finished {
        return None;
    }

    let current_time = chrono::Utc::now().timestamp() as u64 * 1000;
    let start_seq = block_proofs.start_sequence;

    // CASE 1: end_sequence is known - use deterministic binary split
    if let Some(end_seq) = block_proofs.end_sequence {
        debug!("Block {} has known end_sequence={}, using deterministic binary tree merge",
               block_proofs.block_number, end_seq);

        // First, check if we can create the block proof directly
        let full_range: Vec<u64> = (start_seq..=end_seq).collect();
        let block_proof_exists = block_proofs.proofs.iter()
            .any(|p| arrays_equal(&p.sequences, &full_range));

        if !block_proof_exists {
            // Try to create the block proof using binary splits
            if let Some(split) = find_binary_split_point(start_seq, end_seq) {
                let left_range: Vec<u64> = (start_seq..split).collect();
                let right_range: Vec<u64> = (split..=end_seq).collect();

                // Check if this merge is excluded
                if !excluded.iter().any(|(s1, s2)|
                    (arrays_equal(s1, &left_range) && arrays_equal(s2, &right_range))
                ) {
                    // Check if both halves exist and are available
                    let left_proof = block_proofs.proofs.iter()
                        .find(|p| arrays_equal(&p.sequences, &left_range));
                    let right_proof = block_proofs.proofs.iter()
                        .find(|p| arrays_equal(&p.sequences, &right_range));

                    if let (Some(left), Some(right)) = (left_proof, right_proof) {
                        if is_proof_available(left, current_time) && is_proof_available(right, current_time) {
                            debug!("Found block proof merge: {:?} + {:?}", left_range, right_range);
                            return Some(MergeRequest {
                                sequences1: left_range,
                                sequences2: right_range,
                                block_proof: true,
                            });
                        }
                    }
                }
            }
        }

        // If we can't create the block proof directly, work on creating the necessary parts
        if let Some(merge) = find_merge_for_target_range(start_seq, end_seq, block_proofs, current_time, excluded) {
            debug!("Found intermediate merge for block completion: {:?} + {:?}",
                  merge.sequences1, merge.sequences2);
            return Some(merge);
        }
    }

    // CASE 2: end_sequence is unknown - build binary tree from bottom up
    debug!("Block {} has unknown end_sequence, building binary tree from bottom up",
           block_proofs.block_number);

    // Try merging at each level, starting from singles (level 1)
    // Level 1: merge singles into pairs [2n, 2n+1]
    // Level 2: merge pairs into quads [[4n, 4n+1], [4n+2, 4n+3]]
    // Level 4: merge quads into groups of 8, etc.

    let mut level = 1u64;
    while level <= 256 { // Reasonable upper bound
        // Find all proofs at this level
        let proofs_at_level: Vec<&Proof> = block_proofs.proofs.iter()
            .filter(|p| p.sequences.len() as u64 == level)
            .collect();

        if proofs_at_level.is_empty() {
            level *= 2;
            continue;
        }

        // Try to find mergeable pairs at this level
        for proof1 in &proofs_at_level {
            if !is_proof_available(proof1, current_time) {
                continue;
            }

            let proof1_start = *proof1.sequences.first().unwrap();
            let proof1_end = *proof1.sequences.last().unwrap();

            // For binary tree pattern, we want to merge aligned chunks
            // At level L, chunks should start at positions divisible by 2L relative to start_seq
            let relative_start = proof1_start - start_seq;
            let chunk_size = level * 2;

            // Check if this proof is aligned for merging at the next level
            if relative_start % chunk_size != 0 {
                continue; // This proof is not aligned for merging at this level
            }

            // Look for the adjacent proof to merge with
            let expected_next_start = proof1_end + 1;
            let expected_next_end = expected_next_start + level - 1;
            let expected_next_sequences: Vec<u64> = (expected_next_start..=expected_next_end).collect();

            // Find the adjacent proof
            let proof2 = block_proofs.proofs.iter()
                .find(|p| arrays_equal(&p.sequences, &expected_next_sequences));

            if let Some(proof2) = proof2 {
                if !is_proof_available(proof2, current_time) {
                    continue;
                }

                // Check if this merge is excluded
                if excluded.iter().any(|(s1, s2)|
                    (arrays_equal(s1, &proof1.sequences) && arrays_equal(s2, &proof2.sequences))
                ) {
                    continue;
                }

                // Check if the combined proof already exists
                let combined: Vec<u64> = [&proof1.sequences[..], &proof2.sequences[..]].concat();
                let already_exists = block_proofs.proofs.iter()
                    .any(|p| arrays_equal(&p.sequences, &combined) && is_proof_active_or_completed(p, current_time));

                if !already_exists {
                    debug!("Found level {} merge: {:?} + {:?}", level, proof1.sequences, proof2.sequences);
                    return Some(MergeRequest {
                        sequences1: proof1.sequences.clone(),
                        sequences2: proof2.sequences.clone(),
                        block_proof: false,
                    });
                }
            }
        }

        // Move to the next level
        level *= 2;
    }

    debug!("No merge opportunities found for block {}", block_proofs.block_number);
    None
}

#[allow(dead_code)]
pub fn find_proofs_to_merge(block_proofs: &ProofCalculation) -> Option<MergeRequest> {
    find_proofs_to_merge_excluding(block_proofs, &[])
}

/// Find ALL merge opportunities in a ProofCalculation
/// This function returns all possible merges following the binary tree algorithm
pub fn find_all_proofs_to_merge(block_proofs: &ProofCalculation) -> Vec<MergeRequest> {
    let mut merge_opportunities = Vec::new();

    if block_proofs.is_finished {
        return merge_opportunities;
    }

    let current_time = chrono::Utc::now().timestamp() as u64 * 1000;
    let start_seq = block_proofs.start_sequence;

    // CASE 1: end_sequence is known - use deterministic binary split
    if let Some(end_seq) = block_proofs.end_sequence {
        debug!("Block {} has known end_sequence={}, finding all deterministic binary tree merges",
               block_proofs.block_number, end_seq);

        // First, check if we can create the block proof directly
        let full_range: Vec<u64> = (start_seq..=end_seq).collect();
        let block_proof_exists = block_proofs.proofs.iter()
            .any(|p| arrays_equal(&p.sequences, &full_range));

        if !block_proof_exists {
            // Try to create the block proof using binary splits
            if let Some(split) = find_binary_split_point(start_seq, end_seq) {
                let left_range: Vec<u64> = (start_seq..split).collect();
                let right_range: Vec<u64> = (split..=end_seq).collect();

                // Check if both halves exist and are available
                let left_proof = block_proofs.proofs.iter()
                    .find(|p| arrays_equal(&p.sequences, &left_range));
                let right_proof = block_proofs.proofs.iter()
                    .find(|p| arrays_equal(&p.sequences, &right_range));

                if let (Some(left), Some(right)) = (left_proof, right_proof) {
                    if is_proof_available(left, current_time) && is_proof_available(right, current_time) {
                        debug!("Found block proof merge: {:?} + {:?}", left_range, right_range);
                        merge_opportunities.push(MergeRequest {
                            sequences1: left_range,
                            sequences2: right_range,
                            block_proof: true,
                        });
                    }
                }
            }
        }

        // Find ALL intermediate merges needed for target range
        find_all_merges_for_target_range(start_seq, end_seq, block_proofs, current_time, &mut merge_opportunities);
    }

    // CASE 2: end_sequence is unknown - build binary tree from bottom up
    if block_proofs.end_sequence.is_none() {
        debug!("Block {} has unknown end_sequence, finding all binary tree merges from bottom up",
               block_proofs.block_number);

        // Try merging at each level, starting from singles (level 1)
        let mut level = 1u64;
        while level <= 256 { // Reasonable upper bound
            // Find all proofs at this level
            let proofs_at_level: Vec<&Proof> = block_proofs.proofs.iter()
                .filter(|p| p.sequences.len() as u64 == level)
                .collect();

            if proofs_at_level.is_empty() {
                level *= 2;
                continue;
            }

            // Try to find ALL mergeable pairs at this level
            for proof1 in &proofs_at_level {
                if !is_proof_available(proof1, current_time) {
                    continue;
                }

                let proof1_start = *proof1.sequences.first().unwrap();
                let proof1_end = *proof1.sequences.last().unwrap();

                // For binary tree pattern, we want to merge aligned chunks
                let relative_start = proof1_start - start_seq;
                let chunk_size = level * 2;

                // Check if this proof is aligned for merging at the next level
                if relative_start % chunk_size != 0 {
                    continue;
                }

                // Look for the adjacent proof to merge with
                let expected_next_start = proof1_end + 1;
                let expected_next_end = expected_next_start + level - 1;
                let expected_next_sequences: Vec<u64> = (expected_next_start..=expected_next_end).collect();

                // Find the adjacent proof
                let proof2 = block_proofs.proofs.iter()
                    .find(|p| arrays_equal(&p.sequences, &expected_next_sequences));

                if let Some(proof2) = proof2 {
                    if !is_proof_available(proof2, current_time) {
                        continue;
                    }

                    // Check if the combined proof already exists
                    let combined: Vec<u64> = [&proof1.sequences[..], &proof2.sequences[..]].concat();
                    let already_exists = block_proofs.proofs.iter()
                        .any(|p| arrays_equal(&p.sequences, &combined) && is_proof_active_or_completed(p, current_time));

                    if !already_exists {
                        debug!("Found level {} merge: {:?} + {:?}", level, proof1.sequences, proof2.sequences);
                        merge_opportunities.push(MergeRequest {
                            sequences1: proof1.sequences.clone(),
                            sequences2: proof2.sequences.clone(),
                            block_proof: false,
                        });
                    }
                }
            }

            // Move to the next level
            level *= 2;
        }
    }

    debug!("Found {} merge opportunities for block {}", merge_opportunities.len(), block_proofs.block_number);
    merge_opportunities
}

/// Helper function to find all merges needed for a target range
fn find_all_merges_for_target_range(
    start_seq: u64,
    end_seq: u64,
    block_proofs: &ProofCalculation,
    current_time: u64,
    merge_opportunities: &mut Vec<MergeRequest>,
) {
    // Recursively find all binary splits and check for merge opportunities
    fn recursive_find_merges(
        start: u64,
        end: u64,
        block_proofs: &ProofCalculation,
        current_time: u64,
        merge_opportunities: &mut Vec<MergeRequest>,
    ) {
        // BASE CASE: Invalid range
        if start > end {
            return;
        }

        let range_sequences: Vec<u64> = (start..=end).collect();

        // Check if this range already exists
        let range_exists = block_proofs.proofs.iter()
            .any(|p| arrays_equal(&p.sequences, &range_sequences));

        if range_exists {
            return; // This range already exists, no need to merge
        }

        // Try binary split (only if range has at least 2 elements)
        if let Some(split) = find_binary_split_point(start, end) {
            let left_range: Vec<u64> = (start..split).collect();
            let right_range: Vec<u64> = (split..=end).collect();

            // Check if both parts exist
            let left_proof = block_proofs.proofs.iter()
                .find(|p| arrays_equal(&p.sequences, &left_range));
            let right_proof = block_proofs.proofs.iter()
                .find(|p| arrays_equal(&p.sequences, &right_range));

            if let (Some(left), Some(right)) = (left_proof, right_proof) {
                if is_proof_available(left, current_time) && is_proof_available(right, current_time) {
                    // Check if we haven't already added this merge
                    let already_added = merge_opportunities.iter()
                        .any(|m| arrays_equal(&m.sequences1, &left_range) && arrays_equal(&m.sequences2, &right_range));

                    if !already_added {
                        debug!("Found intermediate merge: {:?} + {:?}", left_range, right_range);
                        merge_opportunities.push(MergeRequest {
                            sequences1: left_range.clone(),
                            sequences2: right_range.clone(),
                            block_proof: false,
                        });
                    }
                }
            }

            // Recursively check both halves with proper bounds checking
            // Left half: from start to split-1
            if start < split && split > 0 {
                recursive_find_merges(start, split - 1, block_proofs, current_time, merge_opportunities);
            }
            // Right half: from split to end
            if split <= end {
                recursive_find_merges(split, end, block_proofs, current_time, merge_opportunities);
            }
        }
    }

    recursive_find_merges(start_seq, end_seq, block_proofs, current_time, merge_opportunities);
}

fn arrays_equal(a: &[u64], b: &[u64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}


fn is_proof_available(proof: &Proof, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated => true,
        ProofStatus::Reserved => {
            // Check if timeout has expired (RESERVED proofs can be used if timed out)
            current_time > proof.timestamp + PROOF_RESERVED_TIMEOUT_MS
        }
        ProofStatus::Started => false, // Started proofs are still being calculated, not available for merging
        ProofStatus::Used => {
            // Allow Used proofs only after timeout to prevent excessive intermediate proofs
            // This gives time for the system to find better merge paths first
            current_time > proof.timestamp + PROOF_USED_MERGE_TIMEOUT_MS
        },
        ProofStatus::Rejected => false, // REJECTED proofs are not available for merging
    }
}

fn is_proof_active_or_completed(proof: &Proof, current_time: u64) -> bool {
    match proof.status {
        ProofStatus::Calculated | ProofStatus::Used | ProofStatus::Reserved => true,
        ProofStatus::Started => {
            // Check if it's still within timeout
            current_time <= proof.timestamp + PROOF_RESERVED_TIMEOUT_MS
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
    state: &crate::state::SharedState, // Add state parameter for multicall queue
) -> Result<()> {
    debug!(
        "Attempting to create merge job for block {} with sequences1: {:?}, sequences2: {:?}",
        block_number, sequences1, sequences2
    );

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

                if time_since_start > PROOF_STARTED_TIMEOUT_MS {
                    debug!(
                        "⏰ Started proof has timed out ({} ms > {} ms), will reject",
                        time_since_start, PROOF_STARTED_TIMEOUT_MS
                    );
                    true
                } else {
                    debug!(
                        "⏳ Started proof is still active ({} ms < {} ms), skipping merge",
                        time_since_start, PROOF_STARTED_TIMEOUT_MS
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
                current_time > proof.timestamp + PROOF_RESERVED_TIMEOUT_MS
            }
            ProofStatus::Rejected => true, // Already rejected, can try again
            ProofStatus::Calculated | ProofStatus::Used => false, // Don't reject completed proofs
        };

        if should_reject {
            debug!(
                "📝 Found existing combined proof for block {} sequences {:?} with status {:?}, will be handled by Move contract",
                block_number, combined_sequences, proof.status
            );
            // The Move contract will handle rejection automatically when creating the merge job
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
    let fresh_app_instance = match sui::fetch::app_instance::fetch_app_instance(app_instance).await
    {
        Ok(instance) => instance,
        Err(e) => {
            warn!("Failed to fetch fresh app instance: {}", e);
            return Err(anyhow::anyhow!("Failed to fetch app instance: {}", e));
        }
    };

    let fresh_proof_calc = match sui::fetch::prover::fetch_proof_calculation(
        &fresh_app_instance,
        block_number,
    )
    .await
    {
        Ok(Some(calc)) => calc,
        Ok(None) => {
            debug!(
                "No proof calculation found for block {} - cannot reserve",
                block_number
            );
            return Err(anyhow::anyhow!(
                "No proof calculation found for block {}",
                block_number
            ));
        }
        Err(e) => {
            warn!("Failed to fetch fresh proof calculation: {}", e);
            return Err(anyhow::anyhow!("Failed to fetch proof calculation: {}", e));
        }
    };

    // Check the combined proof status - if it exists, it must be REJECTED to proceed
    let combined_proof = fresh_proof_calc
        .proofs
        .iter()
        .find(|p| arrays_equal(&p.sequences, &combined_sequences));
    if let Some(proof) = combined_proof {
        if proof.status != ProofStatus::Rejected {
            debug!(
                "❌ Combined proof (sequences {:?}) already exists with status {:?} - cannot start",
                combined_sequences, proof.status
            );
            return Err(anyhow::anyhow!(
                "Combined proof already exists with status {:?}",
                proof.status
            ));
        }
        debug!("✅ Combined proof exists but is REJECTED - can proceed");
    } else {
        debug!("✅ Combined proof does not exist yet - can create new");
    }

    // Check that sequence1 and sequence2 proofs exist and are CALCULATED
    let proof1 = fresh_proof_calc
        .proofs
        .iter()
        .find(|p| arrays_equal(&p.sequences, &sequences1));
    let proof2 = fresh_proof_calc
        .proofs
        .iter()
        .find(|p| arrays_equal(&p.sequences, &sequences2));

    let is_block_proof = combined_sequences.len()
        == (fresh_proof_calc.end_sequence.unwrap_or(0) - fresh_proof_calc.start_sequence + 1)
            as usize;

    let can_reserve = match (proof1, proof2) {
        (Some(p1), Some(p2)) => {
            let current_time = chrono::Utc::now().timestamp() as u64 * 1000;

            // Move contract logic:
            // - For block proofs: CALCULATED, USED, or RESERVED (including timed out)
            // - For regular merges: CALCULATED, (RESERVED if timed out), or (USED if timed out)
            let p1_ok = if is_block_proof {
                // Block proofs: CALCULATED, USED, or RESERVED (Move contract allows RESERVED for block proofs)
                p1.status == ProofStatus::Calculated
                    || p1.status == ProofStatus::Used
                    || p1.status == ProofStatus::Reserved
            } else {
                // Regular merges: CALCULATED, (RESERVED if timed out), or (USED if timed out to prevent blocking)
                p1.status == ProofStatus::Calculated
                    || (p1.status == ProofStatus::Reserved
                        && current_time > p1.timestamp + PROOF_RESERVED_TIMEOUT_MS)
                    || (p1.status == ProofStatus::Used
                        && current_time > p1.timestamp + PROOF_USED_MERGE_TIMEOUT_MS)
            };

            let p2_ok = if is_block_proof {
                // Block proofs: CALCULATED, USED, or RESERVED (Move contract allows RESERVED for block proofs)
                p2.status == ProofStatus::Calculated
                    || p2.status == ProofStatus::Used
                    || p2.status == ProofStatus::Reserved
            } else {
                // Regular merges: CALCULATED, (RESERVED if timed out), or (USED if timed out to prevent blocking)
                p2.status == ProofStatus::Calculated
                    || (p2.status == ProofStatus::Reserved
                        && current_time > p2.timestamp + PROOF_RESERVED_TIMEOUT_MS)
                    || (p2.status == ProofStatus::Used
                        && current_time > p2.timestamp + PROOF_USED_MERGE_TIMEOUT_MS)
            };

            if !p1_ok {
                debug!(
                    "❌ Proof1 (sequences {:?}) cannot be reserved - status: {:?}, is_block_proof: {}",
                    sequences1, p1.status, is_block_proof
                );
                if p1.status == ProofStatus::Reserved {
                    let time_since = current_time.saturating_sub(p1.timestamp);
                    debug!(
                        "   Reserved time: {}ms, timeout: {}ms",
                        time_since, PROOF_RESERVED_TIMEOUT_MS
                    );
                }
            }
            if !p2_ok {
                debug!(
                    "❌ Proof2 (sequences {:?}) cannot be reserved - status: {:?}, is_block_proof: {}",
                    sequences2, p2.status, is_block_proof
                );
                if p2.status == ProofStatus::Reserved {
                    let time_since = current_time.saturating_sub(p2.timestamp);
                    debug!(
                        "   Reserved time: {}ms, timeout: {}ms",
                        time_since, PROOF_RESERVED_TIMEOUT_MS
                    );
                }
            }

            p1_ok && p2_ok
        }
        (None, _) => {
            debug!(
                "❌ Proof1 (sequences {:?}) not found in proof calculation",
                sequences1
            );
            false
        }
        (_, None) => {
            debug!(
                "❌ Proof2 (sequences {:?}) not found in proof calculation",
                sequences2
            );
            false
        }
    };

    if !can_reserve {
        debug!(
            "⚠️ Proofs cannot be reserved for merge job {} - skipping transaction",
            job_id
        );
        return Err(anyhow::anyhow!(
            "Proofs not in reservable state for sequences {:?} and {:?}",
            sequences1,
            sequences2
        ));
    }

    // Step 3: Add the merge job creation request to the shared state for multicall processing
    debug!(
        "🔒 Proofs are reservable, adding merge job creation request to multicall queue: {}",
        job_id
    );

    let job_description = Some(format!(
        "Merge proof job for block {} - merging sequences {:?} with {:?}",
        block_number, sequences1, sequences2
    ));

    // Add the request to the shared state for batch processing
    state
        .add_create_merge_job_request(
            app_instance.to_string(),
            crate::state::CreateMergeJobRequest {
                block_number,
                sequences: combined_sequences.clone(),
                sequences1: sequences1.clone(),
                sequences2: sequences2.clone(),
                job_description,
                _timestamp: tokio::time::Instant::now(),
            },
        )
        .await;

    info!(
        "✅ Merge job creation request added to multicall queue: block={}, sequences={:?}+{:?}",
        block_number, sequences1, sequences2
    );

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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::time::interval;

    info!("📦 Starting periodic block creation task (runs every minute)");

    let is_running = Arc::new(AtomicBool::new(false));
    let mut check_interval = interval(Duration::from_secs(BLOCK_CREATION_CHECK_INTERVAL_SECS));
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
            debug!(
                "Checking {} app instances for block creation",
                app_instances.len()
            );

            for app_instance_id in app_instances {
                if state.is_shutting_down() {
                    break;
                }

                // Try to create a block for this app instance
                let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
                match crate::block::try_create_block(&mut sui_interface, &app_instance_id).await {
                    Ok(Some((tx_digest, sequences, time_since))) => {
                        info!(
                            "✅ Created block for app instance {} - tx: {}, sequences: {}, time since last: {}s",
                            app_instance_id, tx_digest, sequences, time_since
                        );
                    }
                    Ok(None) => {
                        debug!("No block needed for app instance {}", app_instance_id);
                    }
                    Err(e) => {
                        error!(
                            "Failed to create block for app instance {}: {}",
                            app_instance_id, e
                        );
                    }
                }
            }
        }

        is_running.store(false, Ordering::Release);
    }
}

/// Start periodic proof analysis task
pub async fn start_periodic_proof_analysis(state: crate::state::SharedState) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::time::interval;

    info!(
        "🔬 Starting periodic proof analysis task (runs every {} minutes)",
        PROOF_ANALYSIS_INTERVAL_SECS / 60
    );

    let is_running = Arc::new(AtomicBool::new(false));
    let mut check_interval = interval(Duration::from_secs(PROOF_ANALYSIS_INTERVAL_SECS));
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
            debug!(
                "Analyzing proof completion for {} app instances",
                app_instances.len()
            );

            let mut analyzed_count = 0;

            for app_instance_id in app_instances {
                if state.is_shutting_down() {
                    break;
                }

                // Fetch the app instance first
                match sui::fetch::fetch_app_instance(&app_instance_id).await {
                    Ok(app_instance) => {
                        // Analyze proof completion for this app instance
                        match crate::proof::analyze_proof_completion(&app_instance, &state).await {
                            Ok(()) => {
                                analyzed_count += 1;
                                debug!(
                                    "Completed proof analysis for app instance {}",
                                    app_instance_id
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to analyze proofs for app instance {}: {}",
                                    app_instance_id, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch app instance {}: {}", app_instance_id, e);
                    }
                }
            }

            if analyzed_count > 0 {
                info!(
                    "✅ Proof analysis complete: {} instances analyzed",
                    analyzed_count
                );
            }
        }

        is_running.store(false, Ordering::Release);
    }
}
