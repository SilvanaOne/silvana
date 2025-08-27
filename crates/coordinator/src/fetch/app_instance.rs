use anyhow::Result;
use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
use sui_rpc::Client;
use tracing::debug;
use crate::error::CoordinatorError;
use crate::fetch::jobs_types::Jobs;
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
    client: &mut Client,
    instance_id: &str,
) -> Result<AppInstance> {
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
            CoordinatorError::RpcConnectionError(format!(
                "Failed to fetch AppInstance {}: {}",
                instance_id, e
            ))
        })?;
    
    let object_data = response.into_inner();
    
    // Extract and parse the AppInstance from the response
    if let Some(proto_object) = object_data.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
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
fn parse_app_instance_from_struct(
    struct_value: &prost_types::Struct,
    object_id: &str,
) -> Result<AppInstance> {
    debug!("Parsing AppInstance from struct with fields: {:?}", 
        struct_value.fields.keys().collect::<Vec<_>>());
    
    // Helper function to extract string value
    let get_string = |field_name: &str| -> Option<String> {
        struct_value.fields.get(field_name).and_then(|f| {
            match &f.kind {
                Some(prost_types::value::Kind::StringValue(s)) => Some(s.clone()),
                _ => None,
            }
        })
    };
    
    // Helper function to extract number value as u64
    let get_u64 = |field_name: &str| -> u64 {
        struct_value.fields.get(field_name).and_then(|f| {
            match &f.kind {
                Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
                Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
                _ => None,
            }
        }).unwrap_or(0)
    };
    
    // Helper function to extract boolean value
    let get_bool = |field_name: &str| -> bool {
        struct_value.fields.get(field_name).and_then(|f| {
            match &f.kind {
                Some(prost_types::value::Kind::BoolValue(b)) => Some(*b),
                Some(prost_types::value::Kind::StringValue(s)) => {
                    Some(s.to_lowercase() == "true")
                }
                _ => None,
            }
        }).unwrap_or(false)
    };
    
    // Helper function to extract ObjectTable ID from nested struct
    let get_table_id = |field_name: &str| -> Result<String> {
        struct_value.fields.get(field_name)
            .and_then(|f| {
                if let Some(prost_types::value::Kind::StructValue(table_struct)) = &f.kind {
                    table_struct.fields.get("id").and_then(|id_field| {
                        if let Some(prost_types::value::Kind::StringValue(id)) = &id_field.kind {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to extract {} table ID from AppInstance",
                    field_name
                )
            })
    };
    
    // Define proto_to_json as a standalone function within this function scope
    fn proto_to_json(value: &prost_types::Value) -> serde_json::Value {
        match &value.kind {
            Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
            Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Value::Number(
                serde_json::Number::from_f64(*n).unwrap_or(serde_json::Number::from(0))
            ),
            Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
            Some(prost_types::value::Kind::NullValue(_)) => serde_json::Value::Null,
            Some(prost_types::value::Kind::ListValue(list)) => {
                serde_json::Value::Array(
                    list.values.iter().map(proto_to_json).collect()
                )
            }
            Some(prost_types::value::Kind::StructValue(s)) => {
                let map: serde_json::Map<String, serde_json::Value> = s.fields.iter()
                    .map(|(k, v)| (k.clone(), proto_to_json(v)))
                    .collect();
                serde_json::Value::Object(map)
            }
            None => serde_json::Value::Null,
        }
    }
    
    // Helper function to parse VecMap<String, String> into HashMap
    let parse_string_vecmap = |field_name: &str| -> HashMap<String, String> {
        if let Some(field) = struct_value.fields.get(field_name) {
            if let Some(prost_types::value::Kind::StructValue(struct_)) = &field.kind {
                if let Some(contents_field) = struct_.fields.get("contents") {
                    if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                        let mut map = HashMap::new();
                        for entry in &list.values {
                            if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &entry.kind {
                                if let (Some(key_field), Some(value_field)) = 
                                    (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                    if let (Some(prost_types::value::Kind::StringValue(key)),
                                            Some(prost_types::value::Kind::StringValue(value))) = 
                                        (&key_field.kind, &value_field.kind) {
                                        map.insert(key.clone(), value.clone());
                                    }
                                }
                            }
                        }
                        return map;
                    }
                }
            }
        }
        HashMap::new()
    };
    
    // Parse methods VecMap into HashMap
    let methods = if let Some(methods_field) = struct_value.fields.get("methods") {
        if let Some(prost_types::value::Kind::StructValue(methods_struct)) = &methods_field.kind {
            if let Some(contents_field) = methods_struct.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                    let mut map = HashMap::new();
                    for entry in &list.values {
                        if let Some(prost_types::value::Kind::StructValue(entry_struct)) = &entry.kind {
                            if let (Some(key_field), Some(value_field)) = 
                                (entry_struct.fields.get("key"), entry_struct.fields.get("value")) {
                                if let Some(prost_types::value::Kind::StringValue(key)) = &key_field.kind {
                                    map.insert(key.clone(), proto_to_json(value_field));
                                }
                            }
                        }
                    }
                    map
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };
    
    // Build the AppInstance struct
    let app_instance = AppInstance {
        id: object_id.to_string(),
        silvana_app_name: get_string("silvana_app_name").unwrap_or_default(),
        description: get_string("description"),
        metadata: parse_string_vecmap("metadata"),
        kv: parse_string_vecmap("kv"),
        methods,
        state: struct_value.fields.get("state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        blocks_table_id: get_table_id("blocks")?,
        proof_calculations_table_id: get_table_id("proof_calculations")?,
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
        sequence: get_u64("sequence"),
        admin: get_string("admin").unwrap_or_default(),
        block_number: get_u64("block_number"),
        previous_block_timestamp: get_u64("previous_block_timestamp"),
        previous_block_last_sequence: get_u64("previous_block_last_sequence"),
        previous_block_actions_state: struct_value.fields.get("previous_block_actions_state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        last_proved_block_number: get_u64("last_proved_block_number"),
        last_settled_block_number: get_u64("last_settled_block_number"),
        settlement_chain: get_string("settlement_chain"),
        settlement_address: get_string("settlement_address"),
        is_paused: get_bool("isPaused"),
        created_at: get_u64("created_at"),
        updated_at: get_u64("updated_at"),
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