use crate::error::{CoordinatorError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::debug;

/// Block information fetched from blockchain
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub name: String,
}

/// Fetch Block information from AppInstance by block number
pub async fn fetch_block_info(
    client: &mut Client,
    app_instance: &str,
    block_number: u64,
) -> Result<Option<BlockInfo>> {
    debug!("Fetching Block info for block {} from app_instance {}", block_number, app_instance);
    
    // Ensure the app_instance has 0x prefix
    let formatted_id = if app_instance.starts_with("0x") {
        app_instance.to_string()
    } else {
        format!("0x{}", app_instance)
    };
    
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };
    
    let object_response = client.ledger_client().get_object(request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch app_instance {}: {}", app_instance, e)
    ))?;
    
    let response = object_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Get the blocks ObjectTable
                if let Some(blocks_field) = struct_value.fields.get("blocks") {
                    if let Some(prost_types::value::Kind::StructValue(blocks_struct)) = &blocks_field.kind {
                        if let Some(table_id_field) = blocks_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                // Fetch the Block from the ObjectTable using block_number
                                return fetch_block_from_table(client, table_id, block_number).await;
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(None)
}

/// Fetch Block from ObjectTable by block number
async fn fetch_block_from_table(
    client: &mut Client,
    table_id: &str,
    block_number: u64,
) -> Result<Option<BlockInfo>> {
    let request = ListDynamicFieldsRequest {
        parent: Some(table_id.to_string()),
        page_size: Some(100),
        page_token: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "field_id".to_string(),
                "name_type".to_string(),
                "name_value".to_string(),
            ],
        }),
    };
    
    let fields_response = client.live_data_client().list_dynamic_fields(request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to list dynamic fields: {}", e)
    ))?;
    
    let response = fields_response.into_inner();
    
    for field in &response.dynamic_fields {
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (block_number)
            if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                if field_block_number == block_number {
                    if let Some(field_id) = &field.field_id {
                        // Found the block, fetch its content
                        let block_request = GetObjectRequest {
                            object_id: Some(field_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let block_response = client.ledger_client().get_object(block_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch block {}: {}", field_block_number, e)
    ))?;
                        
                        let block_response_inner = block_response.into_inner();
                        if let Some(block_object) = block_response_inner.object {
                            if let Some(block_json) = &block_object.json {
                                // Extract the actual block object ID from the Field wrapper
                                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &block_json.kind {
                                    if let Some(value_field) = struct_value.fields.get("value") {
                                        if let Some(prost_types::value::Kind::StringValue(block_object_id)) = &value_field.kind {
                                            // Fetch the actual block object
                                            let actual_block_request = GetObjectRequest {
                                                object_id: Some(block_object_id.clone()),
                                                version: None,
                                                read_mask: Some(prost_types::FieldMask {
                                                    paths: vec![
                                                        "object_id".to_string(),
                                                        "json".to_string(),
                                                    ],
                                                }),
                                            };
                                            
                                            let actual_block_response = client.ledger_client().get_object(actual_block_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
                                                format!("Failed to fetch actual block {}: {}", block_number, e)
                                            ))?;
                                            
                                            if let Some(actual_block_object) = actual_block_response.into_inner().object {
                                                if let Some(actual_block_json) = &actual_block_object.json {
                                                    return extract_block_info_from_json(actual_block_json, block_number);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(None)
}

/// Extract Block information from JSON
fn extract_block_info_from_json(json_value: &prost_types::Value, block_number: u64) -> Result<Option<BlockInfo>> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        let mut name = String::new();
        let mut start_sequence = 0u64;
        let mut end_sequence = 0u64;
        
        if let Some(name_field) = struct_value.fields.get("name") {
            if let Some(prost_types::value::Kind::StringValue(name_str)) = &name_field.kind {
                name = name_str.clone();
            }
        }
        
        if let Some(start_field) = struct_value.fields.get("start_sequence") {
            if let Some(prost_types::value::Kind::StringValue(start_str)) = &start_field.kind {
                start_sequence = start_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(end_field) = struct_value.fields.get("end_sequence") {
            if let Some(prost_types::value::Kind::StringValue(end_str)) = &end_field.kind {
                end_sequence = end_str.parse().unwrap_or(0);
            }
        }
        
        return Ok(Some(BlockInfo {
            block_number,
            start_sequence,
            end_sequence,
            name,
        }));
    }
    
    Ok(None)
}