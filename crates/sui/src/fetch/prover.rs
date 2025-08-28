use crate::error::{Result, SilvanaSuiInterfaceError};
use crate::fetch::AppInstance;
use crate::parse::{
    get_bool, get_option_string, get_option_u64, get_string, get_u8, get_u16, get_u64,
};
use crate::state::SharedSuiState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest, BatchGetObjectsRequest};
use tracing::{debug, warn};

// Constants matching Move definitions
pub const PROOF_STATUS_STARTED: u8 = 1;
pub const PROOF_STATUS_CALCULATED: u8 = 2;
pub const PROOF_STATUS_REJECTED: u8 = 3;
pub const PROOF_STATUS_RESERVED: u8 = 4;
pub const PROOF_STATUS_USED: u8 = 5;

/// Helper enum for proof status (matches Move constants)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProofStatus {
    Started,    // PROOF_STATUS_STARTED = 1
    Calculated, // PROOF_STATUS_CALCULATED = 2
    Rejected,   // PROOF_STATUS_REJECTED = 3
    Reserved,   // PROOF_STATUS_RESERVED = 4
    Used,       // PROOF_STATUS_USED = 5
}

impl ProofStatus {
    pub fn from_u8(status: u8) -> Self {
        match status {
            1 => ProofStatus::Started,
            2 => ProofStatus::Calculated,
            3 => ProofStatus::Rejected,
            4 => ProofStatus::Reserved,
            5 => ProofStatus::Used,
            _ => ProofStatus::Rejected, // Default for unknown status
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            ProofStatus::Started => PROOF_STATUS_STARTED,
            ProofStatus::Calculated => PROOF_STATUS_CALCULATED,
            ProofStatus::Rejected => PROOF_STATUS_REJECTED,
            ProofStatus::Reserved => PROOF_STATUS_RESERVED,
            ProofStatus::Used => PROOF_STATUS_USED,
        }
    }
}

/// Proof struct matching Move definition
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Proof {
    pub status: ProofStatus,
    pub da_hash: Option<String>,
    pub sequence1: Option<Vec<u64>>,
    pub sequence2: Option<Vec<u64>>,
    pub rejected_count: u16,
    pub timestamp: u64,
    pub prover: String,       // address in Move
    pub user: Option<String>, // Option<address> in Move
    pub job_id: String,
    // Additional field for the key in VecMap
    pub sequences: Vec<u64>,
}

/// ProofCalculation struct matching Move definition
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProofCalculation {
    pub id: String, // UID in Move
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    pub proofs: Vec<Proof>, // VecMap<vector<u64>, Proof> in Move
    pub block_proof: Option<String>,
    pub is_finished: bool,
}

