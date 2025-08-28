use crate::error::{SilvanaSuiInterfaceError, Result};
use crate::fetch::AppInstance;
use crate::state::SharedSuiState;
use std::collections::HashMap;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest, BatchGetObjectsRequest};
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

/// Fetch Block information from AppInstance by block number (legacy single-block function)
#[allow(dead_code)]
pub async fn fetch_block_info(
    app_instance: &str,
    block_number: u64,
) -> Result<Option<Block>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
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
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Get the blocks ObjectTable
                if let Some(blocks_field) = struct_value.fields.get("blocks") {
                    if let Some(prost_types::value::Kind::StructValue(blocks_struct)) = &blocks_field.kind {
                        if let Some(table_id_field) = blocks_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                // Fetch the Block from the ObjectTable using block_number
                                return fetch_block_from_table(&mut client, table_id, block_number).await;
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

/// Fetch multiple Blocks from AppInstance for a range of block numbers
/// Returns a HashMap of block_number -> Block for all found blocks in the range
#[allow(dead_code)]
pub async fn fetch_blocks_range(
    app_instance: &AppInstance,
    start_block: u64,
    end_block: u64,
) -> Result<HashMap<u64, Block>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching Blocks from {} to {} for app_instance {}", 
        start_block, end_block, app_instance.id);
    
    // Get the blocks table ID from the AppInstance
    let blocks_table_id = &app_instance.blocks_table_id;
    
    // Fetch all blocks in the range from the table
    fetch_blocks_from_table_range(&mut client, blocks_table_id, start_block, end_block).await
}

/// Fetch multiple Blocks from ObjectTable for a range of block numbers
/// Returns a HashMap of block_number -> Block for all found blocks
#[allow(dead_code)]
async fn fetch_blocks_from_table_range(
    client: &mut Client,
    table_id: &str,
    start_block: u64,
    end_block: u64,
) -> Result<HashMap<u64, Block>> {
    debug!("🔍 Fetching blocks {} to {} from table {}", start_block, end_block, table_id);
    
    // First, collect all field IDs for blocks in the range
    let mut field_ids_to_fetch = Vec::new(); // (field_id, block_number)
    let mut page_token = None;
    const PAGE_SIZE: u32 = 100;
    let mut pages_searched = 0;
    const MAX_PAGES: u32 = 200;
    
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
        
        // Collect field IDs for blocks in our range
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                    // Check if this block is in our desired range
                    if field_block_number >= start_block && field_block_number <= end_block {
                        if let Some(field_id) = &field.field_id {
                            field_ids_to_fetch.push((field_id.clone(), field_block_number));
                        }
                    }
                }
            }
        }
        
        // Check if we should continue pagination
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() && pages_searched < MAX_PAGES {
                page_token = Some(next_token);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    debug!(
        "📋 Collected {} field IDs for blocks in range {}-{}",
        field_ids_to_fetch.len(), start_block, end_block
    );
    
    if field_ids_to_fetch.is_empty() {
        warn!(
            "❌ No blocks found in range {}-{} after searching {} pages",
            start_block, end_block, pages_searched
        );
        return Ok(HashMap::new());
    }
    
    // Now batch fetch all blocks
    fetch_block_objects_batch(client, field_ids_to_fetch).await
}

