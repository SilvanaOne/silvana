use crate::error::{Result, SilvanaSuiInterfaceError};
use crate::fetch::AppInstance;
use crate::parse::{get_option_string, get_option_u64, get_string, get_u64};
use crate::state::SharedSuiState;
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use std::fmt;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{
    BatchGetObjectsRequest, GetObjectRequest, ListDynamicFieldsRequest,
};
use tracing::{debug, warn};

/// Block information fetched from blockchain, mirroring the Move struct
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Block {
    pub name: String,
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub actions_commitment: Vec<u8>, // Element<Scalar> bytes field
    pub state_commitment: Vec<u8>,   // Element<Scalar> bytes field
    pub time_since_last_block: u64,
    pub number_of_transactions: u64,
    pub start_actions_commitment: Vec<u8>, // Element<Scalar> bytes field
    pub end_actions_commitment: Vec<u8>,   // Element<Scalar> bytes field
    pub state_data_availability: Option<String>,
    pub proof_data_availability: Option<String>,
    pub created_at: u64,
    pub state_calculated_at: Option<u64>,
    pub proved_at: Option<u64>,
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Block {{ name: \"{}\", block_number: {}, start_sequence: {}, end_sequence: {}, time_since_last_block: {}, number_of_transactions: {}, state_data_availability: {:?}, proof_data_availability: {:?}, created_at: {}, state_calculated_at: {:?}, proved_at: {:?} }}",
            self.name,
            self.block_number,
            self.start_sequence,
            self.end_sequence,
            self.time_since_last_block,
            self.number_of_transactions,
            self.state_data_availability,
            self.proof_data_availability,
            self.created_at,
            self.state_calculated_at,
            self.proved_at
        )
    }
}

/// Fetch Block information from AppInstance by block number (legacy single-block function)
#[allow(dead_code)]
pub async fn fetch_block_info(
    app_instance: &AppInstance,
    block_number: u64,
) -> Result<Option<Block>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!(
        "Fetching Block info for block {} from app_instance {}",
        block_number, app_instance.id
    );

    // Use the blocks_table_id directly from the AppInstance struct
    fetch_block_from_table(&mut client, &app_instance.blocks_table_id, block_number).await
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
    debug!(
        "Fetching Blocks from {} to {} for app_instance {}",
        start_block, end_block, app_instance.id
    );

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
    debug!(
        "🔍 Fetching blocks {} to {} from table {}",
        start_block, end_block, table_id
    );

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

        let fields_response = client
            .live_data_client()
            .list_dynamic_fields(request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to list dynamic fields: {}",
                    e
                ))
            })?;

        let response = fields_response.into_inner();
        pages_searched += 1;
        debug!(
            "📋 Page {}: Found {} dynamic fields in blocks table",
            pages_searched,
            response.dynamic_fields.len()
        );

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
        field_ids_to_fetch.len(),
        start_block,
        end_block
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
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(
                field_object,
            )) = &get_result.result
            {
                if let Some(field_json) = &field_object.json {
                    if let Some(prost_types::value::Kind::StructValue(struct_value)) =
                        &field_json.kind
                    {
                        if let Some(value_field) = struct_value.fields.get("value") {
                            if let Some(prost_types::value::Kind::StringValue(block_object_id)) =
                                &value_field.kind
                            {
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
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(
                block_object,
            )) = &get_result.result
            {
                if let Some(block_json) = &block_object.json {
                    let (_, block_number) = block_object_ids[i];
                    if let Ok(Some(block)) = extract_block_info_from_json(block_json, block_number)
                    {
                        blocks_map.insert(block_number, block);
                    }
                }
            }
        }
    }

    debug!("📊 Successfully fetched {} blocks", blocks_map.len());

    Ok(blocks_map)
}

/// Fetch Block from ObjectTable by block number using optimized pagination (legacy single-block function)
#[allow(dead_code)]
async fn fetch_block_from_table(
    client: &mut Client,
    table_id: &str,
    block_number: u64,
) -> Result<Option<Block>> {
    debug!(
        "🔍 Optimized search for block {} in table {}",
        block_number, table_id
    );

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

        let fields_response = client
            .live_data_client()
            .list_dynamic_fields(request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to list dynamic fields: {}",
                    e
                ))
            })?;

        let response = fields_response.into_inner();
        pages_searched += 1;
        debug!(
            "📋 Page {}: Found {} dynamic fields in blocks table",
            pages_searched,
            response.dynamic_fields.len()
        );

        // Search in current page for our target block
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                // The name_value is BCS-encoded u64 (block_number)
                if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                    all_found_blocks.push(field_block_number);
                    //debug!("🔢 Found block {} in dynamic fields", field_block_number);
                    if field_block_number == block_number {
                        debug!(
                            "🎯 Found matching block {} in dynamic fields (page {})",
                            block_number, pages_searched
                        );
                        if let Some(field_id) = &field.field_id {
                            // Found the block, fetch its content
                            return fetch_block_object_by_field_id(client, field_id, block_number)
                                .await;
                        }
                    }
                } else {
                    // Log the raw bytes if BCS decoding fails
                    debug!(
                        "⚠️ Failed to decode block number from bytes: {:?}",
                        name_value
                    );
                }
            }
        }

        // Check if we should continue pagination
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() && pages_searched < MAX_PAGES {
                page_token = Some(next_token);
                debug!(
                    "⏭️ Continuing to page {} to search for block {}",
                    pages_searched + 1,
                    block_number
                );
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

    warn!(
        "❌ Block {} not found in table {} after searching {} pages",
        block_number, table_id, pages_searched
    );
    warn!(
        "📊 All block numbers found in the blocks table: {:?}",
        all_found_blocks
    );
    warn!("📊 Total unique blocks found: {}", all_found_blocks.len());

    Ok(None)
}

