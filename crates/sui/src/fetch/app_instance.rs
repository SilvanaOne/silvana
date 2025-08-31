use anyhow::Result;
use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
use tracing::{debug, info, warn};
use crate::error::SilvanaSuiInterfaceError;
use crate::parse::{get_string, get_u64, get_bool, get_table_id, proto_to_json, parse_string_vecmap, parse_struct_vecmap};
use crate::state::SharedSuiState;
use super::jobs::Jobs;
use std::collections::HashMap;

/// Rust representation of the Move AppInstance struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInstance {
    /// The unique identifier of the AppInstance object
    pub id: String,
    /// The name of the Silvana app
    pub silvana_app_name: String,
    /// Optional description of the app
    pub description: Option<String>,
    /// Metadata key-value store (VecMap in Move)
    pub metadata: HashMap<String, String>,
    /// Key-value store (VecMap in Move)
    pub kv: HashMap<String, String>,
    /// Map of method names to AppMethod (stored as JSON for simplicity)
    pub methods: HashMap<String, serde_json::Value>,
    /// App state (stored as JSON for simplicity)
    pub state: serde_json::Value,
    /// ObjectTable ID for blocks (u64 -> Block mapping)
    pub blocks_table_id: String,
    /// ObjectTable ID for proof_calculations (u64 -> ProofCalculation mapping)
    pub proof_calculations_table_id: String,
    /// Sequence state manager (stored as JSON for simplicity)
    pub sequence_state_manager: serde_json::Value,
    /// Jobs struct with full parsing
    pub jobs: Option<Jobs>,
    /// Current sequence number
    pub sequence: u64,
    /// Admin address
    pub admin: String,
    /// Current block number
    pub block_number: u64,
    /// Previous block timestamp
    pub previous_block_timestamp: u64,
    /// Previous block last sequence
    pub previous_block_last_sequence: u64,
    /// Previous block actions state (stored as JSON for simplicity)
    pub previous_block_actions_state: serde_json::Value,
    /// Last proved block number
    pub last_proved_block_number: u64,
    /// Last settled block number
    pub last_settled_block_number: u64,
    /// Settlement chain name (optional)
    pub settlement_chain: Option<String>,
    /// Settlement address (optional)
    pub settlement_address: Option<String>,
    /// Whether the app is paused
    pub is_paused: bool,
    /// Creation timestamp
    pub created_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
}

/// Fetch an AppInstance from the blockchain by its ID
/// This only fetches the AppInstance object itself, not the contents of its ObjectTables
pub async fn fetch_app_instance(
    instance_id: &str,
) -> Result<AppInstance> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching AppInstance with ID: {}", instance_id);
    
    // Ensure the instance_id has 0x prefix
    let formatted_id = if instance_id.starts_with("0x") {
        instance_id.to_string()
    } else {
        format!("0x{}", instance_id)
    };
    
    // Create request to fetch just the AppInstance object
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string(), "object_id".to_string()],
        }),
    };
    
    // Fetch the object
    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch AppInstance {}: {}",
                instance_id, e
            ))
        })?;
    
    let object_data = response.into_inner();
    
    // Extract and parse the AppInstance from the response
    if let Some(proto_object) = object_data.object {
        if let Some(json_value) = &proto_object.json {
            // Debug: Show the raw AppInstance struct from Sui
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                debug!("=== Raw AppInstance struct from Sui ===");
                debug!("AppInstance {{");
                for (key, value) in &struct_value.fields {
                    debug!("    {}: {:?},", key, value.kind);
                }
                debug!("}}");
                debug!("=== End Raw AppInstance ===");
                
                return parse_app_instance_from_struct(struct_value, &formatted_id);
            }
        }
    }
    
    Err(anyhow::anyhow!(
        "Failed to parse AppInstance {} from blockchain response",
        instance_id
    ))
}

