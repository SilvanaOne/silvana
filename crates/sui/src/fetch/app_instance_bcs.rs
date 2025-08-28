use anyhow::{anyhow, Result};
use sui_rpc::proto::sui::rpc::v2beta2::GetObjectRequest;
use tracing::debug;

use crate::error::SilvanaSuiInterfaceError;
use crate::state::SharedSuiState;

use super::AppInstance;
use super::jobs::Jobs as FetchJobs;
use serde_json::{json, Value as JsonValue};
use base64ct::Encoding;
use std::collections::HashMap;

// ---------- BCS mirror types for Move ----------
// Minimal core types
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UID { pub id: [u8; 32] }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MoveString { pub bytes: Vec<u8> }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct VecMap<K, V> { pub contents: Vec<Entry<K, V>> }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Entry<K, V> { pub key: K, pub value: V }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ObjectTable<K, V> {
    pub id: UID,
    pub size: u64,
    #[serde(skip)]
    _phantom_k: std::marker::PhantomData<K>,
    #[serde(skip)]
    _phantom_v: std::marker::PhantomData<V>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Element<T> {
    pub bytes: Vec<u8>,
    #[serde(skip)]
    _phantom: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Scalar { pub bytes: Vec<u8> }

type Address = [u8; 32];

// commitment::actions::ActionsCommitment
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ActionsCommitment {
    pub id: UID,
    pub sequence: u64,
    pub r_power: Element<Scalar>,
    pub commitment: Element<Scalar>,
}

// commitment::action::Action
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Action {
    pub action: MoveString,
    pub action_data: Vec<[u8; 32]>, // vector<u256>
}

// commitment::state::{StateElement, StateUpdate, AppState}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateElement { pub id: UID, pub state: Vec<[u8; 32]> }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StateUpdate { pub index: u32, pub new_state: Vec<[u8; 32]> }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RollbackElement {
    pub index: u32,
    pub previous_state: Option<Vec<[u8; 32]>>,
    pub new_state: Vec<[u8; 32]>,
    pub commitment_before: Element<Scalar>,
    pub commitment_after: Element<Scalar>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RollbackSequence {
    pub id: UID,
    pub sequence: u64,
    pub action: Action,
    pub elements: Vec<RollbackElement>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Rollback {
    pub id: UID,
    pub start_sequence: u64,
    pub traced_sequence: u64,
    pub end_sequence: u64,
    pub rollback_sequences: ObjectTable<u64, RollbackSequence>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AppState {
    pub id: UID,
    pub sequence: u64,
    pub actions_commitment: ActionsCommitment,
    pub state: ObjectTable<u32, StateElement>,
    pub state_commitment: Element<Scalar>,
    pub rollback: Rollback,
}

// coordination::app_method::AppMethod
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AppMethod {
    pub description: Option<MoveString>,
    pub developer: MoveString,
    pub agent: MoveString,
    pub agent_method: MoveString,
}

// coordination::block::Block
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Block {
    pub id: UID,
    pub name: MoveString,
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub actions_commitment: Element<Scalar>,
    pub state_commitment: Element<Scalar>,
    pub time_since_last_block: u64,
    pub number_of_transactions: u64,
    pub start_actions_commitment: Element<Scalar>,
    pub end_actions_commitment: Element<Scalar>,
    pub state_data_availability: Option<MoveString>,
    pub proof_data_availability: Option<MoveString>,
    pub settlement_tx_hash: Option<MoveString>,
    pub settlement_tx_included_in_block: bool,
    pub created_at: u64,
    pub state_calculated_at: Option<u64>,
    pub proved_at: Option<u64>,
    pub sent_to_settlement_at: Option<u64>,
    pub settled_at: Option<u64>,
}

// coordination::prover::ProofCalculation
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProofCalculation {
    pub id: UID,
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    pub proofs: VecMap<Vec<u64>, Proof>,
    pub block_proof: Option<MoveString>,
    pub is_finished: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Proof {
    pub status: u8,
    pub da_hash: Option<MoveString>,
    pub sequence1: Option<Vec<u64>>,
    pub sequence2: Option<Vec<u64>>,
    pub rejected_count: u16,
    pub timestamp: u64,
    pub prover: Address,
    pub user: Option<Address>,
    pub job_id: MoveString,
}

// coordination::sequence_state
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SequenceState {
    pub id: UID,
    pub sequence: u64,
    pub state: Option<Vec<u8>>,
    pub data_availability: Option<MoveString>,
    pub optimistic_state: Vec<u8>,
    pub transition_data: Vec<u8>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SequenceStateManager {
    pub id: UID,
    pub sequence_states: ObjectTable<u64, SequenceState>,
    pub lowest_sequence: Option<u64>,
    pub highest_sequence: Option<u64>,
}

// coordination::jobs
#[derive(Debug, Clone, serde::Deserialize)]
pub enum JobStatus { Pending, Running, Failed(MoveString) }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Job {
    pub id: UID,
    pub job_sequence: u64,
    pub description: Option<MoveString>,
    pub developer: MoveString,
    pub agent: MoveString,
    pub agent_method: MoveString,
    pub app: MoveString,
    pub app_instance: MoveString,
    pub app_instance_method: MoveString,
    pub block_number: Option<u64>,
    pub sequences: Option<Vec<u64>>,
    pub sequences1: Option<Vec<u64>>,
    pub sequences2: Option<Vec<u64>>,
    pub data: Vec<u8>,
    pub status: JobStatus,
    pub attempts: u8,
    pub interval_ms: Option<u64>,
    pub next_scheduled_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct VecSet<T> { pub contents: Vec<T> }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Jobs {
    pub id: UID,
    pub jobs: ObjectTable<u64, Job>,
    pub pending_jobs: VecSet<u64>,
    pub pending_jobs_count: u64,
    pub pending_jobs_indexes: VecMap<MoveString, VecMap<MoveString, VecMap<MoveString, VecSet<u64>>>>,
    pub next_job_sequence: u64,
    pub max_attempts: u8,
    pub settlement_job: Option<u64>,
}

// coordination::app_instance::AppInstance
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AppInstanceBcs {
    pub id: UID,
    pub silvana_app_name: MoveString,
    pub description: Option<MoveString>,
    pub metadata: VecMap<MoveString, MoveString>,
    pub kv: VecMap<MoveString, MoveString>,
    pub methods: VecMap<MoveString, AppMethod>,
    pub state: AppState,
    pub blocks: ObjectTable<u64, Block>,
    pub proof_calculations: ObjectTable<u64, ProofCalculation>,
    pub sequence_state_manager: SequenceStateManager,
    pub jobs: Jobs,
    pub sequence: u64,
    pub admin: Address,
    pub block_number: u64,
    pub previous_block_timestamp: u64,
    pub previous_block_last_sequence: u64,
    pub previous_block_actions_state: Element<Scalar>,
    pub last_proved_block_number: u64,
    pub last_settled_block_number: u64,
    pub settlement_chain: Option<MoveString>,
    pub settlement_address: Option<MoveString>,
    #[serde(rename = "isPaused")]
    pub is_paused: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

// ---------- Converters to match server JSON shapes ----------

fn to_hex(addr: &[u8; 32]) -> String { format!("0x{}", hex::encode(addr)) }

fn elem_bytes_json(e: &Element<Scalar>) -> JsonValue {
    json!({
        "bytes": base64ct::Base64::encode_string(&e.bytes)
    })
}

fn object_table_json<K, V>(t: &ObjectTable<K, V>) -> JsonValue {
    json!({
        "id": to_hex(&t.id.id),
        "size": t.size.to_string(),
    })
}

fn actions_commitment_json(a: &ActionsCommitment) -> JsonValue {
    json!({
        "id": to_hex(&a.id.id),
        "sequence": a.sequence.to_string(),
        "r_power": elem_bytes_json(&a.r_power),
        "commitment": elem_bytes_json(&a.commitment),
    })
}

fn rollback_json(r: &Rollback) -> JsonValue {
    json!({
        "id": to_hex(&r.id.id),
        "start_sequence": r.start_sequence.to_string(),
        "traced_sequence": r.traced_sequence.to_string(),
        "end_sequence": r.end_sequence.to_string(),
        "rollback_sequences": object_table_json::<u64, RollbackSequence>(&r.rollback_sequences),
    })
}

fn app_state_json(s: &AppState) -> JsonValue {
    json!({
        "id": to_hex(&s.id.id),
        "sequence": s.sequence.to_string(),
        "actions_commitment": actions_commitment_json(&s.actions_commitment),
        "state": object_table_json::<u32, StateElement>(&s.state),
        "state_commitment": elem_bytes_json(&s.state_commitment),
        "rollback": rollback_json(&s.rollback),
    })
}

fn seq_state_mgr_json(m: &SequenceStateManager) -> JsonValue {
    json!({
        "id": to_hex(&m.id.id),
        "sequence_states": object_table_json::<u64, SequenceState>(&m.sequence_states),
        "lowest_sequence": m.lowest_sequence.map(|v| v.to_string()),
        "highest_sequence": m.highest_sequence.map(|v| v.to_string()),
    })
}

fn methods_json(methods: VecMap<MoveString, AppMethod>) -> HashMap<String, JsonValue> {
    let mut out = HashMap::new();
    for Entry { key, value } in methods.contents {
        let name = String::from_utf8_lossy(&key.bytes).to_string();
        let obj = json!({
            "description": value.description.as_ref().map(|s| String::from_utf8_lossy(&s.bytes).to_string()),
            "developer": String::from_utf8_lossy(&value.developer.bytes).to_string(),
            "agent": String::from_utf8_lossy(&value.agent.bytes).to_string(),
            "agent_method": String::from_utf8_lossy(&value.agent_method.bytes).to_string(),
        });
        out.insert(name, obj);
    }
    out
}

fn vecmap_nested_to_hashmap(m: VecMap<MoveString, VecMap<MoveString, VecMap<MoveString, VecSet<u64>>>>) -> HashMap<String, HashMap<String, HashMap<String, Vec<u64>>>> {
    let mut level1 = HashMap::new();
    for Entry { key: dev, value: agents } in m.contents {
        let dev_key = String::from_utf8_lossy(&dev.bytes).to_string();
        let mut level2 = HashMap::new();
        for Entry { key: agent, value: methods } in agents.contents {
            let agent_key = String::from_utf8_lossy(&agent.bytes).to_string();
            let mut level3 = HashMap::new();
            for Entry { key: method, value: vecset } in methods.contents {
                let method_key = String::from_utf8_lossy(&method.bytes).to_string();
                level3.insert(method_key, vecset.contents);
            }
            level2.insert(agent_key, level3);
        }
        level1.insert(dev_key, level2);
    }
    level1
}

/// Fetch an AppInstance using gRPC with the BCS read mask and attempt to decode via BCS.
///
/// Notes:
/// - This requests only the Move struct contents (BCS) over gRPC.
/// - Fully decoding the BCS requires Rust mirror types for every nested Move type.
pub async fn fetch_app_instance_bcs(
    instance_id: &str,
) -> Result<AppInstance> {
    let mut client = SharedSuiState::get_instance().get_sui_client();

    // Ensure the instance_id has 0x prefix
    let formatted_id = if instance_id.starts_with("0x") {
        instance_id.to_string()
    } else {
        format!("0x{}", instance_id)
    };

    debug!("Fetching AppInstance (BCS) with ID: {}", formatted_id);

    // Ask only for the fields we need: Move struct contents (BCS) and object_id for convenience
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["contents".to_string(), "object_id".to_string()],
        }),
    };

    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| {
            SilvanaSuiInterfaceError::RpcConnectionError(format!(
                "Failed to fetch AppInstance {} (BCS): {}",
                instance_id, e
            ))
        })?;

    let object = response.into_inner().object.ok_or_else(|| {
        anyhow!("No object found for {}", formatted_id)
    })?;

    let contents_bcs = object
        .contents
        .as_ref()
        .and_then(|b| b.value.clone())
        .ok_or_else(|| anyhow!("Object.contents (BCS) missing for {}", formatted_id))?;

    debug!("Fetched {} BCS bytes for AppInstance {}", contents_bcs.len(), formatted_id);

    // At this point we have the raw BCS for the Move value of AppInstance.
    // Decoding requires Rust mirror types for ALL nested Move types used by AppInstance.
    // If you already implemented those mirrors (e.g., `AppInstanceBcs`), replace the `unimplemented!()`
    // block with: `let bcs_value: AppInstanceBcs = bcs::from_bytes(&contents_bcs)?;` and then
    // map it into your high-level `AppInstance` below.

    // Decode into BCS mirror
    let raw: AppInstanceBcs = bcs::from_bytes::<AppInstanceBcs>(&contents_bcs)?;

    // Map MoveString and ids into your high-level AppInstance used elsewhere
    let to_utf8 = |s: &MoveString| String::from_utf8_lossy(&s.bytes).to_string();
    let id_hex = format!("0x{}", hex::encode(raw.id.id));
    let admin_hex = format!("0x{}", hex::encode(raw.admin));

    // Helper to extract table id from ObjectTable<K,V>
    let table_id_hex = |uid: &UID| format!("0x{}", hex::encode(uid.id));

    // VecMap<String, String> → HashMap<String, String>
    let vecmap_to_map = |m: VecMap<MoveString, MoveString>| -> std::collections::HashMap<String, String> {
        m.contents
            .into_iter()
            .map(|e| (to_utf8(&e.key), to_utf8(&e.value)))
            .collect()
    };

    // Build methods to match server JSON
    let methods_hm = methods_json(raw.methods);

    Ok(AppInstance {
        id: id_hex,
        silvana_app_name: to_utf8(&raw.silvana_app_name),
        description: raw.description.as_ref().map(|s| to_utf8(s)),
        metadata: vecmap_to_map(raw.metadata),
        kv: vecmap_to_map(raw.kv),
        methods: methods_hm,
        state: app_state_json(&raw.state),
        blocks_table_id: table_id_hex(&raw.blocks.id),
        proof_calculations_table_id: table_id_hex(&raw.proof_calculations.id),
        sequence_state_manager: seq_state_mgr_json(&raw.sequence_state_manager),
        jobs: Some(FetchJobs {
            id: to_hex(&raw.jobs.id.id),
            jobs_table_id: table_id_hex(&raw.jobs.jobs.id),
            pending_jobs: raw.jobs.pending_jobs.contents,
            pending_jobs_count: raw.jobs.pending_jobs_count,
            pending_jobs_indexes: vecmap_nested_to_hashmap(raw.jobs.pending_jobs_indexes),
            next_job_sequence: raw.jobs.next_job_sequence,
            max_attempts: raw.jobs.max_attempts,
            settlement_job: raw.jobs.settlement_job,
        }),
        sequence: raw.sequence,
        admin: admin_hex,
        block_number: raw.block_number,
        previous_block_timestamp: raw.previous_block_timestamp,
        previous_block_last_sequence: raw.previous_block_last_sequence,
        previous_block_actions_state: elem_bytes_json(&raw.previous_block_actions_state),
        last_proved_block_number: raw.last_proved_block_number,
        last_settled_block_number: raw.last_settled_block_number,
        settlement_chain: raw.settlement_chain.as_ref().map(|s| to_utf8(s)),
        settlement_address: raw.settlement_address.as_ref().map(|s| to_utf8(s)),
        is_paused: raw.is_paused,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}


