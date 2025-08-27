use crate::error::{SilvanaSuiInterfaceError, Result};
use crate::parse::{get_u64, get_bytes, get_option_bytes, get_option_string};
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, info, error};

/// Represents a sequence state from the Move SequenceState struct
#[derive(Debug, Clone)]
pub struct SequenceState {
    pub sequence: u64,
    pub state: Option<Vec<u8>>,
    pub data_availability: Option<String>,
    pub optimistic_state: Vec<u8>,
    pub transition_data: Vec<u8>,
}

/// Extract SequenceState from JSON representation
pub fn extract_sequence_state_from_json(json_value: &prost_types::Value) -> Result<SequenceState> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        // Build the SequenceState struct using helper functions
        let sequence_state = SequenceState {
            sequence: get_u64(struct_value, "sequence"),
            state: get_option_bytes(struct_value, "state"),
            data_availability: get_option_string(struct_value, "data_availability"),
            optimistic_state: get_bytes(struct_value, "optimistic_state"),
            transition_data: get_bytes(struct_value, "transition_data"),
        };
        
        return Ok(sequence_state);
    }
    
    Err(SilvanaSuiInterfaceError::ParseError(
        "Failed to extract sequence state from JSON".to_string()
    ))
}

/// Fetch a specific SequenceState by sequence number from the sequence_states ObjectTable
pub async fn fetch_sequence_state_by_id(
    client: &mut Client,
    sequence_states_table_id: &str,
    sequence: u64,
) -> Result<Option<SequenceState>> {
    debug!("🔍 Fetching sequence {} from sequence_states table {}", sequence, sequence_states_table_id);
    
    // List dynamic fields to find the specific sequence state
    let list_request = ListDynamicFieldsRequest {
        parent: Some(sequence_states_table_id.to_string()),
        page_size: Some(100),
        page_token: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "field_id".to_string(),
                "name_type".to_string(),
                "name_value".to_string(),
            ],
        }),
    };
    
    let list_response = client
        .live_data_client()
        .list_dynamic_fields(list_request)
        .await
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
            format!("Failed to list sequence states in table: {}", e)
        ))?;
    
    let response = list_response.into_inner();
    debug!("📋 Found {} dynamic fields in sequence_states table", response.dynamic_fields.len());
    
    // Find the specific sequence state entry
    for field in &response.dynamic_fields {
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (sequence)
            if let Ok(field_sequence) = bcs::from_bytes::<u64>(name_value) {
                if field_sequence == sequence {
                    debug!("🎯 Found matching sequence {} in dynamic fields", sequence);
                    if let Some(field_id) = &field.field_id {
                        debug!("📄 Fetching sequence state field object: {}", field_id);
                        // Fetch the sequence state field wrapper
                        let sequence_state_field_request = GetObjectRequest {
                            object_id: Some(field_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let sequence_state_field_response = client
                            .ledger_client()
                            .get_object(sequence_state_field_request)
                            .await
                            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                                format!("Failed to fetch sequence state field {}: {}", sequence, e)
                            ))?;
                        
                        if let Some(sequence_state_field_object) = sequence_state_field_response.into_inner().object {
                            // Extract the actual sequence state object ID from the Field wrapper
                            if let Some(json_value) = &sequence_state_field_object.json {
                                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                    if let Some(value_field) = struct_value.fields.get("value") {
                                        if let Some(prost_types::value::Kind::StringValue(sequence_state_object_id)) = &value_field.kind {
                                            // Fetch the actual sequence state object
                                            let sequence_state_request = GetObjectRequest {
                                                object_id: Some(sequence_state_object_id.clone()),
                                                version: None,
                                                read_mask: Some(prost_types::FieldMask {
                                                    paths: vec![
                                                        "object_id".to_string(),
                                                        "json".to_string(),
                                                    ],
                                                }),
                                            };
                                            
                                            let sequence_state_response = client
                                                .ledger_client()
                                                .get_object(sequence_state_request)
                                                .await
                                                .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                                                    format!("Failed to fetch sequence state {}: {}", sequence, e)
                                                ))?;
                                            
                                            if let Some(sequence_state_object) = sequence_state_response.into_inner().object {
                                                if let Some(sequence_state_json) = &sequence_state_object.json {
                                                    //debug!("🔍 FULL SequenceState JSON for sequence {}: {:#?}", sequence, sequence_state_json);
                                                    if let Ok(sequence_state) = extract_sequence_state_from_json(sequence_state_json) {
                                                        debug!("✅ Successfully extracted sequence state {}: has_state={}, has_data_availability={}", 
                                                            sequence, sequence_state.state.is_some(), sequence_state.data_availability.is_some());
                                                        return Ok(Some(sequence_state));
                                                    } else {
                                                        error!("❌ Failed to extract sequence state {} from JSON", sequence);
                                                    }
                                                } else {
                                                    error!("❌ No JSON found for sequence state object {}", sequence);
                                                }
                                            } else {
                                                error!("❌ No sequence state object found for sequence {}", sequence);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("❌ Sequence state {} not found in table {}", sequence, sequence_states_table_id);
    Ok(None)
}

/// Get the SequenceStateManager information from an AppInstance
pub async fn get_sequence_state_manager_info_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
) -> Result<Option<(u64, u64, String)>> { // (lowest_sequence, highest_sequence, table_id)
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    debug!("Fetching SequenceStateManager info from app_instance: {}", formatted_id);
    
    // Fetch the AppInstance object
    let app_instance_request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };

    let app_instance_response = client
        .ledger_client()
        .get_object(app_instance_request)
        .await
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Look for the sequence_state_manager field in the AppInstance
                if let Some(manager_field) = struct_value.fields.get("sequence_state_manager") {
                    if let Some(prost_types::value::Kind::StructValue(manager_struct)) = &manager_field.kind {
                        let mut lowest_sequence = 0u64;
                        let mut highest_sequence = 0u64;
                        let mut table_id = String::new();
                        
                        // Extract lowest_sequence
                        if let Some(lowest_field) = manager_struct.fields.get("lowest_sequence") {
                            debug!("Found lowest_sequence field: {:?}", lowest_field);
                            match &lowest_field.kind {
                                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                                    if let Some(some_field) = option_struct.fields.get("Some") {
                                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &some_field.kind {
                                            lowest_sequence = seq_str.parse().unwrap_or(0);
                                            debug!("Parsed lowest_sequence from Some: {}", lowest_sequence);
                                        }
                                    } else {
                                        debug!("lowest_sequence is None (empty Some field)");
                                    }
                                }
                                Some(prost_types::value::Kind::StringValue(seq_str)) => {
                                    // Handle direct string value
                                    lowest_sequence = seq_str.parse().unwrap_or(0);
                                    debug!("Parsed lowest_sequence from direct string: {}", lowest_sequence);
                                }
                                Some(prost_types::value::Kind::NullValue(_)) => {
                                    debug!("lowest_sequence is null");
                                }
                                _ => {
                                    debug!("lowest_sequence has unexpected type: {:?}", lowest_field.kind);
                                }
                            }
                        } else {
                            debug!("lowest_sequence field not found");
                        }
                        
                        // Extract highest_sequence
                        if let Some(highest_field) = manager_struct.fields.get("highest_sequence") {
                            debug!("Found highest_sequence field: {:?}", highest_field);
                            match &highest_field.kind {
                                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                                    if let Some(some_field) = option_struct.fields.get("Some") {
                                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &some_field.kind {
                                            highest_sequence = seq_str.parse().unwrap_or(0);
                                            debug!("Parsed highest_sequence from Some: {}", highest_sequence);
                                        }
                                    } else {
                                        debug!("highest_sequence is None (empty Some field)");
                                    }
                                }
                                Some(prost_types::value::Kind::StringValue(seq_str)) => {
                                    // Handle direct string value
                                    highest_sequence = seq_str.parse().unwrap_or(0);
                                    debug!("Parsed highest_sequence from direct string: {}", highest_sequence);
                                }
                                Some(prost_types::value::Kind::NullValue(_)) => {
                                    debug!("highest_sequence is null");
                                }
                                _ => {
                                    debug!("highest_sequence has unexpected type: {:?}", highest_field.kind);
                                }
                            }
                        } else {
                            debug!("highest_sequence field not found");
                        }
                        
                        // Extract sequence_states table ID
                        if let Some(table_field) = manager_struct.fields.get("sequence_states") {
                            if let Some(prost_types::value::Kind::StructValue(table_struct)) = &table_field.kind {
                                if let Some(id_field) = table_struct.fields.get("id") {
                                    if let Some(prost_types::value::Kind::StringValue(id_str)) = &id_field.kind {
                                        table_id = id_str.clone();
                                    }
                                }
                            }
                        }
                        
                        if !table_id.is_empty() {
                            debug!("Found SequenceStateManager: lowest={}, highest={}, table_id={}", 
                                lowest_sequence, highest_sequence, table_id);
                            return Ok(Some((lowest_sequence, highest_sequence, table_id)));
                        }
                    }
                }
            }
        }
    }
    
    debug!("SequenceStateManager not found in app_instance {}", app_instance_id);
    Ok(None)
}

