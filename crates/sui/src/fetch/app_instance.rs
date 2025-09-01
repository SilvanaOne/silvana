use anyhow::Result;
use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
use tracing::debug;
use crate::error::SilvanaSuiInterfaceError;
use crate::parse::{get_string, get_u64, get_bool, get_table_id, proto_to_json, parse_string_vecmap, parse_struct_vecmap};
use crate::state::SharedSuiState;
use super::jobs::Jobs;
use std::collections::HashMap;

/// Rust representation of the Move BlockSettlement struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSettlement {
    pub block_number: u64,
    pub settlement_tx_hash: Option<String>,
    pub settlement_tx_included_in_block: bool,
    pub sent_to_settlement_at: Option<u64>,
    pub settled_at: Option<u64>,
}

/// Rust representation of the Move Settlement struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub chain: String,
    pub last_settled_block_number: u64,
    pub settlement_address: Option<String>,
    pub block_settlements: HashMap<u64, BlockSettlement>,
    pub settlement_job: Option<u64>, // ID of the active settlement job for this chain
}

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
    /// Settlements map (chain -> Settlement)
    pub settlements: HashMap<String, Settlement>,
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

/// Parse settlements from the protocol buffer struct
fn parse_settlements(struct_value: &prost_types::Struct) -> HashMap<String, Settlement> {
    let mut settlements = HashMap::new();
    
    if let Some(field) = struct_value.fields.get("settlements") {
        if let Some(prost_types::value::Kind::StructValue(settlements_struct)) = &field.kind {
            // Parse the VecMap contents
            if let Some(contents_field) = settlements_struct.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                    for item in &list.values {
                        if let Some(prost_types::value::Kind::StructValue(entry)) = &item.kind {
                            // Parse Entry struct (has k and v fields)
                            if let (Some(key_field), Some(value_field)) = 
                                (entry.fields.get("k"), entry.fields.get("v")) {
                                
                                // Parse the chain string (key)
                                let chain = if let Some(prost_types::value::Kind::StringValue(s)) = &key_field.kind {
                                    s.clone()
                                } else {
                                    continue;
                                };
                                
                                // Parse the Settlement struct (value)
                                if let Some(prost_types::value::Kind::StructValue(settlement_struct)) = &value_field.kind {
                                    let settlement = Settlement {
                                        chain: get_string(settlement_struct, "chain").unwrap_or_default(),
                                        last_settled_block_number: get_u64(settlement_struct, "last_settled_block_number"),
                                        settlement_address: get_string(settlement_struct, "settlement_address"),
                                        block_settlements: parse_block_settlements(settlement_struct),
                                        settlement_job: settlement_struct.fields.get("settlement_job")
                                            .and_then(|f| if let Some(prost_types::value::Kind::StringValue(s)) = &f.kind {
                                                s.parse::<u64>().ok()
                                            } else { None }),
                                    };
                                    settlements.insert(chain, settlement);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    settlements
}

/// Parse block settlements from the protocol buffer struct
fn parse_block_settlements(settlement_struct: &prost_types::Struct) -> HashMap<u64, BlockSettlement> {
    let mut block_settlements = HashMap::new();
    
    if let Some(field) = settlement_struct.fields.get("block_settlements") {
        if let Some(prost_types::value::Kind::StructValue(bs_struct)) = &field.kind {
            // Parse the VecMap contents
            if let Some(contents_field) = bs_struct.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                    for item in &list.values {
                        if let Some(prost_types::value::Kind::StructValue(entry)) = &item.kind {
                            // Parse Entry struct (has k and v fields)
                            if let (Some(key_field), Some(value_field)) = 
                                (entry.fields.get("k"), entry.fields.get("v")) {
                                
                                // Parse the block number (key)
                                let block_number = if let Some(prost_types::value::Kind::StringValue(s)) = &key_field.kind {
                                    s.parse::<u64>().unwrap_or(0)
                                } else {
                                    continue;
                                };
                                
                                // Parse the BlockSettlement struct (value)
                                if let Some(prost_types::value::Kind::StructValue(bs)) = &value_field.kind {
                                    let block_settlement = BlockSettlement {
                                        block_number: get_u64(bs, "block_number"),
                                        settlement_tx_hash: get_string(bs, "settlement_tx_hash"),
                                        settlement_tx_included_in_block: get_bool(bs, "settlement_tx_included_in_block"),
                                        sent_to_settlement_at: bs.fields.get("sent_to_settlement_at")
                                            .and_then(|f| if let Some(prost_types::value::Kind::StringValue(s)) = &f.kind {
                                                s.parse::<u64>().ok()
                                            } else { None }),
                                        settled_at: bs.fields.get("settled_at")
                                            .and_then(|f| if let Some(prost_types::value::Kind::StringValue(s)) = &f.kind {
                                                s.parse::<u64>().ok()
                                            } else { None }),
                                    };
                                    block_settlements.insert(block_number, block_settlement);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    block_settlements
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
        settlements: parse_settlements(struct_value),
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
            settlements: {
                let mut settlements = HashMap::new();
                settlements.insert("mina".to_string(), Settlement {
                    chain: "mina".to_string(),
                    last_settled_block_number: 8,
                    settlement_address: Some("0xsettlement".to_string()),
                    block_settlements: HashMap::new(),
                });
                settlements
            },
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
/// Get all settlement job IDs per chain for an app instance
pub async fn get_settlement_job_ids_for_instance(
    app_instance: &AppInstance,
) -> Result<HashMap<String, u64>> {
    debug!("Getting settlement job IDs for app instance {}", app_instance.id);
    
    let mut settlement_jobs = HashMap::new();
    
    // Check each settlement for its job ID
    for (chain, settlement) in &app_instance.settlements {
        if let Some(job_id) = settlement.settlement_job {
            debug!("Found settlement job {} for chain {}", job_id, chain);
            settlement_jobs.insert(chain.clone(), job_id);
        }
    }
    
    if settlement_jobs.is_empty() {
        debug!("No settlement jobs found");
    } else {
        debug!("Found {} settlement jobs", settlement_jobs.len());
    }
    
    Ok(settlement_jobs)
}

