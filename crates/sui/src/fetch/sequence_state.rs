use crate::error::{SilvanaSuiInterfaceError, Result};
use crate::parse::{get_u64, get_bytes, get_option_bytes, get_option_string};
use crate::state::SharedSuiState;
use super::AppInstance;
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, error};

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
    sequence_states_table_id: &str,
    sequence: u64,
) -> Result<Option<SequenceState>> {
    debug!("🔍 Fetching sequence {} from sequence_states table {}", sequence, sequence_states_table_id);
    
    let mut client = SharedSuiState::get_instance().get_sui_client();
    let mut page_token: Option<tonic::codegen::Bytes> = None;
    let mut total_fields_checked = 0;
    
    // Loop through pages to find the sequence
    loop {
        // List dynamic fields to find the specific sequence state
        let mut list_request = ListDynamicFieldsRequest::default();
        list_request.parent = Some(sequence_states_table_id.to_string());
        list_request.page_size = Some(500); // Process 500 at a time
        list_request.page_token = page_token.clone();
        list_request.read_mask = Some(FieldMask::from_paths([
            "field_id",
            "name",
        ]));

        let list_response = client
            .state_client()
            .list_dynamic_fields(list_request)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                format!("Failed to list sequence states in table: {}", e)
            ))?;
        
            let response = list_response.into_inner();
        let current_page_size = response.dynamic_fields.len();
        total_fields_checked += current_page_size;
        
        debug!("📋 Page: found {} dynamic fields (total checked: {})", current_page_size, total_fields_checked);
        
        // Log sequences found in this page for debugging (only on first page or if sequence might be here)
        if page_token.is_none() || current_page_size > 0 {
            let mut found_sequences: Vec<u64> = Vec::new();
            for field in &response.dynamic_fields {
                if let Some(name_bcs) = &field.name {
                    if let Some(name_value) = &name_bcs.value {
                        if let Ok(field_sequence) = bcs::from_bytes::<u64>(name_value) {
                            found_sequences.push(field_sequence);
                        }
                    }
                }
            }
            
            if !found_sequences.is_empty() {
                found_sequences.sort();
                debug!("📊 Page sequences: min={}, max={}, count={}", 
                    found_sequences.first().unwrap(), 
                    found_sequences.last().unwrap(),
                    found_sequences.len());
                
                // Check if our target sequence is in range
                let min_seq = *found_sequences.first().unwrap();
                let max_seq = *found_sequences.last().unwrap();
                if sequence >= min_seq && sequence <= max_seq {
                    debug!("  Target sequence {} is within page range [{}, {}]", sequence, min_seq, max_seq);
                }
            }
        }
        
        // Find the specific sequence state entry
        for field in &response.dynamic_fields {
            if let Some(name_bcs) = &field.name {
                if let Some(name_value) = &name_bcs.value {
                    // The name_value is BCS-encoded u64 (sequence)
                    if let Ok(field_sequence) = bcs::from_bytes::<u64>(name_value) {
                        if field_sequence == sequence {
                        debug!("🎯 Found matching sequence {} in dynamic fields", sequence);
                        if let Some(field_id) = &field.field_id {
                        debug!("📄 Fetching sequence state field object: {}", field_id);
                        // Fetch the sequence state field wrapper
                        let mut sequence_state_field_request = GetObjectRequest::default();
                        sequence_state_field_request.object_id = Some(field_id.clone());
                        sequence_state_field_request.version = None;
                        sequence_state_field_request.read_mask = Some(FieldMask::from_paths([
                            "object_id",
                            "json",
                        ]));

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
                                            let mut sequence_state_request = GetObjectRequest::default();
                                            sequence_state_request.object_id = Some(sequence_state_object_id.clone());
                                            sequence_state_request.version = None;
                                            sequence_state_request.read_mask = Some(FieldMask::from_paths([
                                                "object_id",
                                                "json",
                                            ]));

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
        }
        
        // Check if there are more pages
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() {
                debug!("📄 Moving to next page with token length: {}", next_token.len());
                page_token = Some(next_token);
                continue;
            }
        }
        
        // No more pages and sequence not found
        break;
    }
    
    debug!("❌ Sequence state {} not found in table {} after checking {} fields", 
        sequence, sequence_states_table_id, total_fields_checked);
    Ok(None)
}

/// Get the SequenceStateManager information from an AppInstance
pub async fn get_sequence_state_manager_info_from_app_instance(
    app_instance: &AppInstance,
) -> Result<Option<(u64, u64, String)>> { // (lowest_sequence, highest_sequence, table_id)
    debug!("Getting SequenceStateManager info from app_instance: {}", app_instance.id);
    
    // Use the sequence_state_manager field that's already in the AppInstance
    if app_instance.sequence_state_manager.is_null() {
        debug!("SequenceStateManager not found in app_instance {}", app_instance.id);
        return Ok(None);
    }
    
    let manager = &app_instance.sequence_state_manager;
    let mut lowest_sequence = 0u64;
    let mut highest_sequence = 0u64;
    let mut table_id = String::new();
    
    // Extract lowest_sequence
    if let Some(lowest_value) = manager.get("lowest_sequence") {
        if let Some(seq_str) = lowest_value.as_str() {
            lowest_sequence = seq_str.parse().unwrap_or(0);
            debug!("Parsed lowest_sequence: {}", lowest_sequence);
        } else if let Some(seq_num) = lowest_value.as_u64() {
            lowest_sequence = seq_num;
            debug!("Parsed lowest_sequence as u64: {}", lowest_sequence);
        } else if let Some(option_obj) = lowest_value.get("Some") {
            // Handle Option::Some case
            if let Some(seq_str) = option_obj.as_str() {
                lowest_sequence = seq_str.parse().unwrap_or(0);
                debug!("Parsed lowest_sequence from Some: {}", lowest_sequence);
            } else if let Some(seq_num) = option_obj.as_u64() {
                lowest_sequence = seq_num;
                debug!("Parsed lowest_sequence from Some as u64: {}", lowest_sequence);
            }
        }
    }
    
    // Extract highest_sequence
    if let Some(highest_value) = manager.get("highest_sequence") {
        if let Some(seq_str) = highest_value.as_str() {
            highest_sequence = seq_str.parse().unwrap_or(0);
            debug!("Parsed highest_sequence: {}", highest_sequence);
        } else if let Some(seq_num) = highest_value.as_u64() {
            highest_sequence = seq_num;
            debug!("Parsed highest_sequence as u64: {}", highest_sequence);
        } else if let Some(option_obj) = highest_value.get("Some") {
            // Handle Option::Some case
            if let Some(seq_str) = option_obj.as_str() {
                highest_sequence = seq_str.parse().unwrap_or(0);
                debug!("Parsed highest_sequence from Some: {}", highest_sequence);
            } else if let Some(seq_num) = option_obj.as_u64() {
                highest_sequence = seq_num;
                debug!("Parsed highest_sequence from Some as u64: {}", highest_sequence);
            }
        }
    }
    
    // Extract sequence_states table ID
    if let Some(states_value) = manager.get("sequence_states") {
        if let Some(id_value) = states_value.get("id") {
            if let Some(id_str) = id_value.as_str() {
                table_id = id_str.to_string();
            }
        }
    }
    
    if !table_id.is_empty() {
        debug!("Found SequenceStateManager: lowest={}, highest={}, table_id={}", 
            lowest_sequence, highest_sequence, table_id);
        return Ok(Some((lowest_sequence, highest_sequence, table_id)));
    }
    
    debug!("SequenceStateManager table_id not found in app_instance {}", app_instance.id);
    Ok(None)
}

/// Validates that the sequence states meet required conditions:
/// 1. All sequences are sequential (no gaps)
/// 2. First sequence has data availability OR is sequence 0 (if first_must_have_da is false)
/// 3. Last sequence matches the requested sequence
fn validate_sequence_states(
    states: &[SequenceState], 
    requested_sequence: u64,
    first_must_have_da: bool
) -> Result<()> {
    if states.is_empty() {
        error!("❌ Validation failed: No sequence states returned");
        return Err(SilvanaSuiInterfaceError::ParseError(
            "No sequence states returned".to_string()
        ));
    }
    
    // Check 1: All sequences are sequential
    for i in 1..states.len() {
        let expected = states[i - 1].sequence + 1;
        let actual = states[i].sequence;
        if actual != expected {
            let gap = actual - states[i - 1].sequence;
            error!(
                "❌ Validation failed: Non-sequential sequences detected! Gap of {} between sequence {} and {}",
                gap, states[i - 1].sequence, actual
            );
            error!(
                "  Expected sequence {}, but got {}. Total sequences: {}, Range: {} to {}",
                expected, actual, states.len(), states[0].sequence, states[states.len() - 1].sequence
            );
            return Err(SilvanaSuiInterfaceError::ParseError(
                format!("Non-sequential sequences: missing sequences between {} and {}", 
                    states[i - 1].sequence, actual)
            ));
        }
    }
    debug!("✅ Sequences are sequential: {} sequences from {} to {}", 
        states.len(), states[0].sequence, states[states.len() - 1].sequence);
    
    // Check 2: First sequence has data availability or is sequence 0
    let first = &states[0];
    if first_must_have_da && first.data_availability.is_none() && first.sequence != 0 {
        error!(
            "❌ Validation failed: First sequence {} does not have data availability and is not sequence 0",
            first.sequence
        );
        return Err(SilvanaSuiInterfaceError::ParseError(
            format!("First sequence {} must have data availability or be sequence 0", first.sequence)
        ));
    }
    
    if first.data_availability.is_some() {
        debug!("✅ First sequence {} has data availability", first.sequence);
    } else if first.sequence == 0 {
        debug!("✅ First sequence is 0 (genesis sequence)");
    } else {
        debug!("⚠️ First sequence {} has no data availability (allowed in this context)", first.sequence);
    }
    
    // Check 3: Last sequence matches requested sequence
    let last = &states[states.len() - 1];
    if last.sequence != requested_sequence {
        error!(
            "❌ Validation failed: Last sequence {} does not match requested sequence {}",
            last.sequence, requested_sequence
        );
        error!(
            "  Received sequences: {} to {} ({} total)",
            states[0].sequence, last.sequence, states.len()
        );
        return Err(SilvanaSuiInterfaceError::ParseError(
            format!("Last sequence mismatch: got {}, expected {}", last.sequence, requested_sequence)
        ));
    }
    debug!("✅ Last sequence {} matches requested sequence", last.sequence);
    
    Ok(())
}

/// Query sequence states from the SequenceStateManager
/// If the requested sequence has data_availability (Some), return just that sequence.
/// Otherwise, find the highest sequence with data_availability set,
/// and return all sequences from that one (inclusive) to the requested one.
pub async fn query_sequence_states(
    app_instance: &AppInstance,
    requested_sequence: u64,
) -> Result<Vec<SequenceState>> {
    debug!("Querying sequence states for app_instance: {}, sequence: {}", app_instance.id, requested_sequence);
    
    // Get SequenceStateManager info from AppInstance
    let (lowest_sequence, highest_sequence, table_id) = match get_sequence_state_manager_info_from_app_instance(app_instance).await? {
        Some(info) => {
            debug!("SequenceStateManager found: lowest={}, highest={}, table_id={}", info.0, info.1, info.2);
            info
        },
        None => {
            error!("SequenceStateManager not found in app_instance {}", app_instance.id);
            return Err(SilvanaSuiInterfaceError::ParseError(
                format!("SequenceStateManager not found in app_instance {}", app_instance.id)
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
    let requested_state = match fetch_sequence_state_by_id(&table_id, requested_sequence).await? {
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
        debug!("Requested sequence {} has data availability, returning single state", requested_sequence);
        return Ok(vec![requested_state]);
    }
    
    // Find the highest sequence with data_availability set (going backwards from requested_sequence-1)
    debug!("Requested sequence {} does not have data availability, searching for highest sequence with DA", requested_sequence);
    let mut start_sequence = None;
    for seq in (lowest_sequence..requested_sequence).rev() {
        debug!("Checking sequence {} for data availability", seq);
        if let Ok(Some(state)) = fetch_sequence_state_by_id(&table_id, seq).await {
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
            // Found a sequence with data availability, return ALL sequences from that one to requested
            let mut result_states = Vec::new();
            
            debug!("Found start sequence {} with data availability, fetching all sequences from {} to {}", 
                seq, seq, requested_sequence);
            
            // Fetch ALL sequences from start to requested (inclusive)
            for sequence_num in seq..=requested_sequence {
                if let Ok(Some(state)) = fetch_sequence_state_by_id(&table_id, sequence_num).await {
                    result_states.push(state);
                } else {
                    debug!("Warning: Could not fetch sequence state for {}", sequence_num);
                }
            }
            
            debug!("Fetched {} sequence states, validating before return", result_states.len());
            
            // Validate the results before returning
            validate_sequence_states(&result_states, requested_sequence, true)?;
            
            debug!("✅ Validation passed, returning {} sequence states from {} to {}", 
                result_states.len(), seq, requested_sequence);
            return Ok(result_states);
        }
        None => {
            // No sequence with data availability found, return all sequences from lowest to requested
            debug!("No sequence with data availability found, returning all sequences from {} to {}", 
                lowest_sequence, requested_sequence);
            let mut result_states = Vec::new();
            for seq in lowest_sequence..=requested_sequence {
                if let Ok(Some(state)) = fetch_sequence_state_by_id(&table_id, seq).await {
                    result_states.push(state);
                } else {
                    debug!("Warning: Could not fetch sequence state for {}", seq);
                }
            }
            
            debug!("Fetched {} sequence states, validating before return", result_states.len());
            
            // Validate the results before returning
            // When no DA found and starting from lowest, first sequence being 0 is acceptable
            let first_can_be_zero = lowest_sequence == 0;
            validate_sequence_states(&result_states, requested_sequence, !first_can_be_zero)?;
            
            debug!("✅ Validation passed, returning {} sequence states from {} to {}", 
                result_states.len(), lowest_sequence, requested_sequence);
            return Ok(result_states);
        }
    }
}