/// Batch fetch multiple blocks at once
async fn fetch_block_objects_batch(
    client: &mut Client,
    field_ids_with_blocks: Vec<(String, u64)>, // (field_id, block_number)
) -> Result<HashMap<u64, Block>> {
    let mut blocks_map = HashMap::new();
    
    // Process in batches of 50 to avoid overwhelming the RPC
    const BATCH_SIZE: usize = 50;
    
    for chunk in field_ids_with_blocks.chunks(BATCH_SIZE) {
        debug!("📦 Batch fetching {} block field wrappers", chunk.len());
        
        // First batch: fetch all field wrapper objects to get the actual block object IDs
        let field_requests: Vec<GetObjectRequest> = chunk
            .iter()
            .map(|(field_id, _)| GetObjectRequest {
                object_id: Some(field_id.clone()),
                version: None,
                read_mask: None, // Use batch-level mask instead
            })
            .collect();
        
        let batch_request = BatchGetObjectsRequest {
            requests: field_requests,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            }),
        };
        
        let batch_response = client
            .ledger_client()
            .batch_get_objects(batch_request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to batch fetch field wrappers: {}",
                    e
                ))
            })?;
        
        let field_results = batch_response.into_inner().objects;
        
        // Extract block object IDs from field wrappers
        let mut block_object_ids = Vec::new(); // (block_object_id, block_number)
        for (i, get_result) in field_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(field_object)) = &get_result.result {
                if let Some(field_json) = &field_object.json {
                    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                        if let Some(value_field) = struct_value.fields.get("value") {
                            if let Some(prost_types::value::Kind::StringValue(block_object_id)) = &value_field.kind {
                                let (_, block_number) = chunk[i];
                                block_object_ids.push((block_object_id.clone(), block_number));
                            }
                        }
                    }
                }
            }
        }
        
        if block_object_ids.is_empty() {
            continue;
        }
        
        debug!("📦 Batch fetching {} block objects", block_object_ids.len());
        
        // Second batch: fetch all actual block objects
        let block_requests: Vec<GetObjectRequest> = block_object_ids
            .iter()
            .map(|(block_id, _)| GetObjectRequest {
                object_id: Some(block_id.clone()),
                version: None,
                read_mask: None, // Use batch-level mask instead
            })
            .collect();
        
        let batch_request = BatchGetObjectsRequest {
            requests: block_requests,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            }),
        };
        
        let batch_response = client
            .ledger_client()
            .batch_get_objects(batch_request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to batch fetch blocks: {}",
                    e
                ))
            })?;
        
        let block_results = batch_response.into_inner().objects;
        
        // Extract Block data from results
        for (i, get_result) in block_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(block_object)) = &get_result.result {
                if let Some(block_json) = &block_object.json {
                    let (_, block_number) = block_object_ids[i];
                    if let Ok(Some(block)) = extract_block_info_from_json(block_json, block_number) {
                        blocks_map.insert(block_number, block);
                    }
                }
            }
        }
    }
    
    debug!(
        "📊 Successfully fetched {} blocks",
        blocks_map.len()
    );
    
    Ok(blocks_map)
}

/// Fetch Block from ObjectTable by block number using optimized pagination (legacy single-block function)
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

/// Fetch Block object content by field ID (already verified to be the correct block)
#[allow(dead_code)]
async fn fetch_block_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    block_number: u64,
) -> Result<Option<Block>> {
    debug!("📄 Fetching block {} content from field {}", block_number, field_id);
    
    // Fetch the Field wrapper object
    let field_request = GetObjectRequest {
        object_id: Some(field_id.to_string()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };
    
    let field_response = client.ledger_client().get_object(field_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
        format!("Failed to fetch field wrapper for block {}: {}", block_number, e)
    ))?;
    
    let field_response_inner = field_response.into_inner();
    if let Some(field_object) = field_response_inner.object {
        if let Some(field_json) = &field_object.json {
            debug!("📄 Field wrapper JSON retrieved for block {}", block_number);
            // Extract the actual block object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(block_object_id)) = &value_field.kind {
                        debug!("📄 Found block object ID: {}", block_object_id);
                        // Fetch the actual block object
                        let block_request = GetObjectRequest {
                            object_id: Some(block_object_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let block_response = client.ledger_client().get_object(block_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                            format!("Failed to fetch block object {}: {}", block_number, e)
                        ))?;
                        
                        if let Some(block_object) = block_response.into_inner().object {
                            if let Some(block_json) = &block_object.json {
                                debug!("🧱 Block {} JSON retrieved, extracting data", block_number);
                                // We already know this is the correct block from the name_value check
                                // so we can directly extract the block data
                                return extract_block_info_from_json(block_json, block_number);
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