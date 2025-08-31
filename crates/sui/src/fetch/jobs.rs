use crate::error::{SilvanaSuiInterfaceError, Result};
use crate::parse::{get_string, get_u64, get_u8, get_option_u64, get_vec_u64, get_bytes};
use crate::state::SharedSuiState;
use super::AppInstance;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest, BatchGetObjectsRequest};
use tracing::{debug, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};

/// Rust representation of the Move JobStatus enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Pending,
    Running,
    Failed(String),
}

impl JobStatus {
    /// Parse JobStatus from protobuf value
    #[allow(dead_code)]
    pub fn from_proto_value(value: &prost_types::Value) -> Self {
        match &value.kind {
            Some(prost_types::value::Kind::StringValue(s)) => {
                match s.as_str() {
                    "Pending" => JobStatus::Pending,
                    "Running" => JobStatus::Running,
                    _ if s.starts_with("Failed(") => {
                        // Parse "Failed(error message)" format
                        let error_msg = s.trim_start_matches("Failed(")
                            .trim_end_matches(')')
                            .to_string();
                        JobStatus::Failed(error_msg)
                    }
                    _ => JobStatus::Failed(format!("Unknown status: {}", s))
                }
            }
            Some(prost_types::value::Kind::StructValue(struct_val)) => {
                // Check if it's a struct with a variant field (enum representation)
                if let Some(variant_field) = struct_val.fields.get("variant") {
                    if let Some(prost_types::value::Kind::StringValue(variant)) = &variant_field.kind {
                        match variant.as_str() {
                            "Pending" => return JobStatus::Pending,
                            "Running" => return JobStatus::Running,
                            "Failed" => {
                                // Look for the error message in the struct
                                if let Some(msg_field) = struct_val.fields.get("fields") {
                                    if let Some(prost_types::value::Kind::ListValue(list)) = &msg_field.kind {
                                        if let Some(first) = list.values.first() {
                                            if let Some(prost_types::value::Kind::StringValue(msg)) = &first.kind {
                                                return JobStatus::Failed(msg.clone());
                                            }
                                        }
                                    }
                                }
                                return JobStatus::Failed("Unknown error".to_string());
                            }
                            _ => {}
                        }
                    }
                }
                JobStatus::Failed("Unknown status format".to_string())
            }
            _ => JobStatus::Failed("Invalid status type".to_string())
        }
    }
}

/// Rust representation of the Move Job struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// The unique identifier of the Job object
    pub id: String,
    /// Job sequence number
    pub job_sequence: u64,
    /// Optional description
    pub description: Option<String>,
    /// Developer identifier
    pub developer: String,
    /// Agent identifier
    pub agent: String,
    /// Agent method name
    pub agent_method: String,
    /// App name
    pub app: String,
    /// App instance identifier
    pub app_instance: String,
    /// App instance method name
    pub app_instance_method: String,
    /// Optional block number
    pub block_number: Option<u64>,
    /// Optional sequence numbers
    pub sequences: Option<Vec<u64>>,
    /// Optional sequences1 for merge operations
    pub sequences1: Option<Vec<u64>>,
    /// Optional sequences2 for merge operations
    pub sequences2: Option<Vec<u64>>,
    /// Job data as bytes
    pub data: Vec<u8>,
    /// Job status
    pub status: JobStatus,
    /// Number of attempts
    pub attempts: u8,
    /// Interval in milliseconds for periodic jobs
    pub interval_ms: Option<u64>,
    /// Next scheduled time for periodic jobs (absolute timestamp in ms)
    pub next_scheduled_at: Option<u64>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
}

/// Rust representation of the Move Jobs struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jobs {
    /// The unique identifier of the Jobs object
    pub id: String,
    /// ObjectTable ID for jobs (u64 -> Job mapping)
    pub jobs_table_id: String,
    /// ObjectTable ID for failed_jobs (u64 -> Job mapping)
    pub failed_jobs_table_id: String,
    /// Count of failed jobs
    pub failed_jobs_count: u64,
    /// Set of failed job IDs
    pub failed_jobs_index: Vec<u64>,
    /// Set of pending job IDs
    pub pending_jobs: Vec<u64>,
    /// Count of pending jobs
    pub pending_jobs_count: u64,
    /// Nested index structure: developer -> agent -> app_method -> job_ids
    /// Stored as nested HashMap for easier access
    pub pending_jobs_indexes: HashMap<String, HashMap<String, HashMap<String, Vec<u64>>>>,
    /// Next job sequence number
    pub next_job_sequence: u64,
    /// Maximum attempts allowed
    pub max_attempts: u8,
    /// The settlement job ID if one exists
    pub settlement_job: Option<u64>,
}

