use crate::error::{Result, SilvanaSuiInterfaceError};
use crate::fetch::AppInstance;
use crate::parse::{get_option_string, get_option_u64, get_string, get_u64};
use crate::state::SharedSuiState;
use base64::{Engine as _, engine::general_purpose};
use std::collections::HashMap;
use std::fmt;
use sui_rpc::client::v2::Client;
use sui_rpc::proto::sui::rpc::v2::{
    BatchGetObjectsRequest, GetObjectRequest, ListDynamicFieldsRequest,
};
use tracing::{debug, info, warn};

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
        write!(
            f,
            "Block {{ name: \"{}\", block_number: {}, start_sequence: {}, end_sequence: {}, time_since_last_block: {}, number_of_transactions: {}, state_data_availability: {:?}, proof_data_availability: {:?}, created_at: {}, state_calculated_at: {:?}, proved_at: {:?} }}",
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

/// Parse block number from a Field wrapper object's JSON, tolerant to shapes like:
/// - { name: "140", value: "0x..." }
/// - { name: { name: "140" }, value: "0x..." }
/// - { name: { Some: "140" }, value: "0x..." }
fn parse_block_number_from_wrapper_json(json: &prost_types::Value) -> Option<u64> {
    if let Some(prost_types::value::Kind::StructValue(sv)) = &json.kind {
        if let Some(name_val) = sv.fields.get("name") {
            match &name_val.kind {
                Some(prost_types::value::Kind::StringValue(s)) => return s.parse::<u64>().ok(),
                Some(prost_types::value::Kind::NumberValue(n)) => return Some(n.round() as u64),
                Some(prost_types::value::Kind::StructValue(inner)) => {
                    if let Some(inner_name) = inner.fields.get("name") {
                        if let Some(prost_types::value::Kind::StringValue(s)) = &inner_name.kind {
                            return s.parse::<u64>().ok();
                        }
                    }
                    if let Some(some) = inner.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(s)) = &some.kind {
                            return s.parse::<u64>().ok();
                        }
                        if let Some(prost_types::value::Kind::NumberValue(n)) = &some.kind {
                            return Some(n.round() as u64);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
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
        // Always use minimal mask on this endpoint
        let mut request = ListDynamicFieldsRequest::default();
        request.parent = Some(table_id.to_string());
        request.page_size = Some(PAGE_SIZE);
        request.page_token = page_token.clone();
        request.read_mask = Some(prost_types::FieldMask {
            paths: vec![
                "parent".to_string(),
                "field_id".to_string(),
                "kind".to_string(),
            ],
        });

        let fields_response = client
            .state_client()
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

        // Collect field IDs for blocks in our range (from DynamicField if available)
        let mut page_field_ids: Vec<String> = Vec::new();
        let mut added_from_dynamic = 0usize;
        for field in &response.dynamic_fields {
            if let Some(name) = &field.name {
                if let Some(name_value) = &name.value {
                    if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                        if field_block_number >= start_block && field_block_number <= end_block {
                            if let Some(field_id) = &field.field_id {
                                field_ids_to_fetch.push((field_id.clone(), field_block_number));
                                added_from_dynamic += 1;
                            }
                        }
                    }
                }
            }
            if let Some(fid) = &field.field_id {
                page_field_ids.push(fid.clone());
            }
        }

        // If server didn't return name, decode block numbers from wrapper JSON
        if added_from_dynamic == 0 && !page_field_ids.is_empty() {
            let mut batch_request = BatchGetObjectsRequest::default();
            batch_request.requests = page_field_ids
                .iter()
                .map(|fid| {
                    let mut r = GetObjectRequest::default();
                    r.object_id = Some(fid.clone());
                    r
                })
                .collect();
            batch_request.read_mask = Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            });
            if let Ok(batch_response) = client
                .ledger_client()
                .batch_get_objects(batch_request)
                .await
            {
                let results = batch_response.into_inner().objects;
                for (i, gr) in results.iter().enumerate() {
                    if let Some(sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(
                        obj,
                    )) = &gr.result
                    {
                        if let Some(json) = &obj.json {
                            if let Some(field_block_number) =
                                parse_block_number_from_wrapper_json(json)
                            {
                                if field_block_number >= start_block
                                    && field_block_number <= end_block
                                {
                                    let fid = page_field_ids[i].clone();
                                    field_ids_to_fetch.push((fid, field_block_number));
                                }
                            }
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
            .map(|(field_id, _)| {
                let mut req = GetObjectRequest::default();
                req.object_id = Some(field_id.clone());
                // version and read_mask remain None/default
                req
            })
            .collect();

        let mut batch_request = BatchGetObjectsRequest::default();
        batch_request.requests = field_requests;
        batch_request.read_mask = Some(prost_types::FieldMask {
            paths: vec!["object_id".to_string(), "json".to_string()],
        });

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
            if let Some(sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(
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
            .map(|(block_id, _)| {
                let mut req = GetObjectRequest::default();
                req.object_id = Some(block_id.clone());
                // version and read_mask remain None/default
                req
            })
            .collect();

        let mut batch_request = BatchGetObjectsRequest::default();
        batch_request.requests = block_requests;
        batch_request.read_mask = Some(prost_types::FieldMask {
            paths: vec!["object_id".to_string(), "json".to_string()],
        });

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
            if let Some(sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(
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
        // Always use minimal mask on this endpoint
        let mut request = ListDynamicFieldsRequest::default();
        request.parent = Some(table_id.to_string());
        request.page_size = Some(PAGE_SIZE);
        request.page_token = page_token.clone();
        request.read_mask = Some(prost_types::FieldMask {
            paths: vec![
                "parent".to_string(),
                "field_id".to_string(),
                "kind".to_string(),
            ],
        });

        let fields_response = client
            .state_client()
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
        let mut page_field_ids: Vec<String> = Vec::new();
        let mut found_in_dynamic = false;
        for field in &response.dynamic_fields {
            if let Some(name) = &field.name {
                if let Some(name_value) = &name.value {
                    if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                        all_found_blocks.push(field_block_number);
                        if field_block_number == block_number {
                            debug!(
                                "🎯 Found matching block {} in dynamic fields (page {})",
                                block_number, pages_searched
                            );
                            if let Some(field_id) = &field.field_id {
                                return fetch_block_object_by_field_id(
                                    client,
                                    field_id,
                                    block_number,
                                )
                                .await;
                            }
                            found_in_dynamic = true;
                        }
                    }
                }
            }
            if let Some(fid) = &field.field_id {
                page_field_ids.push(fid.clone());
            }
        }

        // If not found, try wrapper JSON to decode key and match
        if !found_in_dynamic && !page_field_ids.is_empty() {
            let mut batch_request = BatchGetObjectsRequest::default();
            batch_request.requests = page_field_ids
                .iter()
                .map(|fid| {
                    let mut r = GetObjectRequest::default();
                    r.object_id = Some(fid.clone());
                    r
                })
                .collect();
            batch_request.read_mask = Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            });
            if let Ok(batch_response) = client
                .ledger_client()
                .batch_get_objects(batch_request)
                .await
            {
                let objects = batch_response.into_inner().objects;
                for (i, gr) in objects.iter().enumerate() {
                    if let Some(sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(
                        obj,
                    )) = &gr.result
                    {
                        if let Some(json) = &obj.json {
                            if let Some(num) = parse_block_number_from_wrapper_json(json) {
                                all_found_blocks.push(num);
                                if num == block_number {
                                    let fid = page_field_ids[i].clone();
                                    debug!(
                                        "✅ Found matching block {} via wrapper JSON; field {}",
                                        num, fid
                                    );
                                    return fetch_block_object_by_field_id(
                                        client,
                                        &fid,
                                        block_number,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
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

    info!(
        "lock {} not found in table {} after searching {} pages. 📊 Total unique blocks found: {}.",
        block_number,
        table_id,
        pages_searched,
        all_found_blocks.len(),
    );

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
    let mut field_request = GetObjectRequest::default();
    field_request.object_id = Some(field_id.to_string());
    field_request.read_mask = Some(prost_types::FieldMask {
        paths: vec!["object_id".to_string(), "json".to_string()],
    });

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
                        let mut block_request = GetObjectRequest::default();
                        block_request.object_id = Some(block_object_id.clone());
                        block_request.read_mask = Some(prost_types::FieldMask {
                            paths: vec!["object_id".to_string(), "json".to_string()],
                        });

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
