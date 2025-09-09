use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use sui::interface::SilvanaSuiInterface;
use tracing::{debug, error};

/// Try to create a new block for the app instance
/// This function checks if the conditions are met to create a new block:
/// 1. No new sequences pending (sequence != previous_block_last_sequence + 1)
/// 2. Sufficient time has passed since the last block (current_time - previous_block_timestamp > app_instance.min_time_between_blocks)
pub async fn try_create_block(
    sui_interface: &mut SilvanaSuiInterface,
    app_instance_id: &str,
) -> Result<Option<(String, u64, u64)>> {
    // Returns Some((tx_digest, new_sequences_count, time_since_last_block)) on success
    debug!(
        "Checking if we should create a new block for app_instance {}",
        app_instance_id
    );

    // Use the fetch_app_instance function from sui crate
    let app_instance = sui::fetch::fetch_app_instance(app_instance_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch AppInstance {}: {}", app_instance_id, e))?;

    // Get current time in milliseconds
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Check conditions:
    // 1. There are new sequences not in blocks (sequence != previous_block_last_sequence + 1)
    // 2. Sufficient time has passed since last block
    let has_new_sequences = app_instance.sequence != app_instance.previous_block_last_sequence + 1;
    let time_since_last_block = current_time.saturating_sub(app_instance.previous_block_timestamp);
    let sufficient_time_passed = time_since_last_block > app_instance.min_time_between_blocks;

    debug!(
        "Block creation check for {}: sequence={}, prev_block_last_seq={}, prev_block_timestamp={}, current_time={}, time_since_last={}, has_new_sequences={}, sufficient_time={}",
        app_instance_id,
        app_instance.sequence,
        app_instance.previous_block_last_sequence,
        app_instance.previous_block_timestamp,
        current_time,
        time_since_last_block,
        has_new_sequences,
        sufficient_time_passed
    );

    // Both conditions must be met to create a block
    if has_new_sequences && sufficient_time_passed {
        debug!(
            "Conditions met for creating block in app_instance {}: {} new sequences, {}ms since last block",
            app_instance_id,
            app_instance.sequence - app_instance.previous_block_last_sequence - 1,
            time_since_last_block
        );

        // Call the blockchain to try creating the block
        match sui_interface.try_create_block(app_instance_id).await {
            Ok(tx_digest) => {
                debug!(
                    "Successfully created block for app_instance {}, tx: {}",
                    app_instance_id, tx_digest
                );
                let new_sequences =
                    app_instance.sequence - app_instance.previous_block_last_sequence - 1;
                Ok(Some((tx_digest, new_sequences, time_since_last_block)))
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("NonEntryFunctionInvoked")
                    || error_str.contains("not an entry function")
                {
                    // Critical error - function not found
                    error!(
                        "CRITICAL: try_create_block function not found in Move contract: {}",
                        error_str
                    );
                    Err(anyhow::anyhow!(
                        "try_create_block function not found in Move contract: {}",
                        error_str
                    ))
                } else {
                    // Expected failures (conditions not met, another coordinator created it, etc.)
                    debug!(
                        "Block not created for app_instance {} (expected): {}",
                        app_instance_id, error_str
                    );
                    Ok(None)
                }
            }
        }
    } else {
        debug!(
            "Conditions not met for creating block in app_instance {}: has_new_sequences={}, sufficient_time_passed={}",
            app_instance_id, has_new_sequences, sufficient_time_passed
        );
        Ok(None)
    }
}