impl Jobs {
    /// Parse Jobs from protobuf struct value
    pub fn from_proto_struct(struct_value: &prost_types::Struct) -> Option<Self> {
        // Debug: Show the raw Jobs struct from Sui
        debug!("=== Raw Jobs struct from Sui ===");
        debug!("Jobs {{");
        for (key, value) in &struct_value.fields {
            // Special handling for nested structures
            match key.as_str() {
                "pending_jobs_indexes" => {
                    debug!("    pending_jobs_indexes: {{");
                    if let Some(prost_types::value::Kind::StructValue(indexes_struct)) = &value.kind {
                        if let Some(contents) = indexes_struct.fields.get("contents") {
                            if let Some(prost_types::value::Kind::ListValue(list)) = &contents.kind {
                                debug!("        contents: [{} entries]", list.values.len());
                                for (i, entry) in list.values.iter().enumerate().take(3) {
                                    debug!("            Entry {}: {:?}", i, entry.kind);
                                }
                                if list.values.len() > 3 {
                                    debug!("            ... and {} more entries", list.values.len() - 3);
                                }
                            }
                        }
                    }
                    debug!("    }},");
                },
                "pending_jobs" | "failed_jobs_index" => {
                    if let Some(prost_types::value::Kind::StructValue(vecset_struct)) = &value.kind {
                        if let Some(contents) = vecset_struct.fields.get("contents") {
                            if let Some(prost_types::value::Kind::ListValue(list)) = &contents.kind {
                                debug!("    {}: VecSet with {} items,", key, list.values.len());
                            } else {
                                debug!("    {}: {:?},", key, value.kind);
                            }
                        } else {
                            debug!("    {}: {:?},", key, value.kind);
                        }
                    } else {
                        debug!("    {}: {:?},", key, value.kind);
                    }
                },
                _ => {
                    debug!("    {}: {:?},", key, value.kind);
                }
            }
        }
        debug!("}}");
        debug!("=== End Raw Jobs ===");
        
        // Extract Jobs table ID
        let jobs_table_id = struct_value.fields.get("jobs")
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
            })?;
        
        // Extract failed_jobs table ID
        let failed_jobs_table_id = struct_value.fields.get("failed_jobs")
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
            }).unwrap_or_default();
        
        // Extract failed_jobs_index VecSet
        let failed_jobs_index = struct_value.fields.get("failed_jobs_index")
            .and_then(|f| {
                if let Some(prost_types::value::Kind::StructValue(vecset_struct)) = &f.kind {
                    if let Some(contents) = vecset_struct.fields.get("contents") {
                        if let Some(prost_types::value::Kind::ListValue(list)) = &contents.kind {
                            let mut jobs = Vec::new();
                            for value in &list.values {
                                match &value.kind {
                                    Some(prost_types::value::Kind::StringValue(s)) => {
                                        if let Ok(num) = s.parse::<u64>() {
                                            jobs.push(num);
                                        }
                                    }
                                    Some(prost_types::value::Kind::NumberValue(n)) => {
                                        jobs.push(n.round() as u64);
                                    }
                                    _ => {}
                                }
                            }
                            Some(jobs)
                        } else {
                            Some(Vec::new())
                        }
                    } else {
                        Some(Vec::new())
                    }
                } else {
                    Some(Vec::new())
                }
            }).unwrap_or_else(Vec::new);
        
        // Extract pending_jobs VecSet
        let pending_jobs = struct_value.fields.get("pending_jobs")
            .and_then(|f| {
                if let Some(prost_types::value::Kind::StructValue(vecset_struct)) = &f.kind {
                    if let Some(contents) = vecset_struct.fields.get("contents") {
                        if let Some(prost_types::value::Kind::ListValue(list)) = &contents.kind {
                            let mut jobs = Vec::new();
                            for value in &list.values {
                                match &value.kind {
                                    Some(prost_types::value::Kind::StringValue(s)) => {
                                        if let Ok(num) = s.parse::<u64>() {
                                            jobs.push(num);
                                        }
                                    }
                                    Some(prost_types::value::Kind::NumberValue(n)) => {
                                        jobs.push(n.round() as u64);
                                    }
                                    _ => {}
                                }
                            }
                            Some(jobs)
                        } else {
                            Some(Vec::new())
                        }
                    } else {
                        Some(Vec::new())
                    }
                } else {
                    Some(Vec::new())
                }
            }).unwrap_or_else(Vec::new);
        
        // Parse pending_jobs_indexes - complex nested VecMap structure
        let pending_jobs_indexes = parse_nested_vecmap_indexes(&struct_value.fields);
        
        // Parse settlement_job field (Option<u64>)
        let settlement_job = struct_value.fields.get("settlement_job")
            .and_then(|field| {
                match &field.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
                    Some(prost_types::value::Kind::NullValue(_)) => None,
                    _ => None,
                }
            });
        
        Some(Jobs {
            id: get_string(struct_value, "id").unwrap_or_default(),
            jobs_table_id,
            failed_jobs_table_id,
            failed_jobs_count: get_u64(struct_value, "failed_jobs_count"),
            failed_jobs_index,
            pending_jobs,
            pending_jobs_count: get_u64(struct_value, "pending_jobs_count"),
            pending_jobs_indexes,
            next_job_sequence: get_u64(struct_value, "next_job_sequence"),
            max_attempts: get_u8(struct_value, "max_attempts"),
            settlement_job,
        })
    }
}