/// Fetch Block object content by field ID (already verified to be the correct block)
#[allow(dead_code)]
async fn fetch_block_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    block_number: u64,
) -> Result<Option<Block>> {
    debug!(
        "📄 Fetching block {} content from field {}",
        block_number, field_id
    );

    // Fetch the Field wrapper object
    let field_request = GetObjectRequest {
        object_id: Some(field_id.to_string()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["object_id".to_string(), "json".to_string()],
        }),
    };

    let field_response = client
        .ledger_client()
        .get_object(field_request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch field wrapper for block {}: {}",
                block_number, e
            ))
        })?;

    let field_response_inner = field_response.into_inner();
    if let Some(field_object) = field_response_inner.object {
        if let Some(field_json) = &field_object.json {
            debug!("📄 Field wrapper JSON retrieved for block {}", block_number);
            // Extract the actual block object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(block_object_id)) =
                        &value_field.kind
                    {
                        debug!("📄 Found block object ID: {}", block_object_id);
                        // Fetch the actual block object
                        let block_request = GetObjectRequest {
                            object_id: Some(block_object_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec!["object_id".to_string(), "json".to_string()],
                            }),
                        };

                        let block_response = client
                            .ledger_client()
                            .get_object(block_request)
                            .await
                            .map_err(|e| {
                                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                                    "Failed to fetch block object {}: {}",
                                    block_number, e
                                ))
                            })?;

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

/// Extract Block information from JSON
fn extract_block_info_from_json(
    json_value: &prost_types::Value,
    block_number: u64,
) -> Result<Option<Block>> {
    debug!(
        "🔍 Extracting block info for block {} from JSON",
        block_number
    );
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("🧱 Block {} fields: {:?}", block_number, struct_value.fields.keys().collect::<Vec<_>>());

        // Extract required fields - if any required field is missing, return None
        let name = match get_string(struct_value, "name") {
            Some(n) => n,
            None => {
                debug!("Missing required field 'name' for block {}", block_number);
                return Ok(None);
            }
        };

        let start_sequence = get_u64(struct_value, "start_sequence");
        let end_sequence = get_u64(struct_value, "end_sequence");
        let time_since_last_block = get_u64(struct_value, "time_since_last_block");
        let number_of_transactions = get_u64(struct_value, "number_of_transactions");
        let created_at = get_u64(struct_value, "created_at");

        // Helper function to extract Element<Scalar> bytes
        let extract_element_bytes = |field_name: &str| -> Vec<u8> {
            struct_value
                .fields
                .get(field_name)
                .and_then(|field| {
                    if let Some(prost_types::value::Kind::StructValue(element)) = &field.kind {
                        element.fields.get("bytes").and_then(|bytes_field| {
                            if let Some(prost_types::value::Kind::StringValue(base64_str)) =
                                &bytes_field.kind
                            {
                                general_purpose::STANDARD.decode(base64_str).ok()
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or_else(Vec::new)
        };

        // Extract commitment fields (Element<Scalar> with bytes field)
        // These are required fields in Move, so we extract them but use empty Vec if not found
        let actions_commitment = extract_element_bytes("actions_commitment");
        let state_commitment = extract_element_bytes("state_commitment");
        let start_actions_commitment = extract_element_bytes("start_actions_commitment");
        let end_actions_commitment = extract_element_bytes("end_actions_commitment");

        // Check if we got valid commitment data (at least actions_commitment should have data)
        if actions_commitment.is_empty() && state_commitment.is_empty() {
            debug!(
                "Warning: Block {} has empty commitment fields",
                block_number
            );
        }

        // Extract optional fields
        let state_data_availability = get_option_string(struct_value, "state_data_availability");
        let proof_data_availability = get_option_string(struct_value, "proof_data_availability");

        // Extract optional timestamp fields
        let state_calculated_at = get_option_u64(struct_value, "state_calculated_at");
        let proved_at = get_option_u64(struct_value, "proved_at");

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
            created_at,
            state_calculated_at,
            proved_at,
        };
        debug!(
            "✅ Successfully extracted block info for block {}: {}",
            block_number, block
        );
        return Ok(Some(block));
    }

    Ok(None)
}
