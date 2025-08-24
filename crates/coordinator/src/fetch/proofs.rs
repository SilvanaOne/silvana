use crate::error::{CoordinatorError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, warn};

/// Individual proof information extracted from ProofCalculation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProofInfo {
    pub sequences: Vec<u64>,
    pub status: u8, // 0=STARTED, 1=CALCULATED, 2=REJECTED, 3=RESERVED, 4=USED
    pub da_hash: Option<String>,
    pub timestamp: u64,
    pub merged_sequences_1: Option<Vec<u64>>,
    pub merged_sequences_2: Option<Vec<u64>>,
    pub job_id: String,
}

/// ProofCalculation information fetched from blockchain
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProofCalculationInfo {
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    pub is_finished: bool,
    pub block_proof: Option<String>,
    pub individual_proofs: Vec<ProofInfo>,
}

/// Fetch all ProofCalculations for a block from AppInstance
pub async fn fetch_proof_calculations(
    client: &mut Client,
    app_instance: &str,
    block_number: u64,
) -> Result<Vec<ProofCalculationInfo>> {
    debug!("Fetching ProofCalculations for block {} from app_instance {}", block_number, app_instance);
    
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
                debug!("📋 AppInstance {} fields: {:?}", app_instance, struct_value.fields.keys().collect::<Vec<_>>());
                
                // Get the proof_calculations ObjectTable
                if let Some(proofs_field) = struct_value.fields.get("proof_calculations") {
                    debug!("🔍 Found proof_calculations field in AppInstance {}", app_instance);
                    if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                        debug!("🔍 Proof calculations struct fields: {:?}", proofs_struct.fields.keys().collect::<Vec<_>>());
                        if let Some(table_id_field) = proofs_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                debug!("🔍 Found proof_calculations table ID: {}", table_id);
                                // Fetch all ProofCalculations from the ObjectTable
                                return fetch_proof_calculations_from_table(client, table_id, block_number).await;
                            } else {
                                warn!("❌ Proof calculations table id field is not a string: {:?}", table_id_field.kind);
                            }
                        } else {
                            warn!("❌ No 'id' field in proof calculations table struct");
                        }
                    } else {
                        warn!("❌ Proof calculations field is not a struct: {:?}", proofs_field.kind);
                    }
                } else {
                    warn!("❌ No 'proof_calculations' field found in AppInstance. Available fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
                }
            }
        } else {
            warn!("❌ No JSON found for AppInstance {}", app_instance);
        }
    } else {
        warn!("❌ No object returned for AppInstance {}", app_instance);
    }
    
    Ok(vec![])
}