/// Fetch ProofCalculation for a block from AppInstance (legacy single-block function)
pub async fn fetch_proof_calculation(
    app_instance: &str,
    block_number: u64,
) -> Result<Option<ProofCalculation>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!(
        "Fetching ProofCalculation for block {} from app_instance {}",
        block_number, app_instance
    );

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
            paths: vec!["object_id".to_string(), "json".to_string()],
        }),
    };

    let object_response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch app_instance {}: {}",
                app_instance, e
            ))
        })?;

    let response = object_response.into_inner();

    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                debug!(
                    "📋 AppInstance {} fields: {:?}",
                    app_instance,
                    struct_value.fields.keys().collect::<Vec<_>>()
                );

                // Get the proof_calculations ObjectTable
                if let Some(proofs_field) = struct_value.fields.get("proof_calculations") {
                    debug!(
                        "🔍 Found proof_calculations field in AppInstance {}",
                        app_instance
                    );
                    if let Some(prost_types::value::Kind::StructValue(proofs_struct)) =
                        &proofs_field.kind
                    {
                        debug!(
                            "🔍 Proof calculations struct fields: {:?}",
                            proofs_struct.fields.keys().collect::<Vec<_>>()
                        );
                        if let Some(table_id_field) = proofs_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) =
                                &table_id_field.kind
                            {
                                debug!("🔍 Found proof_calculations table ID: {}", table_id);
                                // Fetch the ProofCalculation from the ObjectTable
                                return fetch_proof_calculation_from_table(
                                    &mut client,
                                    table_id,
                                    block_number,
                                )
                                .await;
                            } else {
                                warn!(
                                    "❌ Proof calculations table id field is not a string: {:?}",
                                    table_id_field.kind
                                );
                            }
                        } else {
                            warn!("❌ No 'id' field in proof calculations table struct");
                        }
                    } else {
                        warn!(
                            "❌ Proof calculations field is not a struct: {:?}",
                            proofs_field.kind
                        );
                    }
                } else {
                    warn!(
                        "❌ No 'proof_calculations' field found in AppInstance. Available fields: {:?}",
                        struct_value.fields.keys().collect::<Vec<_>>()
                    );
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

/// Fetch multiple ProofCalculations from AppInstance for a range of block numbers  
/// Returns a HashMap of block_number -> ProofCalculation for all found proofs in the range
pub async fn fetch_proof_calculations_range(
    app_instance: &AppInstance,
    start_block: u64,
    end_block: u64,
) -> Result<HashMap<u64, ProofCalculation>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching ProofCalculations from {} to {} for app_instance {}",
        start_block, end_block, app_instance.id);
    
    // Get the proof_calculations table ID from the AppInstance
    let proof_calculations_table_id = &app_instance.proof_calculations_table_id;
    
    // Fetch all proof calculations in the range from the table
    fetch_proof_calculations_from_table_range(
        &mut client,
        proof_calculations_table_id, 
        start_block,
        end_block
    ).await
}

/// Fetch multiple ProofCalculations from ObjectTable for a range of block numbers
/// Returns a HashMap of block_number -> ProofCalculation for all found proofs
async fn fetch_proof_calculations_from_table_range(
    client: &mut Client,
    table_id: &str,
    start_block: u64,
    end_block: u64,
) -> Result<HashMap<u64, ProofCalculation>> {
    debug!(
        "🔍 Fetching proof calculations {} to {} from table {}",
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
            "📋 Page {}: Found {} dynamic fields in proof_calculations table",
            pages_searched,
            response.dynamic_fields.len()
        );

        // Collect field IDs for blocks in our range
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                if let Ok(block_number) = bcs::from_bytes::<u64>(name_value) {
                    // Check if this block is in our desired range
                    if block_number >= start_block && block_number <= end_block {
                        if let Some(field_id) = &field.field_id {
                            field_ids_to_fetch.push((field_id.clone(), block_number));
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
        "📋 Collected {} field IDs for proof calculations in range {}-{}",
        field_ids_to_fetch.len(), start_block, end_block
    );

    if field_ids_to_fetch.is_empty() {
        warn!(
            "❌ No proof calculations found in range {}-{} after searching {} pages",
            start_block, end_block, pages_searched
        );
        return Ok(HashMap::new());
    }

    // Now batch fetch all proof calculations
    fetch_proof_objects_batch(client, field_ids_to_fetch).await
}

/// Batch fetch multiple proof calculations at once
async fn fetch_proof_objects_batch(
    client: &mut Client,
    field_ids_with_blocks: Vec<(String, u64)>, // (field_id, block_number)
) -> Result<HashMap<u64, ProofCalculation>> {
    let mut proofs_map = HashMap::new();
    
    // Process in batches of 50 to avoid overwhelming the RPC
    const BATCH_SIZE: usize = 50;
    
    for chunk in field_ids_with_blocks.chunks(BATCH_SIZE) {
        debug!("📦 Batch fetching {} proof field wrappers", chunk.len());
        
        // First batch: fetch all field wrapper objects to get the actual proof object IDs
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
        
        // Extract proof object IDs from field wrappers
        let mut proof_object_ids = Vec::new(); // (proof_object_id, block_number)
        for (i, get_result) in field_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(field_object)) = &get_result.result {
                if let Some(field_json) = &field_object.json {
                    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                        if let Some(value_field) = struct_value.fields.get("value") {
                            if let Some(prost_types::value::Kind::StringValue(proof_object_id)) = &value_field.kind {
                                let (_, block_number) = chunk[i];
                                proof_object_ids.push((proof_object_id.clone(), block_number));
                            }
                        }
                    }
                }
            }
        }
        
        if proof_object_ids.is_empty() {
            continue;
        }
        
        debug!("📦 Batch fetching {} proof calculation objects", proof_object_ids.len());
        
        // Second batch: fetch all actual proof calculation objects
        let proof_requests: Vec<GetObjectRequest> = proof_object_ids
            .iter()
            .map(|(proof_id, _)| GetObjectRequest {
                object_id: Some(proof_id.clone()),
                version: None,
                read_mask: None, // Use batch-level mask instead
            })
            .collect();
        
        let batch_request = BatchGetObjectsRequest {
            requests: proof_requests,
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
                    "Failed to batch fetch proof calculations: {}",
                    e
                ))
            })?;
        
        let proof_results = batch_response.into_inner().objects;
        
        // Extract ProofCalculation data from results
        for (i, get_result) in proof_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(proof_object)) = &get_result.result {
                if let Some(proof_json) = &proof_object.json {
                    if let Some(proof_info) = extract_proof_calculation_from_json(proof_json) {
                        let (_, block_number) = proof_object_ids[i];
                        proofs_map.insert(block_number, proof_info);
                    }
                }
            }
        }
    }
    
    debug!(
        "📊 Successfully fetched {} proof calculations",
        proofs_map.len()
    );
    
    Ok(proofs_map)
}

