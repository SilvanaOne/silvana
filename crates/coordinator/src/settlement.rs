use sui::fetch::{AppInstance, Job, get_jobs_info_from_app_instance, fetch_job_by_id, fetch_jobs_batch, fetch_pending_job_sequences_from_app_instance, fetch_pending_jobs_from_app_instance, fetch_block_info, fetch_blocks_range};
use sui::fetch::{fetch_proof_calculation, fetch_proof_calculations_range};
use anyhow::{anyhow, Result};
use tracing::{debug, warn, error};

/// Check if an app_instance can be safely removed from tracking
/// An app_instance can only be removed when:
/// 1. All proved blocks are settled (last_settled_block_number == last_proved_block_number)
/// 2. We're at the start of the next block (last_proved_block_number + 1 == current_block_number)
/// 3. No sequences processed in current block (previous_block_last_sequence + 1 == current_sequence)
/// 4. No pending jobs
pub fn can_remove_app_instance(app_instance: &AppInstance) -> bool {
    // Check if all conditions are met
    let all_blocks_settled = app_instance.last_settled_block_number == app_instance.last_proved_block_number;
    let at_block_start = app_instance.last_proved_block_number + 1 == app_instance.block_number;
    let no_sequences_in_current = app_instance.previous_block_last_sequence + 1 == app_instance.sequence;
    let no_pending_jobs = app_instance.jobs.as_ref()
        .map(|jobs| jobs.pending_jobs_count == 0)
        .unwrap_or(true);
    
    if all_blocks_settled && at_block_start && no_sequences_in_current && no_pending_jobs {
        debug!(
            "App instance {} can be removed: settled={}, proved={}, current_block={}, prev_seq={}, current_seq={}, pending_jobs=0",
            app_instance.id,
            app_instance.last_settled_block_number,
            app_instance.last_proved_block_number,
            app_instance.block_number,
            app_instance.previous_block_last_sequence,
            app_instance.sequence
        );
        true
    } else {
        false
    }
}

/// Check if there's a settlement opportunity for a given block (legacy single-block function)
#[allow(dead_code)]
pub async fn check_settlement_opportunity(
    app_instance: &AppInstance,
    block_number: u64,
) -> Result<bool> {
    debug!("Checking settlement opportunity for block {}", block_number);
    
    // Fetch Block details
    let block_details = match fetch_block_info(app_instance, block_number).await {
        Ok(Some(block)) => block,
        Ok(None) => {
            debug!("No block found for block number {}", block_number);
            return Ok(false);
        }
        Err(e) => {
            warn!("Failed to fetch block {}: {}", block_number, e);
            return Ok(false);
        }
    };
    
    // Fetch ProofCalculation to check for block_proof
    let proof_calc = match fetch_proof_calculation(app_instance, block_number).await {
        Ok(proof_calc) => proof_calc,
        Err(e) => {
            warn!("Failed to fetch proof calculation for block {}: {}", block_number, e);
            return Ok(false);
        }
    };
    
    let has_block_proof = proof_calc
        .as_ref()
        .and_then(|pc| pc.block_proof.as_ref())
        .map(|bp| !bp.is_empty())
        .unwrap_or(false);
    
    // Check settlement opportunities:
    // 1. Proof is available but no settlement transaction
    let proof_available = block_details.proof_data_availability.is_some() || has_block_proof;
    let no_settlement_tx = block_details.settlement_tx_hash.is_none();
    
    if proof_available && no_settlement_tx {
        debug!("Settlement opportunity found for block {}: proof available but no settlement tx", block_number);
        return Ok(true);
    }
    
    // 2. Settlement transaction exists but not included in block
    let has_settlement_tx = block_details.settlement_tx_hash.is_some();
    let not_included = !block_details.settlement_tx_included_in_block;
    
    if has_settlement_tx && not_included {
        debug!("Settlement opportunity found for block {}: settlement tx exists but not included", block_number);
        return Ok(true);
    }
    
    debug!("No settlement opportunity for block {}", block_number);
    Ok(false)
}

