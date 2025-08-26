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
    /// Optional sequence numbers
    pub sequences: Option<Vec<u64>>,
    /// Job data as bytes
    pub data: Vec<u8>,
    /// Job status
    pub status: JobStatus,
    /// Number of attempts
    pub attempts: u8,
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
}

impl Jobs {
    /// Parse Jobs from protobuf struct value
    pub fn from_proto_struct(struct_value: &prost_types::Struct) -> Option<Self> {
        // Helper to extract string value
        let get_string = |field_name: &str| -> Option<String> {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => Some(s.clone()),
                    _ => None,
                }
            })
        };
        
        // Helper to extract number as u64
        let get_u64 = |field_name: &str| -> u64 {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u64>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u64),
                    _ => None,
                }
            }).unwrap_or(0)
        };
        
        // Helper to extract number as u8
        let get_u8 = |field_name: &str| -> u8 {
            struct_value.fields.get(field_name).and_then(|f| {
                match &f.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => s.parse::<u8>().ok(),
                    Some(prost_types::value::Kind::NumberValue(n)) => Some(n.round() as u8),
                    _ => None,
                }
            }).unwrap_or(0)
        };
        
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
        
        Some(Jobs {
            id: get_string("id").unwrap_or_default(),
            jobs_table_id,
            pending_jobs,
            pending_jobs_count: get_u64("pending_jobs_count"),
            pending_jobs_indexes,
            next_job_sequence: get_u64("next_job_sequence"),
            max_attempts: get_u8("max_attempts"),
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