/// Fetch ProofCalculation from ObjectTable for a specific block number (legacy single-block function)
async fn fetch_proof_calculation_from_table(
    client: &mut Client,
    table_id: &str,
    target_block_number: u64,
) -> Result<Option<ProofCalculation>> {
    debug!(
        "🔍 Searching for proofs for block {} in table {}",
        target_block_number, table_id
    );

    let mut page_token = None;
    const PAGE_SIZE: u32 = 100; // Smaller page size to reduce data transfer
    let mut pages_searched = 0;
    const MAX_PAGES: u32 = 200; // Higher limit for proofs as there may be many

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
            "📋 Page {}: Found {} dynamic fields in proof_calculations table",
            pages_searched,
            response.dynamic_fields.len()
        );

        // Search in current page for proof calculations matching our target block
        for field in &response.dynamic_fields {
            // The name_value contains the block number as BCS-encoded u64
            if let Some(name_value) = &field.name_value {
                // Decode the block number from BCS bytes
                if let Ok(block_number) = bcs::from_bytes::<u64>(name_value) {
                    debug!(
                        "🔍 Dynamic field - Block: {}, Field: {:?}",
                        block_number, field
                    );

                    // Only fetch if this is our target block
                    if block_number == target_block_number {
                        if let Some(field_id) = &field.field_id {
                            debug!(
                                "✅ Found matching block {}! Fetching proof calculation from field ID: {}",
                                block_number, field_id
                            );

                            // Fetch the proof calculation for this specific block
                            if let Ok(Some(proof_calculation)) = fetch_proof_object_by_field_id(
                                client,
                                field_id,
                                target_block_number,
                            )
                            .await
                            {
                                debug!(
                                    "✅ Successfully fetched proof for block {}. Full ProofCalculation: {:?}",
                                    target_block_number, proof_calculation
                                );
                                return Ok(Some(proof_calculation));
                            }
                        }
                    } else {
                        debug!(
                            "⏩ Skipping block {} (looking for block {})",
                            block_number, target_block_number
                        );
                    }
                } else {
                    warn!(
                        "❌ Failed to deserialize block number from name_value: {:?}",
                        name_value
                    );
                }
            } else {
                warn!("❌ No name_value found in dynamic field");
            }
        }

        // Check if we should continue pagination
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() && pages_searched < MAX_PAGES {
                page_token = Some(next_token);
                debug!(
                    "⏭️ Continuing to page {} to search for proofs for block {}",
                    pages_searched + 1,
                    target_block_number
                );
            } else {
                break;
            }
        } else {
            break;
        }
    }

    warn!(
        "ProofCalculation not found for block {} after searching {} pages",
        target_block_number, pages_searched
    );
    Ok(None)
}

