use sui::fetch::{AppInstance, Job, get_jobs_info_from_app_instance, fetch_job_by_id, fetch_pending_job_sequences_from_app_instance, fetch_pending_jobs_from_app_instance, fetch_block_info};
use sui::fetch::fetch_proof_calculations;
use anyhow::{anyhow, Result};
use sui_rpc::Client;
use tracing::{debug, info, warn, error};

/// Check if there's a settlement opportunity for a given block
pub async fn check_settlement_opportunity(
    app_instance: &AppInstance,
    block_number: u64,
    client: &mut Client,
) -> Result<bool> {
    debug!("Checking settlement opportunity for block {}", block_number);
    
    // Fetch Block details
    let block_details = match fetch_block_info(client, &app_instance.id, block_number).await {
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
    let proof_calculations = match fetch_proof_calculations(client, &app_instance.id, block_number).await {
        Ok(proofs) => proofs,
        Err(e) => {
            warn!("Failed to fetch proof calculations for block {}: {}", block_number, e);
            return Ok(false);
        }
    };
    
    let proof_calc = proof_calculations.first();
    let has_block_proof = proof_calc
        .and_then(|pc| pc.block_proof.as_ref())
        .map(|bp| !bp.is_empty())
        .unwrap_or(false);
    
    // Check settlement opportunities:
    // 1. Proof is available but no settlement transaction
    let proof_available = block_details.proof_data_availability.is_some() || has_block_proof;
    let no_settlement_tx = block_details.settlement_tx_hash.is_none();
    
    if proof_available && no_settlement_tx {
        info!("Settlement opportunity found for block {}: proof available but no settlement tx", block_number);
        return Ok(true);
    }
    
    // 2. Settlement transaction exists but not included in block
    let has_settlement_tx = block_details.settlement_tx_hash.is_some();
    let not_included = !block_details.settlement_tx_included_in_block;
    
    if has_settlement_tx && not_included {
        info!("Settlement opportunity found for block {}: settlement tx exists but not included", block_number);
        return Ok(true);
    }
    
    debug!("No settlement opportunity for block {}", block_number);
    Ok(false)
}

/// Get the settlement job ID for a specific app instance ID
pub async fn get_settlement_job_id_for_instance(
    client: &mut Client,
    app_instance_id: &str,
) -> Result<Option<u64>> {
    debug!("Getting settlement job ID for app instance {}", app_instance_id);
    
    // Fetch the Jobs object to check the settlement_job field
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    // Fetch the AppInstance object
    let request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = match client.ledger_client().get_object(request).await {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            warn!("Failed to fetch AppInstance {}: {}", formatted_id, e);
            return Ok(None);
        }
    };

    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Get the settlement_job field
                        if let Some(settlement_job_field) = jobs_struct.fields.get("settlement_job") {
                            match &settlement_job_field.kind {
                                Some(prost_types::value::Kind::StringValue(s)) => {
                                    if let Ok(job_id) = s.parse::<u64>() {
                                        debug!("Found existing settlement job with ID {}", job_id);
                                        return Ok(Some(job_id));
                                    }
                                }
                                Some(prost_types::value::Kind::NumberValue(n)) => {
                                    let job_id = n.round() as u64;
                                    debug!("Found existing settlement job with ID {}", job_id);
                                    return Ok(Some(job_id));
                                }
                                Some(prost_types::value::Kind::NullValue(_)) => {
                                    debug!("Settlement job field is null");
                                    return Ok(None);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("No settlement job found");
    Ok(None)
}

/// Get the settlement job ID if it exists
pub async fn get_settlement_job_id(
    app_instance: &AppInstance,
    client: &mut Client,
) -> Result<Option<u64>> {
    debug!("Getting settlement job ID for app instance {}", app_instance.id);
    
    // Fetch the Jobs object to check the settlement_job field
    let formatted_id = if app_instance.id.starts_with("0x") {
        app_instance.id.clone()
    } else {
        format!("0x{}", app_instance.id)
    };
    
    // Fetch the AppInstance object
    let request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = match client.ledger_client().get_object(request).await {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            warn!("Failed to fetch AppInstance {}: {}", formatted_id, e);
            return Ok(None);
        }
    };

    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Get the settlement_job field
                        if let Some(settlement_job_field) = jobs_struct.fields.get("settlement_job") {
                            match &settlement_job_field.kind {
                                Some(prost_types::value::Kind::StringValue(s)) => {
                                    if let Ok(job_id) = s.parse::<u64>() {
                                        info!("Found existing settlement job with ID {}", job_id);
                                        return Ok(Some(job_id));
                                    }
                                }
                                Some(prost_types::value::Kind::NumberValue(n)) => {
                                    let job_id = n.round() as u64;
                                    info!("Found existing settlement job with ID {}", job_id);
                                    return Ok(Some(job_id));
                                }
                                Some(prost_types::value::Kind::NullValue(_)) => {
                                    debug!("Settlement job field is null");
                                    return Ok(None);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("No settlement job found");
    Ok(None)
}

/// Create a periodic settle job
pub async fn create_periodic_settle_job(
    app_instance: &AppInstance,
    client: &mut Client,
) -> Result<()> {
    info!("Creating periodic settle job for app instance {}", app_instance.silvana_app_name);
    
    // Create a periodic job with 1 minute interval
    let mut sui_interface = sui::interface::SilvanaSuiInterface::new(client.clone());
    
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
            info!("Successfully created periodic settle job - tx: {}", tx_digest);
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
    client: &mut Client,
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
    
    for app_instance in app_instances {
        // Get Jobs table ID from the AppInstance
        let (_app_instance_id, jobs_table_id) = match get_jobs_info_from_app_instance(client, app_instance).await? {
            Some(info) => info,
            None => {
                warn!("Could not extract Jobs info from app_instance {}", app_instance);
                continue;
            }
        };
        
        // Use the index to get pending job IDs for this method
        let job_sequences = fetch_pending_job_sequences_from_app_instance(
            client,
            app_instance,
            developer,
            agent,
            agent_method,
        ).await?;
        
        // Check if there's a settlement job first
        let settlement_job_id = crate::settlement::get_settlement_job_id_for_instance(client, app_instance).await
            .unwrap_or(None);
        
        // If there's a settlement job and it's in the pending jobs, check it first
        if let Some(settle_id) = settlement_job_id {
            if job_sequences.contains(&settle_id) {
                if let Some(job) = fetch_job_by_id(client, &jobs_table_id, settle_id).await? {
                    // Check if it's ready to run (next_scheduled_at)
                    if job.next_scheduled_at.is_none() || job.next_scheduled_at.unwrap() <= current_time_ms {
                        all_jobs.push((
                            settle_id,
                            app_instance.clone(),
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
        }
        
        // Fetch each job to get its app_instance_method and check if it's ready to run
        for job_sequence in job_sequences {
            // Skip if this is the settlement job we already processed
            if Some(job_sequence) == settlement_job_id {
                continue;
            }
            
            match fetch_job_by_id(client, &jobs_table_id, job_sequence).await? {
                Some(job) => {
                    // Check if job is ready to run (next_scheduled_at)
                    if job.next_scheduled_at.is_none() || job.next_scheduled_at.unwrap() <= current_time_ms {
                        all_jobs.push((
                            job_sequence, 
                            app_instance.clone(), 
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
                None => {
                    warn!("Could not fetch job {} from table {}", job_sequence, jobs_table_id);
                }
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
        info!(
            "Found {} settlement, {} merge, and {} other jobs for {}/{}/{}. Selecting settlement job {} from {}",
            settlement_jobs.len(), merge_jobs.len(), other_jobs.len(), 
            developer, agent, agent_method, job.0, job.1
        );
        job
    } else if !merge_jobs.is_empty() {
        let job = merge_jobs[0];
        info!(
            "Found {} merge jobs and {} other jobs for {}/{}/{}. Selecting merge job {} from {}",
            merge_jobs.len(), other_jobs.len(), developer, agent, agent_method, 
            job.0, job.1
        );
        job
    } else if !other_jobs.is_empty() {
        let job = other_jobs[0];
        info!(
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
    fetch_job_by_id(client, jobs_table_id, *lowest_job_sequence).await
        .map_err(|e| anyhow::anyhow!("Failed to fetch job: {}", e))
}

/// Fetch the pending job with the smallest job_sequence from multiple app_instances
/// Prioritizes: settlement jobs > merge jobs > other jobs
/// Also checks next_scheduled_at to ensure jobs are ready to run
pub async fn fetch_all_pending_jobs(
    client: &mut Client,
    app_instance_ids: &[String],
    only_check: bool,
) -> Result<Option<Job>> {
    // Get current time for checking next_scheduled_at
    let current_time_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let mut all_pending_jobs = Vec::new();
    
    for app_instance_id in app_instance_ids {
        // First check for settlement job if not in check-only mode
        if !only_check {
            if let Ok(Some(settle_job_id)) = crate::settlement::get_settlement_job_id_for_instance(client, app_instance_id).await {
                // Fetch the settlement job to check if it's pending and ready
                if let Ok(Some((_app_instance_id, jobs_table_id))) = get_jobs_info_from_app_instance(client, app_instance_id).await {
                    if let Ok(Some(settle_job)) = fetch_job_by_id(client, &jobs_table_id, settle_job_id).await {
                        // Check if it's pending and ready to run
                        if matches!(settle_job.status, sui::fetch::JobStatus::Pending) &&
                           (settle_job.next_scheduled_at.is_none() || settle_job.next_scheduled_at.unwrap() <= current_time_ms) {
                            debug!("Found pending settlement job {} in app_instance {}", settle_job.job_sequence, app_instance_id);
                            all_pending_jobs.push((settle_job, true)); // true = is_settlement
                        }
                    }
                }
            }
        }
        
        // Fetch regular pending jobs
        match fetch_pending_jobs_from_app_instance(client, app_instance_id, only_check).await {
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
                error!("Failed to fetch pending job from app_instance {}: {}", app_instance_id, e);
            }
        }
    }
    
    // Sort all collected jobs by priority: settlement > merge > others
    if all_pending_jobs.is_empty() {
        if !only_check {
            debug!("No pending jobs found across all app_instances");
        }
        Ok(None)
    } else {
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
        
        // Return the highest priority job
        let selected_job = if !settlement_jobs.is_empty() {
            info!("Returning settlement job with job_sequence: {}", settlement_jobs[0].job_sequence);
            settlement_jobs.into_iter().next().unwrap()
        } else if !merge_jobs.is_empty() {
            info!("Returning merge job with job_sequence: {}", merge_jobs[0].job_sequence);
            merge_jobs.into_iter().next().unwrap()
        } else {
            info!("Returning other job with job_sequence: {}", other_jobs[0].job_sequence);
            other_jobs.into_iter().next().unwrap()
        };
        
        Ok(Some(selected_job))
    }
}