use crate::coordination::ProofCalculation;
use crate::sui_interface::SuiJobInterface;
use anyhow::Result;
use sui_rpc::Client;
use tracing::{info, debug, error};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn settle(proof_calc: ProofCalculation, da_hash: String) -> Result<()> {
    info!(
        "Settling complete block {} with {} sequences: {} (da_hash: {})",
        proof_calc.block_number,
        proof_calc.sequences.len(),
        proof_calc.sequences.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "),
        da_hash
    );

    // This function is called only when we have a complete block proof
    // TODO: Implement actual block settlement logic:
    // 1. Update the Block struct with proof_data_availability = da_hash
    // 2. Set proved_at timestamp
    // 3. Create settlement transaction if needed
    // 4. Emit settlement events

    info!("Block {} settlement complete - proof recorded", proof_calc.block_number);
    
    Ok(())
}

/// Constants from the Move contract
const MIN_TIME_BETWEEN_BLOCKS: u64 = 60000; // 60 seconds in milliseconds

/// Try to create a new block for the app instance
/// This function checks if the conditions are met to create a new block:
/// 1. No new sequences pending (sequence != previous_block_last_sequence + 1)
/// 2. Sufficient time has passed since the last block (current_time - previous_block_timestamp > MIN_TIME_BETWEEN_BLOCKS)
pub async fn try_create_block(
    client: &mut Client,
    sui_interface: &mut SuiJobInterface,
    app_instance_id: &str,
) -> Result<bool> {
    use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
    use crate::error::CoordinatorError;
    
    debug!("Checking if we should create a new block for app_instance {}", app_instance_id);
    
    // Format the app_instance ID
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    // Fetch the AppInstance object to get current state
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };
    
    let response = client.ledger_client().get_object(request).await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch AppInstance {}: {}", app_instance_id, e)
        ))?;
    
    let object = response.into_inner().object
        .ok_or_else(|| CoordinatorError::RpcConnectionError(
            format!("AppInstance not found: {}", app_instance_id)
        ))?;
    
    let json_value = object.json
        .ok_or_else(|| CoordinatorError::RpcConnectionError(
            format!("No JSON data for AppInstance: {}", app_instance_id)
        ))?;
    
    // Extract fields from AppInstance
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        let mut sequence = 0u64;
        let mut previous_block_last_sequence = 0u64;
        let mut previous_block_timestamp = 0u64;
        
        // Extract sequence
        if let Some(seq_field) = struct_value.fields.get("sequence") {
            if let Some(prost_types::value::Kind::StringValue(seq_str)) = &seq_field.kind {
                sequence = seq_str.parse().unwrap_or(0);
            }
        }
        
        // Extract previous_block_last_sequence
        if let Some(prev_seq_field) = struct_value.fields.get("previous_block_last_sequence") {
            if let Some(prost_types::value::Kind::StringValue(prev_seq_str)) = &prev_seq_field.kind {
                previous_block_last_sequence = prev_seq_str.parse().unwrap_or(0);
            }
        }
        
        // Extract previous_block_timestamp
        if let Some(timestamp_field) = struct_value.fields.get("previous_block_timestamp") {
            if let Some(prost_types::value::Kind::StringValue(timestamp_str)) = &timestamp_field.kind {
                previous_block_timestamp = timestamp_str.parse().unwrap_or(0);
            }
        }
        
        // Get current time in milliseconds
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        // Check conditions:
        // 1. There are new sequences not in blocks (sequence != previous_block_last_sequence + 1)
        // 2. Sufficient time has passed since last block
        let has_new_sequences = sequence != previous_block_last_sequence + 1;
        let time_since_last_block = current_time.saturating_sub(previous_block_timestamp);
        let sufficient_time_passed = time_since_last_block > MIN_TIME_BETWEEN_BLOCKS;
        
        debug!(
            "Block creation check for {}: sequence={}, prev_block_last_seq={}, prev_block_timestamp={}, current_time={}, time_since_last={}, has_new_sequences={}, sufficient_time={}",
            app_instance_id, sequence, previous_block_last_sequence, previous_block_timestamp, 
            current_time, time_since_last_block, has_new_sequences, sufficient_time_passed
        );
        
        // Both conditions must be met to create a block
        if has_new_sequences && sufficient_time_passed {
            info!(
                "Conditions met for creating block in app_instance {}: {} new sequences, {}ms since last block",
                app_instance_id, 
                sequence - previous_block_last_sequence - 1,
                time_since_last_block
            );
            
            // Call the blockchain to try creating the block
            match sui_interface.try_create_block(app_instance_id).await {
                Ok(tx_digest) => {
                    info!("Successfully created block for app_instance {}, tx: {}", app_instance_id, tx_digest);
                    Ok(true)
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
                        Ok(false)
                    }
                }
            }
        } else {
            debug!(
                "Conditions not met for creating block in app_instance {}: has_new_sequences={}, sufficient_time_passed={}",
                app_instance_id, has_new_sequences, sufficient_time_passed
            );
            Ok(false)
        }
    } else {
        Err(CoordinatorError::RpcConnectionError(
            format!("Invalid JSON structure for AppInstance: {}", app_instance_id)
        ).into())
    }
}