/// Fetch ProofCalculation object by field ID (already verified to be the correct block)
async fn fetch_proof_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    target_block_number: u64,
) -> Result<Option<ProofCalculation>> {
    debug!(
        "📄 Fetching proof calculation from field {} for block {}",
        field_id, target_block_number
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
                "Failed to fetch field wrapper: {}",
                e
            ))
        })?;

    let field_response_inner = field_response.into_inner();
    if let Some(field_object) = field_response_inner.object {
        if let Some(field_json) = &field_object.json {
            debug!("📄 Field wrapper JSON retrieved");
            // Extract the actual proof calculation object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(proof_object_id)) =
                        &value_field.kind
                    {
                        debug!("📄 Found proof object ID: {}", proof_object_id);
                        // Fetch the actual proof calculation object
                        let proof_request = GetObjectRequest {
                            object_id: Some(proof_object_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec!["object_id".to_string(), "json".to_string()],
                            }),
                        };

                        let proof_response = client
                            .ledger_client()
                            .get_object(proof_request)
                            .await
                            .map_err(|e| {
                                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                                    "Failed to fetch proof calculation object: {}",
                                    e
                                ))
                            })?;

                        if let Some(proof_object) = proof_response.into_inner().object {
                            if let Some(proof_json) = &proof_object.json {
                                debug!("🔍 Proof calculation JSON retrieved, extracting data");
                                // We already know this is the correct block from the name_value check
                                // so we can directly extract the proof calculation
                                if let Some(proof_info) =
                                    extract_proof_calculation_from_json(proof_json)
                                {
                                    return Ok(Some(proof_info));
                                } else {
                                    warn!(
                                        "❌ Failed to extract proof calculation from JSON"
                                    );
                                }
                            } else {
                                warn!("❌ No JSON found for proof object");
                            }
                        } else {
                            warn!("❌ No proof object found");
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}


