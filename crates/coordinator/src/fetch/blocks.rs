use crate::error::{CoordinatorError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, warn};

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
            //debug!("🏗️ AppInstance JSON structure for {}: {:#?}", app_instance, json_value);
            
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                //debug!("📋 AppInstance {} fields: {:?}", app_instance, struct_value.fields.keys().collect::<Vec<_>>());
                
                // Get the blocks ObjectTable
                if let Some(blocks_field) = struct_value.fields.get("blocks") {
                    //debug!("🧱 Found blocks field in AppInstance {}", app_instance);
                    if let Some(prost_types::value::Kind::StructValue(blocks_struct)) = &blocks_field.kind {
                        //debug!("🧱 Blocks struct fields: {:?}", blocks_struct.fields.keys().collect::<Vec<_>>());
                        if let Some(table_id_field) = blocks_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                //debug!("🧱 Found blocks table ID: {}", table_id);
                                // Fetch the Block from the ObjectTable using block_number
                                return fetch_block_from_table(client, table_id, block_number).await;
                            } else {
                                warn!("❌ Blocks table id field is not a string: {:?}", table_id_field.kind);
                            }
                        } else {
                            warn!("❌ No 'id' field in blocks table struct");
                        }
                    } else {
                        warn!("❌ Blocks field is not a struct: {:?}", blocks_field.kind);
                    }
                } else {
                    warn!("❌ No 'blocks' field found in AppInstance. Available fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
                }
            }
        } else {
            warn!("❌ No JSON found for AppInstance {}", app_instance);
        }
    } else {
        warn!("❌ No object returned for AppInstance {}", app_instance);
    }
    
    Ok(None)
}

/// Fetch Block from ObjectTable by block number using optimized pagination
async fn fetch_block_from_table(
    client: &mut Client,
    table_id: &str,
    block_number: u64,
) -> Result<Option<BlockInfo>> {
    debug!("🔍 Optimized search for block {} in table {}", block_number, table_id);
    
    let mut page_token = None;
    const PAGE_SIZE: u32 = 50; // Smaller page size to reduce data transfer
    let mut pages_searched = 0;
    const MAX_PAGES: u32 = 100; // Prevent infinite loops in case of issues
    
    loop {
        let request = ListDynamicFieldsRequest {
            parent: Some(table_id.to_string()),
            page_size: Some(PAGE_SIZE),
            page_token: page_token.clone(),
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
        pages_searched += 1;
        debug!("📋 Page {}: Found {} dynamic fields in blocks table", pages_searched, response.dynamic_fields.len());
        
        // Search in current page for our target block
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                // The name_value is BCS-encoded u64 (block_number)
                if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                    //debug!("🔢 Found block {} in dynamic fields", field_block_number);
                    if field_block_number == block_number {
                        debug!("🎯 Found matching block {} in dynamic fields (page {})", block_number, pages_searched);
                        if let Some(field_id) = &field.field_id {
                            // Found the block, fetch its content
                            return fetch_block_object_by_field_id(client, field_id, block_number).await;
                        }
                    }
                }
            }
        }
        
        // Check if we should continue pagination
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() && pages_searched < MAX_PAGES {
                page_token = Some(next_token);
                debug!("⏭️ Continuing to page {} to search for block {}", pages_searched + 1, block_number);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    debug!("❌ Block {} not found in table {} after searching {} pages", block_number, table_id, pages_searched);
    Ok(None)
}

/// Fetch Block object content by field ID (extracted from dynamic field lookup)
async fn fetch_block_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    block_number: u64,
) -> Result<Option<BlockInfo>> {
    debug!("📄 Fetching block {} content from field {}", block_number, field_id);
    
    let block_request = GetObjectRequest {
        object_id: Some(field_id.to_string()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };
    
    let block_response = client.ledger_client().get_object(block_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch block {}: {}", block_number, e)
    ))?;
    
    let block_response_inner = block_response.into_inner();
    if let Some(block_object) = block_response_inner.object {
        if let Some(block_json) = &block_object.json {
            debug!("📄 Block field wrapper JSON retrieved for block {}", block_number);
            // Extract the actual block object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &block_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(block_object_id)) = &value_field.kind {
                        //debug!("📄 Found block object ID: {}", block_object_id);
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
                                debug!("🧱 Block {} JSON retrieved successfully", block_number);
                                return extract_block_info_from_json(actual_block_json, block_number);
                            } else {
                                warn!("❌ No JSON found for block object {}", block_number);
                            }
                        } else {
                            warn!("❌ No block object found for block {}", block_number);
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
    debug!("🔍 Extracting block info for block {} from JSON", block_number);
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("🧱 Block {} fields: {:?}", block_number, struct_value.fields.keys().collect::<Vec<_>>());
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
        
        let block_info = BlockInfo {
            block_number,
            start_sequence,
            end_sequence,
            name: name.clone(),
        };
        debug!("✅ Successfully extracted block info: {:?}", block_info);
        return Ok(Some(block_info));
    }
    
    Ok(None)
}