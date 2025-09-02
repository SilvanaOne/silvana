use anyhow::Result;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::debug;
use crate::error::SilvanaSuiInterfaceError;
use crate::parse::proto_to_json;
use crate::state::SharedSuiState;
use serde_json::{Map, Value};

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
        .await;
    
    match response {
        Ok(object_response) => {
            let object_data = object_response.into_inner();
            
            // Convert the proto object to JSON
            if let Some(proto_object) = object_data.object {
                if let Some(json_value) = &proto_object.json {
                    // Convert prost_types::Value to serde_json::Value
                    let mut json_obj = proto_to_json(json_value);
                    
                    // Recursively check for and fetch dynamic fields in the object tree
                    enhance_with_dynamic_fields(&mut json_obj).await;
                    
                    return Ok(json_obj);
                }
            }
            
            Err(anyhow::anyhow!(
                "Failed to parse object {} from blockchain response",
                object_id
            ))
        }
        Err(e) => {
            // Check if this might be a dynamic field table ID (NotFound error)
            if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                debug!("Object {} not found as regular object, trying as dynamic field table", formatted_id);
                
                // Try to fetch dynamic fields directly
                match fetch_dynamic_fields(&formatted_id).await {
                    Ok(fields) => {
                        // Return as a synthetic object showing it's a dynamic field table
                        let mut result = Map::new();
                        result.insert("id".to_string(), Value::String(formatted_id.clone()));
                        result.insert("type".to_string(), Value::String("DynamicFieldTable".to_string()));
                        result.insert("fields".to_string(), fields);
                        
                        Ok(Value::Object(result))
                    }
                    Err(fields_err) => {
                        // If fetching dynamic fields also fails, return the original error
                        debug!("Failed to fetch as dynamic fields: {}", fields_err);
                        Err(anyhow::anyhow!(
                            "Failed to fetch object {}: {}",
                            object_id, e
                        ))
                    }
                }
            } else {
                // Other error, return as is
                Err(anyhow::anyhow!(
                    "Failed to fetch object {}: {}",
                    object_id, e
                ))
            }
        }
    }
}

/// Recursively enhance JSON with dynamic fields for any nested objects that are dynamic field collections
async fn enhance_with_dynamic_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // First, recursively process all nested values
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(nested_value) = map.get_mut(&key) {
                    Box::pin(enhance_with_dynamic_fields(nested_value)).await;
                }
            }
            
            // Then check if this object itself is a dynamic field collection
            if is_dynamic_field_collection(map) {
                if let Some(id_value) = map.get("id") {
                    if let Value::String(collection_id) = id_value {
                        debug!("Detected dynamic field collection, fetching fields for: {}", collection_id);
                        match fetch_dynamic_fields(collection_id).await {
                            Ok(fields) => {
                                map.insert("fields".to_string(), fields);
                            }
                            Err(e) => {
                                debug!("Failed to fetch dynamic fields for {}: {}", collection_id, e);
                                // Continue without fields rather than failing
                            }
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            // Recursively process array elements
            for item in arr {
                Box::pin(enhance_with_dynamic_fields(item)).await;
            }
        }
        _ => {} // Primitive values don't need processing
    }
}

/// Check if an object is a dynamic field collection (has only id and size fields)
fn is_dynamic_field_collection(map: &Map<String, Value>) -> bool {
    // Check if object has exactly 2 fields: id and size
    if map.len() != 2 {
        return false;
    }
    
    // Check if it has both id and size fields
    map.contains_key("id") && map.contains_key("size")
}

/// Fetch all dynamic fields for a given parent object ID
async fn fetch_dynamic_fields(parent_id: &str) -> Result<Value> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut all_fields = Vec::new();
    let mut page_token = None;
    const PAGE_SIZE: u32 = 100;
    let mut pages_fetched = 0;
    const MAX_PAGES: u32 = 50; // Limit to prevent infinite loops
    
    debug!("Fetching dynamic fields for parent: {}", parent_id);
    
    loop {
        let request = ListDynamicFieldsRequest {
            parent: Some(parent_id.to_string()),
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
        
        let response = client
            .live_data_client()
            .list_dynamic_fields(request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to list dynamic fields: {}",
                    e
                ))
            })?;
        
        let fields_response = response.into_inner();
        pages_fetched += 1;
        
        debug!(
            "Page {}: Found {} dynamic fields",
            pages_fetched,
            fields_response.dynamic_fields.len()
        );
        
        // Process each field
        for field in fields_response.dynamic_fields {
            let mut field_map = Map::new();
            
            // Add field_id
            if let Some(field_id) = field.field_id {
                field_map.insert("field_id".to_string(), Value::String(field_id));
            }
            
            // Add name_type
            if let Some(name_type) = field.name_type {
                field_map.insert("name_type".to_string(), Value::String(name_type));
            }
            
            // Try to decode name_value
            if let Some(name_value) = field.name_value {
                // Try to decode as various types
                if let Ok(u64_val) = bcs::from_bytes::<u64>(&name_value) {
                    field_map.insert("name".to_string(), Value::String(u64_val.to_string()));
                } else if let Ok(string_val) = bcs::from_bytes::<String>(&name_value) {
                    field_map.insert("name".to_string(), Value::String(string_val));
                } else {
                    // If we can't decode, store as hex
                    field_map.insert("name_raw".to_string(), Value::String(hex::encode(&name_value)));
                }
            }
            
            
            all_fields.push(Value::Object(field_map));
        }
        
        // Check if there are more pages
        if let Some(next_token) = fields_response.next_page_token {
            if !next_token.is_empty() && pages_fetched < MAX_PAGES {
                page_token = Some(next_token);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    debug!("Total dynamic fields fetched: {}", all_fields.len());
    Ok(Value::Array(all_fields))
}