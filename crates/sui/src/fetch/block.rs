use crate::error::{SilvanaSuiInterfaceError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, warn};

/// Block information fetched from blockchain, mirroring the Move struct
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Block {
    pub name: String,
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub actions_commitment: Option<String>, // Element<Scalar> represented as String
    pub state_commitment: Option<String>, // Element<Scalar> represented as String
    pub time_since_last_block: u64,
    pub number_of_transactions: u64,
    pub start_actions_commitment: Option<String>, // Element<Scalar> represented as String
    pub end_actions_commitment: Option<String>, // Element<Scalar> represented as String
    pub state_data_availability: Option<String>,
    pub proof_data_availability: Option<String>,
    pub settlement_tx_hash: Option<String>,
    pub settlement_tx_included_in_block: bool,
    pub created_at: u64,
    pub state_calculated_at: Option<u64>,
    pub proved_at: Option<u64>,
    pub sent_to_settlement_at: Option<u64>,
    pub settled_at: Option<u64>,
}

/// Fetch Block information from AppInstance by block number
#[allow(dead_code)]
pub async fn fetch_block_info(
    client: &mut Client,
    app_instance: &str,
    block_number: u64,
) -> Result<Option<Block>> {
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
    
    let object_response = client.ledger_client().get_object(request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
#[allow(dead_code)]
async fn fetch_block_from_table(
    client: &mut Client,
    table_id: &str,
    block_number: u64,
) -> Result<Option<Block>> {
    debug!("🔍 Optimized search for block {} in table {}", block_number, table_id);
    
    let mut page_token = None;
    const PAGE_SIZE: u32 = 50; // Smaller page size to reduce data transfer
    let mut pages_searched = 0;
    const MAX_PAGES: u32 = 100; // Prevent infinite loops in case of issues
    let mut all_found_blocks = Vec::new(); // Collect all found block numbers for debugging
    
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
        
        let fields_response = client.live_data_client().list_dynamic_fields(request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
                    all_found_blocks.push(field_block_number);
                    //debug!("🔢 Found block {} in dynamic fields", field_block_number);
                    if field_block_number == block_number {
                        debug!("🎯 Found matching block {} in dynamic fields (page {})", block_number, pages_searched);
                        if let Some(field_id) = &field.field_id {
                            // Found the block, fetch its content
                            return fetch_block_object_by_field_id(client, field_id, block_number).await;
                        }
                    }
                } else {
                    // Log the raw bytes if BCS decoding fails
                    debug!("⚠️ Failed to decode block number from bytes: {:?}", name_value);
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
    
    // Sort the found blocks for better readability
    all_found_blocks.sort();
    all_found_blocks.dedup();
    
    warn!("❌ Block {} not found in table {} after searching {} pages", block_number, table_id, pages_searched);
    warn!("📊 All block numbers found in the blocks table: {:?}", all_found_blocks);
    warn!("📊 Total unique blocks found: {}", all_found_blocks.len());
    
    // Now fetch and display full details of all dynamic fields for deeper debugging
    debug_all_dynamic_fields(client, table_id, block_number).await;
    
    Ok(None)
}

/// Fetch Block object content by field ID (extracted from dynamic field lookup)
#[allow(dead_code)]
async fn fetch_block_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    block_number: u64,
) -> Result<Option<Block>> {
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
    
    let block_response = client.ledger_client().get_object(block_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
                        
                        let actual_block_response = client.ledger_client().get_object(actual_block_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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

/// Debug function to print all dynamic fields with full details
#[allow(dead_code)]
async fn debug_all_dynamic_fields(client: &mut Client, table_id: &str, target_block: u64) {
    warn!("🔍 DEBUG: Fetching ALL dynamic fields for blocks table {} to diagnose why block {} is not found", table_id, target_block);
    
    // Fetch dynamic field information
    let request = ListDynamicFieldsRequest {
        parent: Some(table_id.to_string()),
        page_size: Some(100), // Larger page size to get more fields at once
        page_token: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "field_id".to_string(),
                "name_type".to_string(),
                "name_value".to_string(),
            ],
        }),
    };
    
    match client.live_data_client().list_dynamic_fields(request).await {
        Ok(response) => {
            let inner = response.into_inner();
            warn!("📋 DEBUG: Found {} dynamic fields in first page", inner.dynamic_fields.len());
            
            for (i, field) in inner.dynamic_fields.iter().enumerate() {
                warn!("═══════════════════════════════════════════");
                warn!("📌 Dynamic Field #{}", i + 1);
                warn!("  Field ID: {:?}", field.field_id);
                warn!("  Name Type: {:?}", field.name_type);
                
                // Try to decode the name_value as u64
                if let Some(name_value) = &field.name_value {
                    warn!("  Name Value (raw bytes): {:?}", name_value);
                    warn!("  Name Value (hex): {}", hex::encode(name_value));
                    
                    // Try BCS decoding as u64
                    match bcs::from_bytes::<u64>(name_value) {
                        Ok(block_num) => {
                            warn!("  ✅ Decoded as block number: {}", block_num);
                            if block_num == target_block {
                                warn!("  🎯 THIS IS THE TARGET BLOCK!");
                            }
                        }
                        Err(e) => {
                            warn!("  ❌ Failed to decode as u64: {}", e);
                            
                            // Try other decodings
                            if let Ok(u32_val) = bcs::from_bytes::<u32>(name_value) {
                                warn!("  ⚠️ Can decode as u32: {}", u32_val);
                            }
                            if let Ok(string_val) = bcs::from_bytes::<String>(name_value) {
                                warn!("  ⚠️ Can decode as String: {}", string_val);
                            }
                        }
                    }
                }
            }
            warn!("═══════════════════════════════════════════");
            
            if inner.next_page_token.is_some() {
                warn!("⚠️ There are more pages of dynamic fields not shown here");
            }
        }
        Err(e) => {
            warn!("❌ DEBUG: Failed to fetch dynamic fields for debugging: {}", e);
        }
    }
}

/// Extract Block information from JSON
#[allow(dead_code)]
fn extract_block_info_from_json(json_value: &prost_types::Value, block_number: u64) -> Result<Option<Block>> {
    debug!("🔍 Extracting block info for block {} from JSON", block_number);
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("🧱 Block {} fields: {:?}", block_number, struct_value.fields.keys().collect::<Vec<_>>());
        
        // Extract all fields according to the Move struct
        let mut name = String::new();
        let mut start_sequence = 0u64;
        let mut end_sequence = 0u64;
        let mut actions_commitment = None;
        let mut state_commitment = None;
        let mut time_since_last_block = 0u64;
        let mut number_of_transactions = 0u64;
        let mut start_actions_commitment = None;
        let mut end_actions_commitment = None;
        let mut state_data_availability = None;
        let mut proof_data_availability = None;
        let mut settlement_tx_hash = None;
        let mut settlement_tx_included_in_block = false;
        let mut created_at = 0u64;
        let mut state_calculated_at = None;
        let mut proved_at = None;
        let mut sent_to_settlement_at = None;
        let mut settled_at = None;
        
        // Extract name
        if let Some(name_field) = struct_value.fields.get("name") {
            if let Some(prost_types::value::Kind::StringValue(name_str)) = &name_field.kind {
                name = name_str.clone();
            }
        }
        
        // Extract sequences
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
        
        // Extract commitment fields (Element<Scalar> as String)
        if let Some(field) = struct_value.fields.get("actions_commitment") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                actions_commitment = Some(s.clone());
            }
        }
        
        if let Some(field) = struct_value.fields.get("state_commitment") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                state_commitment = Some(s.clone());
            }
        }
        
        if let Some(field) = struct_value.fields.get("start_actions_commitment") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                start_actions_commitment = Some(s.clone());
            }
        }
        
        if let Some(field) = struct_value.fields.get("end_actions_commitment") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                end_actions_commitment = Some(s.clone());
            }
        }
        
        // Extract numeric fields
        if let Some(field) = struct_value.fields.get("time_since_last_block") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                time_since_last_block = s.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("number_of_transactions") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                number_of_transactions = s.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("created_at") {
            if let Some(prost_types::value::Kind::StringValue(s)) = &field.kind {
                created_at = s.parse().unwrap_or(0);
            }
        }
        
        // Extract optional string fields
        if let Some(field) = struct_value.fields.get("state_data_availability") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    state_data_availability = Some(s.clone());
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    state_data_availability = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("proof_data_availability") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    proof_data_availability = Some(s.clone());
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    proof_data_availability = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("settlement_tx_hash") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    settlement_tx_hash = Some(s.clone());
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    settlement_tx_hash = None;
                }
                _ => {}
            }
        }
        
        // Extract boolean field
        if let Some(field) = struct_value.fields.get("settlement_tx_included_in_block") {
            if let Some(prost_types::value::Kind::BoolValue(b)) = &field.kind {
                settlement_tx_included_in_block = *b;
            }
        }
        
        // Extract optional timestamp fields
        if let Some(field) = struct_value.fields.get("state_calculated_at") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    state_calculated_at = s.parse().ok();
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    state_calculated_at = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("proved_at") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    proved_at = s.parse().ok();
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    proved_at = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("sent_to_settlement_at") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    sent_to_settlement_at = s.parse().ok();
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    sent_to_settlement_at = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("settled_at") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(s)) => {
                    settled_at = s.parse().ok();
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    settled_at = None;
                }
                _ => {}
            }
        }
        
        let block = Block {
            name,
            block_number,
            start_sequence,
            end_sequence,
            actions_commitment,
            state_commitment,
            time_since_last_block,
            number_of_transactions,
            start_actions_commitment,
            end_actions_commitment,
            state_data_availability,
            proof_data_availability,
            settlement_tx_hash,
            settlement_tx_included_in_block,
            created_at,
            state_calculated_at,
            proved_at,
            sent_to_settlement_at,
            settled_at,
        };
        debug!("✅ Successfully extracted block info: {:?}", block);
        return Ok(Some(block));
    }
    
    Ok(None)
}