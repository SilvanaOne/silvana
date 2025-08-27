use crate::fetch::app_instance::AppInstance;
use crate::fetch::blocks::fetch_block_info;
use crate::fetch::proofs::fetch_proof_calculations;
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
    let mut sui_interface = crate::sui_interface::SuiJobInterface::new(client.clone());
    
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