/// Fetch ProofCalculations from ObjectTable, filtering by block number using optimized pagination
async fn fetch_proof_calculations_from_table(
    client: &mut Client,
    table_id: &str,
    target_block_number: u64,
) -> Result<Vec<ProofCalculationInfo>> {
    debug!("🔍 Optimized search for proofs for block {} in table {}", target_block_number, table_id);
    
    let mut page_token = None;
    const PAGE_SIZE: u32 = 50; // Smaller page size to reduce data transfer
    let mut pages_searched = 0;
    const MAX_PAGES: u32 = 200; // Higher limit for proofs as there may be many
    let mut proofs = Vec::new();
    
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
        debug!("📋 Page {}: Found {} dynamic fields in proof_calculations table", pages_searched, response.dynamic_fields.len());
        
        // Search in current page for proof calculations matching our target block
        for field in &response.dynamic_fields {
            if let Some(field_id) = &field.field_id {
                // Fetch and check if this proof calculation is for our target block
                if let Ok(Some(proof_info)) = fetch_proof_object_by_field_id(client, field_id, target_block_number).await {
                    debug!("✅ Found proof for block {}, adding to results", target_block_number);
                    proofs.push(proof_info);
                }
            }
        }
        
        // Check if we should continue pagination
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() && pages_searched < MAX_PAGES {
                page_token = Some(next_token);
                debug!("⏭️ Continuing to page {} to search for proofs for block {}", pages_searched + 1, target_block_number);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    debug!("Found {} ProofCalculations for block {} after searching {} pages", proofs.len(), target_block_number, pages_searched);
    Ok(proofs)
}

/// Fetch ProofCalculation object by field ID, only if it matches the target block number
async fn fetch_proof_object_by_field_id(
    client: &mut Client,
    field_id: &str,
    target_block_number: u64,
) -> Result<Option<ProofCalculationInfo>> {
    debug!("📄 Fetching proof calculation from field {} (filtering for block {})", field_id, target_block_number);
    
    // Fetch each ProofCalculation
    let proof_request = GetObjectRequest {
        object_id: Some(field_id.to_string()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };

    let proof_response = client.ledger_client().get_object(proof_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
        format!("Failed to fetch proof calculation: {}", e)
    ))?;
    
    let proof_response_inner = proof_response.into_inner();
    if let Some(proof_object) = proof_response_inner.object {
        if let Some(proof_json) = &proof_object.json {
            debug!("📄 Proof field wrapper JSON retrieved");
            // Extract the actual proof calculation object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &proof_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(proof_object_id)) = &value_field.kind {
                        debug!("📄 Found proof object ID: {}", proof_object_id);
                        // Fetch the actual proof calculation object
                        let actual_proof_request = GetObjectRequest {
                            object_id: Some(proof_object_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let actual_proof_response = client.ledger_client().get_object(actual_proof_request).await.map_err(|e| CoordinatorError::RpcConnectionError(
                            format!("Failed to fetch actual proof calculation: {}", e)
                        ))?;
                        
                        if let Some(actual_proof_object) = actual_proof_response.into_inner().object {
                            if let Some(actual_proof_json) = &actual_proof_object.json {
                                debug!("🔍 Proof calculation JSON retrieved successfully");
                                // First check block number before expensive proof extraction
                                if let Some(block_number) = extract_block_number_from_json(actual_proof_json) {
                                    if block_number == target_block_number {
                                        debug!("✅ Block number {} matches target, extracting full proof calculation", block_number);
                                        if let Some(proof_info) = extract_proof_calculation_from_json(actual_proof_json) {
                                            return Ok(Some(proof_info));
                                        } else {
                                            warn!("❌ Failed to extract proof calculation from JSON");
                                        }
                                    } else {
                                        //debug!("❌ Block number {} doesn't match target block {}, skipping expensive proof extraction", block_number, target_block_number);
                                    }
                                } else {
                                    warn!("❌ Failed to extract block number from proof calculation JSON");
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

/// Extract only block number from JSON (lightweight check before full extraction)
fn extract_block_number_from_json(json_value: &prost_types::Value) -> Option<u64> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        if let Some(block_field) = struct_value.fields.get("block_number") {
            if let Some(prost_types::value::Kind::StringValue(block_str)) = &block_field.kind {
                return block_str.parse().ok();
            }
        }
    }
    None
}

/// Extract ProofCalculation information from JSON
fn extract_proof_calculation_from_json(json_value: &prost_types::Value) -> Option<ProofCalculationInfo> {
    debug!("🔍 Extracting proof calculation from JSON");
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        debug!("🔍 Proof calculation fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        let mut block_number = 0u64;
        let mut start_sequence = 0u64;
        let mut end_sequence: Option<u64> = None;
        let mut is_finished = false;
        let mut block_proof: Option<String> = None;
        let mut individual_proofs = Vec::new();
        
        // Extract block_number
        if let Some(block_field) = struct_value.fields.get("block_number") {
            if let Some(prost_types::value::Kind::StringValue(block_str)) = &block_field.kind {
                block_number = block_str.parse().unwrap_or(0);
            }
        }
        
        // Extract start_sequence
        if let Some(start_field) = struct_value.fields.get("start_sequence") {
            if let Some(prost_types::value::Kind::StringValue(start_str)) = &start_field.kind {
                start_sequence = start_str.parse().unwrap_or(0);
            }
        }
        
        // Extract end_sequence (Option<u64>)
        if let Some(end_field) = struct_value.fields.get("end_sequence") {
            match &end_field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(end_str)) = &some_field.kind {
                            end_sequence = end_str.parse().ok();
                        }
                    }
                }
                Some(prost_types::value::Kind::StringValue(end_str)) => {
                    end_sequence = end_str.parse().ok();
                }
                _ => {}
            }
        }
        
        // Extract is_finished
        if let Some(finished_field) = struct_value.fields.get("is_finished") {
            match &finished_field.kind {
                Some(prost_types::value::Kind::BoolValue(finished_bool)) => {
                    is_finished = *finished_bool;
                }
                Some(prost_types::value::Kind::StringValue(finished_str)) => {
                    is_finished = finished_str == "true";
                }
                _ => {}
            }
        }
        
        // Extract block_proof (Option<String>)
        if let Some(proof_field) = struct_value.fields.get("block_proof") {
            match &proof_field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(proof_str)) = &some_field.kind {
                            block_proof = Some(proof_str.clone());
                        }
                    }
                }
                Some(prost_types::value::Kind::StringValue(proof_str)) => {
                    block_proof = Some(proof_str.clone());
                }
                _ => {}
            }
        }
        
        // Extract individual proofs (from VecMap<vector<u64>, Proof>)
        if let Some(proofs_field) = struct_value.fields.get("proofs") {
            if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                if let Some(contents_field) = proofs_struct.fields.get("contents") {
                    if let Some(prost_types::value::Kind::ListValue(list_value)) = &contents_field.kind {
                        debug!("🔍 Found {} proofs in VecMap", list_value.values.len());
                        
                        for proof_entry in &list_value.values {
                            if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &proof_entry.kind {
                                // Extract key (sequences) and value (Proof)
                                if let (Some(key_field), Some(value_field)) = 
                                    (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                    
                                    // Extract sequences from key
                                    let mut sequences = Vec::new();
                                    if let Some(prost_types::value::Kind::ListValue(key_list)) = &key_field.kind {
                                        for seq_value in &key_list.values {
                                            if let Some(prost_types::value::Kind::StringValue(seq_str)) = &seq_value.kind {
                                                if let Ok(seq) = seq_str.parse::<u64>() {
                                                    sequences.push(seq);
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Extract Proof struct from value
                                    if let Some(prost_types::value::Kind::StructValue(proof_struct)) = &value_field.kind {
                                        let proof_info = extract_individual_proof(proof_struct, sequences);
                                        if let Some(proof) = proof_info {
                                            debug!("🔍 Extracted proof: {:?}", proof);
                                            individual_proofs.push(proof);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let proof_info = ProofCalculationInfo {
            block_number,
            start_sequence,
            end_sequence,
            is_finished,
            block_proof,
            individual_proofs,
        };
        debug!("✅ Extracted proof info: {:?}", proof_info);
        return Some(proof_info);
    }
    
    None
}

/// Extract individual Proof information from Move Proof struct
fn extract_individual_proof(proof_struct: &prost_types::Struct, sequences: Vec<u64>) -> Option<ProofInfo> {
    debug!("🔍 Extracting individual proof with sequences: {:?}", sequences);
    debug!("🔍 Proof struct fields: {:?}", proof_struct.fields.keys().collect::<Vec<_>>());
    
    let mut status = 0u8;
    let mut da_hash: Option<String> = None;
    let mut timestamp = 0u64;
    let mut merged_sequences_1: Option<Vec<u64>> = None;
    let mut merged_sequences_2: Option<Vec<u64>> = None;
    let mut job_id = String::new();
    
    // Extract status
    if let Some(status_field) = proof_struct.fields.get("status") {
        match &status_field.kind {
            Some(prost_types::value::Kind::StringValue(status_str)) => {
                status = status_str.parse().unwrap_or(0);
            }
            Some(prost_types::value::Kind::NumberValue(status_num)) => {
                status = *status_num as u8;
            }
            _ => {}
        }
    }
    
    // Extract da_hash (Option<String>)
    if let Some(da_field) = proof_struct.fields.get("da_hash") {
        match &da_field.kind {
            Some(prost_types::value::Kind::StructValue(option_struct)) => {
                if let Some(some_field) = option_struct.fields.get("Some") {
                    if let Some(prost_types::value::Kind::StringValue(da_str)) = &some_field.kind {
                        da_hash = Some(da_str.clone());
                    }
                }
            }
            Some(prost_types::value::Kind::StringValue(da_str)) => {
                da_hash = Some(da_str.clone());
            }
            _ => {}
        }
    }
    
    // Extract timestamp
    if let Some(timestamp_field) = proof_struct.fields.get("timestamp") {
        if let Some(prost_types::value::Kind::StringValue(timestamp_str)) = &timestamp_field.kind {
            timestamp = timestamp_str.parse().unwrap_or(0);
        }
    }
    
    // Extract sequence1 (merged_sequences_1)
    if let Some(seq1_field) = proof_struct.fields.get("sequence1") {
        if let Some(sequences) = extract_optional_sequence_list(seq1_field) {
            merged_sequences_1 = Some(sequences);
        }
    }
    
    // Extract sequence2 (merged_sequences_2)
    if let Some(seq2_field) = proof_struct.fields.get("sequence2") {
        if let Some(sequences) = extract_optional_sequence_list(seq2_field) {
            merged_sequences_2 = Some(sequences);
        }
    }
    
    // Extract job_id
    if let Some(job_id_field) = proof_struct.fields.get("job_id") {
        if let Some(prost_types::value::Kind::StringValue(job_str)) = &job_id_field.kind {
            job_id = job_str.clone();
        }
    }
    
    Some(ProofInfo {
        sequences,
        status,
        da_hash,
        timestamp,
        merged_sequences_1,
        merged_sequences_2,
        job_id,
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
                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &seq_value.kind {
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
        _ => None
    }
}