/// Extract ProofCalculation information from JSON
fn extract_proof_calculation_from_json(
    json_value: &prost_types::Value,
) -> Option<ProofCalculation> {
    debug!("🔍 Extracting proof calculation from JSON");
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        debug!(
            "🔍 Proof calculation fields: {:?}",
            struct_value.fields.keys().collect::<Vec<_>>()
        );

        // Extract required fields using helper functions
        let id = get_string(struct_value, "id")?;
        let block_number = get_u64(struct_value, "block_number");
        let start_sequence = get_u64(struct_value, "start_sequence");
        let end_sequence = get_option_u64(struct_value, "end_sequence");
        let is_finished = get_bool(struct_value, "is_finished");
        let block_proof = get_option_string(struct_value, "block_proof");

        // Extract proofs (VecMap<vector<u64>, Proof>) - required field but can be empty
        let proofs = if let Some(proofs_field) = struct_value.fields.get("proofs") {
            if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                if let Some(contents_field) = proofs_struct.fields.get("contents") {
                    if let Some(prost_types::value::Kind::ListValue(list_value)) =
                        &contents_field.kind
                    {
                        debug!("🔍 Found {} proofs in VecMap", list_value.values.len());

                        let mut proofs_vec = Vec::new();
                        for proof_entry in &list_value.values {
                            if let Some(prost_types::value::Kind::StructValue(entry_struct)) =
                                &proof_entry.kind
                            {
                                // Extract key (sequences) and value (Proof)
                                if let (Some(key_field), Some(value_field)) = (
                                    entry_struct.fields.get("key"),
                                    entry_struct.fields.get("value"),
                                ) {
                                    // Extract sequences from key
                                    let mut sequences = Vec::new();
                                    if let Some(prost_types::value::Kind::ListValue(key_list)) =
                                        &key_field.kind
                                    {
                                        for seq_value in &key_list.values {
                                            if let Some(prost_types::value::Kind::StringValue(
                                                seq_str,
                                            )) = &seq_value.kind
                                            {
                                                if let Ok(seq) = seq_str.parse::<u64>() {
                                                    sequences.push(seq);
                                                }
                                            }
                                        }
                                    }

                                    // Extract Proof struct from value
                                    if let Some(prost_types::value::Kind::StructValue(
                                        proof_struct,
                                    )) = &value_field.kind
                                    {
                                        if let Some(proof) =
                                            extract_individual_proof(proof_struct, sequences)
                                        {
                                            debug!("🔍 Extracted proof: {:?}", proof);
                                            proofs_vec.push(proof);
                                        }
                                    }
                                }
                            }
                        }
                        proofs_vec
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Create ProofCalculation struct
        let proof_calculation = ProofCalculation {
            id,
            block_number,
            start_sequence,
            end_sequence,
            proofs,
            block_proof,
            is_finished,
        };
        debug!("✅ Extracted proof calculation: {:?}", proof_calculation);
        return Some(proof_calculation);
    }

    None
}

/// Extract individual Proof information from Move Proof struct
fn extract_individual_proof(
    proof_struct: &prost_types::Struct,
    sequences: Vec<u64>,
) -> Option<Proof> {
    debug!(
        "🔍 Extracting individual proof with sequences: {:?}",
        sequences
    );
    debug!(
        "🔍 Proof struct fields: {:?}",
        proof_struct.fields.keys().collect::<Vec<_>>()
    );

    // Extract fields using helper functions
    let status_u8 = get_u8(proof_struct, "status");
    let status = ProofStatus::from_u8(status_u8);
    let da_hash = get_option_string(proof_struct, "da_hash");
    let timestamp = get_u64(proof_struct, "timestamp");
    let sequence1 = proof_struct
        .fields
        .get("sequence1")
        .and_then(extract_optional_sequence_list);
    let sequence2 = proof_struct
        .fields
        .get("sequence2")
        .and_then(extract_optional_sequence_list);
    let rejected_count = get_u16(proof_struct, "rejected_count");
    let prover = get_string(proof_struct, "prover")?;
    let user = get_option_string(proof_struct, "user");
    let job_id = get_string(proof_struct, "job_id")?;

    // Create Proof struct
    Some(Proof {
        status,
        da_hash,
        sequence1,
        sequence2,
        rejected_count,
        timestamp,
        prover,
        user,
        job_id,
        sequences,
    })
}

/// Helper function to extract optional sequence list from JSON
fn extract_optional_sequence_list(value: &prost_types::Value) -> Option<Vec<u64>> {
    match &value.kind {
        Some(prost_types::value::Kind::StructValue(option_struct)) => {
            if let Some(some_field) = option_struct.fields.get("Some") {
                if let Some(prost_types::value::Kind::ListValue(list_value)) = &some_field.kind {
                    let mut sequences = Vec::new();
                    for seq_value in &list_value.values {
                        if let Some(prost_types::value::Kind::StringValue(seq_str)) =
                            &seq_value.kind
                        {
                            if let Ok(seq) = seq_str.parse::<u64>() {
                                sequences.push(seq);
                            }
                        }
                    }
                    return Some(sequences);
                }
            }
            None
        }
        Some(prost_types::value::Kind::ListValue(list_value)) => {
            let mut sequences = Vec::new();
            for seq_value in &list_value.values {
                if let Some(prost_types::value::Kind::StringValue(seq_str)) = &seq_value.kind {
                    if let Ok(seq) = seq_str.parse::<u64>() {
                        sequences.push(seq);
                    }
                }
            }
            Some(sequences)
        }
        _ => None,
    }
}