/// Check for settlement opportunities across a range of blocks using batch fetching
/// Returns a vector of block numbers that have settlement opportunities
#[allow(dead_code)]
pub async fn check_settlement_opportunities_range(
    app_instance: &AppInstance,
    start_block: u64,
    end_block: u64,
) -> Result<Vec<u64>> {
    debug!("Checking settlement opportunities for blocks {} to {}", start_block, end_block);
    
    // Fetch all blocks and proof calculations in the range with single iterations
    let blocks_map = match fetch_blocks_range(app_instance, start_block, end_block).await {
        Ok(blocks) => blocks,
        Err(e) => {
            warn!("Failed to fetch blocks range: {}", e);
            return Ok(Vec::new());
        }
    };
    
    let proofs_map = match fetch_proof_calculations_range(app_instance, start_block, end_block).await {
        Ok(proofs) => proofs,
        Err(e) => {
            warn!("Failed to fetch proof calculations range: {}", e);
            return Ok(Vec::new());
        }
    };
    
    let mut opportunities = Vec::new();
    
    // Check each block for settlement opportunities
    for block_number in start_block..=end_block {
        if let Some(block_details) = blocks_map.get(&block_number) {
            let proof_calc = proofs_map.get(&block_number);
            
            let has_block_proof = proof_calc
                .and_then(|pc| pc.block_proof.as_ref())
                .map(|bp| !bp.is_empty())
                .unwrap_or(false);
            
            // Check settlement opportunities:
            // 1. Proof is available but no settlement transaction
            let proof_available = block_details.proof_data_availability.is_some() || has_block_proof;
            let no_settlement_tx = block_details.settlement_tx_hash.is_none();
            
            if proof_available && no_settlement_tx {
                debug!("Settlement opportunity found for block {}: proof available but no settlement tx", block_number);
                opportunities.push(block_number);
                continue;
            }
            
            // 2. Settlement transaction exists but not included in block
            let has_settlement_tx = block_details.settlement_tx_hash.is_some();
            let not_included = !block_details.settlement_tx_included_in_block;
            
            if has_settlement_tx && not_included {
                debug!("Settlement opportunity found for block {}: settlement tx exists but not included", block_number);
                opportunities.push(block_number);
            }
        }
    }
    
    if opportunities.is_empty() {
        debug!("No settlement opportunities found in range {}-{}", start_block, end_block);
    } else {
        debug!("Found {} settlement opportunities in range {}-{}", opportunities.len(), start_block, end_block);
    }
    
    Ok(opportunities)
}


/// Create a periodic settle job
pub async fn create_periodic_settle_job(
    app_instance: &AppInstance,
) -> Result<()> {
    debug!("Creating periodic settle job for app instance {}", app_instance.silvana_app_name);
    
    // Create a periodic job with 1 minute interval
    let mut sui_interface = sui::interface::SilvanaSuiInterface::new();
    
    // Create job description data
    let job_description = "Periodic settlement check".to_string();
    
    // For periodic jobs, we need to encode the job data properly
    // The data should contain the interval and next run time
    let interval_ms: u64 = 60000; // 1 minute
    let next_run_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    // Create job data - for settle jobs, we typically include:
    // - interval for periodic execution
    // - next run time
    let mut job_data = Vec::new();
    job_data.extend_from_slice(&interval_ms.to_le_bytes());
    job_data.extend_from_slice(&next_run_time.to_le_bytes());
    
    match sui_interface.create_app_job(
        &app_instance.id,
        "settle".to_string(),
        Some(job_description),
        None, // No specific block number
        None, // No specific sequences
        None, // No sequences1
        None, // No sequences2
        job_data,
        Some(interval_ms),
        Some(next_run_time),
        true, // This is a settlement job
    ).await {
        Ok(tx_digest) => {
            debug!("Successfully created periodic settle job - tx: {}", tx_digest);
            Ok(())
        }
        Err(e) => {
            error!("Failed to create periodic settle job: {}", e);
            Err(anyhow!("Failed to create settle job: {}", e))
        }
    }
}


