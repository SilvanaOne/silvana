use crate::error::{CoordinatorError, Result};
use crate::pending::{PendingJob, JobStatus};
use crate::state::SharedState;
use std::collections::HashSet;
use sui_rpc::Client;
use sui_rpc::proto::sui::rpc::v2beta2::{GetObjectRequest, ListDynamicFieldsRequest};
use tracing::{debug, error, info, warn};
use base64::{engine::general_purpose, Engine as _};

/// Represents a sequence state from the Move SequenceState struct
#[derive(Debug, Clone)]
pub struct SequenceState {
    pub sequence: u64,
    pub state: Option<Vec<u8>>,
    pub data_availability: Option<String>,
    pub optimistic_state: Vec<u8>,
    pub transition_data: Vec<u8>,
}

/// Extract PendingJob from JSON representation
pub fn extract_job_from_json(json_value: &prost_types::Value) -> Result<PendingJob> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        debug!("Job JSON fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
        let mut job = PendingJob {
            job_sequence: 0,
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
        if let Some(field) = struct_value.fields.get("job_sequence") {
            if let Some(prost_types::value::Kind::StringValue(id_str)) = &field.kind {
                job.job_sequence = id_str.parse().unwrap_or(0);
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
            debug!("Found data field: {:?}", field.kind);
            if let Some(prost_types::value::Kind::StringValue(data_str)) = &field.kind {
                debug!("Data string: {}", data_str);
                // Data might be base64 encoded or hex encoded
                if let Ok(data) = hex::decode(data_str.trim_start_matches("0x")) {
                    debug!("Decoded hex data length: {}", data.len());
                    job.data = data;
                } else if let Ok(data) = general_purpose::STANDARD.decode(data_str) {
                    debug!("Decoded base64 data length: {}", data.len());
                    job.data = data;
                } else {
                    debug!("Failed to decode data as hex or base64: {}", data_str);
                }
            }
        } else {
            debug!("No data field found in job JSON");
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

/// Fetch pending jobs from a specific app_instance
pub async fn fetch_pending_jobs_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    state: &SharedState,
    only_check: bool,
) -> Result<Option<PendingJob>> {
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    debug!("Fetching app_instance object: {}", formatted_id);
    
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
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Look for the jobs field in the AppInstance
                if let Some(jobs_field) = struct_value.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // For check-only mode, just look at pending_jobs_count
                        if only_check {
                            if let Some(count_field) = jobs_struct.fields.get("pending_jobs_count") {
                                if let Some(prost_types::value::Kind::StringValue(count_str)) = &count_field.kind {
                                    if let Ok(count) = count_str.parse::<u64>() {
                                        if count > 0 {
                                            debug!("Found {} pending jobs in app_instance (check-only mode)", count);
                                            return Ok(None);
                                        } else {
                                            info!("No pending jobs in app_instance {} (count=0), removing from tracking", app_instance_id);
                                            state.remove_app_instance(app_instance_id).await;
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Get the pending_jobs VecSet
                        if let Some(pending_jobs_field) = jobs_struct.fields.get("pending_jobs") {
                            if let Some(prost_types::value::Kind::StructValue(pending_jobs_struct)) = &pending_jobs_field.kind {
                                if let Some(contents_field) = pending_jobs_struct.fields.get("contents") {
                                    if let Some(prost_types::value::Kind::ListValue(list)) = &contents_field.kind {
                                        let mut pending_job_sequences = HashSet::new();
                                        for job_sequence_value in &list.values {
                                            if let Some(prost_types::value::Kind::StringValue(job_sequence_str)) = &job_sequence_value.kind {
                                                if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                                    pending_job_sequences.insert(job_sequence);
                                                }
                                            }
                                        }
                                        
                                        if pending_job_sequences.is_empty() {
                                            info!("No pending jobs in app_instance {}, removing from tracking", app_instance_id);
                                            state.remove_app_instance(app_instance_id).await;
                                            return Ok(None);
                                        }
                                        
                                        // Get the smallest job_sequence
                                        let mut job_sequences: Vec<u64> = pending_job_sequences.iter().cloned().collect();
                                        job_sequences.sort();
                                        let target_job_sequence = job_sequences[0];
                                        
                                        debug!("Found {} pending jobs, will fetch job_sequence {}", job_sequences.len(), target_job_sequence);
                                        
                                        // Now fetch the specific job from the jobs ObjectTable
                                        if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                                            if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                                if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                                    if let Some(prost_types::value::Kind::StringValue(table_id)) = &table_id_field.kind {
                                                        // Fetch the specific job
                                                        if let Some(job) = fetch_job_by_id(client, table_id, target_job_sequence).await? {
                                                            return Ok(Some(job));
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
    }
    
    debug!("No jobs found in app_instance {}", app_instance_id);
    Ok(None)
}

/// Fetch a specific job by ID from the jobs ObjectTable
pub async fn fetch_job_by_id(
    client: &mut Client,
    jobs_table_id: &str,
    job_sequence: u64,
) -> Result<Option<PendingJob>> {
    debug!("Fetching job {} from jobs table {}", job_sequence, jobs_table_id);
    
    // List dynamic fields to find the specific job
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
    
    // Find the specific job entry
    for field in &response.dynamic_fields {
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (job_sequence)
            if let Ok(field_job_sequence) = bcs::from_bytes::<u64>(name_value) {
                if field_job_sequence == job_sequence {
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
                                format!("Failed to fetch job field {}: {}", job_sequence, e)
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
                                                    format!("Failed to fetch job {}: {}", job_sequence, e)
                                                ))?;
                                            
                                            if let Some(job_object) = job_response.into_inner().object {
                                                if let Some(job_json) = &job_object.json {
                                                    if let Ok(job) = extract_job_from_json(job_json) {
                                                        return Ok(Some(job));
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
    
    debug!("Job {} not found in jobs table", job_sequence);
    Ok(None)
}

/// Fetch pending job IDs for a specific (developer, agent, agent_method) from the embedded Jobs in AppInstance
pub async fn fetch_pending_job_sequences_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Vec<u64>> {
    info!(
        "Fetching pending job IDs for {}/{}/{} from app_instance {}",
        developer, agent, agent_method, app_instance_id
    );
    
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    // Fetch the AppInstance object
    let request = GetObjectRequest {
        object_id: Some(formatted_id.clone()),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".to_string()],
        }),
    };

    let response = client
        .ledger_client()
        .get_object(request)
        .await
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch AppInstance {}: {}", formatted_id, e)
        ))?;

    if let Some(proto_object) = response.into_inner().object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(app_instance_struct)) = &json_value.kind {
                // Get the embedded jobs field
                if let Some(jobs_field) = app_instance_struct.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Look for the pending_jobs_indexes field
                        if let Some(indexes_field) = jobs_struct.fields.get("pending_jobs_indexes") {
                            if let Some(prost_types::value::Kind::StructValue(indexes_struct)) = &indexes_field.kind {
                                // Navigate through the nested VecMap structure
                                if let Some(contents_field) = indexes_struct.fields.get("contents") {
                                    if let Some(prost_types::value::Kind::ListValue(dev_list)) = &contents_field.kind {
                                        // Look for the developer entry
                                        for dev_entry in &dev_list.values {
                                            if let Some(prost_types::value::Kind::StructValue(dev_struct)) = &dev_entry.kind {
                                                if let Some(key_field) = dev_struct.fields.get("key") {
                                                    if let Some(prost_types::value::Kind::StringValue(dev_key)) = &key_field.kind {
                                                        if dev_key == developer {
                                                            // Found the developer, now look for agent
                                                            if let Some(value_field) = dev_struct.fields.get("value") {
                                                                if let Some(prost_types::value::Kind::StructValue(agent_map)) = &value_field.kind {
                                                                    if let Some(agent_contents) = agent_map.fields.get("contents") {
                                                                        if let Some(prost_types::value::Kind::ListValue(agent_list)) = &agent_contents.kind {
                                                                            // Look for the agent entry
                                                                            for agent_entry in &agent_list.values {
                                                                                if let Some(prost_types::value::Kind::StructValue(agent_struct)) = &agent_entry.kind {
                                                                                    if let Some(agent_key_field) = agent_struct.fields.get("key") {
                                                                                        if let Some(prost_types::value::Kind::StringValue(agent_key)) = &agent_key_field.kind {
                                                                                            if agent_key == agent {
                                                                                                // Found the agent, now look for agent_method
                                                                                                if let Some(agent_value_field) = agent_struct.fields.get("value") {
                                                                                                    if let Some(prost_types::value::Kind::StructValue(method_map)) = &agent_value_field.kind {
                                                                                                        if let Some(method_contents) = method_map.fields.get("contents") {
                                                                                                            if let Some(prost_types::value::Kind::ListValue(method_list)) = &method_contents.kind {
                                                                                                                // Look for the agent_method entry
                                                                                                                for method_entry in &method_list.values {
                                                                                                                    if let Some(prost_types::value::Kind::StructValue(method_struct)) = &method_entry.kind {
                                                                                                                        if let Some(method_key_field) = method_struct.fields.get("key") {
                                                                                                                            if let Some(prost_types::value::Kind::StringValue(method_key)) = &method_key_field.kind {
                                                                                                                                if method_key == agent_method {
                                                                                                                                    // Found the method, extract job IDs
                                                                                                                                    if let Some(method_value_field) = method_struct.fields.get("value") {
                                                                                                                                        if let Some(prost_types::value::Kind::StructValue(job_set)) = &method_value_field.kind {
                                                                                                                                            if let Some(job_contents) = job_set.fields.get("contents") {
                                                                                                                                                if let Some(prost_types::value::Kind::ListValue(job_list)) = &job_contents.kind {
                                                                                                                                                    let mut job_sequences = Vec::new();
                                                                                                                                                    for job_value in &job_list.values {
                                                                                                                                                        if let Some(prost_types::value::Kind::StringValue(job_sequence_str)) = &job_value.kind {
                                                                                                                                                            if let Ok(job_sequence) = job_sequence_str.parse::<u64>() {
                                                                                                                                                                job_sequences.push(job_sequence);
                                                                                                                                                            }
                                                                                                                                                        }
                                                                                                                                                    }
                                                                                                                                                    debug!("Found {} pending job sequences for {}/{}/{}", 
                                                                                                                                                        job_sequences.len(), developer, agent, agent_method);
                                                                                                                                                    return Ok(job_sequences);
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
    
    debug!("No pending jobs found for {}/{}/{}", developer, agent, agent_method);
    Ok(Vec::new())
}

/// Get the Jobs table ID from an AppInstance (Jobs is embedded, so we just need the table ID)
pub async fn get_jobs_info_from_app_instance(
    client: &mut Client,
    app_instance_id: &str,
) -> Result<Option<(String, String)>> {
    // Ensure the app_instance_id has 0x prefix
    let formatted_id = if app_instance_id.starts_with("0x") {
        app_instance_id.to_string()
    } else {
        format!("0x{}", app_instance_id)
    };
    
    debug!("Fetching Jobs info from app_instance: {}", formatted_id);
    
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
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to fetch app_instance {}: {}", formatted_id, e)
        ))?;

    let response = app_instance_response.into_inner();
    
    if let Some(proto_object) = response.object {
        if let Some(json_value) = &proto_object.json {
            if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
                // Debug: print all fields in AppInstance
                debug!("AppInstance {} fields: {:?}", app_instance_id, struct_value.fields.keys().collect::<Vec<_>>());
                
                // Look for the jobs field in the AppInstance
                if let Some(jobs_field) = struct_value.fields.get("jobs") {
                    if let Some(prost_types::value::Kind::StructValue(jobs_struct)) = &jobs_field.kind {
                        // Debug: print all fields in Jobs struct
                        debug!("Jobs struct fields: {:?}", jobs_struct.fields.keys().collect::<Vec<_>>());
                        
                        // For embedded Jobs, we return the app_instance_id as the "Jobs object ID" 
                        // and extract the jobs table ID
                        if let Some(jobs_table_field) = jobs_struct.fields.get("jobs") {
                            if let Some(prost_types::value::Kind::StructValue(jobs_table_struct)) = &jobs_table_field.kind {
                                debug!("Jobs table struct fields: {:?}", jobs_table_struct.fields.keys().collect::<Vec<_>>());
                                if let Some(table_id_field) = jobs_table_struct.fields.get("id") {
                                    if let Some(prost_types::value::Kind::StringValue(jobs_table_id)) = &table_id_field.kind {
                                        debug!("Found Jobs table ID: {}", jobs_table_id);
                                        // Return app_instance_id as the "Jobs object" since Jobs is embedded
                                        return Ok(Some((formatted_id, jobs_table_id.clone())));
                                    } else {
                                        warn!("Jobs table id field is not a string: {:?}", table_id_field.kind);
                                    }
                                } else {
                                    warn!("No 'id' field in jobs table struct");
                                }
                            } else {
                                warn!("Jobs table field is not a struct: {:?}", jobs_table_field.kind);
                            }
                        } else {
                            warn!("No 'jobs' field in Jobs struct");
                        }
                    } else {
                        warn!("Jobs field is not a struct: {:?}", jobs_field.kind);
                    }
                } else {
                    warn!("No 'jobs' field found in AppInstance. Available fields: {:?}", struct_value.fields.keys().collect::<Vec<_>>());
                }
            }
        }
    } else {
        warn!("No object returned for app_instance {}", app_instance_id);
    }
    
    Ok(None)
}
/// Try to fetch a pending job from any of the given app_instances using the index
pub async fn fetch_pending_job_from_instances(
    client: &mut Client,
    app_instances: &[String],
    developer: &str,
    agent: &str,
    agent_method: &str,
) -> Result<Option<PendingJob>> {
    // Collect all job IDs from all app_instances
    let mut all_jobs: Vec<(u64, String, String)> = Vec::new(); // (job_sequence, app_instance_id, jobs_table_id)
    
    for app_instance in app_instances {
        // Get Jobs table ID from the AppInstance
        let (_app_instance_id, jobs_table_id) = match get_jobs_info_from_app_instance(client, app_instance).await? {
            Some(info) => info,
            None => {
                warn!("Could not extract Jobs info from app_instance {}", app_instance);
                continue;
            }
        };
        
        // Use the index to get pending job IDs for this method
        let job_sequences = fetch_pending_job_sequences_from_app_instance(
            client,
            app_instance,
            developer,
            agent,
            agent_method,
        ).await?;
        
        for job_sequence in job_sequences {
            all_jobs.push((job_sequence, app_instance.clone(), jobs_table_id.clone()));
        }
    }
    
    if all_jobs.is_empty() {
        debug!("No pending jobs found in any app_instance for {}/{}/{}", developer, agent, agent_method);
        return Ok(None);
    }
    
    // Sort by job_sequence to get the lowest one
    all_jobs.sort_by_key(|&(job_sequence, _, _)| job_sequence);
    let (lowest_job_sequence, app_instance, jobs_table_id) = &all_jobs[0];
    
    info!(
        "Found {} total pending jobs across {} app_instances for {}/{}/{}, fetching job {} from {}",
        all_jobs.len(), app_instances.len(), developer, agent, agent_method, lowest_job_sequence, app_instance
    );
    
    // Fetch the specific job by ID
    fetch_job_by_id(client, jobs_table_id, *lowest_job_sequence).await
}

/// Fetch the pending job with the smallest job_sequence from multiple app_instances
pub async fn fetch_all_pending_jobs(
    client: &mut Client,
    app_instance_ids: &[String],
    state: &SharedState,
    only_check: bool,
) -> Result<Option<PendingJob>> {
    let mut all_pending_jobs = Vec::new();
    
    for app_instance_id in app_instance_ids {
        match fetch_pending_jobs_from_app_instance(client, app_instance_id, state, only_check).await {
            Ok(job_opt) => {
                if !only_check {
                    if let Some(job) = job_opt {
                        info!("Found pending job with job_sequence {} in app_instance {}", job.job_sequence, app_instance_id);
                        all_pending_jobs.push(job);
                    }
                }
            }
            Err(e) => {
                error!("Failed to fetch pending job from app_instance {}: {}", app_instance_id, e);
            }
        }
    }
    
    // Sort all collected jobs and return the one with smallest job_sequence
    if all_pending_jobs.is_empty() {
        if !only_check {
            info!("No pending jobs found across all app_instances");
        }
        Ok(None)
    } else {
        all_pending_jobs.sort_by_key(|job| job.job_sequence);
        let job = all_pending_jobs.into_iter().next().unwrap();
        info!("Returning pending job with smallest job_sequence: {}", job.job_sequence);
        Ok(Some(job))
    }
}

/// Extract SequenceState from JSON representation
pub fn extract_sequence_state_from_json(json_value: &prost_types::Value) -> Result<SequenceState> {
    if let Some(prost_types::value::Kind::StructValue(struct_value)) = &json_value.kind {
        let mut sequence_state = SequenceState {
            sequence: 0,
            state: None,
            data_availability: None,
            optimistic_state: Vec::new(),
            transition_data: Vec::new(),
        };
        
        // Extract sequence field
        if let Some(field) = struct_value.fields.get("sequence") {
            if let Some(prost_types::value::Kind::StringValue(seq_str)) = &field.kind {
                sequence_state.sequence = seq_str.parse().unwrap_or(0);
            }
        }
        
        // Extract state field (Option<vector<u8>>)
        if let Some(field) = struct_value.fields.get("state") {
            match &field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    // Check if it's Some variant
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(state_str)) = &some_field.kind {
                            if let Ok(state_data) = hex::decode(state_str.trim_start_matches("0x")) {
                                sequence_state.state = Some(state_data);
                            }
                        }
                    }
                    // If None variant exists or no Some field, state remains None
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    sequence_state.state = None;
                }
                _ => {}
            }
        }
        
        // Extract data_availability field (Option<String>)
        if let Some(field) = struct_value.fields.get("data_availability") {
            match &field.kind {
                Some(prost_types::value::Kind::StructValue(option_struct)) => {
                    // Check if it's Some variant
                    if let Some(some_field) = option_struct.fields.get("Some") {
                        if let Some(prost_types::value::Kind::StringValue(da_str)) = &some_field.kind {
                            sequence_state.data_availability = Some(da_str.clone());
                        }
                    }
                    // If None variant exists or no Some field, data_availability remains None
                }
                Some(prost_types::value::Kind::NullValue(_)) => {
                    sequence_state.data_availability = None;
                }
                _ => {}
            }
        }
        
        // Extract optimistic_state field
        if let Some(field) = struct_value.fields.get("optimistic_state") {
            if let Some(prost_types::value::Kind::StringValue(state_str)) = &field.kind {
                if let Ok(state_data) = hex::decode(state_str.trim_start_matches("0x")) {
                    sequence_state.optimistic_state = state_data;
                }
            }
        }
        
        // Extract transition_data field
        if let Some(field) = struct_value.fields.get("transition_data") {
            if let Some(prost_types::value::Kind::StringValue(data_str)) = &field.kind {
                if let Ok(data) = hex::decode(data_str.trim_start_matches("0x")) {
                    sequence_state.transition_data = data;
                }
            }
        }
        
        return Ok(sequence_state);
    }
    
    Err(CoordinatorError::ConfigError(
        "Failed to extract sequence state from JSON".to_string()
    ))
}

/// Fetch a specific SequenceState by sequence number from the sequence_states ObjectTable
pub async fn fetch_sequence_state_by_id(
    client: &mut Client,
    sequence_states_table_id: &str,
    sequence: u64,
) -> Result<Option<SequenceState>> {
    debug!("Fetching sequence {} from sequence_states table {}", sequence, sequence_states_table_id);
    
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
        .map_err(|e| CoordinatorError::RpcConnectionError(
            format!("Failed to list sequence states in table: {}", e)
        ))?;
    
    let response = list_response.into_inner();
    
    // Find the specific sequence state entry
    for field in &response.dynamic_fields {
        if let Some(name_value) = &field.name_value {
            // The name_value is BCS-encoded u64 (sequence)
            if let Ok(field_sequence) = bcs::from_bytes::<u64>(name_value) {
                if field_sequence == sequence {
                    if let Some(field_id) = &field.field_id {
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
                            .map_err(|e| CoordinatorError::RpcConnectionError(
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
                                                .map_err(|e| CoordinatorError::RpcConnectionError(
                                                    format!("Failed to fetch sequence state {}: {}", sequence, e)
                                                ))?;
                                            
                                            if let Some(sequence_state_object) = sequence_state_response.into_inner().object {
                                                if let Some(sequence_state_json) = &sequence_state_object.json {
                                                    if let Ok(sequence_state) = extract_sequence_state_from_json(sequence_state_json) {
                                                        return Ok(Some(sequence_state));
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
    
    debug!("Sequence state {} not found in table", sequence);
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
        .map_err(|e| CoordinatorError::RpcConnectionError(
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
        Some(info) => info,
        None => {
            return Err(CoordinatorError::ConfigError(
                format!("SequenceStateManager not found in app_instance {}", app_instance_id)
            ));
        }
    };
    
    // Check if requested sequence is within bounds
    if requested_sequence < lowest_sequence || requested_sequence > highest_sequence {
        return Err(CoordinatorError::ConfigError(
            format!("Requested sequence {} is out of bounds [{}, {}]", 
                requested_sequence, lowest_sequence, highest_sequence)
        ));
    }
    
    // Fetch the requested sequence state first
    let requested_state = match fetch_sequence_state_by_id(client, &table_id, requested_sequence).await? {
        Some(state) => state,
        None => {
            return Err(CoordinatorError::ConfigError(
                format!("Sequence state {} not found", requested_sequence)
            ));
        }
    };
    
    // Check if requested sequence has both state and data_availability
    if requested_state.state.is_some() && requested_state.data_availability.is_some() {
        // Return just this sequence
        info!("Requested sequence {} has complete data, returning single state", requested_sequence);
        return Ok(vec![requested_state]);
    }
    
    // Find the highest sequence with both state and data_availability set
    let mut start_sequence = None;
    for seq in (lowest_sequence..=requested_sequence).rev() {
        if let Ok(Some(state)) = fetch_sequence_state_by_id(client, &table_id, seq).await {
            if state.state.is_some() && state.data_availability.is_some() {
                start_sequence = Some(seq);
                break;
            }
        }
    }
    
    let start_seq = match start_sequence {
        Some(seq) => seq,
        None => {
            // No sequence with complete data found, return range from lowest to requested
            lowest_sequence
        }
    };
    
    // Fetch all sequences from start_seq (inclusive) to requested_sequence
    let mut result_states = Vec::new();
    for seq in start_seq..=requested_sequence {
        if let Ok(Some(state)) = fetch_sequence_state_by_id(client, &table_id, seq).await {
            result_states.push(state);
        }
    }
    
    info!("Returning {} sequence states from sequence {} to {}", 
        result_states.len(), start_seq, requested_sequence);
    Ok(result_states)
}