/// Parse the complex nested VecMap structure for pending_jobs_indexes
fn parse_nested_vecmap_indexes(fields: &BTreeMap<String, prost_types::Value>) -> HashMap<String, HashMap<String, HashMap<String, Vec<u64>>>> {
    let mut result = HashMap::new();
    
    if let Some(indexes_field) = fields.get("pending_jobs_indexes") {
        if let Some(prost_types::value::Kind::StructValue(indexes_struct)) = &indexes_field.kind {
            if let Some(contents) = indexes_struct.fields.get("contents") {
                if let Some(prost_types::value::Kind::ListValue(developer_list)) = &contents.kind {
                    // Iterate over developer entries
                    for dev_entry in &developer_list.values {
                        if let Some(prost_types::value::Kind::StructValue(dev_struct)) = &dev_entry.kind {
                            if let (Some(dev_key), Some(dev_value)) = 
                                (dev_struct.fields.get("key"), dev_struct.fields.get("value")) {
                                if let Some(prost_types::value::Kind::StringValue(developer)) = &dev_key.kind {
                                    let mut agent_map = HashMap::new();
                                    
                                    // Parse agent VecMap
                                    if let Some(prost_types::value::Kind::StructValue(agent_vecmap)) = &dev_value.kind {
                                        if let Some(agent_contents) = agent_vecmap.fields.get("contents") {
                                            if let Some(prost_types::value::Kind::ListValue(agent_list)) = &agent_contents.kind {
                                                for agent_entry in &agent_list.values {
                                                    if let Some(prost_types::value::Kind::StructValue(agent_struct)) = &agent_entry.kind {
                                                        if let (Some(agent_key), Some(agent_value)) = 
                                                            (agent_struct.fields.get("key"), agent_struct.fields.get("value")) {
                                                            if let Some(prost_types::value::Kind::StringValue(agent)) = &agent_key.kind {
                                                                let mut method_map = HashMap::new();
                                                                
                                                                // Parse method VecMap
                                                                if let Some(prost_types::value::Kind::StructValue(method_vecmap)) = &agent_value.kind {
                                                                    if let Some(method_contents) = method_vecmap.fields.get("contents") {
                                                                        if let Some(prost_types::value::Kind::ListValue(method_list)) = &method_contents.kind {
                                                                            for method_entry in &method_list.values {
                                                                                if let Some(prost_types::value::Kind::StructValue(method_struct)) = &method_entry.kind {
                                                                                    if let (Some(method_key), Some(method_value)) = 
                                                                                        (method_struct.fields.get("key"), method_struct.fields.get("value")) {
                                                                                        if let Some(prost_types::value::Kind::StringValue(method)) = &method_key.kind {
                                                                                            let mut job_ids = Vec::new();
                                                                                            
                                                                                            // Parse VecSet of job IDs
                                                                                            if let Some(prost_types::value::Kind::StructValue(vecset)) = &method_value.kind {
                                                                                                if let Some(vecset_contents) = vecset.fields.get("contents") {
                                                                                                    if let Some(prost_types::value::Kind::ListValue(job_list)) = &vecset_contents.kind {
                                                                                                        for job_value in &job_list.values {
                                                                                                            match &job_value.kind {
                                                                                                                Some(prost_types::value::Kind::StringValue(s)) => {
                                                                                                                    if let Ok(num) = s.parse::<u64>() {
                                                                                                                        job_ids.push(num);
                                                                                                                    }
                                                                                                                }
                                                                                                                Some(prost_types::value::Kind::NumberValue(n)) => {
                                                                                                                    job_ids.push(n.round() as u64);
                                                                                                                }
                                                                                                                _ => {}
                                                                                                            }
                                                                                                        }
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                            
                                                                                            method_map.insert(method.clone(), job_ids);
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                
                                                                agent_map.insert(agent.clone(), method_map);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    
                                    result.insert(developer.clone(), agent_map);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    result
}


/// Extract Job from JSON representation
pub fn extract_job_from_json(json_value: &prost_types::Value) -> Result<Job> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        //debug!("Job JSON fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        
        // Extract job ID from the nested id field
        let id = struct_value.fields.get("id")
            .and_then(|field| {
                if let Some(prost_types::value::Kind::StructValue(id_struct)) = &field.kind {
                    id_struct.fields.get("id")
                        .and_then(|id_field| {
                            if let Some(prost_types::value::Kind::StringValue(id_str)) = &id_field.kind {
                                Some(id_str.clone())
                            } else {
                                None
                            }
                        })
                } else {
                    None
                }
            })
            .unwrap_or_default();
        
        // Parse status enum - Move enums are represented as structs with a single field
        // where the field name is the variant name
        let status = struct_value.fields.get("status")
            .and_then(|field| {
                // Debug: print the raw status field
                debug!("Raw status field: {:?}", field);
                
                if let Some(prost_types::value::Kind::StructValue(status_struct)) = &field.kind {
                    // Debug: print all fields in the status struct
                    debug!("Status struct fields: {:?}", status_struct.fields.keys().collect::<Vec<_>>());
                    
                    // Parse JobStatus enum - check the @variant field
                    if let Some(variant_field) = status_struct.fields.get("@variant") {
                        if let Some(prost_types::value::Kind::StringValue(variant_name)) = &variant_field.kind {
                            match variant_name.as_str() {
                                "Pending" => Some(JobStatus::Pending),
                                "Running" => Some(JobStatus::Running),
                                "Failed" => {
                                    // For Failed variant, look for the error message in another field
                                    // It might be in a "fields" array or another structure
                                    if let Some(fields_field) = status_struct.fields.get("fields") {
                                        if let Some(prost_types::value::Kind::ListValue(list)) = &fields_field.kind {
                                            if let Some(first) = list.values.first() {
                                                if let Some(prost_types::value::Kind::StringValue(msg)) = &first.kind {
                                                    Some(JobStatus::Failed(msg.clone()))
                                                } else {
                                                    Some(JobStatus::Failed("Unknown error".to_string()))
                                                }
                                            } else {
                                                Some(JobStatus::Failed("Unknown error".to_string()))
                                            }
                                        } else {
                                            Some(JobStatus::Failed("Unknown error".to_string()))
                                        }
                                    } else {
                                        Some(JobStatus::Failed("Unknown error".to_string()))
                                    }
                                }
                                _ => {
                                    warn!("Unknown status variant: {}", variant_name);
                                    None
                                }
                            }
                        } else {
                            warn!("@variant field is not a string");
                            None
                        }
                    } else if status_struct.fields.contains_key("Pending") {
                        // Fallback: check if variant names are direct fields
                        Some(JobStatus::Pending)
                    } else if status_struct.fields.contains_key("Running") {
                        Some(JobStatus::Running)
                    } else if let Some(failed_field) = status_struct.fields.get("Failed") {
                        // Failed variant has an associated string value
                        if let Some(prost_types::value::Kind::StructValue(failed_struct)) = &failed_field.kind {
                            // The error message might be in a nested structure
                            if let Some(msg_field) = failed_struct.fields.values().next() {
                                if let Some(prost_types::value::Kind::StringValue(msg)) = &msg_field.kind {
                                    Some(JobStatus::Failed(msg.clone()))
                                } else {
                                    Some(JobStatus::Failed("Unknown error".to_string()))
                                }
                            } else {
                                Some(JobStatus::Failed("Unknown error".to_string()))
                            }
                        } else if let Some(prost_types::value::Kind::StringValue(msg)) = &failed_field.kind {
                            // Or directly as a string
                            Some(JobStatus::Failed(msg.clone()))
                        } else {
                            Some(JobStatus::Failed("Unknown error".to_string()))
                        }
                    } else {
                        // Fallback: try the old parsing method
                        if let Some(variant_field) = status_struct.fields.iter().next() {
                            warn!("Using fallback status parsing for variant: {}", variant_field.0);
                            match variant_field.0.as_str() {
                                "Pending" => Some(JobStatus::Pending),
                                "Running" => Some(JobStatus::Running),
                                "Failed" => {
                                    if let Some(prost_types::value::Kind::StringValue(msg)) = &variant_field.1.kind {
                                        Some(JobStatus::Failed(msg.clone()))
                                    } else {
                                        Some(JobStatus::Failed("Unknown error".to_string()))
                                    }
                                }
                                _ => {
                                    warn!("Unknown status variant: {}", variant_field.0);
                                    None
                                }
                            }
                        } else {
                            warn!("Status struct has no fields");
                            None
                        }
                    }
                } else {
                    warn!("Status field is not a struct: {:?}", field);
                    None
                }
            })
            .unwrap_or(JobStatus::Pending);
        
        
        // Build the Job struct with all fields
        let job = Job {
            id,
            job_sequence: get_u64(struct_value, "job_sequence"),
            description: get_string(struct_value, "description"),
            developer: get_string(struct_value, "developer").unwrap_or_default(),
            agent: get_string(struct_value, "agent").unwrap_or_default(),
            agent_method: get_string(struct_value, "agent_method").unwrap_or_default(),
            app: get_string(struct_value, "app").unwrap_or_default(),
            app_instance: get_string(struct_value, "app_instance").unwrap_or_default(),
            app_instance_method: get_string(struct_value, "app_instance_method").unwrap_or_default(),
            block_number: if get_u64(struct_value, "block_number") > 0 { Some(get_u64(struct_value, "block_number")) } else { None },
            sequences: get_vec_u64(struct_value, "sequences"),
            sequences1: get_vec_u64(struct_value, "sequences1"),
            sequences2: get_vec_u64(struct_value, "sequences2"),
            data: get_bytes(struct_value, "data"),
            status,
            attempts: get_u8(struct_value, "attempts"),
            interval_ms: get_option_u64(struct_value, "interval_ms"),
            next_scheduled_at: get_option_u64(struct_value, "next_scheduled_at"),
            created_at: get_u64(struct_value, "created_at"),
            updated_at: get_u64(struct_value, "updated_at"),
        };
        
        return Ok(job);
    }
    
    Err(SilvanaSuiInterfaceError::ParseError(
        "Failed to extract job from JSON".to_string()
    ))
}

/// Fetch pending jobs from a specific app_instance
pub async fn fetch_pending_jobs_from_app_instance(
    app_instance: &AppInstance,
    only_check: bool,
) -> Result<Option<Job>> {
    debug!("Checking pending jobs for app_instance: {}", app_instance.id);
    
    // Use the Jobs struct that's already in the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        // For check-only mode, just look at pending_jobs_count
        if only_check {
            if jobs.pending_jobs_count > 0 {
                debug!("Found {} pending jobs in app_instance (check-only mode)", jobs.pending_jobs_count);
                return Ok(None);
            } else {
                debug!("No pending jobs in app_instance {} (count=0)", app_instance.id);
                return Ok(None);
            }
        }
        
        // Check if there are pending jobs
        if jobs.pending_jobs.is_empty() {
            debug!("No pending jobs in app_instance {}", app_instance.id);
            return Ok(None);
        }
        
        // Get the smallest job_sequence (pending_jobs Vec should already be sorted or we sort it)
        let mut job_sequences = jobs.pending_jobs.clone();
        job_sequences.sort();
        let target_job_sequence = job_sequences[0];
        
        debug!("Found {} pending jobs, will fetch job_sequence {}", job_sequences.len(), target_job_sequence);
        
        // Fetch the specific job using the jobs table ID
        match fetch_job_by_id(&jobs.jobs_table_id, target_job_sequence).await {
            Ok(Some(job)) => {
                debug!("Successfully fetched job {} from app_instance {}", target_job_sequence, app_instance.id);
                return Ok(Some(job));
            },
            Ok(None) => {
                debug!("Job {} not found in jobs table {} for app_instance {}", 
                    target_job_sequence, jobs.jobs_table_id, app_instance.id);
            },
            Err(e) => {
                debug!("Error fetching job {} from app_instance {}: {}", 
                    target_job_sequence, app_instance.id, e);
                return Err(e);
            }
        }
    }
    
    debug!("No jobs successfully fetched from app_instance {}", app_instance.id);
    Ok(None)
}

/// Batch fetch multiple jobs by their IDs from any jobs ObjectTable (jobs or failed_jobs)
/// Returns a HashMap of job_sequence -> Job for all found jobs
pub async fn fetch_jobs_batch(
    jobs_table_id: &str,
    job_sequences: &[u64],
) -> Result<HashMap<u64, Job>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    if job_sequences.is_empty() {
        return Ok(HashMap::new());
    }
    
    debug!("Batch fetching {} jobs from jobs table {}", job_sequences.len(), jobs_table_id);
    
    // First, find the field IDs for all requested jobs
    let mut field_ids_map = HashMap::new(); // job_sequence -> field_id
    let mut page_token = None;
    const PAGE_SIZE: u32 = 100;
    
    loop {
        let list_request = ListDynamicFieldsRequest {
            parent: Some(jobs_table_id.to_string()),
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
        
        let list_response = client
            .live_data_client()
            .list_dynamic_fields(list_request)
            .await
            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                format!("Failed to list jobs in table: {}", e)
            ))?;
        
        let response = list_response.into_inner();
        
        // Check each field to see if it's one of our requested jobs
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                if let Ok(field_job_seq) = bcs::from_bytes::<u64>(name_value) {
                    if job_sequences.contains(&field_job_seq) {
                        if let Some(field_id) = &field.field_id {
                            field_ids_map.insert(field_job_seq, field_id.clone());
                            
                            // Stop if we found all requested jobs
                            if field_ids_map.len() == job_sequences.len() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        
        // Check if we should continue pagination
        if field_ids_map.len() == job_sequences.len() {
            break; // Found all requested jobs
        }
        
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() {
                page_token = Some(next_token);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    if field_ids_map.is_empty() {
        debug!("No matching jobs found in table");
        return Ok(HashMap::new());
    }
    
    debug!("Found {} job field IDs, fetching job objects", field_ids_map.len());
    
    // Now batch fetch all job objects
    let mut jobs_map = HashMap::new();
    const BATCH_SIZE: usize = 50;
    
    // Process in batches
    let field_ids_vec: Vec<(u64, String)> = field_ids_map.into_iter().collect();
    
    for chunk in field_ids_vec.chunks(BATCH_SIZE) {
        debug!("📦 Batch fetching {} job field wrappers", chunk.len());
        
        // First batch: fetch field wrapper objects
        let field_requests: Vec<GetObjectRequest> = chunk
            .iter()
            .map(|(_, field_id)| GetObjectRequest {
                object_id: Some(field_id.clone()),
                version: None,
                read_mask: None, // Use batch-level mask instead
            })
            .collect();
        
        let batch_request = BatchGetObjectsRequest {
            requests: field_requests,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            }),
        };
        
        let batch_response = client
            .ledger_client()
            .batch_get_objects(batch_request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to batch fetch job field wrappers: {}",
                    e
                ))
            })?;
        
        let field_results = batch_response.into_inner().objects;
        
        // Extract job object IDs from field wrappers
        let mut job_object_ids = Vec::new(); // (job_object_id, job_sequence)
        for (i, get_result) in field_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(field_object)) = &get_result.result {
                if let Some(field_json) = &field_object.json {
                    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                        if let Some(value_field) = struct_value.fields.get("value") {
                            if let Some(prost_types::value::Kind::StringValue(job_object_id)) = &value_field.kind {
                                let (job_seq, _) = chunk[i];
                                job_object_ids.push((job_object_id.clone(), job_seq));
                            }
                        }
                    }
                }
            }
        }
        
        if job_object_ids.is_empty() {
            continue;
        }
        
        debug!("📦 Batch fetching {} job objects", job_object_ids.len());
        
        // Second batch: fetch actual job objects
        let job_requests: Vec<GetObjectRequest> = job_object_ids
            .iter()
            .map(|(job_id, _)| GetObjectRequest {
                object_id: Some(job_id.clone()),
                version: None,
                read_mask: None, // Use batch-level mask instead
            })
            .collect();
        
        let batch_request = BatchGetObjectsRequest {
            requests: job_requests,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["object_id".to_string(), "json".to_string()],
            }),
        };
        
        let batch_response = client
            .ledger_client()
            .batch_get_objects(batch_request)
            .await
            .map_err(|e| {
                SilvanaSuiInterfaceError::RpcConnectionError(format!(
                    "Failed to batch fetch job objects: {}",
                    e
                ))
            })?;
        
        let job_results = batch_response.into_inner().objects;
        
        // Extract Job data from results
        for (i, get_result) in job_results.iter().enumerate() {
            if let Some(sui_rpc::proto::sui::rpc::v2beta2::get_object_result::Result::Object(job_object)) = &get_result.result {
                if let Some(job_json) = &job_object.json {
                    match extract_job_from_json(job_json) {
                        Ok(mut job) => {
                            // Update the job ID from the object ID
                            job.id = job_object_ids[i].0.clone();
                            let (_, job_seq) = job_object_ids[i];
                            jobs_map.insert(job_seq, job);
                        }
                        Err(e) => {
                            debug!("Failed to extract job from JSON: {}", e);
                        }
                    }
                }
            }
        }
    }
    
    debug!("✅ Successfully fetched {} jobs", jobs_map.len());
    Ok(jobs_map)
}

/// Fetch a specific job by ID from the jobs ObjectTable (legacy single-job function)
pub async fn fetch_job_by_id(
    jobs_table_id: &str,
    job_sequence: u64,
) -> Result<Option<Job>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching job {} from jobs table {}", job_sequence, jobs_table_id);
    
    let mut page_token: Option<tonic::codegen::Bytes> = None;
    let mut total_fields_checked = 0;
    
    // Loop through pages until we find the job or exhaust all pages
    loop {
        // List dynamic fields to find the specific job
        let list_request = ListDynamicFieldsRequest {
            parent: Some(jobs_table_id.to_string()),
            page_size: Some(100),
            page_token: page_token.clone(),
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
                format!("Failed to list jobs in table: {}", e)
            ))?;
        
        let response = list_response.into_inner();
        
        debug!("Listed {} dynamic fields in page (total checked: {})", 
            response.dynamic_fields.len(), 
            total_fields_checked + response.dynamic_fields.len());
        
        // Find the specific job entry in this page
        for field in &response.dynamic_fields {
            if let Some(name_value) = &field.name_value {
                // The name_value is BCS-encoded u64 (job_sequence)
                match bcs::from_bytes::<u64>(name_value) {
                    Ok(field_job_sequence) => {
                        if field_job_sequence == job_sequence {
                            if let Some(field_id) = &field.field_id {
                                debug!("Found job {} in jobs table with field_id {}", job_sequence, field_id);
                                // Found the job, fetch its content
                                return fetch_job_object_by_field_id(field_id, job_sequence).await;
                            }
                        }
                    },
                    Err(e) => {
                        debug!("Failed to decode BCS for field name_value: {:?}, error: {}", name_value, e);
                    }
                }
            }
        }
        
        total_fields_checked += response.dynamic_fields.len();
        
        // Check if there are more pages
        if let Some(next_token) = response.next_page_token {
            if !next_token.is_empty() {
                page_token = Some(next_token);
                debug!("Continuing to next page of dynamic fields...");
            } else {
                break;
            }
        } else {
            break;
        }
    }
    
    debug!("Job {} not found in jobs table {} after checking {} fields", 
        job_sequence, jobs_table_id, total_fields_checked);
    Ok(None)
}

/// Fetch Job object by field ID (already verified to be the correct job)
async fn fetch_job_object_by_field_id(
    field_id: &str,
    job_sequence: u64,
) -> Result<Option<Job>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("📄 Fetching job {} from field {}", job_sequence, field_id);
    
    // Fetch the Field wrapper object
    let field_request = GetObjectRequest {
        object_id: Some(field_id.to_string()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "json".to_string(),
            ],
        }),
    };
    
    let field_response = client
        .ledger_client()
        .get_object(field_request)
        .await
        .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
            format!("Failed to fetch field wrapper for job {}: {}", job_sequence, e)
        ))?;
    
    if let Some(field_object) = field_response.into_inner().object {
        if let Some(field_json) = &field_object.json {
            debug!("📄 Field wrapper JSON retrieved for job {}", job_sequence);
            // Extract the actual job object ID from the Field wrapper
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &field_json.kind {
                if let Some(value_field) = struct_value.fields.get("value") {
                    if let Some(prost_types::value::Kind::StringValue(job_object_id)) = &value_field.kind {
                        debug!("📄 Found job object ID: {}", job_object_id);
                        // Fetch the actual job object
                        let job_request = GetObjectRequest {
                            object_id: Some(job_object_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let job_response = client
                            .ledger_client()
                            .get_object(job_request)
                            .await
                            .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                                format!("Failed to fetch job object {}: {}", job_sequence, e)
                            ))?;
                        
                        if let Some(job_object) = job_response.into_inner().object {
                            if let Some(job_json) = &job_object.json {
                                debug!("📗 Job {} JSON retrieved, extracting data", job_sequence);
                                // We already know this is the correct job from the name_value check
                                // so we can directly extract the job data
                                if let Ok(job) = extract_job_from_json(job_json) {
                                    return Ok(Some(job));
                                } else {
                                    warn!("❌ Failed to extract job {} from JSON", job_sequence);
                                }
                            } else {
                                warn!("❌ No JSON found for job object {}", job_sequence);
                            }
                        } else {
                            warn!("❌ No job object found for job {}", job_sequence);
                        }
                    }
                }
            }
        }
    }
    
    Ok(None)
}

/// Fetch pending job IDs for a specific (developer, agent, agent_method) from the embedded Jobs in AppInstance
pub async fn fetch_pending_job_sequences_from_app_instance(
    app_instance: &AppInstance,
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Vec<u64>> {
    debug!(
        "Fetching pending job IDs for {}/{}/{} from app_instance {}",
        developer, agent, agent_method, app_instance.id
    );
    
    // Use the Jobs struct that's already in the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        // Debug output to see the structure
        debug!("Jobs structure: pending_jobs_count={}, pending_jobs={:?}, pending_jobs_indexes keys={:?}", 
            jobs.pending_jobs_count, 
            jobs.pending_jobs.len(),
            jobs.pending_jobs_indexes.keys().collect::<Vec<_>>()
        );
        
        // Navigate through the nested pending_jobs_indexes structure
        if let Some(dev_agents) = jobs.pending_jobs_indexes.get(developer) {
            debug!("Found developer '{}' with agents: {:?}", developer, dev_agents.keys().collect::<Vec<_>>());
            if let Some(agent_methods) = dev_agents.get(agent) {
                debug!("Found agent '{}' with methods: {:?}", agent, agent_methods.keys().collect::<Vec<_>>());
                if let Some(job_sequences) = agent_methods.get(agent_method) {
                    debug!("Found {} pending job sequences for {}/{}/{}", 
                        job_sequences.len(), developer, agent, agent_method);
                    return Ok(job_sequences.clone());
                } else {
                    debug!("Method '{}' not found in agent '{}'", agent_method, agent);
                }
            } else {
                debug!("Agent '{}' not found in developer '{}'", agent, developer);
            }
        } else {
            debug!("Developer '{}' not found in pending_jobs_indexes", developer);
        }
        
        // If indexes are empty but pending_jobs has items, return all pending jobs
        if jobs.pending_jobs_indexes.is_empty() && !jobs.pending_jobs.is_empty() {
            debug!("WARNING: pending_jobs_indexes is empty but pending_jobs has {} items. Returning all pending jobs.", 
                jobs.pending_jobs.len());
            return Ok(jobs.pending_jobs.clone());
        }
    } else {
        debug!("No Jobs found in AppInstance");
    }
    
    debug!("No pending jobs found for {}/{}/{}", developer, agent, agent_method);
    Ok(Vec::new())
}

/// Get the Jobs table ID from an AppInstance (Jobs is embedded, so we just need the table ID)
pub async fn get_jobs_info_from_app_instance(
    app_instance: &AppInstance,
) -> Result<Option<(String, String)>> {
    debug!("Getting Jobs info from app_instance: {}", app_instance.id);
    
    // Use the jobs table ID directly from the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        debug!("Found Jobs table ID: {}", jobs.jobs_table_id);
        // Return app_instance.id as the "Jobs object" since Jobs is embedded
        return Ok(Some((app_instance.id.clone(), jobs.jobs_table_id.clone())));
    }
    
    debug!("No Jobs found in AppInstance {}", app_instance.id);
    Ok(None)
}

/// Fetch all jobs from an app instance
pub async fn fetch_all_jobs_from_app_instance(
    app_instance: &AppInstance,
) -> Result<Vec<Job>> {
    let mut client = SharedSuiState::get_instance().get_sui_client();
    debug!("Fetching all jobs from app_instance {}", app_instance.id);
    
    let mut all_jobs = Vec::new();

    // Use the jobs table ID directly from the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        let table_id = &jobs.jobs_table_id;
        
        // Fetch all jobs from the ObjectTable
        let mut page_token = None;
        loop {
            let list_request = ListDynamicFieldsRequest {
                parent: Some(table_id.clone()),
                page_size: Some(100),
                page_token: page_token.clone(),
                read_mask: Some(prost_types::FieldMask {
                    paths: vec!["field_id".to_string()],
                }),
            };

            let fields_response = client
                .live_data_client()
                .list_dynamic_fields(list_request)
                .await
                .map_err(|e| SilvanaSuiInterfaceError::RpcConnectionError(
                    format!("Failed to list jobs: {}", e)
                ))?;

            let response = fields_response.into_inner();
            
            // Fetch each job
            for field in &response.dynamic_fields {
                if let Some(field_id) = &field.field_id {
                    // Extract job_sequence from name_value for debugging
                    let job_sequence = field.name_value.as_ref()
                        .and_then(|nv| bcs::from_bytes::<u64>(nv).ok())
                        .unwrap_or(0);
                    
                    // Fetch the job using the helper function
                    if let Ok(Some(job)) = fetch_job_object_by_field_id(field_id, job_sequence).await {
                        all_jobs.push(job);
                    }
                }
            }

            // Check for next page
            if let Some(next_token) = response.next_page_token {
                if !next_token.is_empty() {
                    page_token = Some(next_token);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    
    debug!("Found {} total jobs in app_instance {}", all_jobs.len(), app_instance.id);
    Ok(all_jobs)
}

/// Fetch failed jobs from an app instance
pub async fn fetch_failed_jobs_from_app_instance(
    app_instance: &AppInstance,
) -> Result<Vec<Job>> {
    debug!("Fetching failed jobs from app_instance {}", app_instance.id);
    
    // Use the Jobs struct that's already in the AppInstance
    if let Some(jobs) = &app_instance.jobs {
        if jobs.failed_jobs_count == 0 {
            debug!("No failed jobs in app_instance {}", app_instance.id);
            return Ok(Vec::new());
        }
        
        // Fetch all failed jobs using batch fetch
        let failed_jobs_map = fetch_jobs_batch(&jobs.failed_jobs_table_id, &jobs.failed_jobs_index).await?;
        let mut failed_jobs: Vec<Job> = failed_jobs_map.into_iter().map(|(_, job)| job).collect();
        
        // Sort by job_sequence for consistent ordering
        failed_jobs.sort_by_key(|job| job.job_sequence);
        
        debug!("Found {} failed jobs in app_instance {}", failed_jobs.len(), app_instance.id);
        return Ok(failed_jobs);
    }
    
    debug!("No Jobs found in AppInstance {}", app_instance.id);
    Ok(Vec::new())
}

/// Check if an app instance has any failed jobs
pub async fn has_failed_jobs(app_instance: &AppInstance) -> bool {
    if let Some(jobs) = &app_instance.jobs {
        return jobs.failed_jobs_count > 0;
    }
    false
}

/// Get the count of failed jobs in an app instance
pub async fn get_failed_jobs_count(app_instance: &AppInstance) -> u64 {
    if let Some(jobs) = &app_instance.jobs {
        return jobs.failed_jobs_count;
    }
    0
}
