use crate::coordination_manager::CoordinationManager;
use anyhow::Result;
use silvana_coordination_trait::Coordination;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug};

/// Try to create a new block for the app instance using coordination manager
/// This function checks if the conditions are met to create a new block:
/// 1. No new sequences pending (sequence != previous_block_last_sequence + 1)
/// 2. Sufficient time has passed since the last block (current_time - previous_block_timestamp > app_instance.min_time_between_blocks)
pub async fn try_create_block(
    manager: &Arc<CoordinationManager>,
    app_instance_id: &str,
) -> Result<Option<(String, u64, u64)>> {
    // Returns Some((tx_digest, new_sequences_count, time_since_last_block)) on success
    debug!(
        "Checking if we should create a new block for app_instance {}",
        app_instance_id
    );

    // Get the correct coordination layer for this app instance
    let (_layer_id, coordination) = manager.get_layer_for_app(app_instance_id).await?;

    // Use the coordination layer to fetch the app instance
    let app_instance = coordination.fetch_app_instance(app_instance_id)
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

        // Call the coordination layer to try creating the block
        match coordination.try_create_block(app_instance_id).await {
            Ok(Some(new_block_number)) => {
                debug!(
                    "Successfully created block {} for app_instance {}",
                    new_block_number, app_instance_id
                );
                let new_sequences =
                    app_instance.sequence - app_instance.previous_block_last_sequence - 1;
                // Return block number as string for backward compatibility
                Ok(Some((new_block_number.to_string(), new_sequences, time_since_last_block)))
            }
            Ok(None) => {
                debug!(
                    "Block not created for app_instance {} (conditions not met or already created)",
                    app_instance_id
                );
                Ok(None)
            }
            Err(e) => {
                // Expected failures (conditions not met, another coordinator created it, etc.)
                debug!(
                    "Block not created for app_instance {} (expected): {}",
                    app_instance_id, e
                );
                Ok(None)
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
