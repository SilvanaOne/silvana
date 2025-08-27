use crate::error::{SilvanaSuiInterfaceError, Result};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, warn};
use serde::{Deserialize, Serialize};

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
    pub prover: String, // address in Move
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

/// Fetch all ProofCalculations for a block from AppInstance
pub async fn fetch_proof_calculations(
    client: &mut Client,
    app_instance: &str,
    block_number: u64,
) -> Result<Vec<ProofCalculation>> {
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
    
    let object_response = client.ledger_client().get_object(request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
) -> Result<Vec<ProofCalculation>> {
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
        
        let fields_response = client.live_data_client().list_dynamic_fields(request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
) -> Result<Option<ProofCalculation>> {
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

    let proof_response = client.ledger_client().get_object(proof_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
                        
                        let actual_proof_response = client.ledger_client().get_object(actual_proof_request).await.map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
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
fn extract_proof_calculation_from_json(json_value: &prost_types::Value) -> Option<ProofCalculation> {
    debug!("🔍 Extracting proof calculation from JSON");
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        debug!("🔍 Proof calculation fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        
        // Extract id (required field)
        let id = if let Some(id_field) = struct_value.fields.get("id") {
            if let Some(prost_types::value::Kind::StringValue(id_str)) = &id_field.kind {
                id_str.clone()
            } else {
                debug!("❌ Failed to extract id field");
                return None;
            }
        } else {
            debug!("❌ No id field found");
            return None;
        };
        
        // Extract block_number (required field)
        let block_number = if let Some(block_field) = struct_value.fields.get("block_number") {
            if let Some(prost_types::value::Kind::StringValue(block_str)) = &block_field.kind {
                block_str.parse().ok()?
            } else {
                debug!("❌ Failed to parse block_number");
                return None;
            }
        } else {
            debug!("❌ No block_number field found");
            return None;
        };
        
        // Extract start_sequence (required field)
        let start_sequence = if let Some(start_field) = struct_value.fields.get("start_sequence") {
            if let Some(prost_types::value::Kind::StringValue(start_str)) = &start_field.kind {
                start_str.parse().ok()?
            } else {
                debug!("❌ Failed to parse start_sequence");
                return None;
            }
        } else {
            debug!("❌ No start_sequence field found");
            return None;
        };
        
        // Extract end_sequence (Option<u64>)
        let end_sequence = if let Some(end_field) = struct_value.fields.get("end_sequence") {
            match &end_field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(end_str)) = &some_field.kind {
                            end_str.parse().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Some(prost_types::value::Kind::StringValue(end_str)) => {
                    end_str.parse().ok()
                }
                _ => None
            }
        } else {
            None
        };
        
        // Extract is_finished (required field)
        let is_finished = if let Some(finished_field) = struct_value.fields.get("is_finished") {
            match &finished_field.kind {
                Some(prost_types::value::Kind::BoolValue(finished_bool)) => {
                    *finished_bool
                }
                Some(prost_types::value::Kind::StringValue(finished_str)) => {
                    finished_str == "true"
                }
                _ => {
                    debug!("❌ Failed to parse is_finished");
                    return None;
                }
            }
        } else {
            debug!("❌ No is_finished field found");
            return None;
        };
        
        // Extract block_proof (Option<String>)
        let block_proof = if let Some(proof_field) = struct_value.fields.get("block_proof") {
            match &proof_field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(proof_str)) = &some_field.kind {
                            Some(proof_str.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Some(prost_types::value::Kind::StringValue(proof_str)) if !proof_str.is_empty() => {
                    Some(proof_str.clone())
                }
                _ => None
            }
        } else {
            None
        };
        
        // Extract proofs (VecMap<vector<u64>, Proof>) - required field but can be empty
        let proofs = if let Some(proofs_field) = struct_value.fields.get("proofs") {
            if let Some(prost_types::value::Kind::StructValue(proofs_struct)) = &proofs_field.kind {
                if let Some(contents_field) = proofs_struct.fields.get("contents") {
                    if let Some(prost_types::value::Kind::ListValue(list_value)) = &contents_field.kind {
                        debug!("🔍 Found {} proofs in VecMap", list_value.values.len());
                        
                        let mut proofs_vec = Vec::new();
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
                                        if let Some(proof) = extract_individual_proof(proof_struct, sequences) {
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
        
        // Create ProofCalculation with all validated fields
        let proof_info = ProofCalculation {
            id,
            block_number,
            start_sequence,
            end_sequence,
            proofs,
            block_proof,
            is_finished,
        };
        debug!("✅ Extracted proof info: {:?}", proof_info);
        return Some(proof_info);
    }
    
    None
}

/// Extract individual Proof information from Move Proof struct
fn extract_individual_proof(proof_struct: &prost_types::Struct, sequences: Vec<u64>) -> Option<Proof> {
    debug!("🔍 Extracting individual proof with sequences: {:?}", sequences);
    debug!("🔍 Proof struct fields: {:?}", proof_struct.fields.keys().collect::<Vec<_>>());
    
    // Extract status (required field)
    let status = if let Some(status_field) = proof_struct.fields.get("status") {
        match &status_field.kind {
            Some(prost_types::value::Kind::StringValue(status_str)) => {
                let status_u8: u8 = status_str.parse().ok()?;
                ProofStatus::from_u8(status_u8)
            }
            Some(prost_types::value::Kind::NumberValue(status_num)) => {
                ProofStatus::from_u8(*status_num as u8)
            }
            _ => {
                debug!("❌ Failed to parse status");
                return None;
            }
        }
    } else {
        debug!("❌ No status field found");
        return None;
    };
    
    // Extract da_hash (Option<String>)
    let da_hash = if let Some(da_field) = proof_struct.fields.get("da_hash") {
        match &da_field.kind {
            Some(prost_types::value::Kind::StructValue(option_struct)) => {
                if let Some(some_field) = option_struct.fields.get("Some") {
                    if let Some(prost_types::value::Kind::StringValue(da_str)) = &some_field.kind {
                        Some(da_str.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Some(prost_types::value::Kind::StringValue(da_str)) if !da_str.is_empty() => {
                Some(da_str.clone())
            }
            _ => None
        }
    } else {
        None
    };
    
    // Extract timestamp (required field)
    let timestamp = if let Some(timestamp_field) = proof_struct.fields.get("timestamp") {
        if let Some(prost_types::value::Kind::StringValue(timestamp_str)) = &timestamp_field.kind {
            timestamp_str.parse().ok()?
        } else {
            debug!("❌ Failed to parse timestamp");
            return None;
        }
    } else {
        debug!("❌ No timestamp field found");
        return None;
    };
    
    // Extract sequence1 (Option<vector<u64>>)
    let sequence1 = if let Some(seq1_field) = proof_struct.fields.get("sequence1") {
        extract_optional_sequence_list(seq1_field)
    } else {
        None
    };
    
    // Extract sequence2 (Option<vector<u64>>)
    let sequence2 = if let Some(seq2_field) = proof_struct.fields.get("sequence2") {
        extract_optional_sequence_list(seq2_field)
    } else {
        None
    };
    
    // Extract rejected_count (required field)
    let rejected_count = if let Some(rejected_field) = proof_struct.fields.get("rejected_count") {
        match &rejected_field.kind {
            Some(prost_types::value::Kind::StringValue(rejected_str)) => {
                rejected_str.parse().ok()?
            }
            Some(prost_types::value::Kind::NumberValue(rejected_num)) => {
                *rejected_num as u16
            }
            _ => {
                debug!("❌ Failed to parse rejected_count");
                return None;
            }
        }
    } else {
        debug!("❌ No rejected_count field found");
        return None;
    };
    
    // Extract prover (required field - address)
    let prover = if let Some(prover_field) = proof_struct.fields.get("prover") {
        if let Some(prost_types::value::Kind::StringValue(prover_str)) = &prover_field.kind {
            prover_str.clone()
        } else {
            debug!("❌ Failed to parse prover");
            return None;
        }
    } else {
        debug!("❌ No prover field found");
        return None;
    };
    
    // Extract user (Option<address>)
    let user = if let Some(user_field) = proof_struct.fields.get("user") {
        match &user_field.kind {
            Some(prost_types::value::Kind::StructValue(option_struct)) => {
                if let Some(some_field) = option_struct.fields.get("Some") {
                    if let Some(prost_types::value::Kind::StringValue(user_str)) = &some_field.kind {
                        Some(user_str.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Some(prost_types::value::Kind::StringValue(user_str)) if !user_str.is_empty() => {
                Some(user_str.clone())
            }
            _ => None
        }
    } else {
        None
    };
    
    // Extract job_id (required field)
    let job_id = if let Some(job_id_field) = proof_struct.fields.get("job_id") {
        if let Some(prost_types::value::Kind::StringValue(job_str)) = &job_id_field.kind {
            job_str.clone()
        } else {
            debug!("❌ Failed to parse job_id");
            return None;
        }
    } else {
        debug!("❌ No job_id field found");
        return None;
    };
    
    // Create Proof with all validated fields
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