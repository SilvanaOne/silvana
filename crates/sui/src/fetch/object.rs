use anyhow::Result;
use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
use tracing::debug;
use crate::error::SilvanaSuiInterfaceError;
use crate::parse::proto_to_json;
use crate::state::SharedSuiState;

/// Fetch a raw Sui object from the blockchain by its ID and return as JSON
pub async fn fetch_object(
    object_id: &str,
) -> Result<serde_json::Value> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching object with ID: {}", object_id);
    
    // Ensure the object_id has 0x prefix
    let formatted_id = if object_id.starts_with("0x") {
        object_id.to_string()
    } else {
        format!("0x{}", object_id)
    };
    
    // Create request to fetch the object
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string(), "object_id".to_string()],
        }),
    };
    
    // Fetch the object
    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch object {}: {}",
                object_id, e
            ))
        })?;
    
    let object_data = response.into_inner();
    
    // Convert the proto object to JSON
    if let Some(proto_object) = object_data.object {
        if let Some(json_value) = &proto_object.json {
            // Convert prost_types::Value to serde_json::Value
            return Ok(proto_to_json(json_value));
        }
    }
    
    Err(anyhow::anyhow!(
        "Failed to parse object {} from blockchain response",
        object_id
    ))
}