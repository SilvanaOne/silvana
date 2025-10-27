use crate::error::{Result, SilvanaSuiInterfaceError};
use crate::state::SharedSuiState;
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2::{
    GetCheckpointRequest, GetTransactionRequest
};
use tracing::debug;

/// Fetches a specific event with its contents from a checkpoint
/// 
/// This function retrieves an event by first fetching the checkpoint to get the transaction digest,
/// then fetching the transaction to get the specific event with its contents.
/// 
/// # Arguments
/// * `checkpoint_seq` - The checkpoint sequence number
/// * `tx_index` - The index of the transaction within the checkpoint
/// * `event_index` - The index of the event within the transaction
/// 
/// # Returns
/// * `Ok(Some(Event))` - The event with its contents if found
/// * `Ok(None)` - If the transaction or event doesn't exist at the specified indices
/// * `Err` - If there's an RPC error
pub async fn fetch_event_with_contents(
    checkpoint_seq: u64,
    tx_index: usize,
    event_index: usize,
) -> Result<Option<sui_rpc::proto::sui::rpc::v2::Event>> {
    debug!(
        "Fetching event with contents from checkpoint {} (tx_index: {}, event_index: {})",
        checkpoint_seq, tx_index, event_index
    );

    // Get the shared Sui client
    let mut client = SharedSuiState::get_instance().get_sui_client();

    // First, we need to get the transaction digest from the checkpoint
    // We'll fetch all transaction digests and select the one we need
    let checkpoint_request = GetCheckpointRequest {
        checkpoint_id: Some(sui_rpc::proto::sui::rpc::v2::get_checkpoint_request::CheckpointId::SequenceNumber(checkpoint_seq)),
        read_mask: Some(FieldMask::from_paths([
            "transactions.digest",
        ])),
    };

    let checkpoint_response = client
        .ledger_client()
        .get_checkpoint(checkpoint_request)
        .await
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch checkpoint: {}", e)))?;

    // Get the transaction digest
    let tx_digest = if let Some(checkpoint) = checkpoint_response.into_inner().checkpoint {
        if let Some(transaction) = checkpoint.transactions.get(tx_index) {
            transaction.digest.clone()
        } else {
            debug!("Transaction at index {} not found in checkpoint", tx_index);
            return Ok(None);
        }
    } else {
        debug!("Checkpoint {} not found", checkpoint_seq);
        return Ok(None);
    };

    // Now fetch just the transaction with event contents
    if let Some(digest) = tx_digest {
        let tx_request = GetTransactionRequest {
            digest: Some(digest.clone()),
            read_mask: Some(FieldMask::from_paths([
                "events.events.package_id",
                "events.events.module",
                "events.events.sender",
                "events.events.event_type",
                "events.events.contents",
            ])),
        };

        let tx_response = client
            .ledger_client()
            .get_transaction(tx_request)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(format!("Failed to fetch transaction: {}", e)))?;

        // Extract the specific event from the transaction
        if let Some(transaction) = tx_response.into_inner().transaction {
            if let Some(tx_events) = transaction.events {
                if let Some(event) = tx_events.events.get(event_index) {
                    debug!("Successfully fetched event at index {}", event_index);
                    return Ok(Some(event.clone()));
                } else {
                    debug!("Event at index {} not found in transaction", event_index);
                }
            }
        }
    }

    Ok(None)
}