/// Parse AppInstance from protobuf struct representation
pub fn parse_app_instance_from_struct(
    struct_value: &prost_types::Struct,
    object_id: &str,
) -> Result<AppInstance> {
    debug!("Parsing AppInstance from struct with fields: {:?}", 
        struct_value.fields.keys().collect::<Vec<_>>());
    
    
    // Parse methods VecMap<String, AppMethod> into HashMap
    let methods = parse_struct_vecmap(struct_value, "methods");
    
    // Build the AppInstance struct
    let app_instance = AppInstance {
        id: object_id.to_string(),
        silvana_app_name: get_string(struct_value, "silvana_app_name").unwrap_or_default(),
        description: get_string(struct_value, "description"),
        metadata: parse_string_vecmap(struct_value, "metadata"),
        kv: parse_string_vecmap(struct_value, "kv"),
        methods,
        state: struct_value.fields.get("state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        blocks_table_id: get_table_id(struct_value, "blocks")?,
        proof_calculations_table_id: get_table_id(struct_value, "proof_calculations")?,
        sequence_state_manager: struct_value.fields.get("sequence_state_manager")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        jobs: struct_value.fields.get("jobs")
            .and_then(|f| {
                if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &f.kind {
                    Jobs::from_proto_struct(jobs_struct)
                } else {
                    None
                }
            }),
        sequence: get_u64(struct_value, "sequence"),
        admin: get_string(struct_value, "admin").unwrap_or_default(),
        block_number: get_u64(struct_value, "block_number"),
        previous_block_timestamp: get_u64(struct_value, "previous_block_timestamp"),
        previous_block_last_sequence: get_u64(struct_value, "previous_block_last_sequence"),
        previous_block_actions_state: struct_value.fields.get("previous_block_actions_state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        last_proved_block_number: get_u64(struct_value, "last_proved_block_number"),
        last_settled_block_number: get_u64(struct_value, "last_settled_block_number"),
        settlement_chain: get_string(struct_value, "settlement_chain"),
        settlement_address: get_string(struct_value, "settlement_address"),
        is_paused: get_bool(struct_value, "isPaused"),
        created_at: get_u64(struct_value, "created_at"),
        updated_at: get_u64(struct_value, "updated_at"),
    };
    
    debug!("Successfully parsed AppInstance: {}", app_instance.silvana_app_name);
    debug!("  Block number: {}", app_instance.block_number);
    debug!("  Last proved block: {}", app_instance.last_proved_block_number);
    debug!("  Blocks table ID: {}", app_instance.blocks_table_id);
    debug!("  Proof calculations table ID: {}", app_instance.proof_calculations_table_id);
    
    Ok(app_instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_app_instance_fields() {
        let app = AppInstance {
            id: "0x123".to_string(),
            silvana_app_name: "TestApp".to_string(),
            description: Some("Test Description".to_string()),
            metadata: HashMap::new(),
            kv: HashMap::new(),
            methods: HashMap::new(),
            state: serde_json::Value::Null,
            blocks_table_id: "0xabc".to_string(),
            proof_calculations_table_id: "0xdef".to_string(),
            sequence_state_manager: serde_json::Value::Null,
            jobs: None,
            sequence: 100,
            admin: "0xadmin".to_string(),
            block_number: 10,
            previous_block_timestamp: 1234567890,
            previous_block_last_sequence: 99,
            previous_block_actions_state: serde_json::Value::Null,
            last_proved_block_number: 9,
            last_settled_block_number: 8,
            settlement_chain: Some("mina".to_string()),
            settlement_address: Some("0xsettlement".to_string()),
            is_paused: false,
            created_at: 1234567890,
            updated_at: 1234567891,
        };
        
        assert_eq!(app.silvana_app_name, "TestApp");
        assert_eq!(app.block_number, 10);
        assert_eq!(app.last_proved_block_number, 9);
        assert!(!app.is_paused);
    }
}


/// Get the settlement job ID for a specific app instance ID
pub async fn get_settlement_job_id_for_instance(
    app_instance: &AppInstance,
) -> Result<Option<u64>> {
    debug!("Getting settlement job ID for app instance {}", app_instance.id);
    
    // Use the jobs data directly from the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        if let Some(settlement_job_id) = jobs.settlement_job {
            debug!("Found existing settlement job with ID {}", settlement_job_id);
            return Ok(Some(settlement_job_id));
        }
    }
    
    debug!("No settlement job found");
    Ok(None)
}

/// Get the settlement job ID if it exists
pub async fn get_settlement_job_id(
    app_instance: &AppInstance,
) -> Result<Option<u64>> {
    debug!("Getting settlement job ID for app instance {}", app_instance.id);
    let mut client = SharedSuiState::get_instance().get_sui_client();
    
    // Fetch the Jobs object to check the settlement_job field
    let formatted_id = if app_instance.id.starts_with("0x") {
        app_instance.id.clone()
    } else {
        format!("0x{}", app_instance.id)
    };
    
    // Fetch the AppInstance object
    let request = sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = match client.ledger_client().get_object(request).await {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            warn!("Failed to fetch AppInstance {}: {}", formatted_id, e);
            return Ok(None);
        }
    };

    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Get the settlement_job field
                        if let Some(settlement_job_field) = jobs_struct.fields.get("settlement_job") {
                            match &settlement_job_field.kind {
                                Some(prost_types::value::Kind::StringValue(s)) => {
                                    if let Ok(job_id) = s.parse::<u64>() {
                                        debug!("Found existing settlement job with ID {}", job_id);
                                        return Ok(Some(job_id));
                                    }
                                }
                                Some(prost_types::value::Kind::NumberValue(n)) => {
                                    let job_id = n.round() as u64;
                                    info!("Found existing settlement job with ID {}", job_id);
                                    return Ok(Some(job_id));
                                }
                                Some(prost_types::value::Kind::NullValue(_)) => {
                                    debug!("Settlement job field is null");
                                    return Ok(None);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    
    debug!("No settlement job found");
    Ok(None)
}