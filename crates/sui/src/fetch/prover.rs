use crate::error::{Result, SilvanaSuiInterfaceError};
use crate::fetch::AppInstance;
use crate::parse::{
    get_bool, get_option_string, get_option_u64, get_string, get_u8, get_u16, get_u64,
};
use crate::state::SharedSuiState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sui_rpc::client::v2::Client;
use sui_rpc::proto::sui::rpc::v2::{
    BatchGetObjectsRequest, GetObjectRequest, ListDynamicFieldsRequest,
};
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

/// Try to extract block number from a DynamicField either via `name.value` (BCS u64)
/// or, if not present, from the `field_object.json` representation of the Field wrapper.
pub(crate) fn extract_block_number_from_field(
    field: &sui_rpc::proto::sui::rpc::v2::DynamicField,
) -> Option<u64> {
    // Preferred: v2 path via name.value
    if let Some(name_bcs) = &field.name {
        if let Some(bytes) = &name_bcs.value {
            if let Ok(num) = bcs::from_bytes::<u64>(bytes) {
                return Some(num);
            }
        }
    }

    // Fallback: try to read from Field wrapper JSON
    if let Some(field_object) = &field.field_object {
        if let Some(json) = &field_object.json {
            if let Some(prost_types::value::Kind::StructValue(sv)) = &json.kind {
                if let Some(name_field) = sv.fields.get("name") {
                    match &name_field.kind {
                        Some(prost_types::value::Kind::StringValue(s)) => {
                            if let Ok(num) = s.parse::<u64>() {
                                return Some(num);
                            }
                        }
                        Some(prost_types::value::Kind::NumberValue(n)) => {
                            return Some(n.round() as u64);
                        }
                        Some(prost_types::value::Kind::StructValue(inner)) => {
                            // Handle Option::Some("...") style
                            if let Some(some) = inner.fields.get("Some") {
                                if let Some(prost_types::value::Kind::StringValue(s)) = &some.kind {
                                    if let Ok(num) = s.parse::<u64>() {
                                        return Some(num);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    None
}

/// Parse block number from a Field wrapper object's JSON.
/// Expected shapes observed on chain:
/// - { name: { name: "140" }, value: "0x..." }
/// - { name: "140", ... }
/// - { name: Some("140"), ... }
fn parse_block_number_from_wrapper_json(json: &prost_types::Value) -> Option<u64> {
    if let Some(prost_types::value::Kind::StructValue(sv)) = &json.kind {
        if let Some(name_val) = sv.fields.get("name") {
            match &name_val.kind {
                Some(prost_types::value::Kind::StringValue(s)) => return s.parse::<u64>().ok(),
                Some(prost_types::value::Kind::NumberValue(n)) => return Some(n.round() as u64),
                Some(prost_types::value::Kind::StructValue(inner)) => {
                    // Either Option::Some or nested { name: "..." }
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
    app_instance: &AppInstance,
    block_number: u64,
) -> Result<Option<ProofCalculation>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!(
        "Fetching ProofCalculation for block {} from app_instance {}",
        block_number, app_instance.id
    );

    // Use the proof_calculations table ID directly from the AppInstance
    let table_id = &app_instance.proof_calculations_table_id;
    debug!("🔍 Using proof_calculations table ID: {}", table_id);

    // Fetch the ProofCalculation from the ObjectTable
    fetch_proof_calculation_from_table(&mut client, table_id, block_number).await
}

/// Fetch multiple ProofCalculations from AppInstance for a range of block numbers  
/// Returns a HashMap of block_number -> ProofCalculation for all found proofs in the range
pub async fn fetch_proof_calculations_range(
    app_instance: &AppInstance,
    start_block: u64,
    end_block: u64,
) -> Result<HashMap<u64, ProofCalculation>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!(
        "Fetching ProofCalculations from {} to {} for app_instance {}",
        start_block, end_block, app_instance.id
    );

    // Get the proof_calculations table ID from the AppInstance
    let proof_calculations_table_id = &app_instance.proof_calculations_table_id;

    // Fetch all proof calculations in the range from the table
    fetch_proof_calculations_from_table_range(
        &mut client,
        proof_calculations_table_id,
        start_block,
        end_block,
    )
    .await
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
        // Always use minimal mask; decode keys via wrapper JSON when needed
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
            "📋 Page {}: Found {} dynamic fields in proof_calculations table",
            pages_searched,
            response.dynamic_fields.len()
        );

        // Collect field IDs and decode keys from wrapper JSONs
        let page_field_ids: Vec<String> = response
            .dynamic_fields
            .iter()
            .filter_map(|f| f.field_id.clone())
            .collect();

        if !page_field_ids.is_empty() {
            let before_len = field_ids_to_fetch.len();
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
                            if let Some(block_number) = parse_block_number_from_wrapper_json(json) {
                                if block_number >= start_block && block_number <= end_block {
                                    let fid = page_field_ids[i].clone();
                                    field_ids_to_fetch.push((fid, block_number));
                                }
                            }
                        }
                    }
                }
            }
            let added = field_ids_to_fetch.len() - before_len;
            debug!(
                "Added {} field ids from wrapper JSON decoding (range page)",
                added
            );
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
        field_ids_to_fetch.len(),
        start_block,
        end_block
    );

    if field_ids_to_fetch.is_empty() {
        // ProofCalculations not found - this is expected for settled blocks as they are deleted
        // after settlement to save on-chain storage (see update_block_settlement_tx_included_in_block)
        debug!(
            "No proof calculations found in range {}-{} after searching {} pages (may have been deleted after settlement)",
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

        // Extract proof object IDs from field wrappers
        let mut proof_object_ids = Vec::new(); // (proof_object_id, block_number)
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
                            if let Some(prost_types::value::Kind::StringValue(proof_object_id)) =
                                &value_field.kind
                            {
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

        debug!(
            "📦 Batch fetching {} proof calculation objects",
            proof_object_ids.len()
        );

        // Second batch: fetch all actual proof calculation objects
        let proof_requests: Vec<GetObjectRequest> = proof_object_ids
            .iter()
            .map(|(proof_id, _)| {
                let mut req = GetObjectRequest::default();
                req.object_id = Some(proof_id.clone());
                // version and read_mask remain None/default
                req
            })
            .collect();

        let mut batch_request = BatchGetObjectsRequest::default();
        batch_request.requests = proof_requests;
        batch_request.read_mask = Some(prost_types::FieldMask {
            paths: vec!["object_id".to_string(), "json".to_string()],
        });

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
            if let Some(sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(
                proof_object,
            )) = &get_result.result
            {
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
        // Always use minimal mask; decode key via wrapper JSON
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
            "📋 Page {}: Found {} dynamic fields in proof_calculations table",
            pages_searched,
            response.dynamic_fields.len()
        );

        // Decode via batch wrapper JSON
        if !response.dynamic_fields.is_empty() {
            let page_field_ids: Vec<String> = response
                .dynamic_fields
                .iter()
                .filter_map(|f| f.field_id.clone())
                .collect();
            if !page_field_ids.is_empty() {
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
                        if let Some(
                            sui_rpc::proto::sui::rpc::v2::get_object_result::Result::Object(obj),
                        ) = &gr.result
                        {
                            if let Some(json) = &obj.json {
                                if let Some(num) = parse_block_number_from_wrapper_json(json) {
                                    if num == target_block_number {
                                        let fid = page_field_ids[i].clone();
                                        debug!(
                                            "✅ Found matching block {} via wrapper JSON; field {}",
                                            num, fid
                                        );
                                        if let Ok(Some(proof)) = fetch_proof_object_by_field_id(
                                            client,
                                            &fid,
                                            target_block_number,
                                        )
                                        .await
                                        {
                                            return Ok(Some(proof));
                                        }
                                    }
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

    // ProofCalculation not found - this is expected for settled blocks as they are deleted
    // after settlement to save on-chain storage (see update_block_settlement_tx_included_in_block)
    debug!(
        "ProofCalculation not found for block {} after searching {} pages (may have been deleted after settlement)",
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
                        let mut proof_request = GetObjectRequest::default();
                        proof_request.object_id = Some(proof_object_id.clone());
                        proof_request.read_mask = Some(prost_types::FieldMask {
                            paths: vec!["object_id".to_string(), "json".to_string()],
                        });

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
                                    warn!("❌ Failed to extract proof calculation from JSON");
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
                                            //debug!("🔍 Extracted proof: {:?}", proof);
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
        //debug!("✅ Extracted proof calculation: {:?}", proof_calculation);
        return Some(proof_calculation);
    }

    None
}

/// Extract individual Proof information from Move Proof struct
fn extract_individual_proof(
    proof_struct: &prost_types::Struct,
    sequences: Vec<u64>,
) -> Option<Proof> {
    // debug!(
    //     "🔍 Extracting individual proof with sequences: {:?}",
    //     sequences
    // );
    // debug!(
    //     "🔍 Proof struct fields: {:?}",
    //     proof_struct.fields.keys().collect::<Vec<_>>()
    // );

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
