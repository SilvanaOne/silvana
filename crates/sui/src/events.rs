use crate::state::SharedSuiState;
use sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsResponse;
use tonic::Streaming;
use tracing::info;

/// Create a checkpoint subscription stream using the shared Sui client
pub async fn create_checkpoint_stream() -> anyhow::Result<Streaming<SubscribeCheckpointsResponse>> {
    info!("Creating checkpoint stream from shared Sui state...");

    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut subscription_client = client.subscription_client();

    let request = sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsRequest {
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                // "summary.timestamp".to_string(),
                // "transactions.events.events.package_id".to_string(),
                // "transactions.events.events.module".to_string(),
                // "transactions.events.events.sender".to_string(),
                "transactions.events.events.event_type".to_string(),
            ],
        }),
    };

    let stream = subscription_client
        .subscribe_checkpoints(request)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe to checkpoints: {}", e))?
        .into_inner();

    Ok(stream)
}
