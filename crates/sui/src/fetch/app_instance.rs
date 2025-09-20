use super::jobs::Jobs;
use crate::error::SilvanaSuiInterfaceError;
use crate::parse::{
    get_bool, get_string, get_table_id, get_u64, parse_string_vecmap, parse_struct_vecmap,
    proto_to_json,
};
use crate::state::SharedSuiState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, error, warn};

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
    pub block_settlements_table_id: Option<String>, // ID of the ObjectTable for block_settlements
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
    /// Last settled block number (minimum across all chains)
    pub last_settled_block_number: u64,
    /// Last settled sequence number
    pub last_settled_sequence: u64,
    /// Last purged sequence number
    pub last_purged_sequence: u64,
    /// Settlements map (chain -> Settlement)
    pub settlements: HashMap<String, Settlement>,
    /// Whether the app is paused
    pub is_paused: bool,
    /// Minimum time between blocks in milliseconds
    pub min_time_between_blocks: u64,
    /// Creation timestamp
    pub created_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
}

/// Fetch an AppInstance from the blockchain by its ID
/// This only fetches the AppInstance object itself, not the contents of its ObjectTables
pub async fn fetch_app_instance(instance_id: &str) -> Result<AppInstance> {
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
                            // Parse Entry struct (has key and value fields)
                            if let (Some(key_field), Some(value_field)) =
                                (entry.fields.get("key"), entry.fields.get("value"))
                            {
                                // Parse the chain string (key)
                                let chain = if let Some(prost_types::value::Kind::StringValue(s)) =
                                    &key_field.kind
                                {
                                    s.clone()
                                } else {
                                    continue;
                                };

                                // Parse the Settlement struct (value)
                                if let Some(prost_types::value::Kind::StructValue(
                                    settlement_struct,
                                )) = &value_field.kind
                                {
                                    // Extract the ObjectTable ID for block_settlements
                                    let block_settlements_table_id =
                                        get_table_id(settlement_struct, "block_settlements").ok();

                                    let settlement = Settlement {
                                        chain: get_string(settlement_struct, "chain")
                                            .unwrap_or_default(),
                                        last_settled_block_number: get_u64(
                                            settlement_struct,
                                            "last_settled_block_number",
                                        ),
                                        settlement_address: get_string(
                                            settlement_struct,
                                            "settlement_address",
                                        ),
                                        block_settlements_table_id,
                                        settlement_job: settlement_struct
                                            .fields
                                            .get("settlement_job")
                                            .and_then(|f| {
                                                if let Some(
                                                    prost_types::value::Kind::StringValue(s),
                                                ) = &f.kind
                                                {
                                                    s.parse::<u64>().ok()
                                                } else {
                                                    None
                                                }
                                            }),
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

/// Parse AppInstance from protobuf struct representation
pub fn parse_app_instance_from_struct(
    struct_value: &prost_types::Struct,
    object_id: &str,
) -> Result<AppInstance> {
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
        state: struct_value
            .fields
            .get("state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        blocks_table_id: get_table_id(struct_value, "blocks")?,
        proof_calculations_table_id: get_table_id(struct_value, "proof_calculations")?,
        sequence_state_manager: struct_value
            .fields
            .get("sequence_state_manager")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        jobs: struct_value.fields.get("jobs").and_then(|f| {
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
        previous_block_actions_state: struct_value
            .fields
            .get("previous_block_actions_state")
            .map(proto_to_json)
            .unwrap_or(serde_json::Value::Null),
        last_proved_block_number: get_u64(struct_value, "last_proved_block_number"),
        last_settled_block_number: get_u64(struct_value, "last_settled_block_number"),
        last_settled_sequence: get_u64(struct_value, "last_settled_sequence"),
        last_purged_sequence: get_u64(struct_value, "last_purged_sequence"),
        settlements: parse_settlements(struct_value),
        is_paused: get_bool(struct_value, "isPaused"),
        min_time_between_blocks: get_u64(struct_value, "min_time_between_blocks"),
        created_at: get_u64(struct_value, "created_at"),
        updated_at: get_u64(struct_value, "updated_at"),
    };

    // debug!("Successfully parsed AppInstance: {}", app_instance.silvana_app_name);
    // debug!("  Block number: {}", app_instance.block_number);
    // debug!("  Last proved block: {}", app_instance.last_proved_block_number);
    // debug!("  Last settled block: {}", app_instance.last_settled_block_number);
    // debug!("  Blocks table ID: {}", app_instance.blocks_table_id);
    // debug!("  Proof calculations table ID: {}", app_instance.proof_calculations_table_id);

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
            last_settled_block_number: 7,
            last_settled_sequence: 95,
            last_purged_sequence: 90,
            settlements: {
                let mut settlements = HashMap::new();
                settlements.insert(
                    "mina".to_string(),
                    Settlement {
                        chain: "mina".to_string(),
                        last_settled_block_number: 8,
                        settlement_address: Some("0xsettlement".to_string()),
                        settlement_job: None,
                        block_settlements_table_id: None,
                    },
                );
                settlements
            },
            is_paused: false,
            min_time_between_blocks: 60_000,
            created_at: 1234567890,
            updated_at: 1234567891,
        };

        assert_eq!(app.silvana_app_name, "TestApp");
        assert_eq!(app.block_number, 10);
        assert_eq!(app.last_proved_block_number, 9);
        assert_eq!(app.last_settled_block_number, 7);
        assert!(!app.is_paused);
    }
}

/// Get the settlement job ID for a specific app instance ID
/// Get all settlement job IDs per chain for an app instance
pub async fn get_settlement_job_ids_for_instance(
    app_instance: &AppInstance,
) -> Result<HashMap<String, u64>> {
    debug!(
        "Getting settlement job IDs for app instance {}",
        app_instance.id
    );

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

/// Get the settlement chain for a specific job sequence by fetching app instance
/// Returns Some(chain_name) if the job sequence matches a settlement job, None otherwise
pub async fn get_settlement_chain_by_job_sequence(
    app_instance_id: &str,
    job_sequence: u64,
) -> Result<Option<String>> {
    debug!(
        "Looking up settlement chain for job sequence {} in app instance {}",
        job_sequence, app_instance_id
    );

    // Fetch the app instance
    let app_instance = match fetch_app_instance(app_instance_id).await {
        Ok(instance) => instance,
        Err(e) => {
            warn!("Failed to fetch app instance {}: {}", app_instance_id, e);
            return Err(e);
        }
    };

    // Iterate through settlements to find matching job sequence
    for (chain_name, settlement) in &app_instance.settlements {
        if let Some(settlement_job_sequence) = settlement.settlement_job {
            if settlement_job_sequence == job_sequence {
                debug!(
                    "Found job sequence {} belongs to settlement chain '{}'",
                    job_sequence, chain_name
                );
                return Ok(Some(chain_name.clone()));
            }
        }
    }

    debug!(
        "Job sequence {} not found in any settlement for app instance {}",
        job_sequence, app_instance_id
    );
    Ok(None)
}

/// Fetch a BlockSettlement from a Settlement's ObjectTable
pub async fn fetch_block_settlement(
    settlement: &Settlement,
    block_number: u64,
) -> Result<Option<BlockSettlement>> {
    // Check if we have the table ID
    let table_id = match &settlement.block_settlements_table_id {
        Some(id) => id,
        None => {
            error!(
                "No block_settlements_table_id for chain {}",
                settlement.chain
            );
            return Ok(None);
        }
    };

    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!(
        "Fetching BlockSettlement for block {} from table {} (chain: {})",
        block_number, table_id, settlement.chain
    );

    // Fetch the BlockSettlement from the ObjectTable
    fetch_block_settlement_from_table(&mut client, table_id, block_number).await
}

/// Fetch a BlockSettlement from an ObjectTable by block number
async fn fetch_block_settlement_from_table(
    client: &mut sui_rpc::Client,
    table_id: &str,
    block_number: u64,
) -> Result<Option<BlockSettlement>> {
    // First, find the dynamic field for this block number
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
                    "Failed to list dynamic fields for block settlements: {}",
                    e
                ))
            })?;

        let response = fields_response.into_inner();
        pages_searched += 1;

        // Look for our block number in the dynamic fields
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                // Try to decode the key as u64 (block number)
                if let Ok(field_block_number) = bcs::from_bytes::<u64>(name_value) {
                    if field_block_number == block_number {
                        // Found our block! Now fetch the BlockSettlement object
                        if let Some(field_id) = &field.field_id {
                            debug!(
                                "Found BlockSettlement field for block {}: {}",
                                block_number, field_id
                            );
                            return fetch_block_settlement_object(client, field_id).await;
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
        "BlockSettlement for block {} not found after searching {} pages",
        block_number, pages_searched
    );
    Ok(None)
}

/// Fetch the BlockSettlement object from Sui
async fn fetch_block_settlement_object(
    client: &mut sui_rpc::Client,
    field_id: &str,
) -> Result<Option<BlockSettlement>> {
    // First fetch the Field wrapper object to get the actual BlockSettlement object ID
    debug!("Fetching Field wrapper object: {}", field_id);

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
                "Failed to fetch Field wrapper {}: {}",
                field_id, e
            ))
        })?;

    let field_data = field_response.into_inner();

    // Extract the actual BlockSettlement object ID from the Field wrapper
    let block_settlement_id = if let Some(field_object) = field_data.object {
        if let Some(json_value) = &field_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // The Field wrapper has a "value" field containing the actual object ID
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(obj_id)) = &value_field.kind {
                        debug!(
                            "Found BlockSettlement object ID in Field wrapper: {}",
                            obj_id
                        );
                        obj_id.clone()
                    } else {
                        warn!("Field wrapper value is not a string");
                        return Ok(None);
                    }
                } else {
                    warn!("Field wrapper has no 'value' field");
                    return Ok(None);
                }
            } else {
                warn!("Field wrapper JSON is not a struct");
                return Ok(None);
            }
        } else {
            warn!("Field wrapper has no JSON");
            return Ok(None);
        }
    } else {
        warn!("Field wrapper response has no object");
        return Ok(None);
    };

    // Now fetch the actual BlockSettlement object
    debug!("Fetching BlockSettlement object: {}", block_settlement_id);

    let bs_request = GetObjectRequest {
        object_id: Some(block_settlement_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let bs_response = client
        .ledger_client()
        .get_object(bs_request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch BlockSettlement object {}: {}",
                block_settlement_id, e
            ))
        })?;

    let bs_data = bs_response.into_inner();

    if let Some(proto_object) = bs_data.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Parse the BlockSettlement struct
                let block_settlement = BlockSettlement {
                    block_number: get_u64(struct_value, "block_number"),
                    settlement_tx_hash: get_string(struct_value, "settlement_tx_hash"),
                    settlement_tx_included_in_block: get_bool(
                        struct_value,
                        "settlement_tx_included_in_block",
                    ),
                    sent_to_settlement_at: struct_value
                        .fields
                        .get("sent_to_settlement_at")
                        .and_then(|f| {
                            if let Some(prost_types::value::Kind::StringValue(s)) = &f.kind {
                                s.parse::<u64>().ok()
                            } else {
                                None
                            }
                        }),
                    settled_at: struct_value.fields.get("settled_at").and_then(|f| {
                        if let Some(prost_types::value::Kind::StringValue(s)) = &f.kind {
                            s.parse::<u64>().ok()
                        } else {
                            None
                        }
                    }),
                };

                debug!(
                    "Successfully parsed BlockSettlement for block {}",
                    block_settlement.block_number
                );
                return Ok(Some(block_settlement));
            }
        }
    }

    warn!(
        "Failed to parse BlockSettlement from object {}",
        block_settlement_id
    );
    Ok(None)
}
