use crate::error::{CoordinatorError, Result};
use crate::state::SharedState;
use serde::Deserialize;
use std::collections::HashSet;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, error, info};

#[derive(Debug, Clone, Deserialize)]
pub struct PendingJob {
    pub job_id: u64,
    pub description: Option<String>,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    pub app: String,
    pub app_instance: String,
    pub app_instance_method: String,
    pub sequences: Option<Vec<u64>>,
    pub data: Vec<u8>,
    pub status: JobStatus,
    pub attempts: u8,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Failed(String),
}

/// Fetch pending jobs from an AppInstance
/// If onlyCheck is true, only checks if there are pending jobs without fetching details (fast mode)
/// Removes app_instance from SharedState if no pending jobs exist
/// limit: Optional limit on number of jobs to fetch (None = fetch all)
pub async fn fetch_pending_jobs_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    state: &SharedState,
    only_check: bool,
    limit: Option<usize>,
) -> Result<Vec<PendingJob>> {
    let mode = if only_check { "Checking" } else { "Fetching" };
    let limit_str = limit.map_or("all".to_string(), |l| l.to_string());
    info!("{} {} pending jobs from app_instance: {}", mode, limit_str, app_instance_id);
    
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    // First, fetch the AppInstance object to get the jobs field
    let app_instance_request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec![
                "object_id".to_string(),
                "version".to_string(),
                "object_type".to_string(),
                "json".to_string(),
            ],
        }),
    };

    let app_instance_response = client
        .ledger_client()
        .get_object(app_instance_request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        // Extract jobs object from AppInstance JSON
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Look for the jobs field in the AppInstance
                if let Some(jobs_field) = struct_value.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Extract the Jobs object ID and pending_jobs set
                        if let Some(id_field) = jobs_struct.fields.get("id") {
                            if let Some(prost_types::value::Kind::StringValue(_jobs_object_id)) = &id_field.kind {
                                
                                // Extract pending_jobs VecSet to know which job IDs to fetch
                                let mut pending_job_ids = HashSet::new();
                                if let Some(pending_field) = jobs_struct.fields.get("pending_jobs") {
                                    if let Some(prost_types::value::Kind::StructValue(pending_struct)) = &pending_field.kind {
                                        if let Some(contents_field) = pending_struct.fields.get("contents") {
                                            if let Some(prost_types::value::Kind::ListValue(pending_list)) = &contents_field.kind {
                                                for job_id_value in &pending_list.values {
                                                    if let Some(prost_types::value::Kind::StringValue(job_id_str)) = &job_id_value.kind {
                                                        if let Ok(job_id) = job_id_str.parse::<u64>() {
                                                            pending_job_ids.insert(job_id);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                debug!("Found {} pending job IDs in app_instance", pending_job_ids.len());
                                
                                // If no pending jobs, remove from state
                                if pending_job_ids.is_empty() {
                                    info!("No pending jobs in app_instance {}, removing from tracking", app_instance_id);
                                    state.remove_app_instance(app_instance_id).await;
                                    return Ok(Vec::new());
                                }
                                
                                // If only checking, return early without fetching job details
                                if only_check {
                                    info!("App_instance {} has {} pending jobs (check-only mode)", 
                                        app_instance_id, pending_job_ids.len());
                                    // Return empty vec but indicate there are pending jobs by not removing from state
                                    return Ok(Vec::new());
                                }
                                
                                // Extract jobs ObjectTable ID and fetch details
                                if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                                    if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                        if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                            if let Some(prost_types::value::Kind::StringValue(jobs_table_id)) = &table_id_field.kind {
                                                // Now fetch the pending jobs from the jobs table
                                                return fetch_jobs_from_table(
                                                    client,
                                                    jobs_table_id,
                                                    pending_job_ids,
                                                    limit,
                                                ).await;
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

    Err(CoordinatorError::ConfigError(format!(
        "Failed to extract jobs from app_instance {}",
        formatted_id
    )))
}

/// Fetch specific jobs from the jobs ObjectTable
async fn fetch_jobs_from_table(
    client: &mut Client,
    jobs_table_id: &str,
    pending_job_ids: HashSet<u64>,
    limit: Option<usize>,
) -> Result<Vec<PendingJob>> {
    let mut pending_jobs = Vec::new();
    
    // List all dynamic fields (jobs) in the jobs table
    let list_request = ListDynamicFieldsRequest {
        parent: Some(jobs_table_id.to_string()),
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
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to list jobs in table: {}", e)
        ))?;
    
    let response = list_response.into_inner();
    
    // Fetch each job that is in the pending_jobs set
    for field in &response.dynamic_fields {
        // Check if we've reached the limit
        if let Some(lim) = limit {
            if pending_jobs.len() >= lim {
                info!("Reached limit of {} jobs", lim);
                break;
            }
        }
        
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (job_id)
            if let Ok(job_id) = bcs::from_bytes::<u64>(name_value) {
                // Only fetch if this job is in the pending set
                if pending_job_ids.contains(&job_id) {
                    if let Some(field_id) = &field.field_id {
                        // Fetch the job field wrapper
                        let job_field_request = GetObjectRequest {
                            object_id: Some(field_id.clone()),
                            version: None,
                            read_mask: Some(prost_types::FieldMask {
                                paths: vec![
                                    "object_id".to_string(),
                                    "json".to_string(),
                                ],
                            }),
                        };
                        
                        let job_field_response = client
                            .ledger_client()
                            .get_object(job_field_request)
                            .await
                            .map_err(|e| CoordinatorError::RpcConnectionError(
                                format!("Failed to fetch job field {}: {}", job_id, e)
                            ))?;
                        
                        if let Some(job_field_object) = job_field_response.into_inner().object {
                            // Extract the actual job object ID from the Field wrapper
                            if let Some(json_value) = &job_field_object.json {
                                if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                                    if let Some(value_field) = struct_value.fields.get("value") {
                                        if let Some(prost_types::value::Kind::StringValue(job_object_id)) = &value_field.kind {
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
                                                .map_err(|e| CoordinatorError::RpcConnectionError(
                                                    format!("Failed to fetch job {}: {}", job_id, e)
                                                ))?;
                                            
                                            if let Some(job_object) = job_response.into_inner().object {
                                                if let Some(job_json) = &job_object.json {
                                                    if let Ok(job) = extract_job_from_json(job_json) {
                                                        pending_jobs.push(job);
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
    }
    
    info!("Fetched {} pending jobs from jobs table", pending_jobs.len());
    Ok(pending_jobs)
}

/// Extract PendingJob from JSON representation
fn extract_job_from_json(json_value: &prost_types::Value) -> Result<PendingJob> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        let mut job = PendingJob {
            job_id: 0,
            description: None,
            developer: String::new(),
            agent: String::new(),
            agent_method: String::new(),
            app: String::new(),
            app_instance: String::new(),
            app_instance_method: String::new(),
            sequences: None,
            data: Vec::new(),
            status: JobStatus::Pending,
            attempts: 0,
            created_at: 0,
            updated_at: 0,
        };
        
        // Extract all fields from the job struct
        if let Some(field) = struct_value.fields.get("job_id") {
            if let Some(prost_types::value::Kind::StringValue(id_str)) = &field.kind {
                job.job_id = id_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("description") {
            match &field.kind {
                Some(prost_types::value::Kind::StringValue(desc)) => {
                    job.description = Some(desc.clone());
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    job.description = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("developer") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.developer = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("agent") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.agent = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("agent_method") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.agent_method = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app_instance") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app_instance = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("app_instance_method") {
            if let Some(prost_types::value::Kind::StringValue(val)) = &field.kind {
                job.app_instance_method = val.clone();
            }
        }
        
        if let Some(field) = struct_value.fields.get("sequences") {
            match &field.kind {
                Some(prost_types::value::Kind::ListValue(list)) => {
                    let mut sequences = Vec::new();
                    for val in &list.values {
                        if let Some(prost_types::value::Kind::StringValue(seq_str)) = &val.kind {
                            if let Ok(seq) = seq_str.parse::<u64>() {
                                sequences.push(seq);
                            }
                        }
                    }
                    if !sequences.is_empty() {
                        job.sequences = Some(sequences);
                    }
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    job.sequences = None;
                }
                _ => {}
            }
        }
        
        if let Some(field) = struct_value.fields.get("data") {
            if let Some(prost_types::value::Kind::StringValue(data_str)) = &field.kind {
                // Data might be base64 encoded or hex encoded
                if let Ok(data) = hex::decode(data_str.trim_start_matches("0x")) {
                    job.data = data;
                }
            }
        }
        
        if let Some(field) = struct_value.fields.get("status") {
            if let Some(prost_types::value::Kind::StructValue(status_struct)) = &field.kind {
                // Parse JobStatus enum
                if let Some(variant_field) = status_struct.fields.iter().next() {
                    match variant_field.0.as_str() {
                        "Pending" => job.status = JobStatus::Pending,
                        "Running" => job.status = JobStatus::Running,
                        "Failed" => {
                            if let Some(prost_types::value::Kind::StringValue(msg)) = &variant_field.1.kind {
                                job.status = JobStatus::Failed(msg.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        
        if let Some(field) = struct_value.fields.get("attempts") {
            if let Some(prost_types::value::Kind::NumberValue(attempts)) = &field.kind {
                job.attempts = *attempts as u8;
            } else if let Some(prost_types::value::Kind::StringValue(attempts_str)) = &field.kind {
                job.attempts = attempts_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("created_at") {
            if let Some(prost_types::value::Kind::StringValue(ts_str)) = &field.kind {
                job.created_at = ts_str.parse().unwrap_or(0);
            }
        }
        
        if let Some(field) = struct_value.fields.get("updated_at") {
            if let Some(prost_types::value::Kind::StringValue(ts_str)) = &field.kind {
                job.updated_at = ts_str.parse().unwrap_or(0);
            }
        }
        
        return Ok(job);
    }
    
    Err(CoordinatorError::ConfigError(
        "Failed to extract job from JSON".to_string()
    ))
}

/// Fetch all pending jobs from multiple app_instances
pub async fn fetch_all_pending_jobs(
    client: &mut Client,
    app_instance_ids: &[String],
    state: &SharedState,
    only_check: bool,
) -> Result<Vec<PendingJob>> {
    let mut all_pending_jobs = Vec::new();
    
    for app_instance_id in app_instance_ids {
        match fetch_pending_jobs_from_app_instance(client, app_instance_id, state, only_check, None).await {
            Ok(jobs) => {
                if !only_check && !jobs.is_empty() {
                    info!("Found {} pending jobs in app_instance {}", jobs.len(), app_instance_id);
                    all_pending_jobs.extend(jobs);
                }
            }
            Err(e) => {
                error!("Failed to fetch pending jobs from app_instance {}: {}", app_instance_id, e);
            }
        }
    }
    
    if !only_check {
        info!("Total pending jobs across all app_instances: {}", all_pending_jobs.len());
    }
    Ok(all_pending_jobs)
}