/// Try to fetch a pending job from any of the given app_instances using the index
pub async fn fetch_pending_job_from_instances(
    app_instances: &[String],
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Option<Job>> {
    // Get current time for checking next_scheduled_at
    let current_time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    // Collect all job IDs from all app_instances with their app_instance_method and next_scheduled_at
    let mut all_jobs: Vec<(u64, String, String, String, Option<u64>, bool)> = Vec::new(); 
    // (job_sequence, app_instance_id, jobs_table_id, app_instance_method, next_scheduled_at, is_settlement_job)
    
    for app_instance_id in app_instances {
        // First fetch the AppInstance object
        let app_instance = match sui::fetch::fetch_app_instance(app_instance_id).await {
            Ok(app_inst) => app_inst,
            Err(e) => {
                warn!("Could not fetch AppInstance {}: {}", app_instance_id, e);
                continue;
            }
        };
        
        // Get Jobs table ID from the AppInstance
        let (_app_instance_id, jobs_table_id) = match get_jobs_info_from_app_instance(&app_instance).await? {
            Some(info) => info,
            None => {
                warn!("Could not extract Jobs info from app_instance {}", app_instance_id);
                continue;
            }
        };
        
        // Use the index to get pending job IDs for this method
        let job_sequences = fetch_pending_job_sequences_from_app_instance(
            &app_instance,
            developer,
            agent,
            agent_method,
        ).await?;
        
        if job_sequences.is_empty() {
            continue;
        }
        
        // Check if there's a settlement job first
        let settlement_job_id = sui::fetch::app_instance::get_settlement_job_id_for_instance(&app_instance).await
            .unwrap_or(None);
        
        // Batch fetch all jobs at once
        let jobs_map = fetch_jobs_batch(&jobs_table_id, &job_sequences).await?;
        
        // Process settlement job if it exists and is in the pending jobs
        if let Some(settle_id) = settlement_job_id {
            if let Some(job) = jobs_map.get(&settle_id) {
                // Check if it's ready to run (next_scheduled_at)
                if job.next_scheduled_at.is_none() || job.next_scheduled_at.unwrap() <= current_time_ms {
                    all_jobs.push((
                        settle_id,
                        app_instance_id.clone(),
                        jobs_table_id.clone(),
                        job.app_instance_method.clone(),
                        job.next_scheduled_at,
                        true // is_settlement_job
                    ));
                } else {
                    debug!("Settlement job {} not ready yet, scheduled for {}", 
                        settle_id, job.next_scheduled_at.unwrap());
                }
            }
        }
        
        // Process all other jobs
        for (job_sequence, job) in jobs_map.iter() {
            // Skip if this is the settlement job we already processed
            if Some(*job_sequence) == settlement_job_id {
                continue;
            }
            
            // Check if job is ready to run (next_scheduled_at)
            if job.next_scheduled_at.is_none() || job.next_scheduled_at.unwrap() <= current_time_ms {
                all_jobs.push((
                    *job_sequence, 
                    app_instance_id.clone(), 
                    jobs_table_id.clone(),
                    job.app_instance_method.clone(),
                    job.next_scheduled_at,
                    false // not a settlement job
                ));
            } else {
                debug!("Job {} not ready yet, scheduled for {}", 
                    job_sequence, job.next_scheduled_at.unwrap());
            }
        }
    }
    
    if all_jobs.is_empty() {
        debug!("No pending jobs found in any app_instance for {}/{}/{}", developer, agent, agent_method);
        return Ok(None);
    }
    
    // Separate jobs by priority: settlement > merge > others
    let mut settlement_jobs: Vec<_> = all_jobs.iter()
        .filter(|(_, _, _, _, _, is_settlement)| *is_settlement)
        .collect();
    let mut merge_jobs: Vec<_> = all_jobs.iter()
        .filter(|(_, _, _, method, _, is_settlement)| !is_settlement && method == "merge")
        .collect();
    let mut other_jobs: Vec<_> = all_jobs.iter()
        .filter(|(_, _, _, method, _, is_settlement)| !is_settlement && method != "merge")
        .collect();
    
    // Sort all lists by job_sequence
    settlement_jobs.sort_by_key(|(job_seq, _, _, _, _, _)| *job_seq);
    merge_jobs.sort_by_key(|(job_seq, _, _, _, _, _)| *job_seq);
    other_jobs.sort_by_key(|(job_seq, _, _, _, _, _)| *job_seq);
    
    // Prioritize: settlement > merge > others
    let selected_job = if !settlement_jobs.is_empty() {
        let job = settlement_jobs[0];
        debug!(
            "Found {} settlement, {} merge, and {} other jobs for {}/{}/{}. Selecting settlement job {} from {}",
            settlement_jobs.len(), merge_jobs.len(), other_jobs.len(), 
            developer, agent, agent_method, job.0, job.1
        );
        job
    } else if !merge_jobs.is_empty() {
        let job = merge_jobs[0];
        debug!(
            "Found {} merge jobs and {} other jobs for {}/{}/{}. Selecting merge job {} from {}",
            merge_jobs.len(), other_jobs.len(), developer, agent, agent_method, 
            job.0, job.1
        );
        job
    } else if !other_jobs.is_empty() {
        let job = other_jobs[0];
        debug!(
            "Found {} other jobs for {}/{}/{}. Selecting job {} from {}",
            other_jobs.len(), developer, agent, agent_method, 
            job.0, job.1
        );
        job
    } else {
        // This shouldn't happen since we checked all_jobs.is_empty() above
        return Ok(None);
    };
    
    let (lowest_job_sequence, _app_instance, jobs_table_id, _app_instance_method, _next_scheduled_at, _is_settlement) = selected_job;
    
    // Fetch the specific job by ID
    fetch_job_by_id(jobs_table_id, *lowest_job_sequence).await
        .map_err(|e| anyhow::anyhow!("Failed to fetch job: {}", e))
}

/// Fetch all pending jobs from multiple app_instances, sorted by priority
/// Priority order: settlement jobs > merge jobs > other jobs
/// Within each category, jobs are sorted by job_sequence
/// Also checks next_scheduled_at to ensure jobs are ready to run
/// Returns a vector of jobs to allow the caller to skip failed ones
pub async fn fetch_all_pending_jobs(
    app_instance_ids: &[String],
    only_check: bool,
) -> Result<Vec<Job>> {
    // Get current time for checking next_scheduled_at
    let current_time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let mut all_pending_jobs = Vec::new();
    
    debug!("fetch_all_pending_jobs called with {} app_instance_ids, only_check={}", 
        app_instance_ids.len(), only_check);
    
    for app_instance_id in app_instance_ids {
        // First fetch the AppInstance object
        let app_instance = match sui::fetch::fetch_app_instance(app_instance_id).await {
            Ok(app_inst) => {
                debug!("Successfully fetched AppInstance {}", app_instance_id);
                if let Some(jobs) = &app_inst.jobs {
                    debug!("AppInstance {} has Jobs with {} pending jobs", 
                        app_instance_id, jobs.pending_jobs_count);
                }
                app_inst
            },
            Err(e) => {
                error!("Failed to fetch AppInstance {}: {}", app_instance_id, e);
                continue;
            }
        };
        
        // First check for settlement job if not in check-only mode
        if !only_check {
            if let Ok(Some(settle_job_id)) = sui::fetch::app_instance::get_settlement_job_id_for_instance(&app_instance).await {
                // Fetch the settlement job to check if it's pending and ready
                if let Ok(Some((_app_instance_id, jobs_table_id))) = get_jobs_info_from_app_instance(&app_instance).await {
                    match fetch_job_by_id(&jobs_table_id, settle_job_id).await {
                        Ok(Some(settle_job)) => {
                            // Check if it's pending and ready to run
                            if matches!(settle_job.status, sui::fetch::JobStatus::Pending) {
                                if settle_job.next_scheduled_at.is_none() || settle_job.next_scheduled_at.unwrap() <= current_time_ms {
                                    debug!("Found pending settlement job {} in app_instance {}", settle_job.job_sequence, app_instance_id);
                                    all_pending_jobs.push((settle_job, true)); // true = is_settlement
                                } else {
                                    debug!("Settlement job {} is pending but not ready yet (scheduled for {:?})", 
                                           settle_job.job_sequence, settle_job.next_scheduled_at);
                                }
                            } else {
                                debug!("Settlement job {} exists but status is {:?}, not Pending", 
                                       settle_job.job_sequence, settle_job.status);
                            }
                        }
                        Ok(None) => {
                            debug!("Settlement job ID {} not found in jobs table", settle_job_id);
                        }
                        Err(e) => {
                            debug!("Failed to fetch settlement job {}: {}", settle_job_id, e);
                        }
                    }
                }
            }
        }
        
        // Fetch regular pending jobs
        match fetch_pending_jobs_from_app_instance(&app_instance, only_check).await {
            Ok(job_opt) => {
                if !only_check {
                    if let Some(job) = job_opt {
                        // Check if job is ready to run (next_scheduled_at)
                        if job.next_scheduled_at.is_none() || job.next_scheduled_at.unwrap() <= current_time_ms {
                            debug!("Found pending job with job_sequence {} in app_instance {}", job.job_sequence, app_instance_id);
                            // Check if it's a merge job or other
                            let is_settlement = false; // We handled settlement jobs separately above
                            all_pending_jobs.push((job, is_settlement));
                        } else {
                            debug!("Job {} not ready yet, scheduled for {}", 
                                job.job_sequence, job.next_scheduled_at.unwrap());
                        }
                    }
                }
            }
            Err(e) => {
                // This can happen if a job was terminated between listing and fetching
                if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                    debug!("Job was terminated/deleted before fetch from app_instance {}: {}", app_instance_id, e);
                } else {
                    error!("Failed to fetch pending job from app_instance {}: {}", app_instance_id, e);
                }
            }
        }
    }
    
    // Sort all collected jobs by priority: settlement > merge > others
    if all_pending_jobs.is_empty() {
        if !only_check {
            debug!("No pending jobs found across all app_instances");
        }
        Ok(Vec::new())
    } else {
        debug!("Found {} total pending jobs across all app_instances", all_pending_jobs.len());
        // Separate jobs by type
        let mut settlement_jobs: Vec<_> = all_pending_jobs.iter()
            .filter(|(_, is_settlement)| *is_settlement)
            .map(|(job, _)| job.clone())
            .collect();
        let mut merge_jobs: Vec<_> = all_pending_jobs.iter()
            .filter(|(job, is_settlement)| !is_settlement && job.app_instance_method == "merge")
            .map(|(job, _)| job.clone())
            .collect();
        let mut other_jobs: Vec<_> = all_pending_jobs.iter()
            .filter(|(job, is_settlement)| !is_settlement && job.app_instance_method != "merge")
            .map(|(job, _)| job.clone())
            .collect();
        
        // Sort each category by job_sequence
        settlement_jobs.sort_by_key(|job| job.job_sequence);
        merge_jobs.sort_by_key(|job| job.job_sequence);
        other_jobs.sort_by_key(|job| job.job_sequence);
        
        // Combine all jobs in priority order
        let mut result = Vec::new();
        result.extend(settlement_jobs);
        result.extend(merge_jobs);
        result.extend(other_jobs);
        
        if !result.is_empty() {
            debug!("Found {} pending jobs total, highest priority is job_sequence: {}", 
                   result.len(), result[0].job_sequence);
        }
        
        Ok(result)
    }
}