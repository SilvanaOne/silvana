use crate::state::SharedSuiState;
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsResponse;
use tonic::Streaming;
use tracing::info;

/// Create a checkpoint subscription stream using the shared Sui client
pub async fn create_checkpoint_stream() -> anyhow::Result<Streaming<SubscribeCheckpointsResponse>> {
    info!("Creating checkpoint stream from shared Sui state...");

    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut subscription_client = client.subscription_client();

    let mut request = sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsRequest::default();
    request.read_mask = Some(FieldMask::from_paths([
        // "summary.timestamp",
        // "transactions.events.events.package_id",
        // "transactions.events.events.module",
        // "transactions.events.events.sender",
        "transactions.events.events.event_type",
    ]));

    let stream = subscription_client
        .subscribe_checkpoints(request)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe to checkpoints: {}", e))?
        .into_inner();

    Ok(stream)
}