/// Query sequence states from the SequenceStateManager
/// If the requested sequence has state and data_availability (both Some), return just that sequence.
/// Otherwise, find the highest sequence with both state and data_availability set,
/// and return all sequences from that one (inclusive) to the requested one.
pub async fn query_sequence_states(
    client: &mut Client,
    app_instance_id: &str,
    requested_sequence: u64,
) -> Result<Vec<SequenceState>> {
    info!("Querying sequence states for app_instance: {}, sequence: {}", app_instance_id, requested_sequence);
    
    // Get SequenceStateManager info from AppInstance
    let (lowest_sequence, highest_sequence, table_id) = match get_sequence_state_manager_info_from_app_instance(client, app_instance_id).await? {
        Some(info) => {
            debug!("SequenceStateManager found: lowest={}, highest={}, table_id={}", info.0, info.1, info.2);
            info
        },
        None => {
            error!("SequenceStateManager not found in app_instance {}", app_instance_id);
            return Err(SilvanaSuiInterfaceError::ParseError(
                format!("SequenceStateManager not found in app_instance {}", app_instance_id)
            ));
        }
    };
    
    // Check if requested sequence is within bounds
    if requested_sequence < lowest_sequence || requested_sequence > highest_sequence {
        error!("Requested sequence {} is out of bounds [{}, {}]", 
            requested_sequence, lowest_sequence, highest_sequence);
        return Err(SilvanaSuiInterfaceError::ParseError(
            format!("Requested sequence {} is out of bounds [{}, {}]", 
                requested_sequence, lowest_sequence, highest_sequence)
        ));
    }
    
    debug!("Requested sequence {} is within bounds [{}, {}], fetching state", 
        requested_sequence, lowest_sequence, highest_sequence);
    
    // Fetch the requested sequence state first
    let requested_state = match fetch_sequence_state_by_id(client, &table_id, requested_sequence).await? {
        Some(state) => {
            debug!("Successfully fetched requested sequence state {}: has_state={}, has_data_availability={}", 
                requested_sequence, state.state.is_some(), state.data_availability.is_some());
            state
        },
        None => {
            error!("Sequence state {} not found in table {}", requested_sequence, table_id);
            return Err(SilvanaSuiInterfaceError::ParseError(
                format!("Sequence state {} not found", requested_sequence)
            ));
        }
    };
    
    // Check if requested sequence has data_availability
    if requested_state.data_availability.is_some() {
        // Return just this sequence
        info!("Requested sequence {} has data availability, returning single state", requested_sequence);
        return Ok(vec![requested_state]);
    }
    
    // Find the highest sequence with data_availability set (going backwards from requested_sequence-1)
    debug!("Requested sequence {} does not have data availability, searching for highest sequence with DA", requested_sequence);
    let mut start_sequence = None;
    for seq in (lowest_sequence..requested_sequence).rev() {
        debug!("Checking sequence {} for data availability", seq);
        if let Ok(Some(state)) = fetch_sequence_state_by_id(client, &table_id, seq).await {
            debug!("Sequence {}: has_state={}, has_data_availability={}", 
                seq, state.state.is_some(), state.data_availability.is_some());
            if state.data_availability.is_some() {
                start_sequence = Some(seq);
                debug!("Found start sequence with data availability: {}", seq);
                break;
            }
        } else {
            debug!("Failed to fetch sequence state for {}", seq);
        }
    }
    
    match start_sequence {
        Some(seq) => {
            // Found a sequence with data availability, return only that sequence and the requested sequence
            let mut result_states = Vec::new();
            
            // Add the start sequence (highest with DA)
            if let Ok(Some(start_state)) = fetch_sequence_state_by_id(client, &table_id, seq).await {
                result_states.push(start_state);
            }
            
            // Add the requested sequence if it's different from start sequence
            if seq != requested_sequence {
                result_states.push(requested_state);
            }
            
            info!("Returning {} sequence states: start sequence {} (with DA) and requested sequence {}", 
                result_states.len(), seq, requested_sequence);
            return Ok(result_states);
        }
        None => {
            // No sequence with data availability found, return all sequences from lowest to requested
            info!("No sequence with data availability found, returning all sequences from {} to {}", 
                lowest_sequence, requested_sequence);
            let mut result_states = Vec::new();
            for seq in lowest_sequence..=requested_sequence {
                if let Ok(Some(state)) = fetch_sequence_state_by_id(client, &table_id, seq).await {
                    result_states.push(state);
                }
            }
            return Ok(result_states);
        }
    }
}